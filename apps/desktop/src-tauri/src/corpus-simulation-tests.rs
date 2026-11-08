use super::super::CorpusDatabase;
use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

const EXPERIMENT: &str = "experiment-1";
const MANIFEST: &str = "manifest-1";
const TRIAL: &str = "trial-1";
const AT: &str = "2026-11-08T10:15:30.123456789+03:00";

fn open_memory() -> CorpusDatabase {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    connection
        .execute_batch("CREATE TABLE memory_manifests(id TEXT PRIMARY KEY);")
        .unwrap();
    connection
        .execute_batch(include_str!("corpus-simulation-schema.sql"))
        .unwrap();
    CorpusDatabase { connection }
}
fn open_file() -> (CorpusDatabase, PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "proteus-simulation-{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let connection = rusqlite::Connection::open(directory.join("corpus.db")).unwrap();
    connection.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    connection
        .execute_batch("CREATE TABLE memory_manifests(id TEXT PRIMARY KEY);")
        .unwrap();
    connection
        .execute_batch(include_str!("corpus-simulation-schema.sql"))
        .unwrap();
    (CorpusDatabase { connection }, directory)
}
fn seed_manifest(db: &CorpusDatabase, id: &str) {
    db.connection
        .execute(
            "INSERT INTO memory_manifests(id) VALUES(?1)",
            rusqlite::params![id],
        )
        .unwrap();
}
fn draft(
    experiment: &str,
    manifest: &str,
    trial: &str,
    role: SimulationRole,
    text: &str,
) -> SimulationMemoryDraft {
    SimulationMemoryDraft {
        schema_version: 1,
        experiment_id: experiment.into(),
        manifest_id: manifest.into(),
        trial_id: trial.into(),
        role,
        text: text.into(),
    }
}
fn default_draft(text: &str) -> SimulationMemoryDraft {
    draft(
        EXPERIMENT,
        MANIFEST,
        TRIAL,
        SimulationRole::Simulation,
        text,
    )
}
fn query<'a>(
    manifest: &'a str,
    experiment: &'a str,
    trial: &'a str,
    after_sequence: i64,
    limit: usize,
    max_bytes: usize,
) -> SimulationPageQuery<'a> {
    SimulationPageQuery {
        manifest_id: manifest,
        experiment_id: experiment,
        trial_id: trial,
        after_sequence,
        limit,
        max_bytes,
    }
}
fn page(
    db: &CorpusDatabase,
    manifest: &str,
    experiment: &str,
    trial: &str,
    after_sequence: i64,
    limit: usize,
    max_bytes: usize,
) -> SimulationPage {
    db.simulation_page(&query(
        manifest,
        experiment,
        trial,
        after_sequence,
        limit,
        max_bytes,
    ))
    .unwrap()
}
fn scope_size(db: &CorpusDatabase, manifest: &str, experiment: &str, trial: &str) -> (i64, i64) {
    db.connection
        .query_row(
            "SELECT count(*), coalesce(sum(payload_bytes),0) FROM simulation_entries \
             WHERE manifest_id=?1 AND experiment_id=?2 AND trial_id=?3",
            rusqlite::params![manifest, experiment, trial],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}
#[test]
fn appends_and_preserves_exact_text_across_reopen() {
    let (mut db, dir) = open_file();
    seed_manifest(&db, MANIFEST);
    let text = "\u{FEFF}  Пробелы и 😀 сохранены\r\nвторая строка\u{0}NUL";
    let entry = db
        .append_simulation(&default_draft(text), "entry-1", AT)
        .unwrap();
    assert_eq!(entry.schema_version, 1);
    assert_eq!(entry.text, text);
    assert_eq!(entry.created_at, AT);
    assert_eq!(entry.role, SimulationRole::Simulation);
    drop(db);
    let connection = rusqlite::Connection::open(dir.join("corpus.db")).unwrap();
    let db = CorpusDatabase { connection };
    let stored = page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024);
    assert_eq!(stored.items.len(), 1);
    assert_eq!(stored.items[0].sequence, 1);
    assert_eq!(stored.items[0].entry, entry);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn rejects_unknown_draft_schema_version() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    for version in [0u8, 2u8] {
        let mut bad = default_draft("текст");
        bad.schema_version = version;
        assert!(
            db.append_simulation(&bad, "entry-1", AT).is_err(),
            "schema_version {version} must be rejected"
        );
    }
}
#[test]
fn serde_rejects_unknown_role_and_unknown_fields() {
    let bad_role = serde_json::json!({
        "schemaVersion": 1,
        "experimentId": EXPERIMENT,
        "manifestId": MANIFEST,
        "trialId": TRIAL,
        "role": "narrator",
        "text": "текст"
    });
    assert!(serde_json::from_str::<SimulationMemoryDraft>(&bad_role.to_string()).is_err());
    let extra_field = serde_json::json!({
        "schemaVersion": 1,
        "id": "entry-1",
        "experimentId": EXPERIMENT,
        "manifestId": MANIFEST,
        "trialId": TRIAL,
        "role": "user",
        "text": "текст",
        "createdAt": AT,
        "surprise": true
    });
    assert!(serde_json::from_str::<SimulationMemoryEntry>(&extra_field.to_string()).is_err());
    let wire = serde_json::json!({
        "schemaVersion": 1,
        "id": "entry-1",
        "experimentId": EXPERIMENT,
        "manifestId": MANIFEST,
        "trialId": TRIAL,
        "role": "simulation",
        "text": "текст",
        "createdAt": AT
    });
    let parsed: SimulationMemoryEntry = serde_json::from_str(&wire.to_string()).unwrap();
    assert_eq!(parsed.role, SimulationRole::Simulation);
}
#[test]
fn rejects_blank_oversized_and_control_ids() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    let blank = db.append_simulation(&default_draft("текст"), "   ", AT);
    assert!(blank.is_err());
    let oversized = db.append_simulation(&default_draft("текст"), &"x".repeat(257), AT);
    assert!(oversized.is_err());
    let exactly_max = db.append_simulation(&default_draft("текст"), &"x".repeat(256), AT);
    assert!(exactly_max.is_ok(), "exactly 256 bytes is allowed");
    let exactly_max_again = db.append_simulation(&default_draft("текст"), &"y".repeat(256), AT);
    assert!(exactly_max_again.is_ok());
    let control = db.append_simulation(&default_draft("текст"), "a\nb", AT);
    assert!(control.is_err());
    let bad_manifest = draft(EXPERIMENT, " ", TRIAL, SimulationRole::User, "текст");
    assert!(db.append_simulation(&bad_manifest, "entry-2", AT).is_err());
}
#[test]
fn rejects_invalid_host_timestamps_and_retains_valid_exactly() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    for invalid in [
        "yesterday",
        "2026-11-08",
        "2026-13-08T10:00:00Z",
        "2026-11-08T25:00:00Z",
    ] {
        assert!(
            db.append_simulation(&default_draft("текст"), "entry-1", invalid)
                .is_err(),
            "timestamp {invalid:?} must be rejected"
        );
    }
    let exact = "2026-11-08T10:15:30.123456789+03:00";
    let entry = db
        .append_simulation(&default_draft("текст"), "entry-2", exact)
        .unwrap();
    assert_eq!(entry.created_at, exact);
    let stored = page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 10, 1024);
    assert_eq!(stored.items[0].entry.created_at, exact);
}
#[test]
fn rejects_empty_and_oversized_text_without_trimming() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    assert!(db
        .append_simulation(&default_draft(""), "entry-1", AT)
        .is_err());
    let oversized = "a".repeat(64 * 1024 + 1);
    assert!(db
        .append_simulation(&default_draft(&oversized), "entry-2", AT)
        .is_err());
    let exact = "b".repeat(64 * 1024);
    let entry = db
        .append_simulation(&default_draft(&exact), "entry-3", AT)
        .unwrap();
    assert_eq!(entry.text.len(), 64 * 1024);
    let untrimmed = "  ок \r\n";
    let entry = db
        .append_simulation(&default_draft(untrimmed), "entry-4", AT)
        .unwrap();
    assert_eq!(entry.text, untrimmed);
}
#[test]
fn scopes_are_isolated_and_sequences_are_per_scope() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    seed_manifest(&db, "manifest-2");
    let first = db
        .append_simulation(&default_draft("a1"), "a-1", AT)
        .unwrap();
    let second = db
        .append_simulation(&default_draft("a2"), "a-2", AT)
        .unwrap();
    db.append_simulation(
        &draft(EXPERIMENT, MANIFEST, "trial-2", SimulationRole::User, "b1"),
        "b-1",
        AT,
    )
    .unwrap();
    db.append_simulation(
        &draft(
            EXPERIMENT,
            "manifest-2",
            TRIAL,
            SimulationRole::Simulation,
            "c1",
        ),
        "c-1",
        AT,
    )
    .unwrap();
    db.append_simulation(
        &draft(
            "experiment-2",
            MANIFEST,
            TRIAL,
            SimulationRole::Simulation,
            "d1",
        ),
        "d-1",
        AT,
    )
    .unwrap();
    let trial_a = page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024);
    assert_eq!(
        trial_a
            .items
            .iter()
            .map(|item| (item.sequence, item.entry.id.clone()))
            .collect::<Vec<_>>(),
        vec![(1, first.id.clone()), (2, second.id.clone())]
    );
    assert_eq!(
        page(&db, MANIFEST, EXPERIMENT, "trial-2", 0, 100, 1024)
            .items
            .len(),
        1
    );
    assert_eq!(
        page(&db, "manifest-2", EXPERIMENT, TRIAL, 0, 100, 1024)
            .items
            .len(),
        1
    );
    assert_eq!(
        page(&db, MANIFEST, "experiment-2", TRIAL, 0, 100, 1024)
            .items
            .len(),
        1
    );
    let wrong_scope = page(&db, "manifest-2", "experiment-2", TRIAL, 0, 100, 1024);
    assert!(wrong_scope.items.is_empty());
}
#[test]
fn pagination_is_deterministic_with_cursor_and_byte_budget() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    for index in 1..=5usize {
        db.append_simulation(
            &default_draft(&format!("сообщение-{index}")),
            &format!("msg-{index}"),
            AT,
        )
        .unwrap();
    }
    let page1 = page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 2, 1024 * 1024);
    assert_eq!(
        page1
            .items
            .iter()
            .map(|item| item.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(page1.next_after_sequence, Some(2));
    let page2 = page(
        &db,
        MANIFEST,
        EXPERIMENT,
        TRIAL,
        page1.next_after_sequence.unwrap(),
        2,
        1024 * 1024,
    );
    assert_eq!(
        page2
            .items
            .iter()
            .map(|item| item.sequence)
            .collect::<Vec<_>>(),
        vec![3, 4]
    );
    let page3 = page(
        &db,
        MANIFEST,
        EXPERIMENT,
        TRIAL,
        page2.next_after_sequence.unwrap(),
        2,
        1024 * 1024,
    );
    assert_eq!(page3.items.len(), 1);
    assert_eq!(page3.next_after_sequence, None);
    let repeated = page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 2, 1024 * 1024);
    assert_eq!(repeated, page1);
    let first = &page1.items[0];
    let first_bytes = serde_json::to_string(&first.entry).unwrap().len();
    let byte_capped = page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 100, first_bytes);
    assert_eq!(byte_capped.items.len(), 1);
    assert_eq!(
        byte_capped.next_after_sequence,
        Some(1),
        "byte-budget cutoff must keep a cursor pointing at the last returned row"
    );
    let mut bytes = 0usize;
    for item in &byte_capped.items {
        bytes += serde_json::to_string(&item.entry).unwrap().len();
    }
    assert!(bytes <= first_bytes);
}
#[test]
fn first_item_not_fitting_byte_budget_fails_explicitly() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    db.append_simulation(&default_draft(&"x".repeat(3000)), "big-1", AT)
        .unwrap();
    let result = db.simulation_page(&query(MANIFEST, EXPERIMENT, TRIAL, 0, 100, 100));
    assert!(
        result.is_err(),
        "oversized first item must fail, not loop empty"
    );
}
#[test]
fn identical_retry_is_idempotent_and_consumes_no_capacity() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    let draft = default_draft("одинаковый текст");
    let first = db.append_simulation(&draft, "entry-1", AT).unwrap();
    let retry = db.append_simulation(&draft, "entry-1", AT).unwrap();
    assert_eq!(first, retry);
    let stored = page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024);
    assert_eq!(stored.items.len(), 1);
    assert_eq!(stored.items[0].sequence, 1);
    assert_eq!(
        scope_size(&db, MANIFEST, EXPERIMENT, TRIAL),
        (1, stored_payload_size(&db))
    );
}
fn stored_payload_size(db: &CorpusDatabase) -> i64 {
    db.connection
        .query_row(
            "SELECT sum(payload_bytes) FROM simulation_entries",
            [],
            |row| row.get(0),
        )
        .unwrap()
}
#[test]
fn conflicting_id_reuse_fails_without_overwrite() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    let original = db
        .append_simulation(&default_draft("оригинал"), "entry-1", AT)
        .unwrap();
    let other_text = default_draft("другой текст");
    assert!(db.append_simulation(&other_text, "entry-1", AT).is_err());
    let other_time = default_draft("оригинал");
    assert!(db
        .append_simulation(&other_time, "entry-1", "2026-11-08T11:00:00Z")
        .is_err());
    let other_scope = draft(
        EXPERIMENT,
        MANIFEST,
        "trial-2",
        SimulationRole::Simulation,
        "оригинал",
    );
    assert!(db.append_simulation(&other_scope, "entry-1", AT).is_err());
    let stored = page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024);
    assert_eq!(stored.items.len(), 1);
    assert_eq!(stored.items[0].entry, original);
    let other_trial = page(&db, MANIFEST, EXPERIMENT, "trial-2", 0, 100, 1024);
    assert!(
        other_trial.items.is_empty(),
        "conflict must not create rows"
    );
}
#[test]
fn entry_count_limit_is_enforced_per_scope_and_retry_still_fits() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    let small = default_draft("x");
    for index in 0..1000 {
        db.append_simulation(&small, &format!("entry-{index}"), AT)
            .unwrap_or_else(|error| panic!("append {index} must fit: {error}"));
    }
    let full = db.append_simulation(&small, "entry-1000", AT);
    assert!(full.is_err(), "1001st entry in one scope must be rejected");
    assert_eq!(scope_size(&db, MANIFEST, EXPERIMENT, TRIAL).0, 1000);
    let retry = db.append_simulation(&small, "entry-0", AT);
    assert!(retry.is_ok(), "idempotent replay consumes no capacity");
    db.append_simulation(
        &draft(
            EXPERIMENT,
            MANIFEST,
            "trial-2",
            SimulationRole::User,
            "другая область",
        ),
        "other-1",
        AT,
    )
    .unwrap();
}
#[test]
fn scope_byte_limit_is_enforced_and_failed_attempt_consumes_nothing() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    let text = "y".repeat(64 * 1024);
    let draft = default_draft(&text);
    let mut accepted = 0usize;
    for index in 0..1000 {
        match db.append_simulation(&draft, &format!("chunk-{index}"), AT) {
            Ok(_) => accepted += 1,
            Err(_) => break,
        }
    }
    assert!(
        accepted > 0 && accepted < 1000,
        "byte limit must trigger before entry limit: {accepted}"
    );
    let (count, bytes) = scope_size(&db, MANIFEST, EXPERIMENT, TRIAL);
    assert_eq!(count as usize, accepted);
    assert!(bytes as usize <= 16 * 1024 * 1024);
    let next = db.append_simulation(&draft, &format!("chunk-{accepted}"), AT);
    assert!(next.is_err(), "capacity is unchanged after the limit error");
    assert_eq!(
        scope_size(&db, MANIFEST, EXPERIMENT, TRIAL).0 as usize,
        accepted
    );
}
#[test]
fn failed_check_rolls_back_append_entirely() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    let calls = std::cell::Cell::new(0usize);
    let failing = || {
        calls.set(calls.get() + 1);
        if calls.get() >= 3 {
            return Err("отменено".to_string());
        }
        Ok(())
    };
    let result = db.append_simulation_checked(&default_draft("текст"), "entry-1", AT, &failing);
    assert_eq!(result.unwrap_err(), "отменено");
    let stored = page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024);
    assert!(
        stored.items.is_empty(),
        "rolled-back append must leave no rows"
    );
    let recovered = db
        .append_simulation_checked(&default_draft("после отмены"), "entry-2", AT, &|| Ok(()))
        .unwrap();
    let stored = page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024);
    assert_eq!(stored.items.len(), 1);
    assert_eq!(
        stored.items[0].sequence, 1,
        "no sequence gap after rollback"
    );
    assert_eq!(stored.items[0].entry, recovered);
}
#[test]
fn failing_check_during_read_exposes_nothing() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    for index in 0..3 {
        db.append_simulation(
            &default_draft(&format!("текст-{index}")),
            &format!("e-{index}"),
            AT,
        )
        .unwrap();
    }
    let calls = std::cell::Cell::new(0usize);
    let failing = || {
        calls.set(calls.get() + 1);
        if calls.get() >= 2 {
            return Err("доступ отозван".to_string());
        }
        Ok(())
    };
    let result = db.simulation_page_checked(
        &query(MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024),
        &failing,
    );
    assert_eq!(result.unwrap_err(), "доступ отозван");
    let stored = page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024);
    assert_eq!(
        stored.items.len(),
        3,
        "read cancellation must not corrupt rows"
    );
}
#[test]
fn tampered_rows_fail_closed_on_read() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    let tamper = |db: &CorpusDatabase, trial: &str, sql: &str| {
        db.connection
            .execute(sql, rusqlite::params![trial])
            .unwrap();
    };
    let cases: Vec<(&str, &str)> = vec![
        ("payload", "UPDATE simulation_entries SET data_json='{\"schemaVersion\":1,\"id\":\"t-1\",\"experimentId\":\"experiment-1\",\"manifestId\":\"manifest-1\",\"trialId\":\"x\",\"role\":\"user\",\"text\":\"подделка\",\"createdAt\":\"2026-11-08T10:15:30Z\"}' WHERE trial_id=?1"),
        ("hash", "UPDATE simulation_entries SET canonical_sha256='00' WHERE trial_id=?1"),
        ("indexed-scope", "UPDATE simulation_entries SET trial_id='forged' WHERE trial_id=?1"),
        ("indexed-bytes", "UPDATE simulation_entries SET payload_bytes=1 WHERE trial_id=?1"),
    ];
    for (name, sql) in cases {
        let trial = format!("tamper-{name}");
        db.append_simulation(
            &draft(
                EXPERIMENT,
                MANIFEST,
                &trial,
                SimulationRole::User,
                "оригинал",
            ),
            &format!("t-{name}"),
            AT,
        )
        .unwrap();
        tamper(&db, &trial, sql);
        let read_scope = if name == "indexed-scope" {
            "forged"
        } else {
            &trial
        };
        let result = db.simulation_page(&query(
            MANIFEST,
            EXPERIMENT,
            read_scope,
            0,
            100,
            1024 * 1024,
        ));
        assert!(result.is_err(), "tampered {name} row must fail closed");
    }
    let trial = "tamper-sequence";
    db.append_simulation(
        &draft(EXPERIMENT, MANIFEST, trial, SimulationRole::User, "первый"),
        "s-1",
        AT,
    )
    .unwrap();
    db.append_simulation(
        &draft(EXPERIMENT, MANIFEST, trial, SimulationRole::User, "второй"),
        "s-2",
        AT,
    )
    .unwrap();
    db.connection
        .execute(
            "UPDATE simulation_entries SET sequence=9 WHERE trial_id=?1 AND sequence=2",
            rusqlite::params![trial],
        )
        .unwrap();
    assert!(db
        .simulation_page(&query(MANIFEST, EXPERIMENT, trial, 0, 100, 1024 * 1024))
        .is_err());
}
#[test]
fn manifest_deletion_cascades_only_its_simulation_entries() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    seed_manifest(&db, "manifest-2");
    db.append_simulation(&default_draft("m1-a"), "m1-a", AT)
        .unwrap();
    db.append_simulation(&default_draft("m1-b"), "m1-b", AT)
        .unwrap();
    db.append_simulation(
        &draft(
            EXPERIMENT,
            "manifest-2",
            TRIAL,
            SimulationRole::Simulation,
            "m2",
        ),
        "m2-a",
        AT,
    )
    .unwrap();
    db.connection
        .execute(
            "DELETE FROM memory_manifests WHERE id=?1",
            rusqlite::params![MANIFEST],
        )
        .unwrap();
    assert!(page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024)
        .items
        .is_empty());
    let untouched = page(&db, "manifest-2", EXPERIMENT, TRIAL, 0, 100, 1024 * 1024);
    assert_eq!(untouched.items.len(), 1);
    assert_eq!(untouched.items[0].entry.text, "m2");
}
#[test]
fn generated_records_never_enter_original_namespace() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    db.connection
        .execute_batch("CREATE TABLE source_documents(id TEXT PRIMARY KEY, text TEXT NOT NULL);")
        .unwrap();
    db.connection
        .execute(
            "INSERT INTO source_documents(id, text) VALUES(?1, ?2)",
            rusqlite::params!["original-1", "исходный текст"],
        )
        .unwrap();
    let generated = default_draft("сгенерировано");
    db.append_simulation(&generated, "gen-1", AT).unwrap();
    db.append_simulation(&generated, "gen-1", AT).unwrap();
    assert!(db
        .append_simulation(&default_draft("иное"), "gen-1", AT)
        .is_err());
    page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024);
    let originals: i64 = db
        .connection
        .query_row("SELECT count(*) FROM source_documents", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(originals, 1, "original namespace stays untouched");
    let original_text: String = db
        .connection
        .query_row(
            "SELECT text FROM source_documents WHERE id='original-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(original_text, "исходный текст");
}
#[test]
fn missing_manifest_append_fails_and_read_returns_empty() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    let ghost = draft(
        EXPERIMENT,
        "ghost-manifest",
        TRIAL,
        SimulationRole::User,
        "текст",
    );
    assert!(db.append_simulation(&ghost, "entry-1", AT).is_err());
    let ghost_page = page(
        &db,
        "ghost-manifest",
        EXPERIMENT,
        TRIAL,
        0,
        100,
        1024 * 1024,
    );
    assert!(ghost_page.items.is_empty());
    assert_eq!(ghost_page.next_after_sequence, None);
}
#[test]
fn read_parameters_are_bounded() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    db.append_simulation(&default_draft("текст"), "entry-1", AT)
        .unwrap();
    for (after, limit, max_bytes) in [
        (-1i64, 100usize, 1024usize),
        (0, 0, 1024),
        (0, 101, 1024),
        (0, 100, 0),
        (0, 100, 1024 * 1024 + 1),
    ] {
        assert!(
            db.simulation_page(&query(MANIFEST, EXPERIMENT, TRIAL, after, limit, max_bytes))
                .is_err(),
            "after={after} limit={limit} max_bytes={max_bytes} must be rejected"
        );
    }
    assert!(db
        .simulation_page(&query(MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024))
        .is_ok());
}

