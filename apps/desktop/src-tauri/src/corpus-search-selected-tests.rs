use super::*;
use std::cell::Cell;
fn fixture() -> (Connection, PathBuf, SearchIndexReceipt) {
    let (mut db, dir) = setup();
    let chunks = vec![
        chunk(
            "a",
            "  Автор: ёж 👩🏽‍💻\r\n",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (Some(10), Some(10)),
        ),
        chunk(
            "b",
            "@@@ \t exact  ",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
        chunk(
            "foreign",
            "secret foreign",
            PERSONA_B,
            GRANT_B,
            BINDING_B,
            (None, None),
        ),
    ];
    let receipt = publish(&mut db, SNAPSHOT_A, SNAPSHOT_GEN, "selected", &chunks);
    (db, dir, receipt)
}
#[test]
fn selected_read_preserves_requested_rank_order_and_exact_nonlexical_utf8_bytes() {
    let (db, dir, receipt) = fixture();
    let ids = vec!["b".into(), "a".into()];
    let chunks = read_reference_chunks(&db, &scope_a(), &receipt, &ids, 1024, &|| Ok(())).unwrap();
    assert_eq!(
        chunks
            .iter()
            .map(|c| c.chunk_id.as_str())
            .collect::<Vec<_>>(),
        vec!["b", "a"]
    );
    assert_eq!(chunks[0].text, "@@@ \t exact  ");
    assert_eq!(chunks[1].text, "  Автор: ёж 👩🏽‍💻\r\n");
    assert!(chunks
        .iter()
        .all(|c| c.persona_id == PERSONA_A && c.grant_id == GRANT_A && c.binding_id == BINDING_A));
    assert!(
        read_reference_chunks(&db, &scope_a(), &receipt, &[], 1, &|| Ok(()))
            .unwrap()
            .is_empty()
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn selected_read_rejects_foreign_missing_or_out_of_period_ids_without_partial_success() {
    let (db, dir, receipt) = fixture();
    for id in ["foreign", "missing"] {
        assert!(read_reference_chunks(
            &db,
            &scope_a(),
            &receipt,
            &["a".into(), id.into()],
            1024,
            &|| Ok(())
        )
        .is_err());
    }
    let mut scope = scope_a();
    scope.period.include_undated = false;
    assert!(read_reference_chunks(
        &db,
        &scope,
        &receipt,
        &["a".into(), "b".into()],
        1024,
        &|| Ok(())
    )
    .is_err());
    scope.period.from = Some(11);
    assert!(read_reference_chunks(&db, &scope, &receipt, &["a".into()], 1024, &|| Ok(())).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn selected_read_limits_total_utf8_bytes_and_rejects_duplicate_or_unbounded_ids() {
    let (db, dir, receipt) = fixture();
    let text = "  Автор: ёж 👩🏽‍💻\r\n";
    assert!(read_reference_chunks(
        &db,
        &scope_a(),
        &receipt,
        &["a".into()],
        text.len() - 1,
        &|| Ok(())
    )
    .is_err());
    assert_eq!(
        read_reference_chunks(
            &db,
            &scope_a(),
            &receipt,
            &["a".into()],
            text.len(),
            &|| Ok(())
        )
        .unwrap()[0]
            .text,
        text
    );
    for ids in [
        vec!["a".into(), "a".into()],
        vec!["a".into(); 101],
        vec!["".into()],
        vec!["\n".into()],
    ] {
        assert!(read_reference_chunks(&db, &scope_a(), &receipt, &ids, 1024, &|| Ok(())).is_err());
    }
    assert!(read_reference_chunks(&db, &scope_a(), &receipt, &[], 0, &|| Ok(())).is_err());
    assert!(read_reference_chunks(
        &db,
        &scope_a(),
        &receipt,
        &[],
        MAX_SCAN_TEXT_BYTES + 1,
        &|| Ok(())
    )
    .is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn selected_read_pins_every_receipt_field_and_preserves_caller_transaction_on_late_cancel() {
    let (db, dir, receipt) = fixture();
    let mut stale = receipt.clone();
    stale.created_at = "999999999".into();
    assert!(
        read_reference_chunks(&db, &scope_a(), &stale, &["a".into()], 1024, &|| Ok(())).is_err()
    );
    let calls = Cell::new(0);
    read_reference_chunks(&db, &scope_a(), &receipt, &["a".into()], 1024, &|| {
        calls.set(calls.get() + 1);
        Ok(())
    })
    .unwrap();
    let final_call = calls.get();
    assert!(final_call >= 3);
    calls.set(0);
    db.execute_batch("BEGIN; CREATE TABLE selected_marker(value TEXT); INSERT INTO selected_marker VALUES('keep');").unwrap();
    let error = read_reference_chunks(&db, &scope_a(), &receipt, &["a".into()], 1024, &|| {
        calls.set(calls.get() + 1);
        if calls.get() == final_call {
            Err("cancel exact read  ".into())
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error, "cancel exact read  ");
    assert!(!db.is_autocommit());
    assert_eq!(
        db.query_row("SELECT value FROM selected_marker", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    db.execute_batch("ROLLBACK").unwrap();
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
