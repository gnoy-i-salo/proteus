//! Focused tests for the bounded experiment-history slice.
//!
//! All fixtures live under a unique temporary directory per test and are
//! removed on drop; no user result data is ever touched. Symlink cases are
//! unix-only, matching the crate's existing native test conventions. Tests are
//! white-box: they exercise the private scan helper to keep scan-cap fixtures
//! small instead of materializing a full `MAX_SCAN_ENTRIES` directory.

use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIR_ID: AtomicUsize = AtomicUsize::new(0);

struct TestDir(PathBuf);
impl TestDir {
    fn new(label: &str) -> Self {
        let id = NEXT_DIR_ID.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "proteus-history-test-{}-{id}-{label}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create test dir");
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_run(base: &Path, id: &str, content: &[u8]) {
    let dir = base.join(id);
    fs::create_dir(&dir).expect("create run dir");
    fs::write(dir.join(RESULT_FILE_NAME), content).expect("write result.json");
}

fn ids(page: &SavedRunPage) -> Vec<&str> {
    page.entries.iter().map(|entry| entry.id.as_str()).collect()
}

fn find_entry<'a>(page: &'a SavedRunPage, id: &str) -> &'a SavedRunSummary {
    page.entries
        .iter()
        .find(|entry| entry.id == id)
        .unwrap_or_else(|| panic!("entry {id} missing"))
}

#[test]
fn missing_base_directory_lists_empty() {
    let root = TestDir::new("missing-base");
    let base = root.path().join("experiments");
    let page = list_saved_runs(&base, 0, 10).expect("missing base lists empty");
    assert!(page.entries.is_empty());
    assert_eq!(page.matched, 0);
    assert_eq!(page.next_offset, None);
    assert!(!page.scan_truncated);
}

#[test]
fn base_regular_file_fails_explicitly() {
    let root = TestDir::new("base-is-file");
    let base = root.path().join("experiments");
    fs::write(&base, b"not a directory").expect("write base file");
    assert!(list_saved_runs(&base, 0, 10).is_err());
    assert!(read_saved_run(&base, "run-01").is_err());
}

