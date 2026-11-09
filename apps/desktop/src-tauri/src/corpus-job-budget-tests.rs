use super::super::{Acquisition, CorpusDatabase, RecordInput, RevisionInput, SourceProvenance};
use super::RECORD_BYTES;

fn database() -> (CorpusDatabase, std::path::PathBuf) {
    let directory =
        std::env::temp_dir().join(format!("proteus-byte-budget-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&directory).unwrap();
    let db = CorpusDatabase::open(&directory.join("corpus.db"), &[31; 32]).unwrap();
    (db, directory)
}

#[test]
fn byte_budget_does_not_materialize_retained_payload_columns() {
    let (db, directory) = database();
    let query = format!("EXPLAIN SELECT {RECORD_BYTES} FROM records WHERE records.id=?1");
    let mut statement = db.connection.prepare(&query).unwrap();
    let instructions = statement
        .query_map(["record"], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    // SQLite's OP_Column byte-length flag permits header-only access, avoiding
    // repeated overflow-page reads of every previously committed document.
    // Inspect the actual bundled SQLCipher plan, not a duplicate query fixture.
    for (table, columns) in [
        ("records", &["state_json"][..]),
        ("revisions", &["document_json", "provenance_json"][..]),
    ] {
        let root: i64 = db
            .connection
            .query_row(
                "SELECT rootpage FROM sqlite_schema WHERE name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        let cursor = instructions
            .iter()
            .find(|(op, _, page, _)| op == "OpenRead" && *page == root)
            .unwrap()
            .1;
        for column in columns {
            let index: i64 = db
                .connection
                .query_row(
                    "SELECT cid FROM pragma_table_info(?1) WHERE name=?2",
                    [table, column],
                    |row| row.get(0),
                )
                .unwrap();
            let reads: Vec<_> = instructions
                .iter()
                .filter(|(op, owner, field, _)| {
                    op == "Column" && *owner == cursor && *field == index
                })
                .collect();
            assert!(!reads.is_empty(), "missing {table}.{column} read");
            assert!(
                reads.iter().all(|(_, _, _, flags)| flags & 0xc0 == 0xc0),
                "{table}.{column} loads the payload instead of reading its byte length: {reads:?}"
            );
        }
    }
    drop(statement);
    drop(db);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn byte_budget_preserves_exact_utf8_sizes_and_empty_revision_sets() {
    let (mut db, directory) = database();
    let text = "  Автор e\u{301} 😀\r\n\0exact  ";
    let mut record = RecordInput {
        id: text.into(),
        kind: "grant".into(),
        provider: "telegram".into(),
        state_json: serde_json::json!({"source": text}).to_string(),
        documents: vec![],
    };
    let base_bytes =
        record.id.len() + record.provider.len() + record.state_json.len() + "sourceService".len();
    db.save(&record).unwrap();
    let query = format!("SELECT {RECORD_BYTES} FROM records WHERE id=?1");
    let measure = |db: &CorpusDatabase| {
        db.connection
            .query_row::<i64, _, _>(&query, [text], |row| row.get(0))
            .unwrap()
    };
    assert_eq!(measure(&db), base_bytes as i64);
    let document = RevisionInput {
        source_key: text.into(),
        document_id: text.into(),
        document_json: serde_json::json!({"text": text.repeat(4096)}).to_string(),
        provenance: SourceProvenance::source_service(Acquisition::Api),
    };
    let expected = base_bytes
        + document.source_key.len()
        + document.document_id.len()
        + document.document_json.len()
        + serde_json::to_string(&document.provenance).unwrap().len();
    record.documents.push(document);
    db.save(&record).unwrap();
    assert_eq!(measure(&db), expected as i64);
    assert_eq!(db.read(text).unwrap().unwrap().input, record);
    drop(db);
    std::fs::remove_dir_all(directory).unwrap();
}
