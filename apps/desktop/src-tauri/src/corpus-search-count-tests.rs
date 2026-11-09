//! Regression tests for metadata-only scoped counting over the corpus FTS
//! index. The fixture is synthetic, exactly like the shared search fixtures.
//! Counts must equal what a full keyset scan of the same scope sees, must not
//! read indexed text, must pin the expected publication identity, and must
//! treat an authorized-but-empty scope as a truthful zero.
// Reuse the existing FTS fixture owner, as receipt/scan tests do.
use super::super::types::MAX_LIST_ITEMS;
use super::*;
use std::cell::Cell;

fn open_unmigrated_database() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .pragma_update(
            None,
            "key",
            "x'000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f'",
        )
        .unwrap();
    connection
        .execute_batch(
            "PRAGMA cipher_memory_security=ON; PRAGMA temp_store=MEMORY; \
             PRAGMA foreign_keys=ON; PRAGMA secure_delete=ON; PRAGMA trusted_schema=OFF;",
        )
        .unwrap();
    // Only the base corpus schema: the search schema is intentionally absent,
    // so counting must fail closed instead of lazy-migrating.
    connection
        .execute_batch(include_str!("corpus-schema.sql"))
        .unwrap();
    connection
}

fn count_scope(
    connection: &Connection,
    scope: &SearchScope,
    receipt: &SearchIndexReceipt,
) -> Result<SearchScopeCounts, String> {
    count_reference_documents(connection, scope, receipt, &|| Ok(()))
}

/// Walks every keyset page of the same scope; the scan is the enumeration
/// owner the counts are cross-checked against.
fn scan_all_chunks(
    connection: &Connection,
    scope: &SearchScope,
    receipt: &SearchIndexReceipt,
    limit: usize,
) -> Vec<ReferenceChunk> {
    let mut chunks = Vec::new();
    let mut after = None;
    loop {
        let page = scan_reference_chunks(
            connection,
            scope,
            receipt,
            after.as_deref(),
            limit,
            MAX_SCAN_TEXT_BYTES,
            &|| Ok(()),
        )
        .unwrap();
        let done = !page.has_more;
        chunks.extend(page.chunks);
        if done {
            return chunks;
        }
        after = page.next_cursor;
    }
}