#[test]
fn saved_runs_are_listed_in_stable_byte_order() {
    let root = TestDir::new("stable-order");
    let base = root.path();
    for id in ["run-03", "run-01", "run-02"] {
        write_run(base, id, br#"{"schemaVersion":4}"#);
    }
    let first = list_saved_runs(base, 0, 10).expect("list runs");
    assert_eq!(ids(&first), ["run-01", "run-02", "run-03"]);
    assert!(first.entries.iter().all(|entry| entry.ok));
    assert!(first.entries.iter().all(|entry| entry.error.is_none()));
    let second = list_saved_runs(base, 0, 10).expect("list runs again");
    assert_eq!(ids(&first), ids(&second));
}

#[test]
fn pagination_is_bounded_and_deterministic() {
    let root = TestDir::new("pagination");
    let base = root.path();
    for index in 1..=12 {
        write_run(base, &format!("page-{index:02}"), b"{}");
    }
    let page = list_saved_runs(base, 0, 5).expect("first page");
    assert_eq!(
        ids(&page),
        ["page-01", "page-02", "page-03", "page-04", "page-05"]
    );
    assert_eq!(page.matched, 12);
    assert_eq!(page.next_offset, Some(5));
    let page = list_saved_runs(base, 5, 5).expect("second page");
    assert_eq!(
        ids(&page),
        ["page-06", "page-07", "page-08", "page-09", "page-10"]
    );
    assert_eq!(page.next_offset, Some(10));
    let page = list_saved_runs(base, 10, 5).expect("last page");
    assert_eq!(ids(&page), ["page-11", "page-12"]);
    assert_eq!(page.next_offset, None);
    assert_eq!(
        list_saved_runs(base, 0, 0)
            .expect("default page")
            .entries
            .len(),
        12
    );
    assert!(list_saved_runs(base, 12, 5)
        .expect("past end")
        .entries
        .is_empty());
}

#[test]
fn page_limit_is_clamped_to_max_page_size() {
    let root = TestDir::new("clamped-limit");
    let base = root.path();
    for index in 0..MAX_PAGE_SIZE + 10 {
        write_run(base, &format!("bulk-{index:03}"), b"{}");
    }
    let page = list_saved_runs(base, 0, usize::MAX).expect("clamped page");
    assert_eq!(page.entries.len(), MAX_PAGE_SIZE);
    assert_eq!(page.next_offset, Some(MAX_PAGE_SIZE));
    assert_eq!(page.matched, MAX_PAGE_SIZE + 10);
}

#[test]
fn non_conforming_entries_are_skipped_silently() {
    let root = TestDir::new("skip-nonconforming");
    let base = root.path();
    write_run(base, "run-ok", b"{}");
    for name in ["not an id", ".hidden", "under_score", "привет"] {
        fs::create_dir(base.join(name)).expect("create non-conforming dir");
    }
    fs::write(base.join("run-file"), b"valid id shape but a file").expect("write stray file");
    let page = list_saved_runs(base, 0, 50).expect("list runs");
    assert_eq!(ids(&page), ["run-ok"]);
    assert_eq!(page.matched, 1);
}

#[test]
fn damaged_entries_are_isolated_and_keep_their_ids() {
    let root = TestDir::new("isolated-damage");
    let base = root.path();
    write_run(base, "run-ok", b"{}");
    fs::create_dir(base.join("miss-result")).expect("create run without result");
    write_run(
        base,
        "oversize-result",
        &vec![b'x'; MAX_RESULT_BYTES as usize + 1],
    );
    write_run(base, "not-json", b"this is not json");
    write_run(base, "invalid-utf8", &[0xFF, 0xFE]);
    let page = list_saved_runs(base, 0, 50).expect("list survives damaged entries");
    assert!(find_entry(&page, "run-ok").ok);
    let missing = find_entry(&page, "miss-result");
    assert!(!missing.ok);
    assert_eq!(missing.error.as_deref(), Some("missingResult"));
    let oversize = find_entry(&page, "oversize-result");
    assert!(!oversize.ok);
    assert_eq!(oversize.error.as_deref(), Some("oversizeResult"));
    assert!(find_entry(&page, "not-json").ok);
    assert!(find_entry(&page, "invalid-utf8").ok);
}

#[test]
fn scan_cap_marks_overflow_and_stays_bounded() {
    let root = TestDir::new("scan-cap");
    let base = root.path();
    for index in 0..5 {
        write_run(base, &format!("cap-{index}"), b"{}");
    }
    let (summaries, truncated) = collect_summaries(base, 3).expect("bounded scan");
    assert!(truncated);
    assert_eq!(summaries.len(), 3);
    let (summaries, truncated) = collect_summaries(base, 5).expect("exact scan");
    assert!(!truncated);
    assert_eq!(summaries.len(), 5);
}

#[test]
fn read_returns_exact_bytes_with_crlf_and_unicode() {
    let root = TestDir::new("exact-bytes");
    let base = root.path();
    let content =
        "{\r\n  \"ключ\": \"значение 😀\",\r\n  \"task\": \"  spaced\\tvalue  \"\r\n}\r\n";
    write_run(base, "run-utf8", content.as_bytes());
    let text = read_saved_run(base, "run-utf8").expect("read exact bytes");
    assert_eq!(text.id, "run-utf8");
    assert_eq!(text.text, content);
    assert!(text.text.contains("\r\n"));
}

#[test]
fn read_accepts_any_json_object_as_minimal_envelope() {
    let root = TestDir::new("object-envelope");
    let base = root.path();
    write_run(base, "run-empty-object", b"{}");
    write_run(base, "run-null-fields", br#"{"a":null,"b":[1,2]}"#);
    assert_eq!(
        read_saved_run(base, "run-empty-object")
            .expect("empty object is a valid envelope")
            .text,
        "{}"
    );
    assert!(read_saved_run(base, "run-null-fields").is_ok());
}

#[test]
fn read_rejects_invalid_ids_before_touching_the_filesystem() {
    let root = TestDir::new("invalid-ids");
    let base = root.path();
    write_run(base, "run-01", b"{}");
    let outside = root.path().join("outside-marker");
    fs::write(&outside, b"keep me").expect("write outside marker");
    for id in [
        "",
        "a/b",
        "../outside-marker",
        "..",
        "x".repeat(81).as_str(),
        "under_score",
        "привет",
    ] {
        assert!(
            read_saved_run(base, id).is_err(),
            "id {id:?} must be rejected"
        );
    }
    assert_eq!(fs::read(&outside).expect("outside untouched"), b"keep me");
}

#[test]
fn read_rejects_malformed_and_non_object_json() {
    let root = TestDir::new("bad-json");
    let base = root.path();
    for (id, content) in [
        ("empty-file", ""),
        ("truncated", "{\"schemaVersion\":"),
        ("not-json", "this is not json"),
        ("json-null", "null"),
        ("json-array", "[1,2,3]"),
        ("json-string", "\"x\""),
        ("json-number", "42"),
        ("json-bool", "true"),
    ] {
        write_run(base, id, content.as_bytes());
        assert!(read_saved_run(base, id).is_err(), "{id} must be rejected");
    }
}

#[test]
fn read_rejects_invalid_utf8() {
    let root = TestDir::new("invalid-utf8");
    let base = root.path();
    write_run(base, "run-binary", &[0xFF, 0xFE, 0x7B]);
    assert!(read_saved_run(base, "run-binary").is_err());
}

#[test]
fn read_rejects_oversize_results() {
    let root = TestDir::new("oversize-read");
    let base = root.path();
    write_run(
        base,
        "run-oversize",
        &vec![b'x'; MAX_RESULT_BYTES as usize + 1],
    );
    assert!(read_saved_run(base, "run-oversize").is_err());
    write_run(base, "run-max", &vec![b' '; MAX_RESULT_BYTES as usize]);
    assert!(read_saved_run(base, "run-max").is_err());
    let exact = format!(
        r#"{{"text":"{}"}}"#,
        "x".repeat(MAX_RESULT_BYTES as usize - 11)
    );
    assert_eq!(exact.len() as u64, MAX_RESULT_BYTES);
    write_run(base, "run-max-valid", exact.as_bytes());
    assert_eq!(read_saved_run(base, "run-max-valid").unwrap().text, exact);
}

#[test]
fn read_rejects_missing_runs_and_result_directories() {
    let root = TestDir::new("missing-and-dirs");
    let base = root.path();
    assert!(read_saved_run(base, "never-saved").is_err());
    let run = base.join("run-dir-result");
    fs::create_dir(&run).expect("create run dir");
    fs::create_dir(run.join(RESULT_FILE_NAME)).expect("result.json as directory");
    assert!(read_saved_run(base, "run-dir-result").is_err());
}

#[test]
fn list_and_read_never_write_to_the_base_tree() {
    let root = TestDir::new("read-only");
    let base = root.path();
    write_run(base, "run-01", b"{}");
    fs::create_dir(base.join("empty-run")).expect("create damaged run");
    fn snapshot(base: &Path) -> Vec<PathBuf> {
        fn walk(dir: &Path, paths: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(dir).expect("read dir") {
                let entry = entry.expect("dir entry");
                paths.push(entry.path());
                if entry.file_type().expect("file type").is_dir() {
                    walk(&entry.path(), paths);
                }
            }
        }
        let mut paths = Vec::new();
        walk(base, &mut paths);
        paths.sort();
        paths
    }
    let before = snapshot(base);
    let _ = list_saved_runs(base, 0, 10).expect("list");
    let _ = read_saved_run(base, "run-01").expect("read");
    assert_eq!(before, snapshot(base));
}

#[test]
fn wire_dtos_use_camel_case_field_names() {
    let root = TestDir::new("wire-shape");
    let base = root.path();
    write_run(base, "run-01", b"{}");
    let page =
        serde_json::to_value(list_saved_runs(base, 0, 10).expect("list")).expect("serialize page");
    let mut keys: Vec<&str> = page
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "entries",
            "matched",
            "nextOffset",
            "scanTruncated",
            "schemaVersion"
        ]
    );
    let entry = &page["entries"][0];
    let mut keys: Vec<&str> = entry
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["error", "id", "ok"]);
    let text = serde_json::to_value(read_saved_run(base, "run-01").expect("read"))
        .expect("serialize text");
    let mut keys: Vec<&str> = text
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["id", "schemaVersion", "text"]);
    assert_eq!(text["schemaVersion"], 1);
    assert_eq!(page["schemaVersion"], 1);
}