// ---- Root review follow-up regressions (byte-capped cursor, replay checks,
// timestamp bound, stored-JSON bound, manifest snapshot check, capacity counters).

#[test]
fn pagination_cursor_survives_byte_budget_cutoff() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    // Equal-length ids and identical scope/role/timestamp keep the serialized
    // overhead constant, so payloads differ only by text length: 500/400/300.
    db.append_simulation(&default_draft(&"a".repeat(500)), "w-1", AT)
        .unwrap();
    db.append_simulation(&default_draft(&"b".repeat(400)), "w-2", AT)
        .unwrap();
    db.append_simulation(&default_draft(&"c".repeat(300)), "w-3", AT)
        .unwrap();
    let budget = serde_json::to_string(
        &page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024).items[0].entry,
    )
    .unwrap()
    .len();
    let page1 = page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 100, budget);
    assert_eq!(
        page1.items.len(),
        1,
        "only the first row fits the byte budget"
    );
    assert_eq!(
        page1.next_after_sequence,
        Some(1),
        "byte cutoff must keep a cursor at the last returned row"
    );
    let page2 = page(&db, MANIFEST, EXPERIMENT, TRIAL, 1, 100, budget);
    assert_eq!(page2.items[0].sequence, 2);
    assert_eq!(page2.next_after_sequence, Some(2));
    let page3 = page(&db, MANIFEST, EXPERIMENT, TRIAL, 2, 100, budget);
    assert_eq!(page3.items[0].sequence, 3);
    assert_eq!(page3.next_after_sequence, None, "scope end reached");
    let mut after = 0i64;
    let mut seen = Vec::new();
    loop {
        let chunk = page(&db, MANIFEST, EXPERIMENT, TRIAL, after, 100, budget);
        seen.extend(chunk.items.iter().map(|item| item.sequence));
        match chunk.next_after_sequence {
            Some(next) => after = next,
            None => break,
        }
    }
    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(seen, vec![1, 2, 3], "cursor walk returns all entries");
    assert_eq!(unique.len(), seen.len(), "no duplicates across pages");
}

