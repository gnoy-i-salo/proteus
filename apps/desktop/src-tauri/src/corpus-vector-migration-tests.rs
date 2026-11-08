use super::*;
use std::path::PathBuf;
fn directory() -> PathBuf {
    std::env::temp_dir().join(format!(
        "proteus-vector-migration-{}",
        rand::random::<u64>()
    ))
}
fn version(connection: &Connection) -> i64 {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}
fn schema(connection: &Connection, include_vectors: bool) -> Vec<(String, String)> {
    connection.prepare("SELECT name,sql FROM sqlite_schema WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' AND (?1 OR name NOT LIKE 'vector_%') ORDER BY name").unwrap()
        .query_map([include_vectors],|row| Ok((row.get(0)?,row.get(1)?))).unwrap().collect::<Result<_,_>>().unwrap()
}
#[test]
fn fresh_database_installs_vector_generations_entries_and_active_pointer() {
    let dir = directory();
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(version(&db.connection), 6);
    assert_eq!(db.connection.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN ('vector_generations','vector_entries','vector_active')",[],|r| r.get::<_,i64>(0)).unwrap(),3);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn v5_upgrade_preserves_originals_fts_job_bytes_and_existing_schema_across_restart() {
    let dir = directory();
    let connection = crypto::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    connection
        .execute_batch(include_str!("fixtures/corpus-v5.sql"))
        .unwrap();
    let mut db = CorpusDatabase { connection };
    let before_schema = schema(&db.connection, false);
    let input = RecordInput {
        id: "record".into(),
        kind: "import".into(),
        provider: "telegram".into(),
        state_json: "{\"exact\":\"  é\\r\\n😀  \"}".into(),
        documents: vec![RevisionInput {
            source_key: "chat/message".into(),
            document_id: "message".into(),
            document_json: "{\"text\":\"  исходник é\\r\\n😀  \"}".into(),
            provenance: SourceProvenance::source_service(Acquisition::Archive),
        }],
    };
    let generation = db.save_ingestion(&input).unwrap();
    let before = db.read("record").unwrap().unwrap();
    let snapshot = db.publish("corpus", "publication", &[generation]).unwrap();
    db.connection
        .execute(
            "INSERT INTO search_fts(text) VALUES(?1)",
            ["  индекс é\r\n😀  "],
        )
        .unwrap();
    db.connection.execute("INSERT INTO jobs(id,schema_version,status,phase,state_json,canonical_sha256) VALUES('job',1,'paused','reading',?1,'opaque-hash')",["{\"cursor\":\"  é\\r\\n😀  \"}"]).unwrap();
    drop(db);
    for _ in 0..2 {
        let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
        assert_eq!(version(&db.connection), 6);
        assert_eq!(schema(&db.connection, false), before_schema);
        assert_eq!(db.read("record").unwrap().unwrap(), before);
        assert_eq!(db.snapshot(&snapshot.id).unwrap(), snapshot);
        assert_eq!(
            db.connection
                .query_row("SELECT text FROM search_fts", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "  индекс é\r\n😀  "
        );
        assert_eq!(
            db.connection
                .query_row(
                    "SELECT state_json,canonical_sha256 FROM jobs WHERE id='job'",
                    [],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                )
                .unwrap(),
            (
                "{\"cursor\":\"  é\\r\\n😀  \"}".into(),
                "opaque-hash".into()
            )
        );
        drop(db);
    }
    assert!(CorpusDatabase::open(&dir.join("corpus.db"), &[32; 32]).is_err());
    let ciphertext = std::fs::read(dir.join("corpus.db")).unwrap();
    assert!(!ciphertext
        .windows("исходник".len())
        .any(|bytes| bytes == "исходник".as_bytes()));
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn vector_schema_conflict_rolls_back_new_tables_and_keeps_exact_v5_data() {
    let dir = directory();
    let connection = crypto::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    connection
        .execute_batch(include_str!("fixtures/corpus-v5.sql"))
        .unwrap();
    connection.execute_batch("CREATE TABLE vector_entries(protected TEXT); INSERT INTO vector_entries VALUES('  keep é  ');").unwrap();
    let before = schema(&connection, true);
    drop(connection);
    assert!(CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).is_err());
    let connection = crypto::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(version(&connection), 5);
    assert_eq!(schema(&connection, true), before);
    assert_eq!(
        connection
            .query_row("SELECT protected FROM vector_entries", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "  keep é  "
    );
    connection
        .execute_batch("DROP TABLE vector_entries")
        .unwrap();
    drop(connection);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(version(&db.connection), 6);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn sealed_vectors_require_a_manifest_and_follow_source_generation_deletion() {
    for delete_snapshot in [false, true] {
        let dir = directory();
        let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
        db.connection.execute("INSERT INTO snapshots(id,corpus_id,publication_id,generation,created_at,canonical_sha256) VALUES('snapshot','corpus','publication',1,'0',?1)",["c".repeat(64)]).unwrap();
        db.connection.execute("INSERT INTO search_generations(snapshot_id,index_generation,snapshot_generation,policy_fingerprint,content_sha256,chunk_count,created_at) VALUES('snapshot',1,1,'policy',?1,1,'0')",["a".repeat(64)]).unwrap();
        db.connection.execute("INSERT INTO vector_generations(stage_id,snapshot_id,snapshot_generation,search_generation,source_receipt_json,model_fingerprint,dimensions,created_at) VALUES('stage','snapshot',1,1,'{}','model',2,'0')",[]).unwrap();
        let generation = db.connection.last_insert_rowid();
        assert!(db
            .connection
            .execute(
                "INSERT INTO vector_active VALUES('snapshot',?1)",
                [generation]
            )
            .is_err());
        assert!(db
            .connection
            .execute(
                "INSERT INTO vector_entries VALUES(?1,'short',?2)",
                rusqlite::params![generation, vec![0u8; 4]]
            )
            .is_err());
        let vector: Vec<_> = [1f32, 0.].iter().flat_map(|v| v.to_le_bytes()).collect();
        db.connection
            .execute(
                "INSERT INTO vector_entries VALUES(?1,'chunk',?2)",
                rusqlite::params![generation, vector],
            )
            .unwrap();
        assert!(db
            .connection
            .execute(
                "UPDATE vector_entries SET chunk_id='other' WHERE generation=?1",
                [generation]
            )
            .is_err());
        assert!(db.connection.execute("UPDATE vector_generations SET state='published',published_at='1' WHERE generation=?1",[generation]).is_err(),"NULL must not satisfy the publication manifest CHECK");
        db.connection.execute("UPDATE vector_generations SET state='published',content_sha256=?2,published_at='1',vector_count=1 WHERE generation=?1",rusqlite::params![generation,"b".repeat(64)]).unwrap();
        db.connection
            .execute(
                "INSERT INTO vector_active VALUES('snapshot',?1)",
                [generation],
            )
            .unwrap();
        assert!(db
            .connection
            .execute(
                "INSERT INTO vector_entries VALUES(?1,'late',?2)",
                rusqlite::params![generation, vector]
            )
            .is_err());
        assert!(db
            .connection
            .execute(
                "UPDATE vector_generations SET model_fingerprint='other' WHERE generation=?1",
                [generation]
            )
            .is_err());
        if delete_snapshot {
            db.connection
                .execute("DELETE FROM snapshots WHERE id='snapshot'", [])
                .unwrap();
        } else {
            db.connection.execute("DELETE FROM search_generations WHERE snapshot_id='snapshot' AND index_generation=1",[]).unwrap();
        }
        for table in ["vector_generations", "vector_entries", "vector_active"] {
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
}
