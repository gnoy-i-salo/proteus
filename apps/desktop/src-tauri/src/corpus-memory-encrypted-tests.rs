use super::*;

#[test]
fn encrypted_reopen_replays_memory_without_rewriting_originals() {
    let (mut db, dir) = setup();
    let (_, reference, snapshot, mut items) = single_bundle(&mut db, "encrypted-a");
    let original = db.revision(&reference).unwrap();
    let value: serde_json::Value = serde_json::from_str(&original.document_json).unwrap();
    items[0].source_spans[0].end = value["text"].as_str().unwrap().len();
    let expected = publish(&mut db, &snapshot, &items).unwrap();
    let version: String = db
        .connection
        .query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .unwrap();
    assert!(!version.is_empty());
    drop(db);
    let bytes = std::fs::read(dir.join("corpus.db")).unwrap();
    assert!(!bytes.starts_with(b"SQLite format 3"));
    for text in [original.document_json.as_bytes(), items[0].text.as_bytes()] {
        assert!(!bytes.windows(text.len()).any(|window| window == text));
    }
    assert!(CorpusDatabase::open(&dir.join("corpus.db"), &[32; 32]).is_err());
    for _ in 0..2 {
        let mut reopened = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
        assert_eq!(read(&reopened, &snapshot.id).unwrap(), expected);
        assert_eq!(publish(&mut reopened, &snapshot, &items).unwrap(), expected);
        assert_eq!(reopened.revision(&reference).unwrap(), original);
        drop(reopened);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn encrypted_source_revocation_denies_only_its_research_memory_after_restart() {
    let (mut db, dir) = setup();
    let (_, _, snapshot_a, items_a) = single_bundle(&mut db, "source-a");
    publish(&mut db, &snapshot_a, &items_a).unwrap();
    let (corpus_b, refs_b) = seed_source(
        &mut db,
        "source-b",
        &[("document-b", "Independent source B")],
    );
    let item_b = item_for("item-b", &corpus_b, &refs_b[0], "B");
    let snapshot_b = snapshot_for("snapshot-b", &corpus_b, vec![item_b.id.clone()]);
    let expected_b = publish(&mut db, &snapshot_b, &[item_b]).unwrap();
    db.remove("record-source-a").unwrap();
    assert!(read(&db, &snapshot_a.id).is_err());
    assert_eq!(read(&db, &snapshot_b.id).unwrap(), expected_b);
    drop(db);
    let reopened = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert!(read(&reopened, &snapshot_a.id).is_err());
    assert_eq!(read(&reopened, &snapshot_b.id).unwrap(), expected_b);
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}