#[test]
fn count_collapses_shared_document_revision_span_across_bindings() {
    let (mut connection, directory) = setup();
    let text = "  Общий договор é\r\n👩‍💻  ";
    let mut chunks = vec![
        chunk(
            "cb:1",
            text,
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (Some(1), Some(1)),
        ),
        chunk(
            "cb:2",
            text,
            PERSONA_A,
            GRANT_B,
            BINDING_B,
            (Some(1), Some(1)),
        ),
        chunk(
            "cb:3",
            text,
            PERSONA_A,
            GRANT_C,
            BINDING_C,
            (Some(1), Some(1)),
        ),
    ];
    for input in &mut chunks {
        input.document_id = "shared:document".into();
        input.revision_id = "shared:revision".into();
    }
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "shared:count",
        &chunks,
    );
    let mut wide = scope_a();
    wide.grant_ids = vec![GRANT_A.into(), GRANT_B.into(), GRANT_C.into()];
    wide.binding_ids = vec![BINDING_A.into(), BINDING_B.into(), BINDING_C.into()];
    let counts = count_scope(&connection, &wide, &receipt).unwrap();
    assert_eq!(counts.snapshot_id, SNAPSHOT_A);
    assert_eq!(counts.snapshot_generation, SNAPSHOT_GEN);
    assert_eq!(counts.index_generation, receipt.index_generation);
    // Three chunks over one document, one revision, one span: only the
    // chunk count sees the shared-span rows.
    assert_eq!(counts.document_count, 1);
    assert_eq!(counts.revision_count, 1);
    assert_eq!(counts.chunk_count, 3);
    // Narrowing the scope to one binding leaves a single chunk of the same
    // document: all three counts collapse to one.
    let narrow = count_scope(&connection, &scope_a(), &receipt).unwrap();
    assert_eq!(narrow.document_count, 1);
    assert_eq!(narrow.revision_count, 1);
    assert_eq!(narrow.chunk_count, 1);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn count_splits_multi_span_documents_and_multiple_revisions_truthfully() {
    let (mut connection, directory) = setup();
    let mut first = chunk(
        "ms:1",
        "первая часть",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    );
    let mut second = chunk(
        "ms:2",
        "вторая часть",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    );
    for input in [&mut first, &mut second] {
        input.document_id = "document:multi-span".into();
        input.revision_id = "revision:one".into();
    }
    // Two spans of one revision, then a second revision of the same document.
    let mut third = chunk(
        "ms:3",
        "исправленный текст",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    );
    third.document_id = "document:multi-span".into();
    third.revision_id = "revision:two".into();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "multi-span",
        &[first, second, third],
    );
    let counts = count_scope(&connection, &scope_a(), &receipt).unwrap();
    assert_eq!(counts.document_count, 1);
    assert_eq!(counts.revision_count, 2);
    assert_eq!(counts.chunk_count, 3);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn count_scope_filters_match_search_and_scan_row_selection() {
    let (mut connection, directory) = setup();
    let receipt_a = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let receipt_b = publish(
        &mut connection,
        SNAPSHOT_B,
        1,
        "stage:b",
        &[snapshot_b_chunk()],
    );
    // Persona A scope covers chunks 1, 2, 5 and 6 of the fixture.
    let counts = count_scope(&connection, &scope_a(), &receipt_a).unwrap();
    assert_eq!(counts.document_count, 4);
    assert_eq!(counts.revision_count, 4);
    assert_eq!(counts.chunk_count, 4);
    // Persona B scope sees only its own chunk.
    let mut scope_b = scope_a();
    scope_b.persona_id = PERSONA_B.into();
    scope_b.grant_ids = vec![GRANT_B.into()];
    scope_b.binding_ids = vec![BINDING_B.into()];
    let counts_b = count_scope(&connection, &scope_b, &receipt_a).unwrap();
    assert_eq!(
        (
            counts_b.document_count,
            counts_b.revision_count,
            counts_b.chunk_count
        ),
        (1, 1, 1)
    );
    // Selected time window keeps the two dated chunks inside it.
    let mut dated = scope_a();
    dated.period = SearchPeriod {
        from: Some(1672531200),
        to: Some(1675209600),
        include_undated: false,
    };
    let counts_dated = count_scope(&connection, &dated, &receipt_a).unwrap();
    assert_eq!(counts_dated.chunk_count, 2);
    assert_eq!(counts_dated.document_count, 2);
    // The window matching the 2022 fixture date counts that chunk alone;
    // counts are query-independent, so the nonlexical text still counts.
    let mut dated_window = scope_a();
    dated_window.period = SearchPeriod {
        from: Some(1654041600),
        to: Some(1654041600),
        include_undated: false,
    };
    let counts_dated_window = count_scope(&connection, &dated_window, &receipt_a).unwrap();
    assert_eq!(
        (
            counts_dated_window.document_count,
            counts_dated_window.revision_count,
            counts_dated_window.chunk_count
        ),
        (1, 1, 1)
    );
    // The same window with undated chunks included sees the undated chunk too.
    dated_window.period.include_undated = true;
    let counts_with_undated = count_scope(&connection, &dated_window, &receipt_a).unwrap();
    assert_eq!(counts_with_undated.chunk_count, 2);
    assert_eq!(counts_with_undated.document_count, 2);
    // A window matching nothing is a truthful zero, not an error.
    let mut empty_window = scope_a();
    empty_window.period = SearchPeriod {
        from: Some(1600000000),
        to: Some(1600000000),
        include_undated: false,
    };
    let counts_empty = count_scope(&connection, &empty_window, &receipt_a).unwrap();
    assert_eq!(
        (
            counts_empty.document_count,
            counts_empty.revision_count,
            counts_empty.chunk_count
        ),
        (0, 0, 0)
    );
    // Foreign grants inside the snapshot never widen the count.
    let mut foreign = scope_a();
    foreign.grant_ids = vec![GRANT_C.into()];
    foreign.binding_ids = vec![BINDING_C.into()];
    let counts_foreign = count_scope(&connection, &foreign, &receipt_a).unwrap();
    assert_eq!(
        (
            counts_foreign.document_count,
            counts_foreign.revision_count,
            counts_foreign.chunk_count
        ),
        (0, 0, 0)
    );
    // Snapshot B is isolated but countable with its own receipt.
    let mut scope_b_snapshot = scope_a();
    scope_b_snapshot.snapshot_id = SNAPSHOT_B.into();
    scope_b_snapshot.grant_ids = vec![GRANT_C.into()];
    scope_b_snapshot.binding_ids = vec![BINDING_C.into()];
    let counts_snapshot_b = count_scope(&connection, &scope_b_snapshot, &receipt_b).unwrap();
    assert_eq!(counts_snapshot_b.chunk_count, 1);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn count_matches_scan_distinct_documents_over_a_full_keyset_walk() {
    let (mut connection, directory) = setup();
    let mut first = chunk(
        "w:1",
        "первая часть",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    );
    let mut second = chunk(
        "w:2",
        "вторая часть",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    );
    for input in [&mut first, &mut second] {
        input.document_id = "document:walk".into();
        input.revision_id = "revision:walk".into();
    }
    let third = chunk(
        "w:3",
        "второй документ",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    );
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "walk",
        &[first, second, third],
    );
    let counts = count_scope(&connection, &scope_a(), &receipt).unwrap();
    // One-row pages force a keyset walk across every chunk.
    let rows = scan_all_chunks(&connection, &scope_a(), &receipt, 1);
    assert_eq!(counts.chunk_count, rows.len() as i64);
    let documents = rows
        .iter()
        .map(|row| row.document_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let revisions = rows
        .iter()
        .map(|row| (row.document_id.as_str(), row.revision_id.as_str()))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(counts.document_count, documents.len() as i64);
    assert_eq!(counts.revision_count, revisions.len() as i64);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn count_reports_truthful_zero_for_empty_publication_and_empty_matches() {
    let (mut connection, directory) = setup();
    begin_stage(&mut connection, &stage_input("stage:empty")).unwrap();
    // A reference-empty corpus publishes with a zero-chunk manifest; counting
    // it is a truthful zero, exactly like searching it.
    let empty_receipt = commit_stage(
        &mut connection,
        "stage:empty",
        &content_manifest::<&str, &str>(&[]),
    )
    .unwrap();
    assert_eq!(empty_receipt.chunk_count, 0);
    let counts = count_scope(&connection, &scope_a(), &empty_receipt).unwrap();
    assert_eq!(counts.index_generation, empty_receipt.index_generation);
    assert_eq!(
        (
            counts.document_count,
            counts.revision_count,
            counts.chunk_count
        ),
        (0, 0, 0)
    );
    // A valid scope that matches no rows in a published snapshot is data too.
    let receipt_b = publish(
        &mut connection,
        SNAPSHOT_B,
        1,
        "stage:b",
        &[snapshot_b_chunk()],
    );
    let mut no_match = scope_a();
    no_match.snapshot_id = SNAPSHOT_B.into();
    no_match.grant_ids = vec![GRANT_A.into()];
    no_match.binding_ids = vec![BINDING_A.into()];
    let counts_no_match = count_scope(&connection, &no_match, &receipt_b).unwrap();
    assert_eq!(
        (
            counts_no_match.document_count,
            counts_no_match.revision_count,
            counts_no_match.chunk_count
        ),
        (0, 0, 0)
    );
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn count_chunk_count_is_scoped_while_receipt_chunk_count_stays_generation_global() {
    let (mut connection, directory) = setup();
    // Four persona A chunks and one persona B chunk publish one generation.
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    assert_eq!(receipt.chunk_count, 5);
    let scoped = count_scope(&connection, &scope_a(), &receipt).unwrap();
    assert_eq!(scoped.chunk_count, 4);
    // The scoped chunk count is not the generation-global manifest pin.
    assert_ne!(scoped.chunk_count, receipt.chunk_count);
    let mut scope_b = scope_a();
    scope_b.persona_id = PERSONA_B.into();
    scope_b.grant_ids = vec![GRANT_B.into()];
    scope_b.binding_ids = vec![BINDING_B.into()];
    let scoped_b = count_scope(&connection, &scope_b, &receipt).unwrap();
    assert_eq!(scoped_b.chunk_count, 1);
    // The receipt itself still reports the generation-global number.
    assert_eq!(
        active_receipt(&connection, SNAPSHOT_A).unwrap(),
        Some(receipt.clone())
    );
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn count_rejects_stale_pinned_receipt_after_rebuild_prune_and_deletion() {
    let (mut connection, directory) = setup();
    let first = vec![chunk(
        "chunk:1",
        "первая публикация",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    )];
    let first_receipt = publish(&mut connection, SNAPSHOT_A, SNAPSHOT_GEN, "stage:1", &first);
    let mut second = first.clone();
    second.push(chunk(
        "chunk:2",
        "вторая публикация",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    ));
    let second_receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:2",
        &second,
    );
    assert_ne!(first_receipt, second_receipt);
    prune_generations(&mut connection, SNAPSHOT_A).unwrap();
    // The old publication identity is gone after the rebuild+prune: the pin
    // is an error, never a stale count.
    let stale = count_scope(&connection, &scope_a(), &first_receipt).unwrap_err();
    assert_eq!(
        stale,
        "Квитанция индекса не соответствует активной публикации; обновите область поиска."
    );
    let counts = count_scope(&connection, &scope_a(), &second_receipt).unwrap();
    assert_eq!(counts.chunk_count, 2);
    // A deleted snapshot never serves its obsolete index.
    connection
        .execute("DELETE FROM snapshots WHERE id=?1", [SNAPSHOT_A])
        .unwrap();
    let error = count_scope(&connection, &scope_a(), &second_receipt).unwrap_err();
    assert_eq!(error, "Снимок корпуса не найден.");
    // An unpublished snapshot has no active receipt at all.
    let unpublished_scope = SearchScope {
        snapshot_id: SNAPSHOT_B.into(),
        ..scope_a()
    };
    let unpublished_receipt = SearchIndexReceipt {
        snapshot_id: SNAPSHOT_B.into(),
        snapshot_generation: 1,
        index_generation: 1,
        policy_fingerprint: POLICY.into(),
        content_sha256: EMPTY_CONTENT_SHA256.into(),
        chunk_count: 0,
        created_at: "2026-11-07T00:00:00Z".into(),
    };
    let error = count_scope(&connection, &unpublished_scope, &unpublished_receipt).unwrap_err();
    assert_eq!(error, "Индекс для снимка корпуса не опубликован.");
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn count_rejects_advanced_snapshot_and_scope_receipt_mismatch() {
    let (mut connection, directory) = setup();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &[chunk(
            "chunk:1",
            "текст",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        )],
    );
    // The corpus lifecycle marks snapshots immutable; drop only this fixture
    // trigger so the count-side reaction can be exercised synthetically.
    connection
        .execute_batch("DROP TRIGGER immutable_snapshots")
        .unwrap();
    connection
        .execute(
            "UPDATE snapshots SET generation=2 WHERE id=?1",
            [SNAPSHOT_A],
        )
        .unwrap();
    let error = count_scope(&connection, &scope_a(), &receipt).unwrap_err();
    assert!(
        error.contains("Снимок") || error.contains("Поколение"),
        "unexpected error for advanced snapshot: {error}"
    );
    // A scope describing another snapshot is rejected before any comparison,
    // exactly like the scan owner does.
    let foreign_snapshot = SearchScope {
        snapshot_id: SNAPSHOT_B.into(),
        ..scope_a()
    };
    let error = count_scope(&connection, &foreign_snapshot, &receipt).unwrap_err();
    assert_eq!(
        error,
        "Область подсчёта должна указывать снимок квитанции индекса."
    );
    let drifted_generation = SearchScope {
        snapshot_generation: 2,
        ..scope_a()
    };
    let error = count_scope(&connection, &drifted_generation, &receipt).unwrap_err();
    assert_eq!(
        error,
        "Область подсчёта должна указывать снимок квитанции индекса."
    );
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn count_ignores_corrupted_and_missing_indexed_text() {
    let (mut connection, directory) = setup();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &[
            chunk("a", "ок", PERSONA_A, GRANT_A, BINDING_A, (None, None)),
            chunk("b", "ещё", PERSONA_A, GRANT_A, BINDING_A, (None, None)),
        ],
    );
    let baseline = count_scope(&connection, &scope_a(), &receipt).unwrap();
    assert_eq!(baseline.chunk_count, 2);
    // Oversized damaged text breaks every text reader...
    connection
        .execute(
            "UPDATE search_fts SET text=?1",
            ["x".repeat(MAX_CHUNK_BYTES + 1)],
        )
        .unwrap();
    assert!(scan_reference_chunks(
        &connection,
        &scope_a(),
        &receipt,
        None,
        MAX_SCAN_LIMIT,
        MAX_SCAN_TEXT_BYTES,
        &|| Ok(())
    )
    .is_err());
    // ...but the metadata count is unchanged: no indexed text is read.
    let counts = count_scope(&connection, &scope_a(), &receipt).unwrap();
    assert_eq!(counts, baseline);
    // Even with every indexed text row gone the metadata count holds.
    connection.execute("DELETE FROM search_fts", []).unwrap();
    assert!(scan_reference_chunks(
        &connection,
        &scope_a(),
        &receipt,
        None,
        MAX_SCAN_LIMIT,
        MAX_SCAN_TEXT_BYTES,
        &|| Ok(())
    )
    .is_err());
    let counts = count_scope(&connection, &scope_a(), &receipt).unwrap();
    assert_eq!(counts, baseline);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn count_checks_cancellation_before_work_and_after_aggregate() {
    let (mut connection, directory) = setup();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &[
            chunk("a", "раз", PERSONA_A, GRANT_A, BINDING_A, (None, None)),
            chunk("b", "два", PERSONA_A, GRANT_A, BINDING_A, (None, None)),
            chunk("c", "три", PERSONA_A, GRANT_A, BINDING_A, (None, None)),
        ],
    );
    let calls = Cell::new(0);
    let cancel_at = |failing: usize| {
        let calls = &calls;
        move || -> Result<(), String> {
            calls.set(calls.get() + 1);
            if calls.get() == failing {
                Err(format!("подсчёт отменён на вызове {failing}"))
            } else {
                Ok(())
            }
        }
    };
    // Before any work: the first check failure stops the count untouched.
    let error =
        count_reference_documents(&connection, &scope_a(), &receipt, &cancel_at(1)).unwrap_err();
    assert_eq!(error, "подсчёт отменён на вызове 1");
    assert_eq!(calls.get(), 1);
    // After the aggregate: the pinned snapshot is still released and the
    // error is preserved.
    calls.set(0);
    let error =
        count_reference_documents(&connection, &scope_a(), &receipt, &cancel_at(2)).unwrap_err();
    assert_eq!(error, "подсчёт отменён на вызове 2");
    assert_eq!(calls.get(), 2);
    // Success runs the check exactly twice: before work and after the
    // aggregate.
    calls.set(0);
    let counts =
        count_reference_documents(&connection, &scope_a(), &receipt, &cancel_at(0)).unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(counts.chunk_count, 3);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn count_releases_savepoint_on_error_and_preserves_nested_caller_transaction() {
    let (mut connection, directory) = setup();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &[chunk(
            "chunk:1",
            "0123456789",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        )],
    );
    let calls = Cell::new(0);
    let fail_after_aggregate = || -> Result<(), String> {
        calls.set(calls.get() + 1);
        if calls.get() == 2 {
            return Err("подсчёт отменён после агрегирования".into());
        }
        Ok(())
    };
    // A failed count on an autocommit connection leaves no open transaction.
    assert!(
        count_reference_documents(&connection, &scope_a(), &receipt, &fail_after_aggregate)
            .is_err()
    );
    connection
        .execute_batch("BEGIN; CREATE TABLE leak_probe(n INTEGER); COMMIT;")
        .expect("no transaction may leak from a failed count");
    // The same failure nested in a caller transaction preserves that
    // transaction: the caller's writes before and after the failed count
    // commit together, and the savepoint is fully released.
    connection.execute_batch("BEGIN").unwrap();
    connection
        .execute("CREATE TABLE caller_marker(n INTEGER)", [])
        .unwrap();
    calls.set(0);
    assert!(
        count_reference_documents(&connection, &scope_a(), &receipt, &fail_after_aggregate)
            .is_err()
    );
    connection
        .execute("INSERT INTO caller_marker VALUES (1)", [])
        .unwrap();
    connection.execute_batch("COMMIT").unwrap();
    let count: i64 = connection
        .query_row("SELECT count(*) FROM caller_marker", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    // The connection keeps serving counts after the nested failure.
    let counts = count_scope(&connection, &scope_a(), &receipt).unwrap();
    assert_eq!(counts.chunk_count, 1);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn count_rejects_invalid_scopes_and_unmigrated_databases_before_any_work() {
    let (mut connection, directory) = setup();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &[chunk(
            "chunk:1",
            "текст",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        )],
    );
    // Invalid scopes are rejected by the shared owner, like every reader.
    let mut no_grants = scope_a();
    no_grants.grant_ids.clear();
    let error = count_scope(&connection, &no_grants, &receipt).unwrap_err();
    assert_eq!(
        error,
        "Область поиска должна явно указывать персону, гранты и привязки."
    );
    let mut no_bindings = scope_a();
    no_bindings.binding_ids.clear();
    assert!(count_scope(&connection, &no_bindings, &receipt).is_err());
    let mut oversized = scope_a();
    oversized.grant_ids = (0..=MAX_LIST_ITEMS)
        .map(|index| format!("grant:overflow-{index}"))
        .collect();
    assert!(count_scope(&connection, &oversized, &receipt).is_err());
    let mut inverted = scope_a();
    inverted.period = SearchPeriod {
        from: Some(2),
        to: Some(1),
        include_undated: true,
    };
    assert_eq!(
        count_scope(&connection, &inverted, &receipt).unwrap_err(),
        "Некорректный период области поиска."
    );
    let mut imprecise = scope_a();
    imprecise.period = SearchPeriod {
        from: Some(MAX_JS_SAFE_INTEGER + 1),
        to: None,
        include_undated: true,
    };
    assert!(count_scope(&connection, &imprecise, &receipt).is_err());
    // An unmigrated database fails closed before any comparison.
    let unmigrated = open_unmigrated_database();
    assert!(count_scope(&unmigrated, &scope_a(), &receipt).is_err());
    drop(unmigrated);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn count_rejects_structurally_invalid_receipts_before_any_comparison() {
    let (mut connection, directory) = setup();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let mut tampered = receipt.clone();
    tampered.chunk_count = -1;
    assert_eq!(
        count_scope(&connection, &scope_a(), &tampered).unwrap_err(),
        "Недопустимая квитанция поискового индекса."
    );
    let mut tampered = receipt.clone();
    tampered.index_generation = 0;
    assert!(count_scope(&connection, &scope_a(), &tampered).is_err());
    let mut tampered = receipt.clone();
    tampered.created_at.clear();
    assert!(count_scope(&connection, &scope_a(), &tampered).is_err());
    // Every identity field still pins the publication exactly.
    for mut tampered in [
        {
            let mut value = receipt.clone();
            value.snapshot_id = SNAPSHOT_B.into();
            value
        },
        {
            let mut value = receipt.clone();
            value.snapshot_generation = 2;
            value
        },
        {
            let mut value = receipt.clone();
            value.index_generation = 99;
            value
        },
        {
            let mut value = receipt.clone();
            value.content_sha256 = EMPTY_CONTENT_SHA256.into();
            value
        },
    ] {
        assert!(count_scope(&connection, &scope_a(), &tampered).is_err());
        tampered.policy_fingerprint = "tip8-split/2|corpus-search/1".into();
        assert!(count_scope(&connection, &scope_a(), &tampered).is_err());
    }
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn count_reads_old_generation_completely_inside_a_held_reader_pin() {
    let (mut connection, directory) = setup();
    let receipt_one = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let path = directory.join("corpus.db");
    let reader = open_database(&path);
    // Controlled synchronization: the caller pins a WAL read snapshot at
    // generation 1 before the writer publishes generation 2 and prunes
    // generation 1 while the reader snapshot is held.
    reader.execute_batch("SAVEPOINT reader_pin").unwrap();
    // The first read materializes the WAL snapshot at generation 1 before the
    // writer publishes generation 2 and prunes generation 1.
    let pinned_rows: i64 = reader
        .query_row("SELECT COUNT(*) FROM search_chunks", [], |row| row.get(0))
        .unwrap();
    assert!(pinned_rows > 0);
    let mut rebuilt = fixture_chunks();
    rebuilt[0].text = "Поколение два: Марья перенесла встречу, субботу не отменяем.".into();
    let receipt_two = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:2",
        &rebuilt,
    );
    prune_generations(&mut connection, SNAPSHOT_A).unwrap();
    assert_ne!(receipt_one, receipt_two);
    // Inside the pinned snapshot the old receipt is still the active
    // publication: the count observes generation 1 completely, after the
    // writer already pruned it.
    let counts = count_scope(&reader, &scope_a(), &receipt_one).unwrap();
    assert_eq!(counts.index_generation, receipt_one.index_generation);
    assert_eq!(counts.chunk_count, 4);
    // The newer receipt does not match the pinned snapshot's publication.
    assert!(count_scope(&reader, &scope_a(), &receipt_two).is_err());
    // After the caller releases the pin, the old receipt is obsolete and only
    // the current receipt counts.
    reader
        .execute_batch("RELEASE SAVEPOINT reader_pin")
        .unwrap();
    let error = count_scope(&reader, &scope_a(), &receipt_one).unwrap_err();
    assert_eq!(
        error,
        "Квитанция индекса не соответствует активной публикации; обновите область поиска."
    );
    let counts = count_scope(&reader, &scope_a(), &receipt_two).unwrap();
    assert_eq!(counts.index_generation, receipt_two.index_generation);
    assert_eq!(counts.chunk_count, 4);
    drop(reader);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn corpus_database_wrapper_counts_reference_documents() {
    let (mut connection, directory) = setup();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &[
            chunk("a", "раз", PERSONA_A, GRANT_A, BINDING_A, (None, None)),
            chunk("b", "два", PERSONA_A, GRANT_A, BINDING_A, (None, None)),
        ],
    );
    let database = crate::corpus_database::CorpusDatabase { connection };
    let counts = database
        .count_reference_documents(&scope_a(), &receipt, &|| Ok(()))
        .unwrap();
    assert_eq!(counts.document_count, 2);
    assert_eq!(counts.revision_count, 2);
    assert_eq!(counts.chunk_count, 2);
    assert!(database
        .count_reference_documents(&scope_a(), &receipt, &|| {
            Err("подсчёт отменён".into())
        })
        .is_err());
    drop(database);
    std::fs::remove_dir_all(directory).unwrap();
}
