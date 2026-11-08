use super::super::tests::{publish, setup};
use super::*;
#[test]
fn sequential_pages_preserve_exact_original_spans_and_exclude_holdout() {
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "prepared").unwrap();
    let mut cursor = db
        .prepared_cursor(&prepared.snapshot.id, &|| Ok(()))
        .unwrap();
    assert_eq!(cursor.snapshot(), &prepared);
    assert!(prepared.chunk_count > 1);
    let mut restored = String::new();
    let mut seen = std::collections::HashSet::new();
    for position in 0..prepared.chunk_count {
        assert!(!cursor.is_complete());
        let page = db
            .prepared_page(&mut cursor, 1, 1024 * 1024, &|| Ok(()))
            .unwrap();
        assert_eq!(
            page.len(),
            1,
            "page {position} must contain the next reference chunk"
        );
        let entry = &page[0];
        assert!(seen.insert(entry.chunk.id.clone()));
        assert_eq!(entry.chunk.partition, Partition::Reference);
        assert_eq!(entry.episode.partition, Partition::Reference);
        assert_eq!(entry.original.reference, docs[0].reference);
        assert_eq!(entry.source_record, raw.records[0]);
        let span = &entry.chunk.spans[0];
        assert_eq!(&docs[0].text[span.start..span.end], entry.chunk.text);
        restored.push_str(&entry.chunk.text);
    }
    assert!(cursor.is_complete());
    assert_eq!(restored, docs[0].text);
    assert!(db
        .prepared_page(&mut cursor, 1, 1024, &|| Ok(()))
        .unwrap()
        .is_empty());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn denied_or_oversized_page_does_not_advance_and_checks_even_an_exhausted_cursor() {
    use std::cell::Cell;
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "prepared").unwrap();
    let mut cursor = db
        .prepared_cursor(&prepared.snapshot.id, &|| Ok(()))
        .unwrap();
    assert!(db.prepared_page(&mut cursor, 1, 1, &|| Ok(())).is_err());
    assert_eq!(cursor.position, 0);
    let calls = Cell::new(0);
    let denied = || {
        calls.set(calls.get() + 1);
        if calls.get() >= 3 {
            Err("expired".into())
        } else {
            Ok(())
        }
    };
    assert_eq!(
        db.prepared_page(&mut cursor, 2, 1024 * 1024, &denied)
            .err()
            .unwrap(),
        "expired"
    );
    assert_eq!(cursor.position, 0);
    let page = db
        .prepared_page(&mut cursor, 100, 1024 * 1024, &|| Ok(()))
        .unwrap();
    assert_eq!(page.len(), prepared.chunk_count);
    assert!(cursor.is_complete());
    assert_eq!(
        db.prepared_page(&mut cursor, 1, 1024, &|| Err("revoked".into()))
            .err()
            .unwrap(),
        "revoked"
    );
    assert!(db.connection.is_autocommit());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn a_cursor_cannot_survive_source_revocation_or_deleted_pending_chunks() {
    for revoke in [true, false] {
        let (mut db, dir, raw, docs) = setup();
        let prepared = publish(&mut db, &raw, &docs, "prepared").unwrap();
        let mut cursor = db
            .prepared_cursor(&prepared.snapshot.id, &|| Ok(()))
            .unwrap();
        db.prepared_page(&mut cursor, 1, 1024 * 1024, &|| Ok(()))
            .unwrap();
        if revoke {
            db.remove("record").unwrap();
        } else {
            db.connection
                .execute("DELETE FROM corpus_prepared_chunks WHERE position=1", [])
                .unwrap();
        }
        assert!(db
            .prepared_page(&mut cursor, 1, 1024 * 1024, &|| Ok(()))
            .is_err());
        assert_eq!(cursor.position, 1);
        assert!(db.connection.is_autocommit());
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn cursor_page_shares_originals_and_episodes_but_keeps_a_bounded_prefix() {
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "prepared").unwrap();
    let mut cursor = db
        .prepared_cursor(&prepared.snapshot.id, &|| Ok(()))
        .unwrap();
    let page = db
        .prepared_page(&mut cursor, 100, 1024 * 1024, &|| Ok(()))
        .unwrap();
    assert!(Arc::ptr_eq(&page[0].episode, &page[1].episode));
    assert!(Arc::ptr_eq(&page[0].original, &page[1].original));
    let first = &page[0];
    let budget = serde_json::to_string(&first.chunk).unwrap().len()
        + serde_json::to_string(first.episode.as_ref()).unwrap().len()
        + first.original.document_json.len()
        + first.original.source_key.len()
        + serde_json::to_string(&first.original.provenance)
            .unwrap()
            .len();
    let mut cursor = db
        .prepared_cursor(&prepared.snapshot.id, &|| Ok(()))
        .unwrap();
    let bounded = db
        .prepared_page(&mut cursor, 100, budget, &|| Ok(()))
        .unwrap();
    assert_eq!(bounded.len(), 1);
    assert_eq!(bounded[0].chunk, first.chunk);
    assert_eq!(cursor.position, 1);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn cursor_pins_episode_and_chunk_hashes_and_checks_guards_during_initial_scan() {
    use std::cell::Cell;
    for table in ["corpus_episodes", "corpus_prepared_chunks"] {
        let (mut db, dir, raw, docs) = setup();
        let prepared = publish(&mut db, &raw, &docs, "prepared").unwrap();
        let calls = Cell::new(0);
        assert_eq!(
            db.prepared_cursor(&prepared.snapshot.id, &|| {
                calls.set(calls.get() + 1);
                if calls.get() == 5 {
                    Err("stop scan".into())
                } else {
                    Ok(())
                }
            })
            .err()
            .unwrap(),
            "stop scan"
        );
        let mut cursor = db
            .prepared_cursor(&prepared.snapshot.id, &|| Ok(()))
            .unwrap();
        db.connection.execute_batch(&format!("DROP TRIGGER immutable_{table}; UPDATE {table} SET data_json='{{}}',canonical_sha256='changed';")).unwrap();
        assert!(db
            .prepared_page(&mut cursor, 100, 1024 * 1024, &|| Ok(()))
            .is_err());
        assert_eq!(cursor.position, 0);
        assert!(db.connection.is_autocommit());
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn index_publication_revalidates_finished_cursor_including_already_read_rows() {
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "prepared").unwrap();
    let mut cursor = db
        .prepared_cursor(&prepared.snapshot.id, &|| Ok(()))
        .unwrap();
    assert!(db.validate_prepared_cursor(&cursor, &|| Ok(())).is_err());
    db.prepared_page(&mut cursor, 100, 1024 * 1024, &|| Ok(()))
        .unwrap();
    db.validate_prepared_cursor(&cursor, &|| Ok(())).unwrap();
    db.connection
        .execute("DELETE FROM corpus_prepared_chunks WHERE position=0", [])
        .unwrap();
    assert!(db.validate_prepared_cursor(&cursor, &|| Ok(())).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[path = "corpus-preparation-selection-tests.rs"]
mod selection_tests;