#[test]
fn replay_runs_final_check_before_exposing_existing_text() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    let draft = default_draft("текст для повтора");
    db.append_simulation(&draft, "entry-1", AT).unwrap();
    let calls = std::cell::Cell::new(0usize);
    let fail_at_final_check = || -> Result<(), String> {
        calls.set(calls.get() + 1);
        if calls.get() == 4 {
            return Err("отменено на финальной проверке".to_string());
        }
        Ok(())
    };
    let result = db.append_simulation_checked(&draft, "entry-1", AT, &fail_at_final_check);
    assert_eq!(
        result.unwrap_err(),
        "отменено на финальной проверке",
        "replay must fail when the final pre-return check fails"
    );
    let calls = std::cell::Cell::new(0usize);
    let counting = || -> Result<(), String> {
        calls.set(calls.get() + 1);
        Ok(())
    };
    let replayed = db
        .append_simulation_checked(&draft, "entry-1", AT, &counting)
        .unwrap();
    assert_eq!(
        calls.get(),
        4,
        "replay checks: pre-validation, in-transaction, before decode, final before return"
    );
    assert_eq!(replayed.text, "текст для повтора");
    let stored = page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024);
    assert_eq!(stored.items.len(), 1, "failed replay leaves one intact row");
    assert_eq!(stored.items[0].sequence, 1);
}

