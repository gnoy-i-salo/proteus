//! Storage-boundary tests for bounded research snapshot storage: durable
//! `MemorySnapshot`/`MemoryItem` bundles pinned to an immutable corpus
//! snapshot. These tests exercise structural persistence identity only; the
//! root publication scenario owns original-text, authorship, holdout, lease
//! and permission validation. Corruption fixtures never mirror the
//! implementation: they tamper the real schema rows (dropped immutability
//! triggers, `PRAGMA ignore_check_constraints`) or seed rows that satisfy
//! every database constraint while violating a storage budget.
use super::*;
use crate::corpus_database::episodes::{ChunkSpan, SpanUnit};
use crate::corpus_database::{
    Acquisition, CorpusDatabase, RecordGeneration, RecordInput, RevisionInput, RevisionReference,
    SourceProvenance,
};
use crate::memory::types::{MemoryItemKind, MemoryItemSupport};
use rusqlite::Connection;
use sha2::Digest;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const PERSONA: &str = "persona-a";
const OTHER_PERSONA: &str = "persona-b";
const TS: &str = "2026-11-08T20:00:00Z";

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

fn temp_dir() -> PathBuf {
    let id = NEXT_DIR.fetch_add(1, Ordering::SeqCst);
    let directory =
        std::env::temp_dir().join(format!("proteus-memory-store-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

fn setup() -> (CorpusDatabase, PathBuf) {
    let directory = temp_dir();
    (
        CorpusDatabase::open(&directory.join("corpus.db"), &[31; 32]).unwrap(),
        directory,
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

/// Seeds one source record with the given documents and publishes a corpus
/// snapshot over it. Returns the corpus snapshot id and the immutable
/// revision references memory items may point their source spans at.
fn seed_source(
    db: &mut CorpusDatabase,
    corpus: &str,
    documents: &[(&str, &str)],
) -> (String, Vec<RevisionReference>) {
    let record: RecordGeneration = db
        .save(&RecordInput {
            id: format!("record-{corpus}"),
            kind: "grant".into(),
            provider: "telegram".into(),
            state_json: "{}".into(),
            documents: documents
                .iter()
                .map(|(document_id, text)| RevisionInput {
                    source_key: format!("account/chat/{document_id}"),
                    document_id: (*document_id).into(),
                    document_json: serde_json::json!({ "text": text }).to_string(),
                    provenance: SourceProvenance::source_service(Acquisition::Api),
                })
                .collect(),
        })
        .unwrap();
    let snapshot = db
        .publish(
            corpus,
            &format!("job-{corpus}"),
            std::slice::from_ref(&record),
        )
        .unwrap();
    let references = snapshot.documents.clone();
    (snapshot.id, references)
}

fn span(reference: &RevisionReference, start: usize, end: usize) -> ChunkSpan {
    ChunkSpan {
        document_id: reference.document_id.clone(),
        revision_id: reference.revision_id.clone(),
        unit: SpanUnit::Utf8Bytes,
        start,
        end,
    }
}

fn item_for(
    id: &str,
    corpus_snapshot_id: &str,
    reference: &RevisionReference,
    text: &str,
) -> MemoryItem {
    MemoryItem {
        schema_version: 1,
        id: id.into(),
        snapshot_id: corpus_snapshot_id.into(),
        kind: MemoryItemKind::Episode,
        text: text.into(),
        source_spans: vec![span(reference, 0, text.len().max(1))],
        episode_ids: vec![format!("episode-{id}")],
        event_at: None,
        observed_at: TS.into(),
        support: MemoryItemSupport::Observed,
        contradiction_ids: vec![],
        limitations: vec!["Один источник.".into()],
        extraction_version: "memory-extraction-v1".into(),
        policy_version: "policy-v1".into(),
    }
}

fn snapshot_for(id: &str, corpus_snapshot_id: &str, item_ids: Vec<String>) -> MemorySnapshot {
    MemorySnapshot {
        schema_version: 1,
        id: id.into(),
        corpus_snapshot_id: corpus_snapshot_id.into(),
        item_ids,
        extraction_version: "memory-extraction-v1".into(),
        created_at: TS.into(),
    }
}

/// One item bundle over a freshly seeded source document.
fn single_bundle(
    db: &mut CorpusDatabase,
    corpus: &str,
) -> (String, RevisionReference, MemorySnapshot, Vec<MemoryItem>) {
    let (corpus_snapshot, references) =
        seed_source(db, corpus, &[("doc-1", "  exact source words\r\n😀")]);
    let reference = references[0].clone();
    let items = vec![item_for(
        "item-1",
        &corpus_snapshot,
        &reference,
        "\u{FEFF}первая строка\r\nвторая   😀",
    )];
    let snapshot = snapshot_for("memory-snapshot-1", &corpus_snapshot, vec!["item-1".into()]);
    (corpus_snapshot, reference, snapshot, items)
}

fn publish(
    db: &mut CorpusDatabase,
    snapshot: &MemorySnapshot,
    items: &[MemoryItem],
) -> Result<MemoryBundle, String> {
    db.publish_memory_snapshot_checked(PERSONA, snapshot, items, &|| Ok(()))
}

fn read(db: &CorpusDatabase, snapshot_id: &str) -> Result<MemoryBundle, String> {
    db.read_memory_snapshot_checked(snapshot_id, PERSONA, &|| Ok(()))
}

fn memory_row_count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn assert_no_memory_rows(connection: &Connection) {
    for table in ["memory_snapshots", "memory_snapshot_items", "memory_items"] {
        assert_eq!(
            memory_row_count(connection, table),
            0,
            "{table} must stay empty"
        );
    }
}

#[test]
fn publish_then_read_roundtrips_exact_bundle() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    let published = publish(&mut db, &snapshot, &items).unwrap();
    assert_eq!(published.snapshot, snapshot);
    assert_eq!(published.items, items);
    let loaded = read(&db, "memory-snapshot-1").unwrap();
    assert_eq!(loaded, published);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unicode_crlf_and_source_metadata_are_retained_byte_for_byte() {
    let (mut db, dir) = setup();
    let text = "\u{FEFF}  Автор 😀 говорит:\r\n\"точные слова\"   \r\n";
    let (corpus_snapshot, references) = seed_source(&mut db, "corpus-a", &[("doc-привет", text)]);
    let item = MemoryItem {
        text: text.into(),
        source_spans: vec![
            span(&references[0], 0, text.len()),
            span(&references[0], 2, text.len() - 3),
        ],
        episode_ids: vec!["  episode с пробелами  ".into()],
        event_at: Some("2026-11-01T09:30:00+03:00".into()),
        limitations: vec![
            "Ограничение с эмодзи 😀.".into(),
            "\r\nCRLF limitation".into(),
        ],
        ..item_for("item-1", &corpus_snapshot, &references[0], "x")
    };
    let snapshot = snapshot_for("memory-snapshot-1", &corpus_snapshot, vec!["item-1".into()]);
    publish(&mut db, &snapshot, std::slice::from_ref(&item)).unwrap();
    drop(db);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    let loaded = read(&db, "memory-snapshot-1").unwrap();
    assert_eq!(loaded.snapshot, snapshot);
    assert_eq!(loaded.items, vec![item]);
    assert_eq!(loaded.items[0].text, text);
    assert_eq!(loaded.items[0].source_spans[0].document_id, "doc-привет");
    assert_eq!(
        loaded.items[0].source_spans[0].revision_id,
        references[0].revision_id
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn replay_with_identical_bundle_is_idempotent() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    let first = publish(&mut db, &snapshot, &items).unwrap();
    let second = publish(&mut db, &snapshot, &items).unwrap();
    assert_eq!(first, second);
    assert_eq!(memory_row_count(&db.connection, "memory_snapshots"), 1);
    assert_eq!(memory_row_count(&db.connection, "memory_items"), 1);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn replay_with_changed_item_fails_and_keeps_original() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &snapshot, &items).unwrap();
    let mut changed = items[0].clone();
    changed.text = "Изменённый текст.".into();
    let error = publish(&mut db, &snapshot, std::slice::from_ref(&changed))
        .expect_err("conflicting replay must fail");
    assert_eq!(error, "Снимок памяти уже опубликован с другими данными.");
    assert_eq!(read(&db, "memory-snapshot-1").unwrap().items, items);
    assert_eq!(memory_row_count(&db.connection, "memory_items"), 1);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn replay_with_changed_owner_fails() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &snapshot, &items).unwrap();
    let error = db
        .publish_memory_snapshot_checked(OTHER_PERSONA, &snapshot, &items, &|| Ok(()))
        .expect_err("replay under another persona must fail");
    assert_eq!(error, "Снимок памяти принадлежит другой области персоны.");
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn publish_requires_existing_verified_source_snapshot() {
    let (mut db, dir) = setup();
    let snapshot = snapshot_for("memory-snapshot-1", "snapshot:missing", vec![]);
    let error = publish(&mut db, &snapshot, &[]).expect_err("missing source");
    assert_eq!(error, "Снимок корпуса не найден или отозван.");
    assert_no_memory_rows(&db.connection);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn empty_snapshot_is_allowed_when_source_snapshot_exists() {
    let (mut db, dir) = setup();
    let (corpus_snapshot, _) = seed_source(&mut db, "corpus-a", &[("doc-1", "text")]);
    let snapshot = snapshot_for("memory-snapshot-1", &corpus_snapshot, vec![]);
    let published = publish(&mut db, &snapshot, &[]).unwrap();
    assert_eq!(published.items, vec![]);
    let loaded = read(&db, "memory-snapshot-1").unwrap();
    assert_eq!(loaded.snapshot, snapshot);
    assert!(loaded.items.is_empty());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn source_span_outside_snapshot_membership_is_rejected() {
    let (mut db, dir) = setup();
    let (corpus_snapshot, references) = seed_source(&mut db, "corpus-a", &[("doc-1", "text one")]);
    let foreign = RevisionReference {
        document_id: "doc-foreign".into(),
        revision_id: "revision-foreign".into(),
    };
    let mut item = item_for("item-1", &corpus_snapshot, &references[0], "text");
    item.source_spans.push(span(&foreign, 0, 3));
    let snapshot = snapshot_for("memory-snapshot-1", &corpus_snapshot, vec!["item-1".into()]);
    let error =
        publish(&mut db, &snapshot, std::slice::from_ref(&item)).expect_err("foreign source span");
    assert_eq!(error, "Интервал источника элемента вне снимка корпуса.");
    assert_no_memory_rows(&db.connection);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn item_with_wrong_snapshot_binding_is_rejected() {
    let (mut db, dir) = setup();
    let (corpus_snapshot, references) = seed_source(&mut db, "corpus-a", &[("doc-1", "text")]);
    let wrong_binding = item_for("item-1", "snapshot:elsewhere", &references[0], "text");
    let snapshot = snapshot_for("memory-snapshot-1", &corpus_snapshot, vec!["item-1".into()]);
    assert!(publish(&mut db, &snapshot, std::slice::from_ref(&wrong_binding)).is_err());
    let mut wrong_version = item_for("item-2", &corpus_snapshot, &references[0], "text");
    wrong_version.extraction_version = "memory-extraction-v2".into();
    let snapshot = snapshot_for("memory-snapshot-2", &corpus_snapshot, vec!["item-2".into()]);
    assert!(publish(&mut db, &snapshot, std::slice::from_ref(&wrong_version)).is_err());
    assert_no_memory_rows(&db.connection);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn item_ids_must_match_snapshot_list_exactly_in_order() {
    let (mut db, dir) = setup();
    let (corpus_snapshot, reference, _, _) = single_bundle(&mut db, "corpus-a");
    let one = item_for("item-1", &corpus_snapshot, &reference, "one");
    let two = item_for("item-2", &corpus_snapshot, &reference, "two");
    // Reordered itemIds vs items.
    let snapshot = snapshot_for(
        "memory-snapshot-1",
        &corpus_snapshot,
        vec!["item-2".into(), "item-1".into()],
    );
    let error =
        publish(&mut db, &snapshot, &[one.clone(), two.clone()]).expect_err("order mismatch");
    assert_eq!(error, "Состав снимка памяти не соответствует элементам.");
    // itemIds reference an id missing from the batch.
    let snapshot = snapshot_for(
        "memory-snapshot-1",
        &corpus_snapshot,
        vec!["item-ghost".into()],
    );
    let error = publish(&mut db, &snapshot, std::slice::from_ref(&one)).expect_err("ghost id");
    assert_eq!(error, "Состав снимка памяти не соответствует элементам.");
    // Duplicate ids inside the batch.
    let snapshot = snapshot_for(
        "memory-snapshot-1",
        &corpus_snapshot,
        vec!["item-1".into(), "item-1".into()],
    );
    assert!(publish(&mut db, &snapshot, &[one.clone(), one.clone()]).is_err());
    assert_no_memory_rows(&db.connection);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn item_order_is_preserved_in_storage() {
    let (mut db, dir) = setup();
    let (corpus_snapshot, reference, _, _) = single_bundle(&mut db, "corpus-a");
    let items: Vec<MemoryItem> = ["item-a", "item-b", "item-c"]
        .iter()
        .map(|id| item_for(id, &corpus_snapshot, &reference, &format!("text {id}")))
        .collect();
    let snapshot = snapshot_for(
        "memory-snapshot-1",
        &corpus_snapshot,
        items.iter().map(|item| item.id.clone()).collect(),
    );
    publish(&mut db, &snapshot, &items).unwrap();
    let loaded = read(&db, "memory-snapshot-1").unwrap();
    let ids: Vec<&str> = loaded.items.iter().map(|item| item.id.as_str()).collect();
    assert_eq!(ids, ["item-a", "item-b", "item-c"]);
    assert_eq!(loaded.items, items);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn existing_item_id_rejects_different_dto_or_scope() {
    let (mut db, dir) = setup();
    let (corpus_snapshot, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &snapshot, &items).unwrap();
    // Same id, different text, same persona/corpus.
    let mut different = items[0].clone();
    different.text = "Другой текст.".into();
    let snapshot_two = snapshot_for("memory-snapshot-2", &corpus_snapshot, vec!["item-1".into()]);
    let error = publish(&mut db, &snapshot_two, std::slice::from_ref(&different))
        .expect_err("same id with different DTO");
    assert_eq!(error, "Элемент памяти уже сохранён с другими данными.");
    // Same id and DTO, different persona.
    let error = db
        .publish_memory_snapshot_checked(OTHER_PERSONA, &snapshot_two, &items, &|| Ok(()))
        .expect_err("same id under another persona");
    assert_eq!(error, "Элемент памяти уже сохранён с другими данными.");
    assert_eq!(memory_row_count(&db.connection, "memory_items"), 1);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn existing_item_id_reused_with_exact_dto_in_new_snapshot() {
    let (mut db, dir) = setup();
    let (corpus_snapshot, reference, snapshot, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &snapshot, &items).unwrap();
    let second = item_for("item-2", &corpus_snapshot, &reference, "second");
    let snapshot_two = snapshot_for(
        "memory-snapshot-2",
        &corpus_snapshot,
        vec!["item-1".into(), "item-2".into()],
    );
    publish(&mut db, &snapshot_two, &[items[0].clone(), second.clone()]).unwrap();
    assert_eq!(memory_row_count(&db.connection, "memory_items"), 2);
    let first = read(&db, "memory-snapshot-1").unwrap();
    assert_eq!(first.items, items);
    let loaded_two = read(&db, "memory-snapshot-2").unwrap();
    assert_eq!(loaded_two.items, vec![items[0].clone(), second]);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn contradiction_ids_must_reference_batch_or_existing_item() {
    let (mut db, dir) = setup();
    let (corpus_snapshot, reference, snapshot, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &snapshot, &items).unwrap();
    // Ghost contradiction.
    let mut ghost = item_for("item-ghost-holder", &corpus_snapshot, &reference, "ghost");
    ghost.contradiction_ids = vec!["nowhere".into()];
    let snapshot_ghost = snapshot_for(
        "memory-snapshot-ghost",
        &corpus_snapshot,
        vec![ghost.id.clone()],
    );
    let error = publish(&mut db, &snapshot_ghost, std::slice::from_ref(&ghost))
        .expect_err("ghost contradiction");
    assert_eq!(
        error,
        "Ссылка противоречия не входит в публикацию и не существует в области персоны."
    );
    // Existing stored item in the same persona/corpus.
    let mut referencing = item_for("item-2", &corpus_snapshot, &reference, "second");
    referencing.contradiction_ids = vec!["item-1".into()];
    let snapshot_two = snapshot_for("memory-snapshot-2", &corpus_snapshot, vec!["item-2".into()]);
    publish(&mut db, &snapshot_two, std::slice::from_ref(&referencing)).unwrap();
    // Another item in the same batch.
    let mut pair = item_for("item-3", &corpus_snapshot, &reference, "third");
    pair.contradiction_ids = vec!["item-4".into()];
    let other = item_for("item-4", &corpus_snapshot, &reference, "fourth");
    let snapshot_three = snapshot_for(
        "memory-snapshot-3",
        &corpus_snapshot,
        vec!["item-3".into(), "item-4".into()],
    );
    publish(&mut db, &snapshot_three, &[pair, other]).unwrap();
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn read_missing_or_foreign_snapshot_is_an_explicit_error() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &snapshot, &items).unwrap();
    let error = read(&db, "memory-snapshot-missing").expect_err("missing read");
    assert_eq!(error, "Снимок памяти не найден или удалён.");
    let error = db
        .read_memory_snapshot_checked("memory-snapshot-1", OTHER_PERSONA, &|| Ok(()))
        .expect_err("foreign persona read");
    assert_eq!(error, "Снимок памяти принадлежит другой области персоны.");
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn malformed_persona_id_is_rejected_on_both_paths() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    for bad in ["", "   ", "a\nb", &"x".repeat(257)] {
        assert!(db
            .publish_memory_snapshot_checked(bad, &snapshot, &items, &|| Ok(()))
            .is_err());
        assert!(db
            .read_memory_snapshot_checked("memory-snapshot-1", bad, &|| Ok(()))
            .is_err());
    }
    publish(&mut db, &snapshot, &items).unwrap();
    // Surrounding whitespace is preserved, never trimmed: a padded persona is
    // simply a different owner, so the stored bundle is foreign to it.
    let error = db
        .read_memory_snapshot_checked("memory-snapshot-1", "  persona-a  ", &|| Ok(()))
        .expect_err("padded persona is a foreign owner");
    assert_eq!(error, "Снимок памяти принадлежит другой области персоны.");
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn callback_error_at_every_point_aborts_without_partial_rows() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    let calls = std::cell::Cell::new(0u32);
    db.publish_memory_snapshot_checked(PERSONA, &snapshot, &items, &|| {
        calls.set(calls.get() + 1);
        Ok(())
    })
    .unwrap();
    let total_checks = calls.get();
    db.connection
        .execute("DELETE FROM memory_snapshots", [])
        .unwrap();
    db.connection
        .execute("DELETE FROM memory_items", [])
        .unwrap();
    for fail_at in 1..=total_checks {
        calls.set(0);
        let check = || -> Result<(), String> {
            let count = calls.get() + 1;
            calls.set(count);
            if count == fail_at {
                return Err(format!("Стоп {fail_at}."));
            }
            Ok(())
        };
        let error = db
            .publish_memory_snapshot_checked(PERSONA, &snapshot, &items, &check)
            .err()
            .unwrap_or_else(|| panic!("check failing at {fail_at} must abort publish"));
        assert_eq!(
            error,
            format!("Стоп {fail_at}."),
            "exact callback error is preserved"
        );
        assert_no_memory_rows(&db.connection);
        // A clean publish afterwards proves the connection is not left inside
        // a failed transaction.
        publish(&mut db, &snapshot, &items).unwrap();
        db.connection
            .execute("DELETE FROM memory_snapshots", [])
            .unwrap();
        db.connection
            .execute("DELETE FROM memory_items", [])
            .unwrap();
    }
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn read_callback_error_is_preserved_and_data_survives() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &snapshot, &items).unwrap();
    let check = || Err("Чтение остановлено.".to_string());
    let error = db
        .read_memory_snapshot_checked("memory-snapshot-1", PERSONA, &check)
        .expect_err("failing check");
    assert_eq!(error, "Чтение остановлено.");
    assert_eq!(read(&db, "memory-snapshot-1").unwrap().items, items);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn success_path_invokes_check_before_during_and_after() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    let calls = std::cell::Cell::new(0u32);
    let check = || -> Result<(), String> {
        calls.set(calls.get() + 1);
        Ok(())
    };
    db.publish_memory_snapshot_checked(PERSONA, &snapshot, &items, &check)
        .unwrap();
    assert!(calls.get() >= 3, "before, during and final checks");
    let before = calls.get();
    db.read_memory_snapshot_checked("memory-snapshot-1", PERSONA, &check)
        .unwrap();
    assert!(calls.get() > before, "read performs its own checks");
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn read_runs_on_a_savepoint_inside_a_caller_transaction() {
    let (mut db, dir) = setup();
    let (corpus_snapshot, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &snapshot, &items).unwrap();
    // The caller's own transaction is preserved: the read nests in a savepoint
    // and neither commits nor rolls back what is already open.
    db.connection.execute_batch("SAVEPOINT caller_tx").unwrap();
    db.connection
        .execute(
            "INSERT INTO memory_items(id,corpus_snapshot_id,persona_id,payload_bytes,data_json,canonical_sha256) \
             VALUES('marker',?1,'y',2,'{}','0000000000000000000000000000000000000000000000000000000000000000')",
            [&corpus_snapshot],
        )
        .unwrap();
    let loaded = db
        .read_memory_snapshot_checked("memory-snapshot-1", PERSONA, &|| Ok(()))
        .unwrap();
    assert_eq!(loaded.items, items);
    db.connection
        .execute_batch("ROLLBACK TO caller_tx; RELEASE caller_tx")
        .unwrap();
    // The read did not commit the caller transaction: the marker is gone and
    // the bundle is still intact.
    assert_eq!(memory_row_count(&db.connection, "memory_items"), 1);
    assert_eq!(read(&db, "memory-snapshot-1").unwrap().items, items);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn bundle_budget_is_enforced_before_storing() {
    let (mut db, dir) = setup();
    let (corpus_snapshot, references) = seed_source(&mut db, "corpus-a", &[("doc-1", "source")]);
    let text = "я".repeat(9000); // 18 KiB per item, ~18 MB serialized in total.
    let items: Vec<MemoryItem> = (0..1000)
        .map(|index| {
            item_for(
                &format!("item-{index}"),
                &corpus_snapshot,
                &references[0],
                &text,
            )
        })
        .collect();
    let snapshot = snapshot_for(
        "memory-snapshot-1",
        &corpus_snapshot,
        items.iter().map(|item| item.id.clone()).collect(),
    );
    let error = publish(&mut db, &snapshot, &items).expect_err("bundle over the 16 MiB budget");
    assert_eq!(error, "Память исследования превышает бюджет 16 МиБ.");
    assert_no_memory_rows(&db.connection);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn snapshot_item_count_is_bounded() {
    let (mut db, dir) = setup();
    let (corpus_snapshot, references) = seed_source(&mut db, "corpus-a", &[("doc-1", "source")]);
    let items: Vec<MemoryItem> = (0..1001)
        .map(|index| {
            item_for(
                &format!("item-{index}"),
                &corpus_snapshot,
                &references[0],
                "text",
            )
        })
        .collect();
    let snapshot = snapshot_for(
        "memory-snapshot-1",
        &corpus_snapshot,
        items.iter().map(|item| item.id.clone()).collect(),
    );
    assert!(publish(&mut db, &snapshot, &items).is_err());
    assert_no_memory_rows(&db.connection);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn tampered_snapshot_hash_is_rejected_without_repair() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &snapshot, &items).unwrap();
    db.connection
        .execute_batch("DROP TRIGGER memory_snapshots_immutable")
        .unwrap();
    db.connection
        .execute(
            "UPDATE memory_snapshots SET canonical_sha256=?2 WHERE id=?1",
            rusqlite::params!["memory-snapshot-1", "b".repeat(64)],
        )
        .unwrap();
    let error = read(&db, "memory-snapshot-1").expect_err("hash mismatch");
    assert_eq!(error, INVALID);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn tampered_payload_counter_is_rejected_before_materialization() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &snapshot, &items).unwrap();
    db.connection
        .execute_batch("DROP TRIGGER memory_items_immutable; PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    db.connection
        .execute(
            "UPDATE memory_items SET payload_bytes=1 WHERE id='item-1'",
            [],
        )
        .unwrap();
    assert_eq!(read(&db, "memory-snapshot-1").err().unwrap(), INVALID);
    db.connection
        .execute_batch("DROP TRIGGER memory_snapshots_immutable;")
        .unwrap();
    db.connection
        .execute(
            "UPDATE memory_snapshots SET payload_bytes=1 WHERE id='memory-snapshot-1'",
            [],
        )
        .unwrap();
    assert_eq!(read(&db, "memory-snapshot-1").err().unwrap(), INVALID);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

fn raw_seed_item(
    connection: &Connection,
    id: &str,
    corpus: &str,
    persona: &str,
    item: &MemoryItem,
) {
    let json = serde_json::to_string(item).unwrap();
    connection
        .execute(
            "INSERT INTO memory_items(id,corpus_snapshot_id,persona_id,payload_bytes,data_json,canonical_sha256) \
             VALUES(?1,?2,?3,?4,?5,?6)",
            rusqlite::params![id, corpus, persona, json.len() as i64, json, sha256_hex(json.as_bytes())],
        )
        .unwrap();
}

#[test]
fn oversized_item_payload_is_rejected_before_materialization() {
    let (mut db, dir) = setup();
    let (corpus_snapshot, _reference, snapshot, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &snapshot, &items).unwrap();
    // Craft a row whose byte length exceeds the per-item cap while keeping
    // the stored counter consistent; schema CHECKs are bypassed exactly like
    // a corrupting writer would have to bypass them.
    db.connection
        .execute_batch("DROP TRIGGER memory_items_immutable; PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    let blob = vec![255u8; 600 * 1024];
    db.connection
        .execute(
            "INSERT INTO memory_items(id,corpus_snapshot_id,persona_id,payload_bytes,data_json,canonical_sha256) \
             VALUES('item-big',?1,?2,?3,?4,?5)",
            rusqlite::params![corpus_snapshot, PERSONA, blob.len() as i64, blob, sha256_hex(&blob)],
        )
        .unwrap();
    db.connection
        .execute(
            "INSERT INTO memory_snapshot_items(snapshot_id,position,item_id) VALUES('memory-snapshot-1',1,'item-big')",
            [],
        )
        .unwrap();
    db.connection
        .execute_batch("DROP TRIGGER memory_snapshots_immutable")
        .unwrap();
    let mut updated_snapshot = snapshot.clone();
    updated_snapshot.item_ids.push("item-big".into());
    let json = serde_json::to_string(&updated_snapshot).unwrap();
    db.connection.execute("UPDATE memory_snapshots SET data_json=?1,payload_bytes=?2,canonical_sha256=?3 WHERE id=?4",params![json,json.len() as i64,sha256_hex(json.as_bytes()),snapshot.id]).unwrap();
    assert_eq!(read(&db, "memory-snapshot-1").err().unwrap(), INVALID);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn oversized_bundle_is_rejected_on_read_before_materialization() {
    let (mut db, dir) = setup();
    let (corpus_snapshot, references) = seed_source(&mut db, "corpus-a", &[("doc-1", "source")]);
    let text = "x".repeat(64 * 1024);
    let items: Vec<MemoryItem> = (0..260)
        .map(|index| {
            item_for(
                &format!("item-{index}"),
                &corpus_snapshot,
                &references[0],
                &text,
            )
        })
        .collect();
    for item in &items {
        raw_seed_item(&db.connection, &item.id, &corpus_snapshot, PERSONA, item);
    }
    let snapshot = snapshot_for(
        "memory-snapshot-1",
        &corpus_snapshot,
        items.iter().map(|item| item.id.clone()).collect(),
    );
    let snapshot_json = serde_json::to_string(&snapshot).unwrap();
    db.connection
        .execute(
            "INSERT INTO memory_snapshots(id,corpus_snapshot_id,persona_id,payload_bytes,data_json,canonical_sha256) \
             VALUES(?1,?2,?3,?4,?5,?6)",
            rusqlite::params![
                snapshot.id,
                corpus_snapshot,
                PERSONA,
                snapshot_json.len() as i64,
                snapshot_json,
                sha256_hex(snapshot_json.as_bytes())
            ],
        )
        .unwrap();
    for (position, item) in items.iter().enumerate() {
        db.connection
            .execute(
                "INSERT INTO memory_snapshot_items(snapshot_id,position,item_id) VALUES(?1,?2,?3)",
                rusqlite::params![snapshot.id, position as i64, item.id],
            )
            .unwrap();
    }
    assert_eq!(read(&db, "memory-snapshot-1").err().unwrap(), INVALID);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn broken_membership_positions_are_rejected() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &snapshot, &items).unwrap();
    // A membership row pointing outside the snapshot scope cannot exist, but
    // deleting membership breaks the exact itemIds contract.
    db.connection
        .execute(
            "DELETE FROM memory_snapshot_items WHERE snapshot_id='memory-snapshot-1' AND position=0",
            [],
        )
        .unwrap();
    assert_eq!(read(&db, "memory-snapshot-1").err().unwrap(), INVALID);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn non_contiguous_membership_positions_are_rejected() {
    let (mut db, dir) = setup();
    let (corpus_snapshot, reference, _, _) = single_bundle(&mut db, "corpus-a");
    let one = item_for("item-1", &corpus_snapshot, &reference, "one");
    let two = item_for("item-2", &corpus_snapshot, &reference, "two");
    let snapshot = snapshot_for(
        "memory-snapshot-1",
        &corpus_snapshot,
        vec!["item-1".into(), "item-2".into()],
    );
    publish(&mut db, &snapshot, &[one, two]).unwrap();
    db.connection
        .execute_batch("DROP TRIGGER memory_snapshot_items_immutable")
        .unwrap();
    db.connection
        .execute(
            "UPDATE memory_snapshot_items SET position=7 WHERE snapshot_id='memory-snapshot-1' AND item_id='item-2'",
            [],
        )
        .unwrap();
    assert_eq!(read(&db, "memory-snapshot-1").err().unwrap(), INVALID);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn deleting_source_snapshot_cascades_derived_memory() {
    let (mut db, dir) = setup();
    let (corpus_snapshot, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &snapshot, &items).unwrap();
    assert_eq!(memory_row_count(&db.connection, "memory_items"), 1);
    db.connection
        .execute("DELETE FROM snapshots WHERE id=?1", [&corpus_snapshot])
        .unwrap();
    assert_no_memory_rows(&db.connection);
    let error = read(&db, "memory-snapshot-1").expect_err("cascaded snapshot is gone");
    assert_eq!(error, "Снимок памяти не найден или удалён.");
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn source_snapshot_revalidated_when_original_record_is_corrupted() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &snapshot, &items).unwrap();
    // Corrupt the underlying record identity without its canonical state:
    // the native snapshot verifier must reject derived memory as well.
    db.connection
        .execute(
            "UPDATE records SET kind='deleted' WHERE id='record-corpus-a'",
            [],
        )
        .unwrap();
    let error = read(&db, "memory-snapshot-1").expect_err("corrupted source");
    assert_eq!(error, INVALID);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn aggregate_metadata_budget_precedes_any_item_payload_decoding() {
    let (mut db, dir) = setup();
    let (corpus, _) = seed_source(&mut db, "corpus-a", &[("doc-1", "source")]);
    let ids: Vec<String> = (0..33).map(|i| format!("oversized-{i}")).collect();
    let snapshot = snapshot_for("oversized-bundle", &corpus, ids.clone());
    let json = serde_json::to_string(&snapshot).unwrap();
    db.connection.execute("INSERT INTO memory_snapshots(id,corpus_snapshot_id,persona_id,payload_bytes,data_json,canonical_sha256) VALUES(?1,?2,?3,?4,?5,?6)", params![snapshot.id,corpus,PERSONA,json.len() as i64,json,sha256_hex(json.as_bytes())]).unwrap();
    for (position, id) in ids.iter().enumerate() {
        db.connection.execute("INSERT INTO memory_items(id,corpus_snapshot_id,persona_id,payload_bytes,data_json,canonical_sha256) VALUES(?1,?2,?3,524288,CAST(x'ff' || zeroblob(524287) AS BLOB),?4)",params![id,corpus,PERSONA,"a".repeat(64)]).unwrap();
        db.connection
            .execute(
                "INSERT INTO memory_snapshot_items(snapshot_id,position,item_id) VALUES(?1,?2,?3)",
                params![snapshot.id, position as i64, id],
            )
            .unwrap();
    }
    // INVALID is the metadata/budget rejection. Materializing the first BLOB
    // as a String instead would report the SQL storage conversion error.
    assert_eq!(read(&db, &snapshot.id).err().unwrap(), INVALID);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn read_rejects_a_deleted_cross_snapshot_contradiction_target() {
    let (mut db, dir) = setup();
    let (corpus, reference, first, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &first, &items).unwrap();
    let mut second_item = item_for("second-item", &corpus, &reference, "contradiction");
    second_item.contradiction_ids = vec![items[0].id.clone()];
    let second = snapshot_for("second-snapshot", &corpus, vec![second_item.id.clone()]);
    publish(&mut db, &second, &[second_item]).unwrap();
    db.connection
        .execute("DELETE FROM memory_items WHERE id=?1", [&items[0].id])
        .unwrap();
    assert!(read(&db, &second.id).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[path = "corpus-memory-encrypted-tests.rs"]
mod encrypted_tests;

#[path = "corpus-research-publication-tests.rs"]
mod publication_tests;

#[path = "corpus-persona-manifest-tests.rs"]
mod manifest_tests;
