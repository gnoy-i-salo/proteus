//! Regression tests for bounded reference-chunk scanning over the corpus
//! FTS index. The fixture is synthetic: fixed Russian/mixed-language text,
//! punctuation-only ("nonlexical") chunks, Unicode/CRLF/quote bytes and
//! deterministic spans. Scanning is query-independent: every authorized
//! reference chunk is returned regardless of any lexical match, and the
//! scope semantics are shared with the FTS hit query.
// Reuse the existing FTS fixture owner, as receipt/recovery tests do.
use super::*;
use std::cell::Cell;

fn scan(
    connection: &Connection,
    scope: &SearchScope,
    receipt: &SearchIndexReceipt,
) -> Result<ReferenceChunkPage, String> {
    scan_reference_chunks(
        connection,
        scope,
        receipt,
        None,
        MAX_SCAN_LIMIT,
        MAX_SCAN_TEXT_BYTES,
        &|| Ok(()),
    )
}

fn scan_page(
    connection: &Connection,
    scope: &SearchScope,
    receipt: &SearchIndexReceipt,
    after: Option<&str>,
    limit: usize,
) -> Result<ReferenceChunkPage, String> {
    scan_reference_chunks(
        connection,
        scope,
        receipt,
        after,
        limit,
        MAX_SCAN_TEXT_BYTES,
        &|| Ok(()),
    )
}

fn page_ids(page: &ReferenceChunkPage) -> Vec<String> {
    page.chunks
        .iter()
        .map(|chunk| chunk.chunk_id.clone())
        .collect()
}

#[test]
fn scan_returns_all_authorized_chunks_including_nonlexical_ones_without_any_query() {
    let (mut connection, _directory) = setup();
    let punctuation_only = chunk(
        "chunk:9",
        "@@@ !!! ??? <<< >>>",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    );
    let foreign_words = chunk(
        "chunk:8",
        "zzz qqq vvv",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    );
    let mut fixture = vec![
        chunk(
            "chunk:1",
            "Марья: Привет! Договорились встретиться в субботу у озера.",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (Some(1672876800), Some(1672876800)),
        ),
        chunk(
            "chunk:2",
            "Ссылка: https://example.test/dogovor-a-b?x_y=1 — проверь пункт 7.",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (Some(1672963200), Some(1672963200)),
        ),
    ];
    fixture.push(foreign_words.clone());
    fixture.push(punctuation_only.clone());
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture,
    );
    let page = scan(&connection, &scope_a(), &receipt).unwrap();
    assert_eq!(page.snapshot_id, SNAPSHOT_A);
    assert_eq!(page.snapshot_generation, SNAPSHOT_GEN);
    assert_eq!(page.index_generation, 1);
    assert!(!page.has_more);
    assert_eq!(page.next_cursor, None);
    let mut ids = page_ids(&page);
    ids.sort();
    assert_eq!(ids, ["chunk:1", "chunk:2", "chunk:8", "chunk:9"]);
    // Exact indexed bytes come back untouched; metadata matches the fixture.
    let first = page
        .chunks
        .iter()
        .find(|chunk| chunk.chunk_id == "chunk:1")
        .expect("chunk:1 must be scanned");
    assert_eq!(first.text.as_bytes(), fixture[0].text.as_bytes());
    assert_eq!(first.document_id, "document:chunk:1");
    assert_eq!(first.revision_id, "revision:chunk:1");
    assert_eq!(
        (first.span_start, first.span_end),
        (0, fixture[0].text.len() as i64)
    );
    assert_eq!(first.persona_id, PERSONA_A);
    assert_eq!(first.grant_id, GRANT_A);
    assert_eq!(first.binding_id, BINDING_A);
    assert_eq!(first.period_from, Some(1672876800));
    assert_eq!(first.period_to, Some(1672876800));
    assert_eq!(
        first.conversation_id.as_deref(),
        Some("conversation:chat-1")
    );
    assert_eq!(first.thread_id.as_deref(), Some("thread:chunk:1"));
    let nonlexical = page
        .chunks
        .iter()
        .find(|chunk| chunk.chunk_id == "chunk:9")
        .expect("punctuation-only chunk must be scanned without any query");
    assert_eq!(nonlexical.text.as_bytes(), punctuation_only.text.as_bytes());
}

