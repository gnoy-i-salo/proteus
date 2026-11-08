use super::*;
use crate::corpus_database::search;
use rusqlite::params;
fn fixture() -> (Connection, VectorStageInput) {
    seed(Connection::open_in_memory().unwrap())
}
fn seed(connection: Connection) -> (Connection, VectorStageInput) {
    connection
        .execute_batch(include_str!("corpus-schema.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!("corpus-search-schema.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!("corpus-vector-schema.sql"))
        .unwrap();
    connection.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    connection.execute("INSERT INTO snapshots(id,corpus_id,publication_id,generation,created_at,canonical_sha256) VALUES('snapshot','corpus','publication',1,'0',?1)",["a".repeat(64)]).unwrap();
    connection
        .execute(
            "INSERT INTO search_generations VALUES('snapshot',1,1,'policy',?1,2,'0')",
            ["b".repeat(64)],
        )
        .unwrap();
    connection
        .execute("INSERT INTO search_active VALUES('snapshot',1)", [])
        .unwrap();
    for id in ["first", "second"] {
        connection.execute("INSERT INTO search_chunks(snapshot_id,index_generation,chunk_id,document_id,revision_id,span_start,span_end,persona_id,grant_id,binding_id,record_id,record_generation) VALUES('snapshot',1,?1,'doc','revision',0,2,'persona','grant','binding','record',1)",[id]).unwrap();
    }
    let input = VectorStageInput {
        stage_id: "stage".into(),
        source: search::active_receipt(&connection, "snapshot")
            .unwrap()
            .unwrap(),
        model_fingerprint: "model:1".into(),
        dimensions: 2,
    };
    (connection, input)
}
fn ok() -> Result<(), String> {
    Ok(())
}
#[test]
fn stage_retry_preserves_generation_and_appended_bytes() {
    let (mut db, input) = fixture();
    let initial = begin_stage(&mut db, &input, &ok).unwrap();
    assert_eq!(initial.vector_count, 0);
    assert!(!initial.published);
    let appended = append_vector(
        &mut db,
        &input,
        "first",
        &input.model_fingerprint,
        &[1., -0.],
        &ok,
    )
    .unwrap();
    assert_eq!(appended.generation, initial.generation);
    assert_eq!(appended.vector_count, 1);
    assert_eq!(begin_stage(&mut db, &input, &ok).unwrap(), appended);
    assert_eq!(
        append_vector(
            &mut db,
            &input,
            "first",
            &input.model_fingerprint,
            &[1., -0.],
            &ok
        )
        .unwrap(),
        appended
    );
    let bytes: Vec<u8> = db
        .query_row("SELECT vector FROM vector_entries", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        bytes,
        [1f32, -0.]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    );
    assert!(append_vector(
        &mut db,
        &input,
        "first",
        &input.model_fingerprint,
        &[1., 0.],
        &ok
    )
    .is_err());
    assert_eq!(begin_stage(&mut db, &input, &ok).unwrap(), appended);
}
#[test]
fn rejected_vectors_and_foreign_chunks_do_not_advance_stage() {
    let (mut db, input) = fixture();
    let initial = begin_stage(&mut db, &input, &ok).unwrap();
    for vector in [
        vec![0., 0.],
        vec![f32::NAN, 1.],
        vec![f32::INFINITY, 1.],
        vec![1.],
        vec![1., 2., 3.],
    ] {
        assert!(append_vector(
            &mut db,
            &input,
            "first",
            &input.model_fingerprint,
            &vector,
            &ok
        )
        .is_err());
    }
    assert!(append_vector(
        &mut db,
        &input,
        "foreign",
        &input.model_fingerprint,
        &[1., 0.],
        &ok
    )
    .is_err());
    assert!(append_vector(&mut db, &input, "first", "other", &[1., 0.], &ok).is_err());
    assert_eq!(begin_stage(&mut db, &input, &ok).unwrap(), initial);
}
#[test]
fn cancellation_rolls_back_begin_and_append_even_after_writes() {
    use std::cell::Cell;
    let (mut db, input) = fixture();
    let calls = Cell::new(0);
    assert_eq!(
        begin_stage(&mut db, &input, &|| {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                Err("cancel exact  ".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err(),
        "cancel exact  "
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM vector_generations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    begin_stage(&mut db, &input, &ok).unwrap();
    calls.set(0);
    assert_eq!(
        append_vector(
            &mut db,
            &input,
            "first",
            &input.model_fingerprint,
            &[1., 0.],
            &|| {
                calls.set(calls.get() + 1);
                if calls.get() == 2 {
                    Err("cancel exact  ".into())
                } else {
                    Ok(())
                }
            }
        )
        .unwrap_err(),
        "cancel exact  "
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM vector_entries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(begin_stage(&mut db, &input, &ok).unwrap().vector_count, 0);
}
#[test]
fn identity_mismatch_and_source_rebuild_fail_without_replacing_staging() {
    let (mut db, input) = fixture();
    let before = begin_stage(&mut db, &input, &ok).unwrap();
    let mut changed = input.clone();
    changed.dimensions = 3;
    assert!(begin_stage(&mut db, &changed, &ok).is_err());
    changed = input.clone();
    changed.model_fingerprint = "other".into();
    assert!(begin_stage(&mut db, &changed, &ok).is_err());
    changed = input.clone();
    changed.source.created_at = "1".into();
    assert!(begin_stage(&mut db, &changed, &ok).is_err());
    assert_eq!(begin_stage(&mut db, &input, &ok).unwrap(), before);
    db.execute(
        "INSERT INTO search_generations VALUES('snapshot',2,1,'policy',?1,2,'0')",
        ["c".repeat(64)],
    )
    .unwrap();
    db.execute("UPDATE search_active SET index_generation=2", [])
        .unwrap();
    assert!(append_vector(
        &mut db,
        &input,
        "first",
        &input.model_fingerprint,
        &[1., 0.],
        &ok
    )
    .is_err());
    assert!(begin_stage(&mut db, &input, &ok).is_err());
    assert_eq!(
        db.query_row(
            "SELECT generation,vector_count FROM vector_generations WHERE stage_id=?1",
            params![input.stage_id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        )
        .unwrap(),
        (before.generation, 0)
    );
}

fn complete(db: &mut Connection, input: &VectorStageInput) {
    begin_stage(db, input, &ok).unwrap();
    append_vector(db, input, "first", &input.model_fingerprint, &[1., 0.], &ok).unwrap();
    append_vector(
        db,
        input,
        "second",
        &input.model_fingerprint,
        &[0., 1.],
        &ok,
    )
    .unwrap();
}
#[test]
fn publication_is_complete_and_replay_never_rolls_back_new_active_generation() {
    let (mut db, input) = fixture();
    begin_stage(&mut db, &input, &ok).unwrap();
    assert!(publish_stage(&mut db, &input, &ok).is_err());
    complete(&mut db, &input);
    let receipt = publish_stage(&mut db, &input, &ok).unwrap();
    assert_eq!(receipt.source, input.source);
    assert_eq!(receipt.vector_count, 2);
    assert_eq!(receipt.content_sha256.len(), 64);
    assert_eq!(publish_stage(&mut db, &input, &ok).unwrap(), receipt);
    assert!(append_vector(
        &mut db,
        &input,
        "first",
        &input.model_fingerprint,
        &[1., 0.],
        &ok
    )
    .is_err());
    let mut second = input.clone();
    second.stage_id = "next".into();
    complete(&mut db, &second);
    let next = publish_stage(&mut db, &second, &ok).unwrap();
    assert!(next.generation > receipt.generation);
    assert_eq!(next.content_sha256, receipt.content_sha256);
    assert_eq!(publish_stage(&mut db, &input, &ok).unwrap(), receipt);
    assert_eq!(
        db.query_row("SELECT generation FROM vector_active", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        next.generation
    );
}
#[test]
fn publication_cancellation_at_each_checkpoint_keeps_prior_active_pointer() {
    use std::cell::Cell;
    let (mut db, input) = fixture();
    complete(&mut db, &input);
    let previous = publish_stage(&mut db, &input, &ok).unwrap();
    let mut next = input.clone();
    next.stage_id = "next".into();
    complete(&mut db, &next);
    let calls = Cell::new(0);
    let mut failures = 0;
    for cancel_at in 1..=20 {
        calls.set(0);
        let result = publish_stage(&mut db, &next, &|| {
            calls.set(calls.get() + 1);
            if calls.get() == cancel_at {
                Err("cancel publish  ".into())
            } else {
                Ok(())
            }
        });
        if result.is_ok() {
            assert_eq!(cancel_at, calls.get() + 1);
            break;
        }
        assert_eq!(result.unwrap_err(), "cancel publish  ");
        failures += 1;
        assert_eq!(
            db.query_row("SELECT generation FROM vector_active", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            previous.generation
        );
        assert!(!begin_stage(&mut db, &next, &ok).unwrap().published);
    }
    assert!(failures >= 4);
}
#[test]
fn empty_reference_generation_publishes_without_fabricating_vectors() {
    let (mut db, mut input) = fixture();
    db.execute(
        "INSERT INTO search_generations VALUES('snapshot',2,1,'policy',?1,0,'0')",
        [search::EMPTY_CONTENT_SHA256],
    )
    .unwrap();
    db.execute("UPDATE search_active SET index_generation=2", [])
        .unwrap();
    input.source = search::active_receipt(&db, "snapshot").unwrap().unwrap();
    begin_stage(&mut db, &input, &ok).unwrap();
    let receipt = publish_stage(&mut db, &input, &ok).unwrap();
    assert_eq!(receipt.vector_count, 0);
    assert_eq!(publish_stage(&mut db, &input, &ok).unwrap(), receipt);
}
#[test]
fn lost_source_chunk_cannot_publish_even_with_matching_staged_count() {
    let (mut db, input) = fixture();
    complete(&mut db, &input);
    db.execute("DELETE FROM search_chunks WHERE chunk_id='first'", [])
        .unwrap();
    assert!(publish_stage(&mut db, &input, &ok).is_err());
    assert_eq!(
        db.query_row("SELECT count(*) FROM vector_active", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn reopened_encrypted_database_replays_staging_and_sealed_receipt() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "proteus-vector-restart-{}-{}.db",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let open = || {
        let db = Connection::open(&path).unwrap();
        db.pragma_update(None, "key", "synthetic-vector-test-key")
            .unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        db
    };
    let (mut db, input) = seed(open());
    let stage = begin_stage(&mut db, &input, &ok).unwrap();
    append_vector(
        &mut db,
        &input,
        "first",
        &input.model_fingerprint,
        &[1., 0.],
        &ok,
    )
    .unwrap();
    drop(db);
    let mut db = open();
    let resumed = begin_stage(&mut db, &input, &ok).unwrap();
    assert_eq!(resumed.generation, stage.generation);
    assert_eq!(resumed.vector_count, 1);
    complete(&mut db, &input);
    let receipt = publish_stage(&mut db, &input, &ok).unwrap();
    drop(db);
    let mut db = open();
    assert_eq!(publish_stage(&mut db, &input, &ok).unwrap(), receipt);
    drop(db);
    let ciphertext = std::fs::read(&path).unwrap();
    assert!(!ciphertext
        .windows(input.model_fingerprint.len())
        .any(|bytes| bytes == input.model_fingerprint.as_bytes()));
    std::fs::remove_file(path).unwrap();
}
#[test]
fn redundant_source_metadata_must_agree_with_pinned_receipt() {
    let (mut db, input) = fixture();
    db.execute("INSERT INTO vector_generations(stage_id,snapshot_id,snapshot_generation,search_generation,source_receipt_json,model_fingerprint,dimensions,created_at) VALUES(?1,'snapshot',999,1,?2,?3,2,'0')",params![input.stage_id,serde_json::to_string(&input.source).unwrap(),input.model_fingerprint]).unwrap();
    assert!(begin_stage(&mut db, &input, &ok).is_err());
}

#[test]
fn active_vector_receipt_rejects_stale_sources_and_never_exposes_staging() {
    let (mut db, input) = fixture();
    assert_eq!(active_receipt(&mut db, "snapshot", &ok).unwrap(), None);
    complete(&mut db, &input);
    assert_eq!(active_receipt(&mut db, "snapshot", &ok).unwrap(), None);
    let receipt = publish_stage(&mut db, &input, &ok).unwrap();
    assert_eq!(
        active_receipt(&mut db, "snapshot", &ok).unwrap(),
        Some(receipt)
    );
    db.execute(
        "INSERT INTO search_generations VALUES('snapshot',2,1,'policy',?1,2,'0')",
        ["c".repeat(64)],
    )
    .unwrap();
    db.execute("UPDATE search_active SET index_generation=2", [])
        .unwrap();
    assert_eq!(active_receipt(&mut db, "snapshot", &ok).unwrap(), None);
    assert_eq!(active_receipt(&mut db, "absent", &ok).unwrap(), None);
}
#[test]
fn receipt_cancellation_preserves_outer_transaction_and_exact_error() {
    use std::cell::Cell;
    let (mut db, input) = fixture();
    complete(&mut db, &input);
    publish_stage(&mut db, &input, &ok).unwrap();
    db.execute_batch(
        "BEGIN; CREATE TABLE outer_marker(value TEXT); INSERT INTO outer_marker VALUES('keep');",
    )
    .unwrap();
    let calls = Cell::new(0);
    let result = active_receipt(&mut db, "snapshot", &|| {
        calls.set(calls.get() + 1);
        if calls.get() == 2 {
            Err("cancel receipt  ".into())
        } else {
            Ok(())
        }
    });
    assert_eq!(result.unwrap_err(), "cancel receipt  ");
    assert!(!db.is_autocommit());
    assert_eq!(
        db.query_row("SELECT value FROM outer_marker", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    db.execute_batch("ROLLBACK").unwrap();
    assert!(db.is_autocommit());
}

#[test]
fn incomplete_sealed_generation_is_an_error_instead_of_a_partial_index() {
    let (mut db, input) = fixture();
    complete(&mut db, &input);
    publish_stage(&mut db, &input, &ok).unwrap();
    db.execute("DELETE FROM vector_entries WHERE chunk_id='first'", [])
        .unwrap();
    assert!(active_receipt(&mut db, "snapshot", &ok).is_err());
    assert_eq!(
        db.query_row("SELECT count(*) FROM vector_entries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn corrupt_sealed_manifest_is_rejected_without_repairing_original_metadata() {
    let (mut db, input) = fixture();
    complete(&mut db, &input);
    publish_stage(&mut db, &input, &ok).unwrap();
    // Simulate damaged storage, beyond the writer's immutable-row trigger.
    db.execute_batch("DROP TRIGGER vector_generations_immutable")
        .unwrap();
    let corrupt = "z".repeat(64);
    db.execute(
        "UPDATE vector_generations SET content_sha256=?1",
        [&corrupt],
    )
    .unwrap();
    assert!(active_receipt(&mut db, "snapshot", &ok).is_err());
    assert_eq!(
        db.query_row("SELECT content_sha256 FROM vector_generations", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        corrupt
    );
}

#[path = "corpus-vectors-query-tests.rs"]
mod query_tests;

#[test]
fn stage_presence_requires_the_exact_source_and_retains_the_existing_vector() {
    let (mut db, input) = fixture();
    assert!(has_stage_vector(&mut db, &input, "first", &ok).is_err());
    begin_stage(&mut db, &input, &ok).unwrap();
    assert!(!has_stage_vector(&mut db, &input, "first", &ok).unwrap());
    append_vector(
        &mut db,
        &input,
        "first",
        &input.model_fingerprint,
        &[1., 0.],
        &ok,
    )
    .unwrap();
    assert!(has_stage_vector(&mut db, &input, "first", &ok).unwrap());
    assert!(!has_stage_vector(&mut db, &input, "second", &ok).unwrap());
    let mut foreign = input.clone();
    foreign.model_fingerprint = "foreign".into();
    assert!(has_stage_vector(&mut db, &foreign, "first", &ok).is_err());
    assert_eq!(
        has_stage_vector(&mut db, &input, "first", &|| Err(
            "cancel staged row  ".into()
        ))
        .unwrap_err(),
        "cancel staged row  "
    );
    assert!(has_stage_vector(&mut db, &input, "first", &ok).unwrap());
}
#[test]
fn stage_presence_refuses_corrupted_vectors_without_repairing_them() {
    let (mut db, input) = fixture();
    complete(&mut db, &input);
    db.execute_batch("DROP TRIGGER vector_entries_immutable; UPDATE vector_entries SET vector=zeroblob(8) WHERE chunk_id='first';").unwrap();
    assert!(has_stage_vector(&mut db, &input, "first", &ok).is_err());
    let bytes: Vec<u8> = db
        .query_row(
            "SELECT vector FROM vector_entries WHERE chunk_id='first'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bytes, vec![0; 8]);
}
