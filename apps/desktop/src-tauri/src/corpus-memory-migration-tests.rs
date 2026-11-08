use super::*;
use std::path::PathBuf;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "proteus-memory-migration-{}",
            rand::random::<u64>()
        )))
    }
    fn path(&self) -> PathBuf {
        self.0.join("corpus.db")
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn version(connection: &Connection) -> i64 {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}
fn schema(connection: &Connection, include_memory: bool) -> Vec<(String, String)> {
    connection.prepare("SELECT name,sql FROM sqlite_schema WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' AND (?1 OR (name NOT LIKE 'memory_%' AND name NOT LIKE 'simulation_%')) ORDER BY name").unwrap()
        .query_map([include_memory], |row| Ok((row.get(0)?,row.get(1)?))).unwrap().collect::<Result<_,_>>().unwrap()
}
#[test]
fn fresh_database_owns_all_persistent_memory_namespaces_in_schema7() {
    let dir = Directory::new();
    let db = CorpusDatabase::open(&dir.path(), &[71; 32]).unwrap();
    assert_eq!(version(&db.connection), 7);
    let tables: i64 = db.connection.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN ('memory_items','memory_snapshots','memory_snapshot_items','memory_manifests','memory_skills','memory_publications','simulation_entries')", [], |row| row.get(0)).unwrap();
    assert_eq!(tables, 7);
    assert_eq!(
        db.connection
            .pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn memory_upgrade_preserves_v6_originals_jobs_search_vectors_and_schema_on_reopen() {
    let dir = Directory::new();
    let connection = crypto::open(&dir.path(), &[71; 32]).unwrap();
    connection
        .execute_batch(include_str!("fixtures/corpus-v6.sql"))
        .unwrap();
    let before_schema = schema(&connection, false);
    let mut db = CorpusDatabase { connection };
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
    let original = db.read("record").unwrap().unwrap();
    let snapshot = db.publish("corpus", "publication", &[generation]).unwrap();
    db.connection
        .execute(
            "INSERT INTO search_fts(text) VALUES(?1)",
            ["  индекс é\r\n😀  "],
        )
        .unwrap();
    db.connection.execute("INSERT INTO jobs(id,schema_version,status,phase,state_json,canonical_sha256) VALUES('job',1,'paused','reading',?1,'opaque-hash')", ["{\"cursor\":\"  é\\r\\n😀  \"}"]).unwrap();
    db.connection.execute("INSERT INTO search_generations(snapshot_id,index_generation,snapshot_generation,policy_fingerprint,content_sha256,chunk_count,created_at) VALUES(?1,1,1,'policy',?2,1,'0')", rusqlite::params![snapshot.id,"a".repeat(64)]).unwrap();
    db.connection.execute("INSERT INTO vector_generations(stage_id,snapshot_id,snapshot_generation,search_generation,source_receipt_json,model_fingerprint,dimensions,created_at) VALUES('stage',?1,1,1,'{}','model',2,'0')", [&snapshot.id]).unwrap();
    let vector_generation = db.connection.last_insert_rowid();
    let vector: Vec<_> = [1f32, 0.]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    db.connection
        .execute(
            "INSERT INTO vector_entries VALUES(?1,'chunk',?2)",
            rusqlite::params![vector_generation, vector],
        )
        .unwrap();
    drop(db);
    for _ in 0..2 {
        let db = CorpusDatabase::open(&dir.path(), &[71; 32]).unwrap();
        assert_eq!(version(&db.connection), 7);
        assert_eq!(schema(&db.connection, false), before_schema);
        assert_eq!(db.read("record").unwrap().unwrap(), original);
        assert_eq!(db.snapshot(&snapshot.id).unwrap(), snapshot);
        assert_eq!(
            db.connection
                .query_row("SELECT text FROM search_fts", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "  индекс é\r\n😀  "
        );
        assert_eq!(
            db.connection
                .query_row("SELECT state_json FROM jobs WHERE id='job'", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "{\"cursor\":\"  é\\r\\n😀  \"}"
        );
        assert_eq!(
            db.connection
                .query_row("SELECT vector FROM vector_entries", [], |row| row
                    .get::<_, Vec<u8>>(0))
                .unwrap(),
            vector
        );
    }
    assert!(CorpusDatabase::open(&dir.path(), &[72; 32]).is_err());
    let encrypted = std::fs::read(dir.path()).unwrap();
    assert!(!encrypted
        .windows("исходник".len())
        .any(|value| value == "исходник".as_bytes()));
}
#[test]
fn memory_schema_conflict_rolls_back_all_new_objects_and_keeps_v6_bytes() {
    let dir = Directory::new();
    let connection = crypto::open(&dir.path(), &[71; 32]).unwrap();
    connection
        .execute_batch(include_str!("fixtures/corpus-v6.sql"))
        .unwrap();
    connection.execute_batch("CREATE TABLE memory_manifests(protected TEXT); INSERT INTO memory_manifests VALUES('  keep é  ');").unwrap();
    let before = schema(&connection, true);
    drop(connection);
    assert!(CorpusDatabase::open(&dir.path(), &[71; 32]).is_err());
    let connection = crypto::open(&dir.path(), &[71; 32]).unwrap();
    assert_eq!(version(&connection), 6);
    assert_eq!(schema(&connection, true), before);
    assert_eq!(
        connection
            .query_row("SELECT protected FROM memory_manifests", [], |row| row
                .get::<_, String>(
                0
            ))
            .unwrap(),
        "  keep é  "
    );
    connection
        .execute_batch("DROP TABLE memory_manifests")
        .unwrap();
    drop(connection);
    let db = CorpusDatabase::open(&dir.path(), &[71; 32]).unwrap();
    assert_eq!(version(&db.connection), 7);
}

#[test]
fn memory_schema_enforces_scope_immutability_and_source_cascades_without_touching_other_persona() {
    let dir = Directory::new();
    let db = CorpusDatabase::open(&dir.path(), &[71; 32]).unwrap();
    for owner in ["a", "b"] {
        db.connection.execute("INSERT INTO snapshots(id,corpus_id,publication_id,generation,created_at,canonical_sha256) VALUES(?1,?1,?1,1,'0',?2)",rusqlite::params![owner,"a".repeat(64)]).unwrap();
        for table in ["memory_items", "memory_snapshots"] {
            db.connection.execute(&format!("INSERT INTO {table}(id,corpus_snapshot_id,persona_id,payload_bytes,data_json,canonical_sha256) VALUES(?1,?1,?1,2,'{{}}',?2)"),rusqlite::params![owner,"a".repeat(64)]).unwrap();
        }
        db.connection
            .execute("INSERT INTO memory_snapshot_items VALUES(?1,0,?1)", [owner])
            .unwrap();
        db.connection.execute("INSERT INTO memory_manifests(id,corpus_snapshot_id,memory_snapshot_id,persona_id,payload_bytes,data_json,canonical_sha256) VALUES(?1,?1,?1,?1,2,'{}',?2)",rusqlite::params![owner,"a".repeat(64)]).unwrap();
        db.connection.execute("INSERT INTO memory_skills(id,manifest_id,payload_bytes,data_json,canonical_sha256) VALUES(?1,?1,2,'{}',?2)",rusqlite::params![owner,"a".repeat(64)]).unwrap();
        db.connection.execute("INSERT INTO memory_publications(id,memory_snapshot_id,input_sha256,payload_bytes,data_json,canonical_sha256) VALUES(?1,?1,?2,2,'{}',?2)",rusqlite::params![owner,"a".repeat(64)]).unwrap();
        db.connection.execute("INSERT INTO simulation_entries(id,experiment_id,manifest_id,trial_id,sequence,payload_bytes,data_json,canonical_sha256) VALUES(?1,'experiment',?1,'trial',1,2,'{}',?2)",rusqlite::params![owner,"a".repeat(64)]).unwrap();
    }
    assert!(db
        .connection
        .execute("INSERT INTO memory_snapshot_items VALUES('a',1,'b')", [])
        .is_err());
    assert!(db.connection.execute("INSERT INTO memory_manifests(id,corpus_snapshot_id,memory_snapshot_id,persona_id,payload_bytes,data_json,canonical_sha256) VALUES('cross','b','a','b',2,'{}',?1)",["a".repeat(64)]).is_err());
    assert!(db
        .connection
        .execute(
            "UPDATE memory_items SET data_json='changed' WHERE id='a'",
            []
        )
        .is_err());
    assert!(db
        .connection
        .execute(
            "UPDATE memory_manifests SET persona_id='b' WHERE id='a'",
            []
        )
        .is_err());
    db.connection
        .execute("DELETE FROM snapshots WHERE id='a'", [])
        .unwrap();
    for table in [
        "memory_items",
        "memory_snapshots",
        "memory_manifests",
        "memory_skills",
        "memory_publications",
        "simulation_entries",
    ] {
        let ids: Vec<String> = db
            .connection
            .prepare(&format!("SELECT id FROM {table} ORDER BY id"))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(ids, vec!["b"], "isolated cascade in {table}");
    }
    assert_eq!(
        db.connection
            .query_row("SELECT snapshot_id FROM memory_snapshot_items", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "b"
    );
    assert_eq!(
        db.connection
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
}
