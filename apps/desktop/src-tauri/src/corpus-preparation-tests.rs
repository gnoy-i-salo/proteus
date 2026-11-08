use super::*;
use crate::corpus_database::{Acquisition, RecordInput, RevisionInput, SourceProvenance};
struct Tokenizer;
impl EpisodeTokenizer for Tokenizer {
    fn version(&self) -> &str {
        "syntheticUnicodeScalars:1"
    }
    fn count(&self, text: &str) -> Result<usize, String> {
        Ok(text.chars().count() + 2)
    }
}
fn policies() -> (EpisodePolicy, ChunkPolicy) {
    (
        EpisodePolicy {
            time_gap_ms: 100,
            holdout_basis_points: 0,
            seed: "fixture".into(),
        },
        ChunkPolicy {
            max_tokens: 10,
            max_bytes: 128,
        },
    )
}
pub(super) fn setup() -> (
    CorpusDatabase,
    std::path::PathBuf,
    Snapshot,
    Vec<EpisodeDocument>,
) {
    let dir = std::env::temp_dir().join(format!("proteus-prepared-{}", rand::random::<u64>()));
    let mut db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    let inputs: Vec<_> = [
        ("a", "  Точно é\r\n👩🏽‍🔬 — original  "),
        ("b", "held-out original"),
    ]
    .into_iter()
    .map(|(id, text)| RevisionInput {
        source_key: format!("source:{id}"),
        document_id: id.into(),
        document_json: serde_json::json!({"text":text,"original":"opaque original envelope"})
            .to_string(),
        provenance: SourceProvenance::source_service(Acquisition::Archive),
    })
    .collect();
    let generation = db
        .save_ingestion(&RecordInput {
            id: "record".into(),
            kind: "import".into(),
            provider: "telegram".into(),
            state_json: "{\"exact\":\"  é\\r\\n\"}".into(),
            documents: inputs.clone(),
        })
        .unwrap();
    let snapshot = db.publish("corpus", "raw", &[generation]).unwrap();
    let docs = inputs
        .into_iter()
        .zip(&snapshot.documents)
        .enumerate()
        .map(|(i, (input, reference))| EpisodeDocument {
            reference: reference.clone(),
            source_key: input.source_key,
            binding_id: "binding".into(),
            conversation_id: "account:chat".into(),
            thread_id: None,
            timestamp_ms: Some(i as i64 * 1000),
            text: serde_json::from_str::<serde_json::Value>(&input.document_json).unwrap()["text"]
                .as_str()
                .unwrap()
                .into(),
            related: vec![],
            relations_complete: true,
            holdout: i == 1,
        })
        .collect();
    (db, dir, snapshot, docs)
}
pub(super) fn publish(
    db: &mut CorpusDatabase,
    snapshot: &Snapshot,
    docs: &[EpisodeDocument],
    publication: &str,
) -> Result<PreparedSnapshot, String> {
    let (split, chunks) = policies();
    db.publish_prepared(&PreparationRequest {
        source_snapshot_id: &snapshot.id,
        publication_id: publication,
        documents: docs,
        split: &split,
        tokenizer: &Tokenizer,
        chunks: &chunks,
    })
}
#[test]
fn prepared_snapshot_is_new_immutable_durable_and_never_changes_raw_receipt() {
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "prepared").unwrap();
    assert_ne!(prepared.snapshot.id, raw.id);
    assert_eq!(
        prepared.snapshot.parent_id.as_deref(),
        Some(raw.id.as_str())
    );
    assert_eq!(prepared.snapshot.documents, raw.documents);
    assert_eq!(prepared.snapshot.records, raw.records);
    assert!(prepared.snapshot.generation > raw.generation);
    assert_eq!(db.snapshot(&raw.id).unwrap(), raw);
    assert_eq!(publish(&mut db, &raw, &docs, "prepared").unwrap(), prepared);
    drop(db);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(
        db.prepared_snapshot(&prepared.snapshot.id).unwrap(),
        prepared
    );
    let mut restored = String::new();
    for id in &prepared.episode_ids {
        let episode = db.prepared_episode(&prepared.snapshot.id, id).unwrap();
        let chunks = db
            .prepared_chunks(&prepared.snapshot.id, id, 0, 100, 1024 * 1024)
            .unwrap();
        if episode.partition == Partition::Holdout {
            assert!(chunks.is_empty());
        }
        for chunk in chunks {
            assert_eq!(chunk.partition, Partition::Reference);
            assert_eq!(chunk.spans[0].document_id, "a");
            restored.push_str(&chunk.text);
        }
    }
    assert_eq!(restored, docs[0].text);
    assert_eq!(db.snapshot(&raw.id).unwrap(), raw);
    drop(db);
    let encrypted = std::fs::read(dir.join("corpus.db")).unwrap();
    assert!(!encrypted
        .windows("held-out".len())
        .any(|w| w == b"held-out"));
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn conflicting_publication_omitted_revision_or_forged_text_never_leave_partial_snapshots() {
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "prepared").unwrap();
    let mut changed = docs.clone();
    changed[0].holdout = true;
    assert!(publish(&mut db, &raw, &changed, "prepared").is_err());
    assert!(publish(&mut db, &raw, &docs[..1], "omitted").is_err());
    changed = docs.clone();
    changed[0].text = "invented text".into();
    assert!(publish(&mut db, &raw, &changed, "forged").is_err());
    assert!(publish(&mut db, &raw, &docs, "raw").is_err());
    assert_eq!(
        db.prepared_snapshot(&prepared.snapshot.id).unwrap(),
        prepared
    );
    assert_eq!(
        db.connection
            .query_row("SELECT count(*) FROM snapshots", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn preparing_a_frozen_snapshot_after_source_edit_uses_old_exact_revisions() {
    let (mut db, dir, raw, docs) = setup();
    let mut record = db.read("record").unwrap().unwrap().input;
    record.documents[0].document_json = "{\"text\":\"new edit\"}".into();
    db.save_ingestion(&record).unwrap();
    let prepared = publish(&mut db, &raw, &docs, "frozen").unwrap();
    assert_eq!(prepared.snapshot.documents, raw.documents);
    assert_eq!(prepared.snapshot.records, raw.records);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn source_revocation_removes_prepared_rows_and_blocks_old_receipts() {
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "prepared").unwrap();
    db.remove("record").unwrap();
    assert!(db.prepared_snapshot(&prepared.snapshot.id).is_err());
    assert!(db
        .prepared_episode(&prepared.snapshot.id, &prepared.episode_ids[0])
        .is_err());
    assert!(publish(&mut db, &raw, &docs, "prepared").is_err());
    for table in [
        "corpus_preparations",
        "corpus_episodes",
        "corpus_prepared_chunks",
    ] {
        assert_eq!(
            db.connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn chunk_reads_are_explicitly_bounded_and_cannot_cross_episode_identity() {
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "prepared").unwrap();
    let episode = prepared
        .episode_ids
        .iter()
        .find(|id| {
            db.prepared_episode(&prepared.snapshot.id, id)
                .unwrap()
                .partition
                == Partition::Reference
        })
        .unwrap();
    let chunks = db
        .prepared_chunks(&prepared.snapshot.id, episode, 0, 100, 1024 * 1024)
        .unwrap();
    assert!(chunks.len() > 1);
    assert_eq!(
        db.prepared_chunks(&prepared.snapshot.id, episode, 1, 1, 1024 * 1024)
            .unwrap(),
        chunks[1..2]
    );
    assert!(db
        .prepared_chunks(&prepared.snapshot.id, episode, 0, 0, 1024)
        .is_err());
    assert!(db
        .prepared_chunks(&prepared.snapshot.id, episode, 0, 101, 1024)
        .is_err());
    assert!(db
        .prepared_chunks(&prepared.snapshot.id, episode, 0, 1, 1)
        .is_err());
    assert!(db
        .prepared_chunks(&prepared.snapshot.id, "foreign", 0, 1, 1024)
        .is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn fresh_schema_contains_atomic_immutable_preparation_tables() {
    let (db, dir, _, _) = setup();
    assert_eq!(
        db.connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
    assert_eq!(db.connection.query_row("SELECT count(*) FROM sqlite_schema WHERE name IN ('corpus_preparations','corpus_episodes','corpus_prepared_chunks')",[],|r|r.get::<_,i64>(0)).unwrap(),3);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn write_failure_rolls_back_snapshot_receipt_and_all_derived_rows_before_retry() {
    let (mut db, dir, raw, docs) = setup();
    db.connection.execute_batch("CREATE TRIGGER refuse_chunk BEFORE INSERT ON corpus_prepared_chunks BEGIN SELECT RAISE(ABORT,'fixture interruption'); END;").unwrap();
    assert!(publish(&mut db, &raw, &docs, "prepared").is_err());
    assert_eq!(db.snapshot(&raw.id).unwrap(), raw);
    assert_eq!(
        db.connection
            .query_row("SELECT count(*) FROM snapshots", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.connection
            .query_row("SELECT count(*) FROM publication_receipts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.connection
            .query_row("SELECT count(*) FROM corpus_preparations", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    db.connection
        .execute_batch("DROP TRIGGER refuse_chunk")
        .unwrap();
    let prepared = publish(&mut db, &raw, &docs, "prepared").unwrap();
    assert_eq!(prepared.snapshot.generation, 2);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn prepared_payloads_are_immutable_and_missing_rows_fail_integrity_checks() {
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "prepared").unwrap();
    assert!(db
        .connection
        .execute("UPDATE corpus_prepared_chunks SET data_json='{}'", [])
        .is_err());
    assert!(db
        .connection
        .execute("UPDATE corpus_episodes SET data_json='{}'", [])
        .is_err());
    assert!(db
        .connection
        .execute("UPDATE corpus_preparations SET split_version='changed'", [])
        .is_err());
    db.connection.execute("DELETE FROM corpus_prepared_chunks WHERE chunk_id=(SELECT chunk_id FROM corpus_prepared_chunks LIMIT 1)",[]).unwrap();
    assert!(db.prepared_snapshot(&prepared.snapshot.id).is_err());
    assert_eq!(db.snapshot(&raw.id).unwrap(), raw);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn reference_empty_preparation_does_not_create_fake_chunks_or_call_tokenizer() {
    struct Forbidden;
    impl EpisodeTokenizer for Forbidden {
        fn version(&self) -> &str {
            "fixtureNoReference:1"
        }
        fn count(&self, _: &str) -> Result<usize, String> {
            panic!("holdout text must not reach tokenizer")
        }
    }
    let (mut db, dir, raw, mut docs) = setup();
    for doc in &mut docs {
        doc.holdout = true;
    }
    let (split, chunks) = policies();
    let result = db
        .publish_prepared(&PreparationRequest {
            source_snapshot_id: &raw.id,
            publication_id: "emptyReference",
            documents: &docs,
            split: &split,
            tokenizer: &Forbidden,
            chunks: &chunks,
        })
        .unwrap();
    assert_eq!(result.chunk_count, 0);
    assert_eq!(result.episode_ids.len(), 2);
    assert!(result.chunking_version.starts_with("episodeChunks:1:"));
    assert_eq!(db.prepared_snapshot(&result.snapshot.id).unwrap(), result);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn v4_migration_preserves_original_job_and_search_bytes_and_is_idempotent() {
    let dir =
        std::env::temp_dir().join(format!("proteus-preparation-v4-{}", rand::random::<u64>()));
    let connection = super::super::crypto::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    connection
        .execute_batch(include_str!("fixtures/corpus-v4.sql"))
        .unwrap();
    connection.execute("INSERT INTO jobs(id,schema_version,status,phase,state_json,canonical_sha256) VALUES('job',1,'paused','reading',?1,'old-hash')",["{\"opaque\":\"  é\\r\\n\"}"]).unwrap();
    connection
        .execute(
            "INSERT INTO search_fts(text) VALUES(?1)",
            ["  Старый индекс é\r\n😀  "],
        )
        .unwrap();
    let mut db = CorpusDatabase { connection };
    db.save_ingestion(&RecordInput {
        id: "record".into(),
        kind: "import".into(),
        provider: "telegram".into(),
        state_json: "{\"exact\":\"  original  \"}".into(),
        documents: vec![],
    })
    .unwrap();
    let before = db.read("record").unwrap().unwrap();
    let job: String = db
        .connection
        .query_row("SELECT state_json FROM jobs", [], |r| r.get(0))
        .unwrap();
    drop(db);
    for _ in 0..2 {
        let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
        assert_eq!(
            db.connection
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            5
        );
        assert_eq!(db.read("record").unwrap().unwrap(), before);
        assert_eq!(
            db.connection
                .query_row("SELECT state_json FROM jobs", [], |r| r.get::<_, String>(0))
                .unwrap(),
            job
        );
        assert_eq!(
            db.connection
                .query_row("SELECT text FROM search_fts", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "  Старый индекс é\r\n😀  "
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn failed_v4_migration_restores_ddl_version_and_conflicting_user_data() {
    let dir = std::env::temp_dir().join(format!(
        "proteus-preparation-rollback-{}",
        rand::random::<u64>()
    ));
    let connection = super::super::crypto::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    connection
        .execute_batch(include_str!("fixtures/corpus-v4.sql"))
        .unwrap();
    connection.execute_batch("CREATE TABLE corpus_episodes(protected TEXT); INSERT INTO corpus_episodes VALUES('keep exact');").unwrap();
    drop(connection);
    assert!(CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).is_err());
    let connection = super::super::crypto::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
    assert_eq!(
        connection
            .query_row("SELECT protected FROM corpus_episodes", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "keep exact"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='corpus_preparations'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    connection
        .execute_batch("DROP TABLE corpus_episodes")
        .unwrap();
    drop(connection);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(
        db.connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn episode_cache_requires_explicit_nullable_thread_and_rejects_unknown_own_fields() {
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "prepared").unwrap();
    let episode = db
        .prepared_episode(&prepared.snapshot.id, &prepared.episode_ids[0])
        .unwrap();
    let mut value = serde_json::to_value(&episode).unwrap();
    assert_eq!(
        serde_json::from_value::<PreparedEpisode>(value.clone()).unwrap(),
        episode
    );
    value.as_object_mut().unwrap().remove("threadId");
    assert!(serde_json::from_value::<PreparedEpisode>(value).is_err());
    let mut value = serde_json::to_value(&episode).unwrap();
    value["thread_id"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<PreparedEpisode>(value).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn revocation_during_tokenization_cannot_republish_old_material() {
    struct Revoking {
        db: std::cell::RefCell<CorpusDatabase>,
        revoked: std::cell::Cell<bool>,
    }
    impl EpisodeTokenizer for Revoking {
        fn version(&self) -> &str {
            "syntheticRevocation:1"
        }
        fn count(&self, text: &str) -> Result<usize, String> {
            if !self.revoked.replace(true) {
                self.db.borrow_mut().remove("record")?;
            }
            Ok(text.chars().count() + 2)
        }
    }
    let (mut db, dir, raw, docs) = setup();
    let tokenizer = Revoking {
        db: std::cell::RefCell::new(
            CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap(),
        ),
        revoked: std::cell::Cell::new(false),
    };
    let (split, chunks) = policies();
    assert!(db
        .publish_prepared(&PreparationRequest {
            source_snapshot_id: &raw.id,
            publication_id: "late",
            documents: &docs,
            split: &split,
            tokenizer: &tokenizer,
            chunks: &chunks
        })
        .is_err());
    assert!(tokenizer.revoked.get());
    assert_eq!(
        db.connection
            .query_row("SELECT count(*) FROM corpus_preparations", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(db.snapshot(&raw.id).is_err());
    drop(tokenizer);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn revoking_attached_grant_purges_preparation_even_when_import_still_exists() {
    let (mut db, dir, raw, docs) = setup();
    let grant = db
        .save_ingestion(&RecordInput {
            id: "grant".into(),
            kind: "grant".into(),
            provider: "telegram".into(),
            state_json: "{}".into(),
            documents: vec![],
        })
        .unwrap();
    let bound = db
        .publish("corpus", "bound", &[raw.records[0].clone(), grant])
        .unwrap();
    let prepared = publish(&mut db, &bound, &docs, "prepared").unwrap();
    db.remove("grant").unwrap();
    assert!(db.revision(&raw.documents[0]).is_ok());
    assert!(db.prepared_snapshot(&prepared.snapshot.id).is_err());
    assert_eq!(
        db.connection
            .query_row("SELECT count(*) FROM corpus_prepared_chunks", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn corrupted_chunk_body_is_rejected_without_releasing_partial_content_or_leaking_read_transaction()
{
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "prepared").unwrap();
    let episode = prepared
        .episode_ids
        .iter()
        .find(|id| {
            db.prepared_episode(&prepared.snapshot.id, id)
                .unwrap()
                .partition
                == Partition::Reference
        })
        .unwrap();
    db.connection.execute_batch("DROP TRIGGER immutable_corpus_prepared_chunks; UPDATE corpus_prepared_chunks SET data_json='{}' WHERE position=0;").unwrap();
    assert!(db
        .prepared_chunks(&prepared.snapshot.id, episode, 0, 100, 1024 * 1024)
        .is_err());
    assert!(db.connection.is_autocommit());
    assert_eq!(db.snapshot(&raw.id).unwrap(), raw);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn shared_originals_persist_distinct_binding_episodes_without_duplicating_raw_revisions() {
    let (mut db, dir, raw, mut docs) = setup();
    let mut shared = docs[0].clone();
    shared.binding_id = "secondBinding".into();
    docs.push(shared);
    let mut forged = docs.clone();
    forged.last_mut().unwrap().text.push('!');
    assert!(publish(&mut db, &raw, &forged, "forged").is_err());
    let missing = vec![docs[0].clone(), docs[2].clone()];
    assert!(publish(&mut db, &raw, &missing, "missing").is_err());
    let prepared = publish(&mut db, &raw, &docs, "shared-bindings").unwrap();
    assert_eq!(prepared.snapshot.documents, raw.documents);
    let mut reconstructed = std::collections::BTreeMap::<String, String>::new();
    let mut chunk_ids = std::collections::BTreeSet::new();
    for id in &prepared.episode_ids {
        let episode = db.prepared_episode(&prepared.snapshot.id, id).unwrap();
        if episode.partition == Partition::Reference {
            for chunk in db
                .prepared_chunks(&prepared.snapshot.id, id, 0, 100, 1024 * 1024)
                .unwrap()
            {
                assert!(chunk_ids.insert(chunk.id));
                assert_eq!(chunk.spans[0].document_id, docs[0].reference.document_id);
                reconstructed
                    .entry(episode.binding_id.clone())
                    .or_default()
                    .push_str(&chunk.text);
            }
        }
    }
    assert_eq!(reconstructed.len(), 2);
    assert!(reconstructed.values().all(|text| *text == docs[0].text));
    docs.reverse();
    assert_eq!(
        publish(&mut db, &raw, &docs, "shared-bindings").unwrap(),
        prepared
    );
    drop(db);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(
        db.prepared_snapshot(&prepared.snapshot.id).unwrap(),
        prepared
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