#[test]
fn missing_manifest_still_checks_before_returning_an_empty_page() {
    let db = open_memory();
    let calls = std::cell::Cell::new(0);
    let check = || {
        calls.set(calls.get() + 1);
        if calls.get() == 2 {
            Err("cancel empty read".into())
        } else {
            Ok(())
        }
    };
    let result =
        db.simulation_page_checked(&query(MANIFEST, EXPERIMENT, TRIAL, 0, 10, 1024), &check);
    assert_eq!(result.unwrap_err(), "cancel empty read");
}

#[test]
fn replay_checks_octet_limit_before_attempting_string_conversion() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    let draft = default_draft("original");
    db.append_simulation(&draft, "entry-1", AT).unwrap();
    // A corrupt oversized BLOB cannot be converted to String. Size rejection
    // must win first, proving the replay row does not eagerly read data_json.
    db.connection
        .execute(
            "UPDATE simulation_entries SET data_json=?1,payload_bytes=?2",
            params![vec![255u8; 600 * 1024], 600 * 1024],
        )
        .unwrap();
    assert_eq!(
        db.append_simulation(&draft, "entry-1", AT).unwrap_err(),
        INVALID
    );
}

#[test]
fn replay_rejects_tampered_payload_bytes() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    let draft = default_draft("неприкосновенный текст");
    let original = db.append_simulation(&draft, "entry-1", AT).unwrap();
    db.connection
        .execute(
            "UPDATE simulation_entries SET payload_bytes = payload_bytes - 1 WHERE id='entry-1'",
            [],
        )
        .unwrap();
    assert!(
        db.append_simulation(&draft, "entry-1", AT).is_err(),
        "undersized payload_bytes must fail the idempotent replay"
    );
    db.connection
        .execute(
            "UPDATE simulation_entries SET payload_bytes = payload_bytes + 2 WHERE id='entry-1'",
            [],
        )
        .unwrap();
    assert!(
        db.append_simulation(&draft, "entry-1", AT).is_err(),
        "oversized payload_bytes must fail the idempotent replay"
    );
    db.connection
        .execute(
            "UPDATE simulation_entries SET payload_bytes = length(CAST(data_json AS BLOB)) \
             WHERE id='entry-1'",
            [],
        )
        .unwrap();
    let stored = page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024);
    assert_eq!(stored.items.len(), 1);
    assert_eq!(stored.items[0].entry, original, "row content is untouched");
}