#[cfg(unix)]
#[test]
fn symlinked_base_run_and_result_are_rejected() {
    use std::os::unix::fs::symlink;
    let root = TestDir::new("symlinks");
    let real_base = root.path().join("real-base");
    fs::create_dir(&real_base).expect("create real base");
    write_run(&real_base, "run-ok", b"{}");
    let base_link = root.path().join("base-link");
    symlink(&real_base, &base_link).expect("create base symlink");
    assert!(list_saved_runs(&base_link, 0, 10).is_err());
    assert!(read_saved_run(&base_link, "run-ok").is_err());
    let base = root.path().join("base");
    fs::create_dir(&base).expect("create base");
    write_run(&base, "run-ok", b"{}");
    symlink(real_base.join("run-ok"), base.join("link-run")).expect("create run symlink");
    let page = list_saved_runs(&base, 0, 10).expect("list flags symlinked run");
    let linked = find_entry(&page, "link-run");
    assert!(!linked.ok);
    assert_eq!(linked.error.as_deref(), Some("symlinkedRun"));
    assert!(read_saved_run(&base, "link-run").is_err());
    let target = root.path().join("secret.txt");
    fs::write(&target, b"{}").expect("write symlink target");
    fs::create_dir(base.join("link-result")).expect("create run dir");
    symlink(&target, base.join("link-result").join(RESULT_FILE_NAME))
        .expect("create result symlink");
    let page = list_saved_runs(&base, 0, 10).expect("list flags symlinked result");
    assert_eq!(
        find_entry(&page, "link-result").error.as_deref(),
        Some("symlinkedResult")
    );
    assert!(read_saved_run(&base, "link-result").is_err());
    assert!(find_entry(&page, "run-ok").ok);
}
