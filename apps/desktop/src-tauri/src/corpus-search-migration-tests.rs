use super::*;
use std::path::PathBuf;
fn legacy() -> (CorpusDatabase, PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "proteus-search-migration-{}",
        rand::random::<u64>()
    ));
    let connection = crypto::open(&directory.join("corpus.db"), &[31; 32]).unwrap();
    connection
        .execute_batch(include_str!("fixtures/corpus-v3.sql"))
        .unwrap();
    (CorpusDatabase { connection }, directory)
}
fn version(connection: &Connection) -> i64 {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}
fn schema(connection: &Connection) -> Vec<(String, String)> {
    connection
        .prepare("SELECT name,sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
#[test]
fn fresh_corpus_installs_encrypted_fts_and_precise_publication_receipts() {
    let directory =
        std::env::temp_dir().join(format!("proteus-search-fresh-{}", rand::random::<u64>()));
    let db = CorpusDatabase::open(&directory.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(version(&db.connection), 4);
    assert_eq!(db.connection.query_row("SELECT count(*) FROM pragma_table_info('search_stages') WHERE name='published_generation'",[],|row|row.get::<_,i64>(0)).unwrap(),1);
    db.connection
        .execute(
            "INSERT INTO search_fts(text) VALUES(?1)",
            ["Секретный точный é\r\n😀"],
        )
        .unwrap();
    assert_eq!(
        db.connection
            .query_row(
                "SELECT text FROM search_fts WHERE search_fts MATCH ?1",
                ["точный"],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        "Секретный точный é\r\n😀"
    );
    drop(db);
    let ciphertext = std::fs::read(directory.join("corpus.db")).unwrap();
    assert!(!ciphertext
        .windows("Секретный".len())
        .any(|window| window == "Секретный".as_bytes()));
    assert!(CorpusDatabase::open(&directory.join("corpus.db"), &[32; 32]).is_err());
    let db = CorpusDatabase::open(&directory.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(version(&db.connection), 4);
    drop(db);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn v3_upgrade_preserves_exact_records_revisions_snapshots_and_job_checkpoints() {
    let (mut db, directory) = legacy();
    let input = RecordInput {
        id: "record".into(),
        kind: "import".into(),
        provider: "telegram".into(),
        state_json: "{\"label\":\"  é\\r\\n😀  \"}".into(),
        documents: vec![RevisionInput {
            source_key: "telegram/chat/message".into(),
            document_id: "message".into(),
            document_json: "{\"text\":\"  Точно é\\r\\n😀  \"}".into(),
            provenance: SourceProvenance::source_service(Acquisition::Archive),
        }],
    };
    let generation = db.save_ingestion(&input).unwrap();
    let before = db.read("record").unwrap().unwrap();
    let snapshot = db.publish("corpus", "publication", &[generation]).unwrap();
    // Existing jobs are opaque to this migration: preserve their exact bytes even
    // when an adapter newer than this migration owns their checkpoint format.
    db.connection.execute("INSERT INTO jobs(id,schema_version,status,phase,state_json,canonical_sha256) VALUES('job',1,'paused','reading',?1,'existing-hash')",["{\"opaqueCursor\":\"  é\\r\\n😀  \"}"]).unwrap();
    let job: (String, String) = db
        .connection
        .query_row(
            "SELECT state_json,canonical_sha256 FROM jobs WHERE id='job'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    drop(db);
    let db = CorpusDatabase::open(&directory.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(version(&db.connection), 4);
    assert_eq!(db.read("record").unwrap().unwrap(), before);
    assert_eq!(db.snapshot(&snapshot.id).unwrap(), snapshot);
    assert_eq!(
        db.connection
            .query_row(
                "SELECT state_json,canonical_sha256 FROM jobs WHERE id='job'",
                [],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            )
            .unwrap(),
        job
    );
    let once = schema(&db.connection);
    drop(db);
    let db = CorpusDatabase::open(&directory.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(schema(&db.connection), once);
    assert_eq!(db.read("record").unwrap().unwrap(), before);
    drop(db);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn failed_fts_migration_restores_all_ddl_and_version_without_overwriting_conflicts() {
    let (db, directory) = legacy();
    db.connection.execute_batch("CREATE TABLE search_chunks(protected TEXT); INSERT INTO search_chunks VALUES('keep exact');").unwrap();
    let before = schema(&db.connection);
    drop(db);
    assert!(CorpusDatabase::open(&directory.join("corpus.db"), &[31; 32]).is_err());
    let connection = crypto::open(&directory.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(version(&connection), 3);
    assert_eq!(schema(&connection), before);
    assert_eq!(
        connection
            .query_row("SELECT protected FROM search_chunks", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "keep exact"
    );
    connection
        .execute_batch("DROP TABLE search_chunks")
        .unwrap();
    drop(connection);
    let db = CorpusDatabase::open(&directory.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(version(&db.connection), 4);
    drop(db);
    std::fs::remove_dir_all(directory).unwrap();
}