#[test]
fn rejects_oversized_timestamp_text_before_parsing() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    // chrono accepts arbitrary fractional precision, so without an explicit
    // byte bound a multi-kilobyte "timestamp" parses as valid RFC3339.
    let huge = format!("2026-11-08T10:15:30.{}Z", "1".repeat(4096));
    assert!(
        db.append_simulation(&default_draft("текст"), "entry-1", &huge)
            .is_err(),
        "timestamp text beyond 64 bytes must be rejected before parsing"
    );
    let boundary = format!("2026-11-08T10:15:30.{}+03:00", "1".repeat(38));
    assert_eq!(boundary.len(), 64);
    let entry = db
        .append_simulation(&default_draft("текст"), "entry-2", &boundary)
        .unwrap();
    assert_eq!(
        entry.created_at, boundary,
        "exact valid RFC3339 text is retained verbatim"
    );
    let over = format!("2026-11-08T10:15:30.{}+03:00", "1".repeat(39));
    assert_eq!(over.len(), 65);
    assert!(
        db.append_simulation(&default_draft("текст"), "entry-3", &over)
            .is_err(),
        "65-byte timestamp must be rejected at the byte bound"
    );
    let stored_json: String = db
        .connection
        .query_row(
            "SELECT data_json FROM simulation_entries WHERE id='entry-2'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let tampered = stored_json.replace(&boundary, &huge);
    let hash = format!("{:x}", Sha256::digest(tampered.as_bytes()));
    db.connection
        .execute(
            "UPDATE simulation_entries SET data_json=?1, canonical_sha256=?2, payload_bytes=?3 \
             WHERE id='entry-2'",
            rusqlite::params![tampered, hash, tampered.len() as i64],
        )
        .unwrap();
    assert!(
        db.simulation_page(&query(MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024))
            .is_err(),
        "oversized stored timestamp must fail closed on read"
    );
}

