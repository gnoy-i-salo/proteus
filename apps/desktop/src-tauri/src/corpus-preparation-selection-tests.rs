use super::*;
use std::cell::Cell;

#[test]
fn selected_prepared_entries_keep_order_exact_spans_and_shared_originals_without_advancing() {
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "selected").unwrap();
    let cursor = db
        .prepared_cursor(&prepared.snapshot.id, &|| Ok(()))
        .unwrap();
    let ids: Vec<_> = cursor
        .chunks
        .iter()
        .rev()
        .map(|(id, _)| id.clone())
        .collect();
    let entries = db
        .prepared_entries(&cursor, &ids, 1024 * 1024, &|| Ok(()))
        .unwrap();
    assert_eq!(entries.len(), ids.len());
    assert!(entries.len() > 1);
    for (entry, id) in entries.iter().zip(&ids) {
        assert_eq!(&entry.chunk.id, id);
        assert_eq!(entry.chunk.episode_id, entry.episode.id);
        assert_eq!(entry.episode.partition, Partition::Reference);
        assert_eq!(entry.source_record, raw.records[0]);
        let span = &entry.chunk.spans[0];
        assert_eq!(entry.chunk.text, docs[0].text[span.start..span.end]);
    }
    assert!(Arc::ptr_eq(&entries[0].episode, &entries[1].episode));
    assert!(Arc::ptr_eq(&entries[0].original, &entries[1].original));
    assert_eq!(cursor.position, 0);
    assert!(db
        .prepared_entries(&cursor, &[], 1, &|| Ok(()))
        .unwrap()
        .is_empty());
    drop(entries);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn selected_entries_reject_partial_budget_missing_duplicate_and_corrupt_references() {
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "selected").unwrap();
    let cursor = db
        .prepared_cursor(&prepared.snapshot.id, &|| Ok(()))
        .unwrap();
    let first = cursor.chunks[0].0.clone();
    let one = db
        .prepared_entries(&cursor, std::slice::from_ref(&first), 1024 * 1024, &|| {
            Ok(())
        })
        .unwrap();
    let entry = &one[0];
    let budget = serde_json::to_string(&entry.chunk).unwrap().len()
        + serde_json::to_string(entry.episode.as_ref()).unwrap().len()
        + entry.original.document_json.len()
        + entry.original.source_key.len()
        + serde_json::to_string(&entry.original.provenance)
            .unwrap()
            .len();
    assert!(db
        .prepared_entries(
            &cursor,
            &[first.clone(), cursor.chunks[1].0.clone()],
            budget,
            &|| Ok(())
        )
        .is_err());
    for ids in [
        vec![first.clone(), "absent".into()],
        vec![first.clone(), first.clone()],
        vec![first.clone(); 101],
    ] {
        assert!(db
            .prepared_entries(&cursor, &ids, 1024 * 1024, &|| Ok(()))
            .is_err());
    }
    // Simulate damaged storage explicitly; normal writes are immutable.
    db.connection
        .execute_batch("DROP TRIGGER immutable_corpus_prepared_chunks;")
        .unwrap();
    db.connection
        .execute(
            "UPDATE corpus_prepared_chunks SET data_json='{}' WHERE chunk_id=?1",
            [&first],
        )
        .unwrap();
    assert!(db
        .prepared_entries(&cursor, &[first], 1024 * 1024, &|| Ok(()))
        .is_err());
    assert_eq!(cursor.position, 0);
    drop(one);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn episode_references_are_bounded_exact_and_reference_only() {
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "selected").unwrap();
    let cursor = db
        .prepared_cursor(&prepared.snapshot.id, &|| Ok(()))
        .unwrap();
    for id in &prepared.episode_ids {
        let episode = db.prepared_episode(&prepared.snapshot.id, id).unwrap();
        let result = db.prepared_episode_references(&cursor, id, 100, 1024 * 1024, &|| Ok(()));
        if episode.partition == Partition::Holdout {
            assert!(result.is_err());
        } else {
            let result = result.unwrap();
            assert_eq!(*result.episode, episode);
            assert_eq!(
                result.chunk_ids,
                cursor
                    .chunks
                    .iter()
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>()
            );
            assert!(db
                .prepared_episode_references(&cursor, id, 1, 1024 * 1024, &|| Ok(()))
                .is_err());
            assert!(db
                .prepared_episode_references(&cursor, id, 100, 1, &|| Ok(()))
                .is_err());
        }
    }
    assert!(db
        .prepared_episode_references(&cursor, "foreign", 100, 1024 * 1024, &|| Ok(()))
        .is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn selected_prepared_reads_reject_late_cancellation_and_revocation() {
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "selected").unwrap();
    let cursor = db
        .prepared_cursor(&prepared.snapshot.id, &|| Ok(()))
        .unwrap();
    let ids = vec![cursor.chunks[0].0.clone()];
    let calls = Cell::new(0);
    db.prepared_entries(&cursor, &ids, 1024 * 1024, &|| {
        calls.set(calls.get() + 1);
        Ok(())
    })
    .unwrap();
    let last = calls.get();
    calls.set(0);
    let error = db
        .prepared_entries(&cursor, &ids, 1024 * 1024, &|| {
            calls.set(calls.get() + 1);
            if calls.get() == last {
                Err("cancel exact  ".into())
            } else {
                Ok(())
            }
        })
        .err()
        .unwrap();
    assert_eq!(error, "cancel exact  ");
    assert!(db.connection.is_autocommit());
    db.remove("record").unwrap();
    assert!(db
        .prepared_entries(&cursor, &ids, 1024 * 1024, &|| Ok(()))
        .is_err());
    assert!(db
        .prepared_episode_references(&cursor, &prepared.episode_ids[0], 100, 1024 * 1024, &|| Ok(
            ()
        ))
        .is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn episode_reference_lookup_does_not_hide_deleted_chunks() {
    let (mut db, dir, raw, docs) = setup();
    let prepared = publish(&mut db, &raw, &docs, "selected").unwrap();
    let cursor = db
        .prepared_cursor(&prepared.snapshot.id, &|| Ok(()))
        .unwrap();
    let first = db
        .prepared_entries(&cursor, &[cursor.chunks[0].0.clone()], 1024 * 1024, &|| {
            Ok(())
        })
        .unwrap();
    let episode_id = first[0].episode.id.clone();
    db.connection
        .execute(
            "DELETE FROM corpus_prepared_chunks WHERE chunk_id=?1",
            [&cursor.chunks[0].0],
        )
        .unwrap();
    assert!(
        db.prepared_episode_references(&cursor, &episode_id, 100, 1024 * 1024, &|| Ok(()))
            .is_err(),
        "missing chunk returned a partial episode"
    );
    drop(first);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
