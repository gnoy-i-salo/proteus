use super::*;
use std::path::PathBuf;
fn setup() -> (CorpusDatabase, PathBuf) {
    let directory =
        std::env::temp_dir().join(format!("proteus-database-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&directory).unwrap();
    (
        CorpusDatabase::open(&directory.join("corpus.db"), &[31; 32]).unwrap(),
        directory,
    )
}
fn record(id: &str, source: &str, text: &str) -> RecordInput {
    RecordInput{id:id.into(),kind:"grant".into(),provider:"telegram".into(),state_json:"{\"exactState\":\"  é\\r\\n\"}".into(),documents:vec![RevisionInput{source_key:source.into(),document_id:"same-provider-id".into(),provenance:SourceProvenance::source_service(Acquisition::Api),document_json:serde_json::json!({"text":text,"authorLabel":"  Автор é 😀  ","replyTo":"parent","authorship":"unknown"}).to_string()}]}
}
#[test]
fn typed_record_roundtrip_and_reimport_are_idempotent() {
    let (mut db, dir) = setup();
    let input = record("record-one", "account-a/chat/1", "  exact\r\n😀");
    let first = db.save(&input).unwrap();
    let second = db.save(&input).unwrap();
    assert_eq!(first, second);
    let saved = db.read(&input.id).unwrap().unwrap();
    assert_eq!(saved.input, input);
    assert_eq!(saved.references.len(), 1);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn identical_words_from_different_source_keys_remain_distinct() {
    let (mut db, dir) = setup();
    let a = record("a", "account-a/chat/1", "same");
    let b = record("b", "account-b/chat/1", "same");
    db.save(&a).unwrap();
    db.save(&b).unwrap();
    let a = db.read("a").unwrap().unwrap();
    let b = db.read("b").unwrap().unwrap();
    assert_eq!(a.references.len(), 1);
    assert_eq!(b.references.len(), 1);
    assert_ne!(a.references[0].revision_id, b.references[0].revision_id);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn edits_create_immutable_revisions_and_snapshot_keeps_original_generation() {
    let (mut db, dir) = setup();
    let before = record("a", "account-a/chat/1", "before");
    let first = db.save(&before).unwrap();
    let snapshot = db
        .publish("corpus-a", "job-a", std::slice::from_ref(&first))
        .unwrap();
    let reference = db.read("a").unwrap().unwrap().references.first().cloned();
    assert!(
        reference.is_some(),
        "saved source must have an immutable revision"
    );
    let reference = reference.unwrap();
    let changed = record("a", "account-a/chat/1", "after");
    let second = db.save(&changed).unwrap();
    assert!(second.generation > first.generation);
    assert_eq!(
        db.revision(&reference).unwrap().document_json,
        before.documents[0].document_json
    );
    let frozen = db.snapshot(&snapshot.id).unwrap();
    assert_eq!(frozen.documents, [reference]);
    assert_eq!(frozen.records, [first]);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn publication_rejects_mixed_generations_and_reuses_only_identical_idempotency_key() {
    let (mut db, dir) = setup();
    let old = db.save(&record("a", "a/1", "old")).unwrap();
    let current = db.save(&record("a", "a/1", "new")).unwrap();
    assert!(
        db.publish("corpus", "job", &[old]).is_err(),
        "stale source generation cannot be published"
    );
    let first = db
        .publish("corpus", "job", std::slice::from_ref(&current))
        .unwrap();
    assert_eq!(db.publish("corpus", "job", &[current]).unwrap(), first);
    let another = db.save(&record("b", "b/1", "other")).unwrap();
    assert!(db.publish("corpus", "job", &[another]).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn snapshots_and_provenance_survive_restart() {
    let (mut db, dir) = setup();
    let input = record("a", "a/1", "  before\r\nhttps://example.test/?x_y=1");
    let generation = db.save(&input).unwrap();
    let frozen = db.publish("corpus", "job", &[generation]).unwrap();
    drop(db);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(db.snapshot(&frozen.id).unwrap(), frozen);
    assert_eq!(db.read("a").unwrap().unwrap().input, input);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn deletion_revokes_frozen_snapshots_and_purges_unshared_source_revisions() {
    let (mut db, dir) = setup();
    let generation = db.save(&record("a", "a/1", "private")).unwrap();
    let frozen = db.publish("corpus", "job", &[generation]).unwrap();
    let refs = db.read("a").unwrap().unwrap().references;
    assert_eq!(refs.len(), 1);
    db.remove("a").unwrap();
    assert!(db.snapshot(&frozen.id).is_err());
    assert!(db.revision(&refs[0]).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn migration_verifies_counts_and_hashes_and_commits_audit_atomically() {
    let (mut db, dir) = setup();
    let input = record("a", "account-a/chat/1", "legacy exact\r\n");
    let audit = MigrationAudit {
        source_generation: 3,
        ciphertext_sha256: "ab".repeat(32),
        document_count: 1,
        canonical_sha256: record_hash(&input),
    };
    let mut invalid = audit.clone();
    invalid.document_count = 2;
    assert!(db.migrate(&input, &invalid).is_err());
    assert!(db.read("a").unwrap().is_none());
    db.migrate(&input, &audit).unwrap();
    assert_eq!(db.audit("a").unwrap(), Some(audit.clone()));
    assert_eq!(
        db.migrate(&input, &audit).unwrap(),
        RecordGeneration {
            id: "a".into(),
            generation: 1
        }
    );
    let mut changed = input;
    changed.state_json = "{}".into();
    let mut changed_audit = audit;
    changed_audit.canonical_sha256 = record_hash(&changed);
    assert!(db.migrate(&changed, &changed_audit).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn failed_batch_rolls_back_all_rows_and_generation() {
    let (mut db, dir) = setup();
    let before = record("a", "account-a/chat/1", "before");
    let first = db.save(&before).unwrap();
    let mut invalid = record("a", "account-a/chat/1", "after");
    let mut duplicate = invalid.documents[0].clone();
    duplicate.document_id = "conflicting-document-id".into();
    invalid.documents.push(duplicate);
    assert!(db.save(&invalid).is_err());
    let stored = db.read("a").unwrap().unwrap();
    assert_eq!(stored.generation, first.generation);
    assert_eq!(stored.input, before);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn removed_record_cannot_be_recreated_by_a_stale_writer() {
    let (mut db, dir) = setup();
    let input = record("a", "account-a/chat/1", "original");
    db.save(&input).unwrap();
    db.remove("a").unwrap();
    assert!(db.save(&input).is_err());
    assert_eq!(db.read("a").unwrap().unwrap().input.kind, "deleted");
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn crash_worker() {
    let Ok(path) = std::env::var("PROTEUS_CORPUS_TEST_CRASH_PATH") else {
        return;
    };
    let db = CorpusDatabase::open(Path::new(&path), &[31; 32]).unwrap();
    db.connection
        .execute_batch("PRAGMA cache_size=1; BEGIN IMMEDIATE;")
        .unwrap();
    db.connection
        .execute(
            "UPDATE records SET state_json=?1",
            ["uncommitted plaintext fixture ".repeat(100000)],
        )
        .unwrap();
    std::process::exit(73);
}
#[test]
fn process_crash_before_commit_preserves_complete_published_generation() {
    let (mut db, dir) = setup();
    let input = record("a", "a/1", "committed exact\r\n😀");
    let generation = db.save(&input).unwrap();
    let snapshot = db.publish("corpus", "job", &[generation]).unwrap();
    drop(db);
    let module = module_path!().split_once("::").unwrap().1;
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &format!("{module}::crash_worker"), "--nocapture"])
        .env("PROTEUS_CORPUS_TEST_CRASH_PATH", dir.join("corpus.db"))
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(73));
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(db.read("a").unwrap().unwrap().input, input);
    assert_eq!(db.snapshot(&snapshot.id).unwrap(), snapshot);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn unsupported_database_schema_and_corrupted_canonical_record_fail_closed() {
    let (mut db, dir) = setup();
    db.save(&record("a", "a/1", "exact")).unwrap();
    db.connection
        .execute("UPDATE records SET state_json='{}' WHERE id='a'", [])
        .unwrap();
    assert!(db.read("a").is_err());
    db.connection
        .pragma_update(None, "user_version", 99)
        .unwrap();
    drop(db);
    assert!(CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn revoked_publication_id_never_rebinds_and_generations_do_not_rewind() {
    let (mut db, dir) = setup();
    let first = db.save(&record("a", "a/1", "first")).unwrap();
    let frozen = db.publish("corpus", "job-old", &[first]).unwrap();
    db.remove("a").unwrap();
    let second = db.save(&record("b", "b/1", "new")).unwrap();
    assert!(
        db.publish("corpus", "job-old", std::slice::from_ref(&second))
            .is_err(),
        "revoked snapshot ID cannot be rebound to different sources"
    );
    let next = db.publish("corpus", "job-new", &[second]).unwrap();
    assert!(
        next.generation > frozen.generation,
        "corpus generations cannot rewind after deletion"
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn publication_hash_frames_corpus_and_job_ids_without_delimiter_collisions() {
    let (mut db, dir) = setup();
    let source = db.save(&record("a", "a/1", "text")).unwrap();
    let first = db
        .publish("a\0b", "c", std::slice::from_ref(&source))
        .unwrap();
    let second = db.publish("a", "b\0c", &[source]);
    assert!(
        second.is_ok(),
        "different structured identifiers must not collide"
    );
    assert_ne!(first.id, second.unwrap().id);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn provenance_is_immutable_revision_data_not_a_recovered_guess() {
    let (mut db, dir) = setup();
    let mut input = record("a", "a/1", "original exact é\r\n");
    input.documents[0].provenance = SourceProvenance::legacy(Acquisition::Archive, 2).unwrap();
    db.save(&input)
        .expect("persist explicit legacy provenance without inventing metadata");
    let saved = db.read("a").unwrap().unwrap();
    assert_eq!(saved.input, input);
    let old = saved.references[0].clone();
    let old_revision = db.revision(&old).unwrap();
    assert!(old_revision.provenance.original_object_id.is_none());
    assert!(old_revision.provenance.original_byte_length.is_none());
    assert_eq!(old_revision.provenance.limitations.len(), 4);
    input.documents[0].provenance = SourceProvenance::source_service(Acquisition::Api);
    db.save(&input).unwrap();
    let changed = db.read("a").unwrap().unwrap();
    assert_ne!(old.revision_id, changed.references[0].revision_id);
    assert_eq!(db.revision(&old).unwrap(), old_revision);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_provenance_cannot_enter_canonical_storage() {
    let (mut db, dir) = setup();
    let mut input = record("a", "a/1", "exact");
    input.documents[0].provenance.original_object_id = Some("invented".into());
    assert!(db.save(&input).is_err());
    assert!(db.read("a").unwrap().is_none());
    input.documents[0].provenance = SourceProvenance::legacy(Acquisition::Archive, 0).unwrap();
    input.documents[0].provenance.limitations.pop();
    assert!(db.save(&input).is_err());
    assert!(db.read("a").unwrap().is_none());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn deleting_one_record_preserves_unrelated_revision_history() {
    let (mut db, dir) = setup();
    db.save(&record("a", "a/1", "first version")).unwrap();
    let old = db.read("a").unwrap().unwrap().references[0].clone();
    db.save(&record("a", "a/1", "edited version")).unwrap();
    db.save(&record("b", "b/1", "unrelated source")).unwrap();
    db.remove("b").unwrap();
    assert!(
        db.revision(&old).is_ok(),
        "deleting b must retain the complete history of a"
    );
    db.remove("a").unwrap();
    assert!(
        db.revision(&old).is_err(),
        "deleting a clears its old revisions too"
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