#[test]
fn scan_orders_chunk_ids_bytewise() {
    let (mut connection, _directory) = setup();
    let chunks = (1..=12)
        .map(|index| {
            chunk(
                &format!("chunk:{index}"),
                &format!("текст {index}"),
                PERSONA_A,
                GRANT_A,
                BINDING_A,
                (None, None),
            )
        })
        .collect::<Vec<_>>();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &chunks,
    );
    let page = scan(&connection, &scope_a(), &receipt).unwrap();
    // Bytewise order is not natural numeric order: "chunk:10" < "chunk:2".
    let mut expected = page_ids(&page);
    expected.sort();
    assert_eq!(page_ids(&page), expected);
    let ten = page_ids(&page)
        .iter()
        .position(|id| id == "chunk:10")
        .unwrap();
    let two = page_ids(&page)
        .iter()
        .position(|id| id == "chunk:2")
        .unwrap();
    assert!(ten < two, "bytewise order places chunk:10 before chunk:2");
}

#[test]
fn scan_pages_by_keyset_over_equal_text_distinct_ids_without_skips() {
    let (mut connection, _directory) = setup();
    let fixture = vec![
        chunk(
            "chunk:dup-a",
            "одинаковый текст",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
        chunk(
            "chunk:dup-b",
            "одинаковый текст",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
        chunk(
            "chunk:tail",
            "хвост",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
    ];
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture,
    );
    let full = scan(&connection, &scope_a(), &receipt).unwrap();
    assert_eq!(
        page_ids(&full),
        ["chunk:dup-a", "chunk:dup-b", "chunk:tail"]
    );
    let page_one = scan_page(&connection, &scope_a(), &receipt, None, 2).unwrap();
    assert_eq!(page_ids(&page_one), ["chunk:dup-a", "chunk:dup-b"]);
    assert!(page_one.has_more);
    assert_eq!(page_one.next_cursor.as_deref(), Some("chunk:dup-b"));
    let cursor = page_one.next_cursor.clone().unwrap();
    let page_two = scan_page(&connection, &scope_a(), &receipt, Some(&cursor), 2).unwrap();
    assert_eq!(page_ids(&page_two), ["chunk:tail"]);
    assert!(!page_two.has_more);
    assert_eq!(page_two.next_cursor, None);
    let mut concatenated = page_ids(&page_one);
    concatenated.extend(page_ids(&page_two));
    assert_eq!(concatenated, page_ids(&full));
    // Equal text under distinct ids stays distinct and byte-exact.
    assert_eq!(page_one.chunks[0].text, "одинаковый текст");
    assert_eq!(page_one.chunks[1].text, "одинаковый текст");
    assert_ne!(page_one.chunks[0].chunk_id, page_one.chunks[1].chunk_id);
}

#[test]
fn scan_excludes_foreign_persona_grant_binding_and_snapshot() {
    let (mut connection, _directory) = setup();
    let mut foreign_snapshot = chunk(
        "chunk:sb",
        "другой снимок",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    );
    foreign_snapshot.snapshot_id = SNAPSHOT_B.into();
    foreign_snapshot.snapshot_generation = 1;
    let fixture = [
        chunk(
            "chunk:a1",
            "своя персона и привязка",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
        chunk(
            "chunk:b1",
            "чужая персона",
            PERSONA_B,
            GRANT_B,
            BINDING_B,
            (None, None),
        ),
        chunk(
            "chunk:a2",
            "чужой грант",
            PERSONA_A,
            GRANT_B,
            BINDING_A,
            (None, None),
        ),
        chunk(
            "chunk:a3",
            "чужая привязка",
            PERSONA_A,
            GRANT_A,
            BINDING_B,
            (None, None),
        ),
        foreign_snapshot,
    ];
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture[..4],
    );
    publish(&mut connection, SNAPSHOT_B, 1, "stage:2", &fixture[4..]);
    let page = scan(&connection, &scope_a(), &receipt).unwrap();
    assert_eq!(page_ids(&page), ["chunk:a1"]);
}

#[test]
fn scan_respects_dated_undated_periods_and_open_endpoints() {
    let (mut connection, _directory) = setup();
    let fixture = vec![
        chunk(
            "chunk:d1",
            "ранняя дата",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (Some(1672876800), Some(1672876800)),
        ),
        chunk(
            "chunk:d2",
            "поздний диапазон",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (Some(1709251200), Some(1709337600)),
        ),
        chunk(
            "chunk:undated",
            "без даты",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
    ];
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture,
    );
    let scope = |from: Option<i64>, to: Option<i64>, include_undated: bool| SearchScope {
        period: SearchPeriod {
            from,
            to,
            include_undated,
        },
        ..scope_a()
    };
    let inclusive = scan(
        &connection,
        &scope(Some(1672876800), Some(1709251200), true),
        &receipt,
    )
    .unwrap();
    assert_eq!(
        page_ids(&inclusive),
        ["chunk:d1", "chunk:d2", "chunk:undated"]
    );
    let dated_only = scan(
        &connection,
        &scope(Some(1672876800), Some(1709251200), false),
        &receipt,
    )
    .unwrap();
    assert_eq!(page_ids(&dated_only), ["chunk:d1", "chunk:d2"]);
    let from_only = scan(&connection, &scope(Some(1700000000), None, false), &receipt).unwrap();
    assert_eq!(page_ids(&from_only), ["chunk:d2"]);
    let to_only = scan(&connection, &scope(None, Some(1672876800), false), &receipt).unwrap();
    assert_eq!(page_ids(&to_only), ["chunk:d1"]);
    let open_with_undated =
        scan(&connection, &scope(Some(1700000000), None, true), &receipt).unwrap();
    assert_eq!(page_ids(&open_with_undated), ["chunk:d2", "chunk:undated"]);
    let undated_only_window = scan(&connection, &scope(None, None, false), &receipt).unwrap();
    assert_eq!(page_ids(&undated_only_window), ["chunk:d1", "chunk:d2"]);
}

#[test]
fn scan_rejects_scope_snapshot_mismatched_with_receipt() {
    let (mut connection, _directory) = setup();
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
    let foreign_snapshot = SearchScope {
        snapshot_id: SNAPSHOT_B.into(),
        ..scope_a()
    };
    let error = scan(&connection, &foreign_snapshot, &receipt).unwrap_err();
    assert_eq!(
        error,
        "Область сканирования должна указывать снимок квитанции индекса."
    );
    let drifted_generation = SearchScope {
        snapshot_generation: 2,
        ..scope_a()
    };
    let error = scan(&connection, &drifted_generation, &receipt).unwrap_err();
    assert_eq!(
        error,
        "Область сканирования должна указывать снимок квитанции индекса."
    );
}

#[test]
fn scan_rejects_receipt_identity_drift_in_every_field() {
    let (mut connection, _directory) = setup();
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
    let mut tampered = receipt.clone();
    tampered.snapshot_id = SNAPSHOT_B.into();
    assert!(scan(&connection, &scope_a(), &tampered).is_err());
    let mut tampered = receipt.clone();
    tampered.snapshot_generation = 2;
    assert!(scan(&connection, &scope_a(), &tampered).is_err());
    let mut tampered = receipt.clone();
    tampered.index_generation = 99;
    assert!(scan(&connection, &scope_a(), &tampered).is_err());
    let mut tampered = receipt.clone();
    tampered.policy_fingerprint = "tip8-split/2|corpus-search/1".into();
    assert!(scan(&connection, &scope_a(), &tampered).is_err());
    let mut tampered = receipt.clone();
    tampered.content_sha256 = EMPTY_CONTENT_SHA256.into();
    let error = scan(&connection, &scope_a(), &tampered).unwrap_err();
    assert_eq!(
        error,
        "Квитанция индекса не соответствует активной публикации; обновите область поиска."
    );
    let mut tampered = receipt.clone();
    tampered.chunk_count += 1;
    assert!(scan(&connection, &scope_a(), &tampered).is_err());
    let mut tampered = receipt.clone();
    tampered.created_at = "0".into();
    assert!(scan(&connection, &scope_a(), &tampered).is_err());
    // A structurally unusable pin is rejected before any comparison.
    let mut tampered = receipt.clone();
    tampered.chunk_count = -1;
    let error = scan(&connection, &scope_a(), &tampered).unwrap_err();
    assert_eq!(error, "Недопустимая квитанция поискового индекса.");
}

#[test]
fn scan_rejects_rebuild_prune_and_deleted_snapshot_between_pages() {
    let (mut connection, _directory) = setup();
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
    // The old publication identity is gone after the rebuild+prune: the
    // continuation pin is an error, never a stale page.
    let stale = scan_page(
        &connection,
        &scope_a(),
        &first_receipt,
        Some("chunk:1"),
        MAX_SCAN_LIMIT,
    )
    .unwrap_err();
    assert_eq!(
        stale,
        "Квитанция индекса не соответствует активной публикации; обновите область поиска."
    );
    let page = scan(&connection, &scope_a(), &second_receipt).unwrap();
    assert_eq!(page_ids(&page), ["chunk:1", "chunk:2"]);
    // A deleted snapshot never serves its obsolete index.
    connection
        .execute("DELETE FROM snapshots WHERE id=?1", [SNAPSHOT_A])
        .unwrap();
    let error = scan(&connection, &scope_a(), &second_receipt).unwrap_err();
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
    let error = scan(&connection, &unpublished_scope, &unpublished_receipt).unwrap_err();
    assert_eq!(error, "Индекс для снимка корпуса не опубликован.");
}

#[test]
fn scan_enforces_row_limit_with_exact_full_page_detection() {
    let (mut connection, _directory) = setup();
    assert_eq!(MAX_SCAN_LIMIT, 100);
    let chunks = (1..=101)
        .map(|index| {
            chunk(
                &format!("chunk:{index:03}"),
                &format!("текст {index}"),
                PERSONA_A,
                GRANT_A,
                BINDING_A,
                (None, None),
            )
        })
        .collect::<Vec<_>>();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &chunks,
    );
    let page_one = scan_page(&connection, &scope_a(), &receipt, None, 100).unwrap();
    assert_eq!(page_one.chunks.len(), 100);
    assert!(page_one.has_more);
    assert_eq!(page_one.next_cursor.as_deref(), Some("chunk:100"));
    let cursor = page_one.next_cursor.clone().unwrap();
    let page_two = scan_page(&connection, &scope_a(), &receipt, Some(&cursor), 100).unwrap();
    assert_eq!(page_ids(&page_two), ["chunk:101"]);
    assert!(!page_two.has_more);
    // An exactly-full last page must not invent more rows.
    let exactly_full = scan_page(&connection, &scope_a(), &receipt, Some("chunk:100"), 1).unwrap();
    assert_eq!(page_ids(&exactly_full), ["chunk:101"]);
    assert!(!exactly_full.has_more);
    assert_eq!(exactly_full.next_cursor, None);
}

#[test]
fn scan_enforces_byte_budget_first_chunk_must_fit_partial_page_keeps_cursor() {
    let (mut connection, _directory) = setup();
    let fixture = vec![
        chunk(
            "chunk:t1",
            "0123456789",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
        chunk(
            "chunk:t2",
            "abcdefghij",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
        chunk(
            "chunk:t3",
            "ABCDEFGHIJ",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
    ];
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture,
    );
    let scanned = |after: Option<&str>, budget: usize| {
        scan_reference_chunks(
            &connection,
            &scope_a(),
            &receipt,
            after,
            MAX_SCAN_LIMIT,
            budget,
            &|| Ok(()),
        )
    };
    // Exact fit: everything fits, no invented continuation.
    let exact = scanned(None, 30).unwrap();
    assert_eq!(page_ids(&exact), ["chunk:t1", "chunk:t2", "chunk:t3"]);
    assert!(!exact.has_more);
    // Partial page: the row beyond the budget is never skipped.
    let partial = scanned(None, 25).unwrap();
    assert_eq!(page_ids(&partial), ["chunk:t1", "chunk:t2"]);
    assert!(partial.has_more);
    assert_eq!(partial.next_cursor.as_deref(), Some("chunk:t2"));
    assert!(
        partial
            .chunks
            .iter()
            .map(|chunk| chunk.text.len())
            .sum::<usize>()
            <= 25
    );
    let cursor = partial.next_cursor.clone().unwrap();
    let rest = scanned(Some(&cursor), 25).unwrap();
    assert_eq!(page_ids(&rest), ["chunk:t3"]);
    assert!(!rest.has_more);
    // The first chunk must fit the budget or the scan fails explicitly.
    let error = scanned(None, 5).unwrap_err();
    assert_eq!(error, "Текст чанка превышает байтовый бюджет сканирования.");
    // The budget ceiling is 64 MiB.
    let cap = scanned(None, MAX_SCAN_TEXT_BYTES).unwrap();
    assert_eq!(page_ids(&cap), ["chunk:t1", "chunk:t2", "chunk:t3"]);
}

#[test]
fn scan_preserves_unicode_crlf_quotes_and_exact_bytes() {
    let (mut connection, _directory) = setup();
    let exact = "  «цитата» \"кавычки\" 'одинарные' 😀\r\n\tвторая строка 日本語 Übergröße  ";
    let fixture = vec![chunk(
        "chunk:uni",
        exact,
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    )];
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture,
    );
    let page = scan(&connection, &scope_a(), &receipt).unwrap();
    let scanned = &page.chunks[0];
    assert_eq!(scanned.text.as_bytes(), exact.as_bytes());
    assert_eq!(scanned.span_end, exact.len() as i64);
}

#[test]
fn scan_rejects_invalid_page_limits_budgets_and_cursors() {
    let (mut connection, _directory) = setup();
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
    let scanned = |after: Option<&str>, limit: usize, budget: usize| {
        scan_reference_chunks(
            &connection,
            &scope_a(),
            &receipt,
            after,
            limit,
            budget,
            &|| Ok(()),
        )
    };
    for (limit, budget) in [
        (0, MAX_SCAN_TEXT_BYTES),
        (MAX_SCAN_LIMIT + 1, MAX_SCAN_TEXT_BYTES),
        (MAX_SCAN_LIMIT, 0),
        (MAX_SCAN_LIMIT, MAX_SCAN_TEXT_BYTES + 1),
    ] {
        let error = scanned(None, limit, budget).unwrap_err();
        assert_eq!(error, "Недопустимые ограничения сканирования.");
    }
    for cursor in ["", &"x".repeat(300), "bad\u{0007}id"] {
        let error = scanned(Some(cursor), MAX_SCAN_LIMIT, MAX_SCAN_TEXT_BYTES).unwrap_err();
        assert_eq!(error, "Недопустимый курсор сканирования.");
    }
    // A well-formed cursor beyond the last row is an empty page, not an error.
    let beyond = scanned(Some("chunk:zzz"), MAX_SCAN_LIMIT, MAX_SCAN_TEXT_BYTES).unwrap();
    assert!(beyond.chunks.is_empty());
    assert!(!beyond.has_more);
    assert_eq!(beyond.next_cursor, None);
}

#[test]
fn scan_checkpoint_cancels_before_during_and_after_work_preserving_errors() {
    let (mut connection, _directory) = setup();
    let fixture = vec![
        chunk(
            "chunk:1",
            "первый",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
        chunk(
            "chunk:2",
            "второй",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
        chunk(
            "chunk:3",
            "третий",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
    ];
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture,
    );
    let calls = Cell::new(0usize);
    let checkpoint = |fail_at: Option<usize>| {
        let calls = &calls;
        move || -> Result<(), String> {
            let seen = calls.get() + 1;
            calls.set(seen);
            if fail_at == Some(seen) {
                Err(format!("сканирование отменено на вызове {seen}"))
            } else {
                Ok(())
            }
        }
    };
    // Before any work: the very first checkpoint already refuses.
    calls.set(0);
    let error = scan_reference_chunks(
        &connection,
        &scope_a(),
        &receipt,
        None,
        MAX_SCAN_LIMIT,
        MAX_SCAN_TEXT_BYTES,
        &checkpoint(Some(1)),
    )
    .unwrap_err();
    assert_eq!(error, "сканирование отменено на вызове 1");
    assert_eq!(calls.get(), 1);
    // During the scan, before the third row is read.
    calls.set(0);
    let error = scan_reference_chunks(
        &connection,
        &scope_a(),
        &receipt,
        None,
        MAX_SCAN_LIMIT,
        MAX_SCAN_TEXT_BYTES,
        &checkpoint(Some(4)),
    )
    .unwrap_err();
    assert_eq!(error, "сканирование отменено на вызове 4");
    assert_eq!(calls.get(), 4);
    // After the work: the final checkpoint still refuses.
    calls.set(0);
    let error = scan_reference_chunks(
        &connection,
        &scope_a(),
        &receipt,
        None,
        MAX_SCAN_LIMIT,
        MAX_SCAN_TEXT_BYTES,
        &checkpoint(Some(6)),
    )
    .unwrap_err();
    assert_eq!(error, "сканирование отменено на вызове 6");
    assert_eq!(calls.get(), 6);
    // Success path: pre-checkpoint, one per read row, the exhaustion read and
    // the final checkpoint fire exactly once each.
    calls.set(0);
    let page = scan_reference_chunks(
        &connection,
        &scope_a(),
        &receipt,
        None,
        MAX_SCAN_LIMIT,
        MAX_SCAN_TEXT_BYTES,
        &checkpoint(None),
    )
    .unwrap();
    assert_eq!(page_ids(&page), ["chunk:1", "chunk:2", "chunk:3"]);
    assert_eq!(calls.get(), 6);
    // A full page stops without the exhaustion read: fewer checkpoint calls.
    calls.set(0);
    let page = scan_reference_chunks(
        &connection,
        &scope_a(),
        &receipt,
        None,
        2,
        MAX_SCAN_TEXT_BYTES,
        &checkpoint(None),
    )
    .unwrap();
    assert_eq!(page_ids(&page), ["chunk:1", "chunk:2"]);
    assert!(page.has_more);
    calls.set(0);
    let error = scan_reference_chunks(
        &connection,
        &scope_a(),
        &receipt,
        None,
        2,
        MAX_SCAN_TEXT_BYTES,
        &checkpoint(Some(4)),
    )
    .unwrap_err();
    assert_eq!(error, "сканирование отменено на вызове 4");
    assert_eq!(calls.get(), 4);
}

#[test]
fn scan_releases_savepoint_on_error_and_preserves_nested_caller_transaction() {
    let (mut connection, _directory) = setup();
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
    // A failed scan on an autocommit connection leaves no open transaction.
    let failed = scan_reference_chunks(
        &connection,
        &scope_a(),
        &receipt,
        None,
        MAX_SCAN_LIMIT,
        5,
        &|| Ok(()),
    );
    assert_eq!(
        failed.unwrap_err(),
        "Текст чанка превышает байтовый бюджет сканирования."
    );
    connection
        .execute_batch("BEGIN; CREATE TABLE leak_probe(n INTEGER); COMMIT;")
        .expect("no transaction may leak from a failed scan");
    // The same failure nested in a caller transaction preserves that
    // transaction: the caller's writes before and after the failed scan
    // commit together, and the savepoint is fully released.
    connection.execute_batch("BEGIN").unwrap();
    connection
        .execute("CREATE TABLE caller_marker(n INTEGER)", [])
        .unwrap();
    let failed = scan_reference_chunks(
        &connection,
        &scope_a(),
        &receipt,
        None,
        MAX_SCAN_LIMIT,
        5,
        &|| Ok(()),
    );
    assert!(failed.is_err());
    connection
        .execute("INSERT INTO caller_marker VALUES (1)", [])
        .unwrap();
    connection.execute_batch("COMMIT").unwrap();
    let count: i64 = connection
        .query_row("SELECT count(*) FROM caller_marker", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    // The connection keeps serving scans after the nested failure.
    let page = scan(&connection, &scope_a(), &receipt).unwrap();
    assert_eq!(page_ids(&page), ["chunk:1"]);
}

#[test]
fn corpus_database_wrapper_scans_reference_chunks() {
    let (mut connection, _directory) = setup();
    let fixture = vec![
        chunk(
            "chunk:1",
            "первая",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
        chunk(
            "chunk:2",
            "вторая",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
    ];
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture,
    );
    let database = crate::corpus_database::CorpusDatabase { connection };
    let page = database
        .scan_reference_chunks(&scope_a(), &receipt, None, 1, MAX_SCAN_TEXT_BYTES, &|| {
            Ok(())
        })
        .unwrap();
    assert_eq!(page_ids(&page), ["chunk:1"]);
    assert!(page.has_more);
    let cursor = page.next_cursor.clone().unwrap();
    let rest = database
        .scan_reference_chunks(
            &scope_a(),
            &receipt,
            Some(&cursor),
            1,
            MAX_SCAN_TEXT_BYTES,
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(page_ids(&rest), ["chunk:2"]);
    assert!(!rest.has_more);
}

#[test]
fn corrupted_oversized_indexed_text_is_not_returned() {
    let (mut db, _dir) = setup();
    let receipt = publish(
        &mut db,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "root-stage",
        &[chunk(
            "a",
            "ok",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        )],
    );
    db.execute(
        "UPDATE search_fts SET text=?1",
        ["x".repeat(MAX_CHUNK_BYTES + 1)],
    )
    .unwrap();
    assert!(scan(&db, &scope_a(), &receipt).is_err());
}
#[test]
fn missing_indexed_text_does_not_silently_drop_a_source_chunk() {
    let (mut db, _dir) = setup();
    let receipt = publish(
        &mut db,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "root-stage",
        &[chunk(
            "a",
            "ok",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        )],
    );
    db.execute("DELETE FROM search_fts", []).unwrap();
    assert!(scan(&db, &scope_a(), &receipt).is_err());
}
#[test]
fn over_budget_next_row_is_not_decoded_or_skipped() {
    let (mut db, _dir) = setup();
    let receipt = publish(
        &mut db,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "root-stage",
        &[
            chunk("a", "ok", PERSONA_A, GRANT_A, BINDING_A, (None, None)),
            chunk("b", "next", PERSONA_A, GRANT_A, BINDING_A, (None, None)),
        ],
    );
    db.execute("UPDATE search_fts SET text=CAST(x'ffffffff' AS TEXT) WHERE rowid=(SELECT rowid FROM search_chunks WHERE chunk_id='b')",[]).unwrap();
    let page = scan_reference_chunks(&db, &scope_a(), &receipt, None, 100, 3, &|| Ok(())).unwrap();
    assert_eq!(page_ids(&page), ["a"]);
    assert!(page.has_more);
    assert_eq!(page.next_cursor.as_deref(), Some("a"));
    assert!(
        scan_reference_chunks(&db, &scope_a(), &receipt, Some("a"), 100, 8, &|| Ok(())).is_err()
    );
}
