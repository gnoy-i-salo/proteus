use super::*;
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "proteus-input-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn contents(file: &mut File) -> Result<Vec<u8>, String> {
    let mut bytes = vec![];
    file.read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

#[test]
fn selected_file_preserves_bytes_and_each_read_starts_at_zero() {
    let fixture = Fixture::new();
    let bytes = "  Имя 👩🏽‍💻\r\n\0точно  ".as_bytes();
    let path = fixture.file("точный.json", bytes);
    let inputs = SelectedInputs::default();
    assert_eq!(
        inputs.insert("selected-1".into(), &path).unwrap(),
        ("точный.json".into(), bytes.len() as u64)
    );
    assert_eq!(inputs.read("selected-1", contents).unwrap(), bytes);
    assert_eq!(inputs.read("selected-1", contents).unwrap(), bytes);
}

#[test]
fn arbitrary_paths_and_unknown_handles_cannot_read_files() {
    let fixture = Fixture::new();
    let path = fixture.file("private.json", b"private");
    let inputs = SelectedInputs::default();
    assert!(inputs.read(path.to_str().unwrap(), contents).is_err());
    assert!(inputs.read("unknown", contents).is_err());
    assert!(inputs.insert("directory".into(), &fixture.0).is_err());
}

#[test]
fn reused_handle_cannot_replace_existing_selection() {
    let fixture = Fixture::new();
    let first = fixture.file("first.json", b"first");
    let second = fixture.file("second.json", b"second");
    let inputs = SelectedInputs::default();
    inputs.insert("selected".into(), &first).unwrap();
    assert!(inputs.insert("selected".into(), &second).is_err());
    assert_eq!(inputs.read("selected", contents).unwrap(), b"first");
}

#[test]
fn selection_limit_is_reclaimed_only_by_explicit_release() {
    let fixture = Fixture::new();
    let path = fixture.file("archive.json", b"{}");
    let inputs = SelectedInputs::default();
    for index in 0..32 {
        inputs.insert(format!("selected-{index}"), &path).unwrap();
    }
    assert!(inputs.insert("over-limit".into(), &path).is_err());
    inputs.release("selected-0").unwrap();
    assert!(inputs.read("selected-0", contents).is_err());
    inputs.release("selected-0").unwrap();
    inputs.insert("replacement".into(), &path).unwrap();
    assert_eq!(inputs.read("selected-1", contents).unwrap(), b"{}");
}

#[test]
fn read_failure_does_not_poison_or_advance_the_next_read() {
    let fixture = Fixture::new();
    let path = fixture.file("archive.json", b"abcdef");
    let inputs = SelectedInputs::default();
    inputs.insert("selected".into(), &path).unwrap();
    let failure: Result<(), String> = inputs.read("selected", |file| {
        file.seek(SeekFrom::Start(4)).unwrap();
        Err("parser failed".into())
    });
    assert_eq!(failure.unwrap_err(), "parser failed");
    assert_eq!(inputs.read("selected", contents).unwrap(), b"abcdef");
}

#[test]
fn replacing_selected_path_does_not_redirect_the_open_file() {
    let fixture = Fixture::new();
    let path = fixture.file("archive.json", b"original");
    let inputs = SelectedInputs::default();
    inputs.insert("selected".into(), &path).unwrap();
    fs::rename(&path, fixture.0.join("original.json")).unwrap();
    fs::write(&path, b"replacement").unwrap();
    assert_eq!(inputs.read("selected", contents).unwrap(), b"original");
}