#[test]
fn padded_oversized_payload_is_rejected_before_materializing() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    let draft = default_draft("запись");
    db.append_simulation(&draft, "entry-1", AT).unwrap();
    let stored_json: String = db
        .connection
        .query_row(
            "SELECT data_json FROM simulation_entries WHERE id='entry-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    // serde_json tolerates trailing whitespace, so this row would otherwise
    // decode successfully; only the stored-JSON octet bound catches it first.
    let padded = format!("{stored_json}{}", " ".repeat(600 * 1024));
    assert!(padded.len() > 512 * 1024);
    let hash = format!("{:x}", Sha256::digest(padded.as_bytes()));
    db.connection
        .execute(
            "UPDATE simulation_entries SET data_json=?1, canonical_sha256=?2, payload_bytes=?3 \
             WHERE id='entry-1'",
            rusqlite::params![padded, hash, padded.len() as i64],
        )
        .unwrap();
    assert!(
        db.simulation_page(&query(MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024))
            .is_err(),
        "read must reject a payload beyond the stored-JSON bound"
    );
    assert!(
        db.append_simulation(&draft, "entry-1", AT).is_err(),
        "replay must reject a payload beyond the stored-JSON bound"
    );
}

#[test]
fn maximal_control_character_text_roundtrips_within_json_bound() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    // Worst legitimate payload: 64KiB of control characters JSON-escapes to
    // ~6x (~384KiB) and must stay well inside the 512KiB stored-JSON bound.
    let text = "\u{0001}".repeat(64 * 1024);
    assert_eq!(text.len(), 64 * 1024);
    let entry = db
        .append_simulation(&default_draft(&text), "entry-1", AT)
        .unwrap();
    let stored = page(&db, MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024);
    assert_eq!(stored.items.len(), 1);
    assert_eq!(stored.items[0].entry, entry);
    assert_eq!(stored.items[0].entry.text, text);
    let payload: i64 = db
        .connection
        .query_row(
            "SELECT payload_bytes FROM simulation_entries WHERE id='entry-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        payload as usize <= 512 * 1024,
        "legitimate worst-case payload {payload} bytes must fit the bound"
    );
}

