use super::*;
fn input(id: &str) -> RecordInput {
    RecordInput {
        id: id.into(),
        kind: "import".into(),
        provider: "telegram".into(),
        state_json: "{\"label\":\"  Точно é\\r\\n👩🏽‍💻  \"}".into(),
        documents: vec![],
    }
}
fn setup() -> (CorpusDatabase, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("proteus-namespace-{}", rand::random::<u64>()));
    (
        CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap(),
        dir,
    )
}
#[test]
fn ingestion_records_are_not_enumerated_as_legacy_source_payloads() {
    let (mut db, dir) = setup();
    db.save(&input("legacy")).unwrap();
    let generation = db.save_ingestion(&input("modern")).unwrap();
    assert_eq!(
        db.ids_in_namespace(RecordNamespace::SourceService).unwrap(),
        ["legacy"]
    );
    assert_eq!(
        db.ids_in_namespace(RecordNamespace::Ingestion).unwrap(),
        ["modern"]
    );
    assert_eq!(
        db.read("modern").unwrap().unwrap().namespace,
        RecordNamespace::Ingestion
    );
    assert_eq!(db.save_ingestion(&input("modern")).unwrap(), generation);
    drop(db);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(
        db.ids_in_namespace(RecordNamespace::Ingestion).unwrap(),
        ["modern"]
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn a_record_id_cannot_be_reassigned_between_owning_codecs_even_with_identical_payload() {
    let (mut db, dir) = setup();
    db.save(&input("legacy")).unwrap();
    assert!(db.save_ingestion(&input("legacy")).is_err());
    db.save_ingestion(&input("modern")).unwrap();
    assert!(db.save(&input("modern")).is_err());
    assert_eq!(
        db.read("legacy").unwrap().unwrap().namespace,
        RecordNamespace::SourceService
    );
    assert_eq!(
        db.read("modern").unwrap().unwrap().namespace,
        RecordNamespace::Ingestion
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn deletion_retains_namespace_and_does_not_reactivate_a_removed_record() {
    let (mut db, dir) = setup();
    db.save_ingestion(&input("modern")).unwrap();
    db.remove("modern").unwrap();
    assert_eq!(
        db.read("modern").unwrap().unwrap().namespace,
        RecordNamespace::Ingestion
    );
    assert!(db.save_ingestion(&input("modern")).is_err());
    assert!(db.save(&input("modern")).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn actual_v2_fixture_upgrades_without_rewriting_legacy_payload_or_hash() {
    let dir = std::env::temp_dir().join(format!("proteus-namespace-v2-{}", rand::random::<u64>()));
    let connection = crypto::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    connection
        .execute_batch(include_str!("fixtures/corpus-v2.sql"))
        .unwrap();
    let old = input("legacy");
    connection.execute("INSERT INTO records(id,kind,provider,state_json,canonical_sha256,generation) VALUES(?1,?2,?3,?4,?5,1)",rusqlite::params![old.id,old.kind,old.provider,old.state_json,record_hash(&old)]).unwrap();
    drop(connection);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(
        db.connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        5
    );
    let saved = db.read("legacy").unwrap().unwrap();
    assert_eq!(saved.namespace, RecordNamespace::SourceService);
    assert_eq!(saved.input, old);
    let hash: String = db
        .connection
        .query_row(
            "SELECT canonical_sha256 FROM records WHERE id='legacy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(hash, record_hash(&old));
    drop(db);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(db.read("legacy").unwrap().unwrap(), saved);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn failed_v2_namespace_migration_rolls_back_column_and_version() {
    let dir = std::env::temp_dir().join(format!(
        "proteus-namespace-rollback-{}",
        rand::random::<u64>()
    ));
    let connection = crypto::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    connection
        .execute_batch(include_str!("fixtures/corpus-v2.sql"))
        .unwrap();
    // Force the second migration statement to fail, after ALTER TABLE succeeds.
    connection.execute_batch("CREATE TRIGGER immutable_record_namespace BEFORE UPDATE ON records BEGIN SELECT 1; END;").unwrap();
    drop(connection);
    assert!(CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).is_err());
    let connection = crypto::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM pragma_table_info('records') WHERE name='namespace'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    connection
        .execute_batch("DROP TRIGGER immutable_record_namespace;")
        .unwrap();
    drop(connection);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert!(db
        .ids_in_namespace(RecordNamespace::Ingestion)
        .unwrap()
        .is_empty());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn sqlite_rejects_reassigning_an_existing_records_codec() {
    let (mut db, dir) = setup();
    db.save(&input("legacy")).unwrap();
    assert!(db
        .connection
        .execute(
            "UPDATE records SET namespace='ingestion' WHERE id='legacy'",
            []
        )
        .is_err());
    assert_eq!(
        db.read("legacy").unwrap().unwrap().namespace,
        RecordNamespace::SourceService
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn legacy_revision_lookup_does_not_inherit_ingestion_provenance() {
    let (mut db, dir) = setup();
    let revision = RevisionInput {
        source_key: "telegram/account/chat/message".into(),
        document_id: "message".into(),
        document_json: "{\"text\":\"  exact é\\r\\n\"}".into(),
        provenance: SourceProvenance::source_service(Acquisition::Archive),
    };
    let mut modern = input("modern");
    modern.documents.push(revision.clone());
    db.save_ingestion(&modern).unwrap();
    assert!(db
        .matching_revision(
            RecordNamespace::SourceService,
            &revision.source_key,
            &revision.document_json
        )
        .unwrap()
        .is_none());
    let mut legacy = input("legacy");
    legacy.documents.push(revision.clone());
    db.save(&legacy).unwrap();
    assert!(db
        .matching_revision(
            RecordNamespace::SourceService,
            &revision.source_key,
            &revision.document_json
        )
        .unwrap()
        .is_some());
    db.remove("legacy").unwrap();
    assert!(db
        .matching_revision(
            RecordNamespace::SourceService,
            &revision.source_key,
            &revision.document_json
        )
        .unwrap()
        .is_none());
    assert!(db
        .matching_revision(
            RecordNamespace::Ingestion,
            &revision.source_key,
            &revision.document_json
        )
        .unwrap()
        .is_some());
    assert!(db.read("modern").unwrap().is_some());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
