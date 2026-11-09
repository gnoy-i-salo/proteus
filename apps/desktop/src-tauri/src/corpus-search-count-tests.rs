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

// ==== Ignored synthetic latency measurement (proteus-tip.9 evidence) ====
//
// `measure_scoped_count_latency_over_fixed_100k_chunk_fixture` is an explicitly
// ignored measurement, never a regression gate: it reports observed durations
// of the metadata-only `count_reference_documents` over one fixed 100 000-chunk
// synthetic fixture on the machine that ran it and its actual Cargo profile. There is no pass/fail threshold and no performance promise; peak
// RSS and end-to-end retrieval/IPC latency are deliberately not claimed. The
// fixture publishes through the normal stage/receipt path on the real
// SQLCipher-backed search schema, and all texts and ids are synthetic. Run it
// explicitly with `cargo test -- --ignored --nocapture`.

/// Fixed fixture shape: 12 500 synthetic documents × 2 revisions × 4 chunks =
/// 100 000 chunk rows, comfortably under the 1 000 000-row
/// `MAX_STAGE_CHUNKS` publication cap. Every document is present under each of
/// the four authorized bindings (chunk index mod 4), half of the documents are
/// dated on one fixed synthetic day and the rest are undated, so the narrow
/// scopes have closed-form expected counts independent of the SQL aggregate.
const MEASURE_DOCUMENTS: usize = 12_500;
const MEASURE_REVISIONS_PER_DOCUMENT: usize = 2;
const MEASURE_CHUNKS_PER_REVISION: usize = 4;
const MEASURE_BINDINGS: usize = 4;
/// Fixed synthetic window anchor (2023-01-04T22:40:00Z in milliseconds), inside the
/// JS-safe integer boundary the validators enforce.
const MEASURE_DATED_DAY: i64 = 1_672_872_000_000;
const MEASURE_REPETITIONS: usize = 10;

const MEASURE_TOTAL_CHUNKS: i64 =
    (MEASURE_DOCUMENTS * MEASURE_REVISIONS_PER_DOCUMENT * MEASURE_CHUNKS_PER_REVISION) as i64;

struct MeasureFixture {
    chunks: Vec<SearchChunkInput>,
    expected_full: (i64, i64, i64),
    expected_single_binding: (i64, i64, i64),
    expected_dated_window: (i64, i64, i64),
    expected_empty_window: (i64, i64, i64),
}

fn measure_grant_id(binding: usize) -> String {
    format!("grant:measure-{binding}")
}

fn measure_binding_id(binding: usize) -> String {
    format!("binding:measure-{binding}")
}

fn measure_fixture() -> MeasureFixture {
    let mut chunks = Vec::with_capacity(
        MEASURE_DOCUMENTS * MEASURE_REVISIONS_PER_DOCUMENT * MEASURE_CHUNKS_PER_REVISION,
    );
    for document in 0..MEASURE_DOCUMENTS {
        for revision in 0..MEASURE_REVISIONS_PER_DOCUMENT {
            for part in 0..MEASURE_CHUNKS_PER_REVISION {
                let index = document
                    * (MEASURE_REVISIONS_PER_DOCUMENT * MEASURE_CHUNKS_PER_REVISION)
                    + revision * MEASURE_CHUNKS_PER_REVISION
                    + part;
                let binding = index % MEASURE_BINDINGS;
                let dated = document % 2 == 0;
                let (period_from, period_to) = if dated {
                    (Some(MEASURE_DATED_DAY), Some(MEASURE_DATED_DAY))
                } else {
                    (None, None)
                };
                let text = format!("synthetic measurement fixture chunk {index}");
                chunks.push(SearchChunkInput {
                    stage_id: "stage:measure".into(),
                    chunk_id: format!("measure-chunk-{index:07}"),
                    snapshot_id: SNAPSHOT_A.into(),
                    snapshot_generation: SNAPSHOT_GEN,
                    partition: "reference".into(),
                    text: text.clone(),
                    document_id: format!("measure-document-{document:05}"),
                    revision_id: format!("measure-revision-{document:05}-{revision}"),
                    span_start: 0,
                    span_end: text.len() as i64,
                    persona_id: PERSONA_A.into(),
                    grant_id: measure_grant_id(binding),
                    binding_id: measure_binding_id(binding),
                    record_id: format!("measure-record-{index:07}"),
                    record_generation: 1,
                    period_from,
                    period_to,
                    conversation_id: Some("conversation:measure".into()),
                    thread_id: Some(format!("thread:measure-{binding}")),
                });
            }
        }
    }
    assert_eq!(chunks.len() as i64, MEASURE_TOTAL_CHUNKS);
    let dated_documents = (MEASURE_DOCUMENTS / 2) as i64;
    let documents = MEASURE_DOCUMENTS as i64;
    let revisions = documents * MEASURE_REVISIONS_PER_DOCUMENT as i64;
    MeasureFixture {
        chunks,
        expected_full: (documents, revisions, MEASURE_TOTAL_CHUNKS),
        expected_single_binding: (
            documents,
            revisions,
            MEASURE_TOTAL_CHUNKS / MEASURE_BINDINGS as i64,
        ),
        expected_dated_window: (
            dated_documents,
            dated_documents * MEASURE_REVISIONS_PER_DOCUMENT as i64,
            dated_documents
                * MEASURE_REVISIONS_PER_DOCUMENT as i64
                * MEASURE_CHUNKS_PER_REVISION as i64,
        ),
        expected_empty_window: (0, 0, 0),
    }
}