#[test]
fn orphaned_entries_stay_hidden_when_manifest_is_missing() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    db.append_simulation(&default_draft("сирота без манифеста"), "orphan-1", AT)
        .unwrap();
    db.connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    db.connection
        .execute(
            "DELETE FROM memory_manifests WHERE id=?1",
            rusqlite::params![MANIFEST],
        )
        .unwrap();
    db.connection
        .execute_batch("PRAGMA foreign_keys=ON;")
        .unwrap();
    let physical: i64 = db
        .connection
        .query_row("SELECT count(*) FROM simulation_entries", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(physical, 1, "orphan row remains physically stored");
    let ghost = db
        .simulation_page(&query(MANIFEST, EXPERIMENT, TRIAL, 0, 100, 1024 * 1024))
        .unwrap();
    assert!(
        ghost.items.is_empty(),
        "orphaned text must not be exposed when the manifest is gone"
    );
    assert_eq!(ghost.next_after_sequence, None);
}

#[test]
fn tampered_scope_counters_reject_new_appends() {
    let mut db = open_memory();
    seed_manifest(&db, MANIFEST);
    for index in 1..=3 {
        db.append_simulation(
            &default_draft(&format!("запись-{index}")),
            &format!("t-{index}"),
            AT,
        )
        .unwrap();
    }
    db.connection
        .execute("UPDATE simulation_entries SET payload_bytes = 1", [])
        .unwrap();
    assert!(
        db.append_simulation(&default_draft("ещё одна"), "t-4", AT)
            .is_err(),
        "undersized counters must reject the append"
    );
    db.connection
        .execute("UPDATE simulation_entries SET payload_bytes = 100000", [])
        .unwrap();
    assert!(
        db.append_simulation(&default_draft("ещё одна"), "t-4", AT)
            .is_err(),
        "oversized counters must reject the append"
    );
    assert_eq!(
        scope_size(&db, MANIFEST, EXPERIMENT, TRIAL).0,
        3,
        "rejected appends leave the scope untouched"
    );
    db.connection
        .execute(
            "UPDATE simulation_entries SET payload_bytes = length(CAST(data_json AS BLOB))",
            [],
        )
        .unwrap();
    db.append_simulation(&default_draft("ещё одна"), "t-4", AT)
        .unwrap();
    assert_eq!(scope_size(&db, MANIFEST, EXPERIMENT, TRIAL).0, 4);
}