fn measure_full_scope() -> SearchScope {
    SearchScope {
        snapshot_id: SNAPSHOT_A.into(),
        snapshot_generation: SNAPSHOT_GEN,
        persona_id: PERSONA_A.into(),
        grant_ids: (0..MEASURE_BINDINGS).map(measure_grant_id).collect(),
        binding_ids: (0..MEASURE_BINDINGS).map(measure_binding_id).collect(),
        period: SearchPeriod {
            from: None,
            to: None,
            include_undated: true,
        },
    }
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

/// Min, median and p95 (nearest-rank) of the measured warm samples.
fn duration_stats(samples: &mut [Duration]) -> (Duration, Duration, Duration) {
    samples.sort();
    let len = samples.len();
    let median = if len % 2 == 1 {
        samples[len / 2]
    } else {
        (samples[len / 2 - 1] + samples[len / 2]) / 2
    };
    let p95_rank = ((len as f64) * 0.95).ceil() as usize;
    let p95 = samples[p95_rank.saturating_sub(1).min(len - 1)];
    (samples[0], median, p95)
}

#[test]
#[ignore = "synthetic latency measurement; run explicitly with --ignored --nocapture"]
fn measure_scoped_count_latency_over_fixed_100k_chunk_fixture() {
    let fixture_started = Instant::now();
    let (mut connection, directory) = setup();
    let fixture = measure_fixture();
    let fixture_built = fixture_started.elapsed();
    let staged_started = Instant::now();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:measure",
        &fixture.chunks,
    );
    let staged = staged_started.elapsed();
    let setup_total = fixture_started.elapsed();
    assert_eq!(receipt.chunk_count, MEASURE_TOTAL_CHUNKS);
    assert_eq!(receipt.index_generation, 1);

    let full = measure_full_scope();
    let mut single_binding = full.clone();
    single_binding.grant_ids = vec![measure_grant_id(0)];
    single_binding.binding_ids = vec![measure_binding_id(0)];
    let mut dated_window = full.clone();
    dated_window.period = SearchPeriod {
        from: Some(MEASURE_DATED_DAY),
        to: Some(MEASURE_DATED_DAY),
        include_undated: false,
    };
    let mut empty_window = full.clone();
    empty_window.period = SearchPeriod {
        from: Some(MEASURE_DATED_DAY - 86_400_000),
        to: Some(MEASURE_DATED_DAY - 86_400_000),
        include_undated: false,
    };
    let cases = [
        ("full", full, fixture.expected_full),
        (
            "single_binding",
            single_binding,
            fixture.expected_single_binding,
        ),
        ("dated_window", dated_window, fixture.expected_dated_window),
        ("empty_window", empty_window, fixture.expected_empty_window),
    ];

    // First count after publication, reported separately. Publication has
    // already touched these pages: this is not a cold disk-cache measurement.
    let first_started = Instant::now();
    let first_counts = count_scope(&connection, &cases[0].1, &receipt).unwrap();
    let first = first_started.elapsed();
    assert_eq!(
        (
            first_counts.document_count,
            first_counts.revision_count,
            first_counts.chunk_count
        ),
        cases[0].2
    );

    println!(
        "measurement: debug_assertions={}, bundled SQLCipher, single connection, synthetic fixture",
        cfg!(debug_assertions)
    );
    println!(
        "fixture: documents={MEASURE_DOCUMENTS} revisions_per_document={MEASURE_REVISIONS_PER_DOCUMENT} \
         chunks_per_revision={MEASURE_CHUNKS_PER_REVISION} total_chunk_rows={MEASURE_TOTAL_CHUNKS} \
         bindings={MEASURE_BINDINGS}"
    );
    println!(
        "setup: fixture_build_ms={:.3} stage_and_commit_ms={:.3} setup_total_ms={:.3}",
        millis(fixture_built),
        millis(staged),
        millis(setup_total)
    );
    println!(
        "receipt: snapshot_id={} snapshot_generation={} index_generation={} chunk_count={}",
        receipt.snapshot_id,
        receipt.snapshot_generation,
        receipt.index_generation,
        receipt.chunk_count
    );
    for (name, scope, expected) in &cases {
        let mut samples = Vec::with_capacity(MEASURE_REPETITIONS);
        for _ in 0..MEASURE_REPETITIONS {
            let started = Instant::now();
            let counts = count_scope(&connection, scope, &receipt).unwrap();
            samples.push(started.elapsed());
            assert_eq!(
                (
                    counts.document_count,
                    counts.revision_count,
                    counts.chunk_count
                ),
                *expected,
                "scope {name}"
            );
        }
        let (min, median, p95) = duration_stats(&mut samples);
        let observed = count_scope(&connection, scope, &receipt).unwrap();
        let observed = (
            observed.document_count,
            observed.revision_count,
            observed.chunk_count,
        );
        println!(
            "scope={name} expected_documents={} expected_revisions={} expected_chunks={} \
             observed={observed:?} reps={MEASURE_REPETITIONS} min_ms={:.3} median_ms={:.3} \
             p95_nearest_rank_ms={:.3}",
            expected.0,
            expected.1,
            expected.2,
            millis(min),
            millis(median),
            millis(p95)
        );
        let millis_samples = samples
            .iter()
            .map(|sample| millis(*sample))
            .collect::<Vec<_>>();
        println!("scope={name} durations_ms={millis_samples:.3?}");
    }
    println!("first_count_after_commit_ms={:.3}", millis(first));
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}
