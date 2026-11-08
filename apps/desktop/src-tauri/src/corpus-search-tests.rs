//! Regression tests for the reference-only FTS5/BM25 corpus search.
//! The fixture is synthetic: fixed Russian/mixed-language names, URLs, spans
//! and one typo case. FTS5 is lexical: exact tokens and prefix terms match,
//! a typo term returns nothing instead of fabricating a correction.
use super::*;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier};

const SNAPSHOT_A: &str = "snapshot:corpus-a";
const SNAPSHOT_B: &str = "snapshot:corpus-b";
const SNAPSHOT_GEN: i64 = 1;
const POLICY: &str = "tip8-split/1|corpus-search/1";
const PERSONA_A: &str = "persona:marya";
const PERSONA_B: &str = "persona:petya";
const GRANT_A: &str = "grant:vk-messages";
const GRANT_B: &str = "grant:tg-messages";
const GRANT_C: &str = "grant:archive-pages";
const BINDING_A: &str = "binding:vk-dasha";
const BINDING_B: &str = "binding:tg-dasha";
const BINDING_C: &str = "binding:archive-dasha";

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_directory() -> PathBuf {
    let id = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "proteus-corpus-search-{}-{}",
        std::process::id(),
        id
    ));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

fn open_database(path: &Path) -> Connection {
    let fresh = !path.exists();
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )
    .unwrap();
    connection
        .pragma_update(
            None,
            "key",
            "x'000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f'",
        )
        .unwrap();
    connection
        .execute_batch(
            "PRAGMA cipher_memory_security=ON; PRAGMA temp_store=MEMORY; PRAGMA journal_mode=WAL; \
             PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA secure_delete=ON; \
             PRAGMA trusted_schema=OFF; PRAGMA busy_timeout=5000;",
        )
        .unwrap();
    if fresh {
        connection
            .execute_batch(include_str!("corpus-schema.sql"))
            .unwrap();
        connection
            .execute_batch(include_str!("corpus-search-schema.sql"))
            .unwrap();
    }
    connection
}

fn insert_snapshot_row(connection: &Connection, snapshot_id: &str, generation: i64) {
    connection
        .execute(
            "INSERT INTO snapshots(id,corpus_id,publication_id,generation,parent_id,created_at,canonical_sha256) \
             VALUES(?1,?2,?3,?4,NULL,'2026-11-07T00:00:00Z','fixture-hash')",
            rusqlite::params![snapshot_id, format!("corpus:{snapshot_id}"), format!("publication:{snapshot_id}"), generation],
        )
        .unwrap();
}

fn setup() -> (Connection, PathBuf) {
    let directory = temp_directory();
    let path = directory.join("corpus.db");
    let connection = open_database(&path);
    insert_snapshot_row(&connection, SNAPSHOT_A, SNAPSHOT_GEN);
    insert_snapshot_row(&connection, SNAPSHOT_B, 1);
    (connection, directory)
}

fn chunk(
    id: &str,
    text: &str,
    persona: &str,
    grant: &str,
    binding: &str,
    period: (Option<i64>, Option<i64>),
) -> SearchChunkInput {
    SearchChunkInput {
        stage_id: "stage:pending".into(),
        chunk_id: id.into(),
        snapshot_id: SNAPSHOT_A.into(),
        snapshot_generation: SNAPSHOT_GEN,
        partition: "reference".into(),
        text: text.into(),
        document_id: format!("document:{id}"),
        revision_id: format!("revision:{id}"),
        span_start: 0,
        span_end: text.len() as i64,
        persona_id: persona.into(),
        grant_id: grant.into(),
        binding_id: binding.into(),
        record_id: format!("record:{id}"),
        record_generation: 1,
        period_from: period.0,
        period_to: period.1,
        conversation_id: Some("conversation:chat-1".into()),
        thread_id: Some(format!("thread:{id}")),
    }
}

fn fixture_chunks() -> Vec<SearchChunkInput> {
    vec![
        chunk(
            "chunk:1",
            "Марья: Привет! Договорились встретиться в субботу у озера.\r\nАнна: Отлично, я принесу договор на подпись.",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (Some(1672876800), Some(1672876800)),
        ),
        chunk(
            "chunk:2",
            "Ссылка на черновик: https://example.test/dogovor-a-b?x_y=1 — проверь пункт 7.",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (Some(1672963200), Some(1672963200)),
        ),
        chunk(
            "chunk:3",
            "Петя: Переносим встречу на воскресенье, суббота занята.",
            PERSONA_B,
            GRANT_B,
            BINDING_B,
            (Some(1709251200), Some(1709337600)),
        ),
        chunk(
            "chunk:5",
            "Архивная заметка без даты: однажды в субботу.",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
        chunk(
            "chunk:6",
            "  точный текст с переводом строки\r\nи emoji 😀  ",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (Some(1654041600), Some(1654041600)),
        ),
    ]
}

fn holdout_chunk() -> SearchChunkInput {
    let mut input = chunk(
        "chunk:4",
        "Holdout реплика про субботу, которой не должно быть в индексе.",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (Some(1672876800), Some(1672876800)),
    );
    input.partition = "holdout".into();
    input
}

fn snapshot_b_chunk() -> SearchChunkInput {
    let mut input = chunk(
        "chunk:b1",
        "Отдельный снимок тоже упоминает субботу и ozera.",
        PERSONA_A,
        GRANT_C,
        BINDING_C,
        (Some(1672876800), Some(1672876800)),
    );
    input.snapshot_id = SNAPSHOT_B.into();
    input.snapshot_generation = 1;
    input
}

fn publish(
    connection: &mut Connection,
    snapshot_id: &str,
    snapshot_generation: i64,
    stage_id: &str,
    chunks: &[SearchChunkInput],
) -> SearchIndexReceipt {
    begin_stage(
        connection,
        &SearchStageInput {
            stage_id: stage_id.into(),
            snapshot_id: snapshot_id.into(),
            snapshot_generation,
            policy_fingerprint: POLICY.into(),
        },
    )
    .unwrap();
    let staged = chunks
        .iter()
        .cloned()
        .map(|mut input| {
            input.stage_id = stage_id.into();
            input
        })
        .collect::<Vec<_>>();
    for input in &staged {
        stage_chunk(connection, input).unwrap();
    }
    let digests = staged
        .iter()
        .map(|input| (input.chunk_id.as_str(), chunk_content_hash(input)))
        .collect::<Vec<_>>();
    commit_stage(connection, stage_id, &content_manifest(&digests)).unwrap()
}

fn stage_input(stage_id: &str) -> SearchStageInput {
    SearchStageInput {
        stage_id: stage_id.into(),
        snapshot_id: SNAPSHOT_A.into(),
        snapshot_generation: SNAPSHOT_GEN,
        policy_fingerprint: POLICY.into(),
    }
}

fn scope_a() -> SearchScope {
    SearchScope {
        snapshot_id: SNAPSHOT_A.into(),
        snapshot_generation: SNAPSHOT_GEN,
        persona_id: PERSONA_A.into(),
        grant_ids: vec![GRANT_A.into()],
        binding_ids: vec![BINDING_A.into()],
        period: SearchPeriod {
            from: None,
            to: None,
            include_undated: true,
        },
    }
}

fn search_ids(connection: &Connection, request: &SearchRequest) -> Vec<String> {
    search(connection, request)
        .unwrap()
        .hits
        .into_iter()
        .map(|hit| hit.chunk_id)
        .collect()
}

fn assert_ordering(hits: &[SearchHit]) {
    for pair in hits.windows(2) {
        let (left, right) = (&pair[0], &pair[1]);
        assert!(
            left.bm25 < right.bm25 || (left.bm25 == right.bm25 && left.chunk_id <= right.chunk_id),
            "hits must be ordered by bm25 then chunk_id"
        );
    }
}

#[test]
fn exact_prefix_and_mixed_language_terms_find_fixture_chunks_with_exact_refs() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let request = |text: &str| SearchRequest {
        scope: scope_a(),
        query_text: text.into(),
        limit: 20,
        offset: 0,
    };
    let response = search(&connection, &request("субботу")).unwrap();
    let mut ids = response
        .hits
        .iter()
        .map(|hit| hit.chunk_id.as_str())
        .collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, ["chunk:1", "chunk:5"]);
    assert_ordering(&response.hits);
    // Exact tokens only: the morphological variant without a prefix star is
    // not expanded, so "суббота" does not match "субботу".
    assert!(search(&connection, &request("суббота"))
        .unwrap()
        .hits
        .is_empty());
    let first = response
        .hits
        .iter()
        .find(|hit| hit.chunk_id == "chunk:1")
        .expect("chunk:1 must be found");
    let chunk_one_len = "Марья: Привет! Договорились встретиться в субботу у озера.\r\nАнна: Отлично, я принесу договор на подпись.".len() as i64;
    assert_eq!((first.span_start, first.span_end), (0, chunk_one_len));
    assert_eq!(first.document_id, "document:chunk:1");
    assert_eq!(first.revision_id, "revision:chunk:1");
    assert_eq!(
        first.conversation_id.as_deref(),
        Some("conversation:chat-1")
    );
    assert_eq!(first.thread_id.as_deref(), Some("thread:chunk:1"));
    assert_eq!(first.index_generation, 1);
    let mut prefix_ids = search_ids(&connection, &request("суббо*"));
    prefix_ids.sort();
    assert_eq!(prefix_ids, ["chunk:1", "chunk:5"]);
    assert_eq!(search_ids(&connection, &request("договор")), ["chunk:1"]);
    assert_eq!(search_ids(&connection, &request("example")), ["chunk:2"]);
    assert_eq!(search_ids(&connection, &request("x_y")), ["chunk:2"]);
    assert_eq!(search_ids(&connection, &request("марья")), ["chunk:1"]);
    assert_eq!(
        search_ids(&connection, &request("субботу, озера!")),
        ["chunk:1"]
    );
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn fixture_recall_is_measurable_and_complete_for_both_scopes() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    // Fixed query set with expected hits per authorized scope; recall is the
    // fraction of expected chunks returned and must stay 1.0 for the fixture.
    let cases_a: &[(&str, &[&str])] = &[
        ("субботу", &["chunk:1", "chunk:5"]),
        ("суббо*", &["chunk:1", "chunk:5"]),
        ("суббота", &[]),
        ("договор", &["chunk:1"]),
        ("example", &["chunk:2"]),
        ("x_y", &["chunk:2"]),
        ("марья", &["chunk:1"]),
        ("подпись", &["chunk:1"]),
        ("субботу озера", &["chunk:1"]),
        ("точный переводом", &["chunk:6"]),
        ("emoji", &["chunk:6"]),
    ];
    for (query, expected) in cases_a {
        let request = SearchRequest {
            scope: scope_a(),
            query_text: query.to_string(),
            limit: 50,
            offset: 0,
        };
        let mut got = search_ids(&connection, &request);
        got.sort();
        let expected = expected.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        assert_eq!(
            got, expected,
            "query {query:?} must have full fixture recall"
        );
    }
    let mut scope_b = scope_a();
    scope_b.persona_id = PERSONA_B.into();
    scope_b.grant_ids = vec![GRANT_B.into()];
    scope_b.binding_ids = vec![BINDING_B.into()];
    let request = SearchRequest {
        scope: scope_b,
        query_text: "суббота".into(),
        limit: 50,
        offset: 0,
    };
    assert_eq!(search_ids(&connection, &request), ["chunk:3"]);
    // The exact token really is matched lexically on both scopes.
    let request = SearchRequest {
        scope: scope_a(),
        query_text: "субботу".into(),
        limit: 50,
        offset: 0,
    };
    let mut scope_a_ids = search_ids(&connection, &request);
    scope_a_ids.sort();
    assert_eq!(scope_a_ids, ["chunk:1", "chunk:5"]);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn typo_term_returns_empty_instead_of_fabricated_correction() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let request = |text: &str| SearchRequest {
        scope: scope_a(),
        query_text: text.into(),
        limit: 20,
        offset: 0,
    };
    // No fuzzy correction: a mistyped token matches nothing.
    assert!(search(&connection, &request("субботаа"))
        .unwrap()
        .hits
        .is_empty());
    assert!(search(&connection, &request("подпис"))
        .unwrap()
        .hits
        .is_empty());
    // A prefix term covers morphology (договор/договора), not typos.
    assert_eq!(search_ids(&connection, &request("договор*")), ["chunk:1"]);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn holdout_and_unknown_partitions_fail_staging_explicitly() {
    let (mut connection, directory) = setup();
    begin_stage(
        &mut connection,
        &SearchStageInput {
            stage_id: "stage:1".into(),
            snapshot_id: SNAPSHOT_A.into(),
            snapshot_generation: SNAPSHOT_GEN,
            policy_fingerprint: POLICY.into(),
        },
    )
    .unwrap();
    let error = stage_chunk(&mut connection, &holdout_chunk()).unwrap_err();
    assert!(
        error.to_lowercase().contains("holdout"),
        "unexpected error: {error}"
    );
    let mut unknown = holdout_chunk();
    unknown.partition = "training".into();
    assert!(stage_chunk(&mut connection, &unknown).is_err());
    let mut valid = fixture_chunks().remove(0);
    valid.chunk_id = "chunk:ok".into();
    valid.stage_id = "stage:1".into();
    stage_chunk(&mut connection, &valid).unwrap();
    let digests = [("chunk:ok", chunk_content_hash(&valid))];
    let receipt = commit_stage(&mut connection, "stage:1", &content_manifest(&digests)).unwrap();
    assert_eq!(receipt.chunk_count, 1);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn malformed_and_oversized_queries_and_limits_are_rejected() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    // Natural words and/or/not/near and neutral punctuation (including `:`
    // inside URLs) are valid once rendered quoted; see
    // reserved_operator_words_are_legitimate_search_terms and
    // urls_underscores_and_unicode_accents_are_searchable_terms.
    let bad_queries = [
        "",
        "   ",
        "\"",
        "(",
        ")",
        "{a}",
        "[x]",
        "^x",
        "a\\b",
        "a*b",
        "a**",
        "*",
        "a**b",
        "суббота (воскресенье)",
        "\"суббота\"",
        "суббот\u{7}",
        "\u{1}",
    ];
    for query in bad_queries {
        let request = SearchRequest {
            scope: scope_a(),
            query_text: query.into(),
            limit: 10,
            offset: 0,
        };
        assert!(
            search(&connection, &request).is_err(),
            "query {query:?} must be rejected"
        );
    }
    let oversized = "а".repeat(MAX_QUERY_BYTES + 1);
    let request = SearchRequest {
        scope: scope_a(),
        query_text: oversized,
        limit: 10,
        offset: 0,
    };
    assert!(search(&connection, &request).is_err());
    let many_terms = (0..=MAX_TERMS).map(|_| "а").collect::<Vec<_>>().join(" ");
    let request = SearchRequest {
        scope: scope_a(),
        query_text: many_terms,
        limit: 10,
        offset: 0,
    };
    assert!(search(&connection, &request).is_err());
    let long_term = "а".repeat(MAX_TERM_BYTES + 1);
    let request = SearchRequest {
        scope: scope_a(),
        query_text: long_term,
        limit: 10,
        offset: 0,
    };
    assert!(search(&connection, &request).is_err());
    for (limit, offset) in [(0, 0), (MAX_LIMIT + 1, 0), (10, -1), (10, MAX_OFFSET + 1)] {
        let request = SearchRequest {
            scope: scope_a(),
            query_text: "суббота".into(),
            limit,
            offset,
        };
        assert!(
            search(&connection, &request).is_err(),
            "limit {limit} offset {offset} must be rejected"
        );
    }
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn unknown_and_future_snapshot_generations_are_rejected() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let mut future = scope_a();
    future.snapshot_generation = 7;
    let request = SearchRequest {
        scope: future,
        query_text: "суббота".into(),
        limit: 10,
        offset: 0,
    };
    assert!(search(&connection, &request).is_err());
    let mut unknown = scope_a();
    unknown.snapshot_id = "snapshot:never-published".into();
    let request = SearchRequest {
        scope: unknown,
        query_text: "суббота".into(),
        limit: 10,
        offset: 0,
    };
    assert!(search(&connection, &request).is_err());
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn rebuild_switches_generation_without_mixed_results_and_failed_commit_preserves_previous() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let request = |text: &str| SearchRequest {
        scope: scope_a(),
        query_text: text.into(),
        limit: 20,
        offset: 0,
    };
    assert_eq!(
        search_ids(&connection, &request("договорились")),
        ["chunk:1"]
    );
    // Rebuild generation 2: chunk:1 loses "Договорились" and gains "перенос".
    let mut rebuilt = fixture_chunks();
    rebuilt[0].text = "Марья: Привет! Перенос встречи, суббота занята.".into();
    rebuilt[0].revision_id = "revision:chunk:1:v2".into();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:2",
        &rebuilt,
    );
    assert_eq!(receipt.index_generation, 2);
    let response = search(&connection, &request("суббота")).unwrap();
    assert_eq!(response.index_generation, 2);
    assert!(response
        .hits
        .iter()
        .all(|hit| hit.index_generation == 2 && hit.revision_id != "revision:chunk:1"));
    assert!(search(&connection, &request("договорились"))
        .unwrap()
        .hits
        .is_empty());
    assert_eq!(search_ids(&connection, &request("перенос")), ["chunk:1"]);
    // A commit with a wrong manifest must fail and keep generation 2 active.
    publish_expecting_manifest_failure(&mut connection);
    assert_eq!(
        search(&connection, &request("суббота"))
            .unwrap()
            .index_generation,
        2
    );
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

fn publish_expecting_manifest_failure(connection: &mut Connection) {
    begin_stage(
        connection,
        &SearchStageInput {
            stage_id: "stage:3".into(),
            snapshot_id: SNAPSHOT_A.into(),
            snapshot_generation: SNAPSHOT_GEN,
            policy_fingerprint: POLICY.into(),
        },
    )
    .unwrap();
    let mut input = fixture_chunks().remove(0);
    input.stage_id = "stage:3".into();
    stage_chunk(connection, &input).unwrap();
    let wrong = StageManifest {
        chunk_count: 2,
        content_sha256: "0".repeat(64),
    };
    assert!(commit_stage(connection, "stage:3", &wrong).is_err());
    // The failed stage can be aborted cleanly and leaves no active switch.
    abort_stage(connection, "stage:3").unwrap();
}

#[test]
fn commit_blocked_by_concurrent_write_fails_closed_and_recovers() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    // Windows keeps an open database file locked even after a binding is shadowed.
    drop(connection);
    let mut input = fixture_chunks();
    input[0].text = "Марья: совершенно новый текст для поколения 2.".into();
    input[0].stage_id = "stage:2".into();
    let path = directory.join("corpus.db");
    // Short busy window so the blocked commit fails fast instead of waiting 5s.
    let mut blocked = open_database(&path);
    blocked.pragma_update(None, "busy_timeout", 250).unwrap();
    begin_stage(
        &mut blocked,
        &SearchStageInput {
            stage_id: "stage:2".into(),
            snapshot_id: SNAPSHOT_A.into(),
            snapshot_generation: SNAPSHOT_GEN,
            policy_fingerprint: POLICY.into(),
        },
    )
    .unwrap();
    stage_chunk(&mut blocked, &input[0]).unwrap();
    drop(blocked);
    // A concurrent writer holds an immediate transaction across the commit.
    let mut writer = open_database(&path);
    writer
        .execute_batch("CREATE TABLE IF NOT EXISTS blocker(value TEXT);")
        .unwrap();
    let tx = writer
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    tx.execute("INSERT INTO blocker VALUES ('held')", [])
        .unwrap();
    let mut blocked = open_database(&path);
    blocked.pragma_update(None, "busy_timeout", 250).unwrap();
    let digests = [("chunk:1", chunk_content_hash(&input[0]))];
    let result = commit_stage(&mut blocked, "stage:2", &content_manifest(&digests));
    assert!(result.is_err(), "commit under a held write lock must fail");
    tx.commit().unwrap();
    drop(writer);
    drop(blocked);
    let mut connection = open_database(&path);
    let request = SearchRequest {
        scope: scope_a(),
        query_text: "суббота".into(),
        limit: 10,
        offset: 0,
    };
    assert_eq!(search(&connection, &request).unwrap().index_generation, 1);
    // Retry after the lock is gone publishes normally.
    let receipt = publish(&mut connection, SNAPSHOT_A, SNAPSHOT_GEN, "stage:2", &input);
    assert_eq!(receipt.index_generation, 2);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn staged_checkpoint_survives_reopen_and_identical_rebegin_without_loss() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    drop(connection);
    // Crash between begin_stage and commit_stage: the durable staged
    // checkpoint must survive a reopen, and an identical begin_stage retry
    // must be a strict no-op (never a purge). Resuming then reproduces the
    // exact staged bytes.
    let path = directory.join("corpus.db");
    let staged: Vec<SearchChunkInput> = {
        let mut crashed = open_database(&path);
        begin_stage(&mut crashed, &stage_input("stage:crashed")).unwrap();
        let mut input = fixture_chunks();
        for item in &mut input {
            item.stage_id = "stage:crashed".into();
        }
        for item in &input {
            stage_chunk(&mut crashed, item).unwrap();
        }
        input
    };
    let mut connection = open_database(&path);
    // Identical retry preserves the checkpoint instead of clearing it.
    begin_stage(&mut connection, &stage_input("stage:crashed")).unwrap();
    let staged_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM search_stage_chunks WHERE stage_id='stage:crashed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(staged_count, staged.len() as i64);
    // Re-staging the same bytes is idempotent and still lossless.
    for item in &staged {
        stage_chunk(&mut connection, item).unwrap();
    }
    let digests = staged
        .iter()
        .map(|input| (input.chunk_id.as_str(), chunk_content_hash(input)))
        .collect::<Vec<_>>();
    let receipt = commit_stage(
        &mut connection,
        "stage:crashed",
        &content_manifest(&digests),
    )
    .unwrap();
    assert_eq!(receipt.chunk_count, staged.len() as i64);
    assert_eq!(receipt.index_generation, 2);
    // The published bytes are exactly the staged checkpoint bytes.
    let text = indexed_text(&connection, SNAPSHOT_A, 2, "chunk:1")
        .unwrap()
        .unwrap();
    assert_eq!(text, staged[0].text);
    let request = SearchRequest {
        scope: scope_a(),
        query_text: "договор".into(),
        limit: 10,
        offset: 0,
    };
    assert_eq!(search_ids(&connection, &request), ["chunk:1"]);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn conflicting_checkpoint_input_fails_until_explicit_abort() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    begin_stage(&mut connection, &stage_input("stage:crashed")).unwrap();
    let mut input = fixture_chunks();
    input[0].stage_id = "stage:crashed".into();
    stage_chunk(&mut connection, &input[0]).unwrap();
    // A retry with different stage parameters conflicts and fails.
    let mut conflicting = stage_input("stage:crashed");
    conflicting.policy_fingerprint = "tip8-split/2|corpus-search/1".into();
    assert!(begin_stage(&mut connection, &conflicting).is_err());
    // Mutating staged bytes under the same stage id fails instead of merging.
    let mut mutated = input[0].clone();
    mutated.text = "Марья: другой текст той же аварийной стадии.".into();
    assert!(stage_chunk(&mut connection, &mutated).is_err());
    // The staged checkpoint is untouched by both failed attempts.
    let staged_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM search_stage_chunks WHERE stage_id='stage:crashed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(staged_count, 1);
    let digest: String = connection
        .query_row(
            "SELECT content_sha256 FROM search_stage_chunks WHERE stage_id='stage:crashed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(digest, chunk_content_hash(&input[0]));
    // An explicit abort clears the checkpoint; only then may the id be reused
    // with different content.
    abort_stage(&mut connection, "stage:crashed").unwrap();
    begin_stage(&mut connection, &stage_input("stage:crashed")).unwrap();
    stage_chunk(&mut connection, &mutated).unwrap();
    let manifest_digests = [(mutated.chunk_id.as_str(), chunk_content_hash(&mutated))];
    let receipt = commit_stage(
        &mut connection,
        "stage:crashed",
        &content_manifest(&manifest_digests),
    )
    .unwrap();
    assert_eq!(receipt.chunk_count, 1);
    let request = SearchRequest {
        scope: scope_a(),
        query_text: "другой".into(),
        limit: 10,
        offset: 0,
    };
    assert_eq!(search_ids(&connection, &request), ["chunk:1"]);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn published_stage_retry_returns_existing_receipt_and_never_resets() {
    let (mut connection, directory) = setup();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let staged: Vec<SearchChunkInput> = fixture_chunks()
        .into_iter()
        .map(|mut input| {
            input.stage_id = "stage:1".into();
            input
        })
        .collect();
    let digests = || {
        staged
            .iter()
            .map(|input| (input.chunk_id.as_str(), chunk_content_hash(input)))
            .collect::<Vec<_>>()
    };
    // Retry begin_stage with identical parameters: explicit no-op; the
    // published stage must never be reset to staging or purged.
    begin_stage(&mut connection, &stage_input("stage:1")).unwrap();
    let state: String = connection
        .query_row(
            "SELECT state FROM search_stages WHERE stage_id='stage:1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "published");
    // Staging more content onto a published stage is rejected.
    let mut extra = fixture_chunks().remove(0);
    extra.stage_id = "stage:1".into();
    assert!(stage_chunk(&mut connection, &extra).is_err());
    // A same-manifest commit retry returns the existing publication receipt.
    let replay = commit_stage(&mut connection, "stage:1", &content_manifest(&digests())).unwrap();
    assert_eq!(replay, receipt);
    // A conflicting manifest on a published stage fails without overwriting.
    let wrong = StageManifest {
        chunk_count: receipt.chunk_count + 1,
        content_sha256: receipt.content_sha256.clone(),
    };
    assert!(commit_stage(&mut connection, "stage:1", &wrong).is_err());
    let state: String = connection
        .query_row(
            "SELECT state FROM search_stages WHERE stage_id='stage:1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "published");
    // The index is still the original generation and still searchable.
    assert_eq!(
        active_generation(&connection, SNAPSHOT_A).unwrap(),
        Some((receipt.index_generation, receipt.snapshot_generation))
    );
    let request = SearchRequest {
        scope: scope_a(),
        query_text: "субботу".into(),
        limit: 10,
        offset: 0,
    };
    let response = search(&connection, &request).unwrap();
    assert_eq!(response.index_generation, receipt.index_generation);
    assert!(!response.hits.is_empty());
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn abort_after_publish_keeps_durable_generation_receipt_and_index() {
    let (mut connection, directory) = setup();
    let staged = fixture_chunks();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &staged,
    );
    // Receipt fields must match the durable generation row exactly.
    let generation_row: (i64, i64, String, String, i64, String) = connection
        .query_row(
            "SELECT index_generation,snapshot_generation,policy_fingerprint,content_sha256,\
             chunk_count,created_at FROM search_generations WHERE snapshot_id=?1 AND index_generation=?2",
            rusqlite::params![SNAPSHOT_A, receipt.index_generation],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(generation_row.0, receipt.index_generation);
    assert_eq!(generation_row.1, receipt.snapshot_generation);
    assert_eq!(generation_row.2, receipt.policy_fingerprint);
    assert_eq!(generation_row.3, receipt.content_sha256);
    assert_eq!(generation_row.4, receipt.chunk_count);
    assert_eq!(generation_row.5, receipt.created_at);
    // Aborting after a successful publication removes only the stage
    // bookkeeping row; the durable idempotency receipt (the generation row
    // and the published chunks) must survive.
    abort_stage(&mut connection, "stage:1").unwrap();
    let stage_state: Option<String> = connection
        .query_row(
            "SELECT state FROM search_stages WHERE stage_id='stage:1'",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    assert!(stage_state.is_none());
    assert_eq!(
        active_generation(&connection, SNAPSHOT_A).unwrap(),
        Some((receipt.index_generation, receipt.snapshot_generation))
    );
    let request = SearchRequest {
        scope: scope_a(),
        query_text: "субботу".into(),
        limit: 10,
        offset: 0,
    };
    let response = search(&connection, &request).unwrap();
    assert_eq!(response.index_generation, receipt.index_generation);
    assert!(!response.hits.is_empty());
    // A later commit attempt on the aborted stage fails explicitly instead of
    // fabricating a second publication.
    let digests = staged
        .iter()
        .map(|input| (input.chunk_id.as_str(), chunk_content_hash(input)))
        .collect::<Vec<_>>();
    assert!(commit_stage(&mut connection, "stage:1", &content_manifest(&digests)).is_err());
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

fn manifest_for(stage_id: &str, chunks: &[SearchChunkInput]) -> StageManifest {
    let digests = chunks
        .iter()
        .cloned()
        .map(|mut input| {
            input.stage_id = stage_id.into();
            (input.chunk_id.clone(), chunk_content_hash(&input))
        })
        .collect::<Vec<_>>();
    content_manifest(&digests)
}

#[test]
fn published_retry_of_empty_stage_returns_its_own_receipt_not_latest_digest_match() {
    let (mut connection, directory) = setup();
    let empty = || content_manifest::<&str, &str>(&[]);
    // Two stages of different split/chunk policies over the same
    // reference-empty corpus produce the identical zero-chunk manifest.
    let mut stage_a = stage_input("stage:a");
    stage_a.policy_fingerprint = "tip8-split/1|corpus-search/1".into();
    let mut stage_b = stage_input("stage:b");
    stage_b.policy_fingerprint = "tip8-split/2|corpus-search/1".into();
    begin_stage(&mut connection, &stage_a).unwrap();
    let receipt_a = commit_stage(&mut connection, "stage:a", &empty()).unwrap();
    assert_eq!(receipt_a.index_generation, 1);
    assert_eq!(receipt_a.policy_fingerprint, stage_a.policy_fingerprint);
    begin_stage(&mut connection, &stage_b).unwrap();
    let receipt_b = commit_stage(&mut connection, "stage:b", &empty()).unwrap();
    assert_eq!(receipt_b.index_generation, 2);
    assert_ne!(receipt_a.policy_fingerprint, receipt_b.policy_fingerprint);
    // Retrying stage A replays exactly A's publication: generation 1 with A's
    // policy. Resolving the retry by digest would fabricate a mixed receipt —
    // B's newer generation/time under A's policy.
    let replay_a = commit_stage(&mut connection, "stage:a", &empty()).unwrap();
    assert_eq!(replay_a, receipt_a);
    let replay_b = commit_stage(&mut connection, "stage:b", &empty()).unwrap();
    assert_eq!(replay_b, receipt_b);
    // Replays never reset the durable publication or the active pointer.
    assert_eq!(
        active_generation(&connection, SNAPSHOT_A).unwrap(),
        Some((2, SNAPSHOT_GEN))
    );
    let state: String = connection
        .query_row(
            "SELECT state FROM search_stages WHERE stage_id='stage:a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "published");
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn published_retry_returns_original_receipt_after_newer_same_policy_publish() {
    let (mut connection, directory) = setup();
    // Per-chunk digests bind the stage id, so two non-empty stages can never
    // share a manifest; the stage-independent zero-chunk manifest of a
    // reference-empty corpus is the honest same-digest case. Both stages use
    // the same split/chunk policy here.
    let empty = || content_manifest::<&str, &str>(&[]);
    begin_stage(&mut connection, &stage_input("stage:1")).unwrap();
    let receipt = commit_stage(&mut connection, "stage:1", &empty()).unwrap();
    assert_eq!(receipt.index_generation, 1);
    // A second stage with the same policy publishes a newer generation;
    // retrying the first stage must still replay its own original receipt,
    // never the newer generation sharing the digest.
    begin_stage(&mut connection, &stage_input("stage:2")).unwrap();
    let second = commit_stage(&mut connection, "stage:2", &empty()).unwrap();
    assert_eq!(second.index_generation, 2);
    assert_eq!(second.content_sha256, receipt.content_sha256);
    assert_eq!(second.policy_fingerprint, receipt.policy_fingerprint);
    let replay = commit_stage(&mut connection, "stage:1", &empty()).unwrap();
    assert_eq!(replay, receipt);
    assert_eq!(replay.index_generation, 1);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn published_retry_after_prune_is_explicitly_unavailable() {
    let (mut connection, directory) = setup();
    let empty = || content_manifest::<&str, &str>(&[]);
    // Both stages publish the identical zero-chunk manifest under different
    // policies, so a digest-based retry cannot tell them apart after prune.
    let mut stage_a = stage_input("stage:a");
    stage_a.policy_fingerprint = "policy:a".into();
    let mut stage_b = stage_input("stage:b");
    stage_b.policy_fingerprint = "policy:b".into();
    begin_stage(&mut connection, &stage_a).unwrap();
    let receipt_a = commit_stage(&mut connection, "stage:a", &empty()).unwrap();
    begin_stage(&mut connection, &stage_b).unwrap();
    let receipt_b = commit_stage(&mut connection, "stage:b", &empty()).unwrap();
    assert_eq!(receipt_a.index_generation, 1);
    assert_eq!(receipt_b.index_generation, 2);
    // Prune leaves only the active generation 2.
    prune_generations(&mut connection, SNAPSHOT_A).unwrap();
    let remaining: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM search_generations WHERE snapshot_id=?1",
            [SNAPSHOT_A],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 1);
    // The pruned stage's retry is explicitly unavailable; it must never fall
    // back to the surviving generation that shares the zero-chunk digest.
    let error = commit_stage(&mut connection, "stage:a", &empty()).unwrap_err();
    assert!(
        error.contains("недоступн"),
        "unexpected prune-replay error: {error}"
    );
    // The surviving stage still replays its precise receipt.
    let replay_b = commit_stage(&mut connection, "stage:b", &empty()).unwrap();
    assert_eq!(replay_b, receipt_b);
    // The failed replay changed nothing durable.
    assert_eq!(
        active_generation(&connection, SNAPSHOT_A).unwrap(),
        Some((2, SNAPSHOT_GEN))
    );
    let state: String = connection
        .query_row(
            "SELECT state FROM search_stages WHERE stage_id='stage:a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "published");
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn published_retry_rechecks_snapshot_row_and_generation() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let manifest = manifest_for("stage:1", &fixture_chunks());
    // The corpus lifecycle marks snapshots immutable; drop only this fixture
    // trigger so the replay-side reaction to lifecycle changes can be
    // exercised synthetically.
    connection
        .execute_batch("DROP TRIGGER immutable_snapshots")
        .unwrap();
    // The snapshot generation advanced underneath the still-published index:
    // the retry must fail closed instead of replaying an obsolete receipt.
    connection
        .execute(
            "UPDATE snapshots SET generation=2 WHERE id=?1",
            [SNAPSHOT_A],
        )
        .unwrap();
    let error = commit_stage(&mut connection, "stage:1", &manifest).unwrap_err();
    assert!(
        error.contains("Поколение"),
        "unexpected replay error for advanced snapshot: {error}"
    );
    // The snapshot row was deleted entirely: fail closed as well, never
    // replay a publication into a deleted snapshot.
    connection
        .execute("DELETE FROM snapshots WHERE id=?1", [SNAPSHOT_A])
        .unwrap();
    let error = commit_stage(&mut connection, "stage:1", &manifest).unwrap_err();
    assert!(
        error.contains("Снимок"),
        "unexpected replay error for deleted snapshot: {error}"
    );
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn grant_persona_binding_period_and_snapshot_filters_enforced_before_candidates() {
    let (mut connection, directory) = setup();
    let mut all = fixture_chunks();
    all.push(snapshot_b_chunk());
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &all[..5],
    );
    publish(
        &mut connection,
        SNAPSHOT_B,
        1,
        "stage:b",
        std::slice::from_ref(all.last().unwrap()),
    );
    let request = |scope: SearchScope, query: &str| SearchRequest {
        scope,
        query_text: query.into(),
        limit: 50,
        offset: 0,
    };
    // Persona and grant filters: persona B scope sees only its own chunk,
    // which lexically contains the exact token "суббота".
    let mut scope_b = scope_a();
    scope_b.persona_id = PERSONA_B.into();
    scope_b.grant_ids = vec![GRANT_B.into()];
    scope_b.binding_ids = vec![BINDING_B.into()];
    assert_eq!(
        search_ids(&connection, &request(scope_b, "суббота")),
        ["chunk:3"]
    );
    // A foreign grant/binding from another snapshot can never widen snapshot A.
    let mut mixed = scope_a();
    mixed.grant_ids = vec![GRANT_A.into(), GRANT_C.into()];
    mixed.binding_ids = vec![BINDING_A.into(), BINDING_C.into()];
    let mut mixed_ids = search_ids(&connection, &request(mixed, "субботу"));
    mixed_ids.sort();
    assert_eq!(mixed_ids, ["chunk:1", "chunk:5"]);
    // Snapshot B is isolated: same persona, foreign grant only -> no hits there either.
    let mut scope_b_snapshot = scope_a();
    scope_b_snapshot.snapshot_id = SNAPSHOT_B.into();
    scope_b_snapshot.grant_ids = vec![GRANT_C.into()];
    scope_b_snapshot.binding_ids = vec![BINDING_C.into()];
    assert_eq!(
        search_ids(&connection, &request(scope_b_snapshot, "субботу")),
        ["chunk:b1"]
    );
    // Selected time filter: dated 2023 window keeps chunk:1, drops undated chunk:5.
    let mut dated = scope_a();
    dated.period = SearchPeriod {
        from: Some(1672531200),
        to: Some(1675209600),
        include_undated: false,
    };
    assert_eq!(
        search_ids(&connection, &request(dated, "субботу")),
        ["chunk:1"]
    );
    // Unknown time cannot bypass a selected filter when undated chunks are off.
    let mut strict = scope_a();
    strict.period = SearchPeriod {
        from: Some(1654041600),
        to: Some(1654041600),
        include_undated: false,
    };
    assert!(search(&connection, &request(strict.clone(), "субботу"))
        .unwrap()
        .hits
        .is_empty());
    strict.period.include_undated = true;
    assert_eq!(
        search_ids(&connection, &request(strict, "субботу")),
        ["chunk:5"]
    );
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn unicode_crlf_and_whitespace_survive_indexing_exactly() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let text = indexed_text(&connection, SNAPSHOT_A, 1, "chunk:1")
        .unwrap()
        .unwrap();
    assert_eq!(
        text,
        "Марья: Привет! Договорились встретиться в субботу у озера.\r\nАнна: Отлично, я принесу договор на подпись."
    );
    let text = indexed_text(&connection, SNAPSHOT_A, 1, "chunk:6")
        .unwrap()
        .unwrap();
    assert_eq!(text, "  точный текст с переводом строки\r\nи emoji 😀  ");
    let request = SearchRequest {
        scope: scope_a(),
        query_text: "переводом".into(),
        limit: 10,
        offset: 0,
    };
    assert_eq!(search_ids(&connection, &request), ["chunk:6"]);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn results_are_bounded_pages_are_deterministic_and_offsets_work() {
    let (mut connection, directory) = setup();
    let mut chunks = fixture_chunks();
    for index in 0..12 {
        let mut input = chunk(
            &format!("chunk:tie:{index:02}"),
            "одинаковый текст про субботу",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (Some(1672876800), Some(1672876800)),
        );
        input.snapshot_id = SNAPSHOT_A.into();
        chunks.push(input);
    }
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &chunks,
    );
    let page = |limit, offset| {
        let request = SearchRequest {
            scope: scope_a(),
            query_text: "субботу".into(),
            limit,
            offset,
        };
        search(&connection, &request).unwrap()
    };
    let first = page(5, 0);
    assert_eq!(first.hits.len(), 5);
    assert!(first.has_more);
    let second = page(5, 5);
    assert_eq!(second.hits.len(), 5);
    assert!(second.has_more);
    let third = page(5, 10);
    assert_eq!(third.hits.len(), 4);
    assert!(!third.has_more);
    let ids = |response: &SearchResponse| {
        response
            .hits
            .iter()
            .map(|hit| hit.chunk_id.clone())
            .collect::<Vec<_>>()
    };
    // Identical scores must tie-break by chunk_id, so paging never duplicates.
    let mut all = ids(&first);
    all.extend(ids(&second));
    all.extend(ids(&third));
    let mut sorted = all.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(all.len(), sorted.len(), "pages must not repeat a chunk");
    let repeat = page(7, 0);
    assert_eq!(&ids(&repeat)[..5], &ids(&first)[..5]);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn duplicate_stage_and_chunk_ids_fail_or_are_strictly_idempotent() {
    let (mut connection, directory) = setup();
    let stage = SearchStageInput {
        stage_id: "stage:1".into(),
        snapshot_id: SNAPSHOT_A.into(),
        snapshot_generation: SNAPSHOT_GEN,
        policy_fingerprint: POLICY.into(),
    };
    begin_stage(&mut connection, &stage).unwrap();
    // Same id and same parameters is a strict no-op.
    begin_stage(&mut connection, &stage).unwrap();
    let mut different = stage.clone();
    different.snapshot_generation = 2;
    assert!(begin_stage(&mut connection, &different).is_err());
    let mut input = fixture_chunks().remove(0);
    input.stage_id = "stage:1".into();
    stage_chunk(&mut connection, &input).unwrap();
    // Re-staging byte-identical content is idempotent; mutation is rejected.
    stage_chunk(&mut connection, &input).unwrap();
    let mut changed = input.clone();
    changed.text = "Марья: изменённый текст.".into();
    assert!(stage_chunk(&mut connection, &changed).is_err());
    abort_stage(&mut connection, "stage:1").unwrap();
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn invalid_scope_and_invalid_chunk_inputs_are_rejected() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let request = SearchRequest {
        scope: scope_a(),
        query_text: "суббота".into(),
        limit: 10,
        offset: 0,
    };
    let mut empty_persona = request.clone();
    empty_persona.scope.persona_id.clear();
    assert!(search(&connection, &empty_persona).is_err());
    let mut no_grants = request.clone();
    no_grants.scope.grant_ids.clear();
    assert!(search(&connection, &no_grants).is_err());
    let mut empty_grant = request.clone();
    empty_grant.scope.grant_ids = vec![String::new()];
    assert!(search(&connection, &empty_grant).is_err());
    let mut no_bindings = request.clone();
    no_bindings.scope.binding_ids.clear();
    assert!(search(&connection, &no_bindings).is_err());
    let mut inverted = request.clone();
    inverted.scope.period = SearchPeriod {
        from: Some(200),
        to: Some(100),
        include_undated: true,
    };
    assert!(search(&connection, &inverted).is_err());
    // Chunks: empty text, bad spans, empty ids, bad generations, snapshot drift.
    let mut bad = fixture_chunks().remove(0);
    bad.stage_id = "stage:2".into();
    begin_stage(
        &mut connection,
        &SearchStageInput {
            stage_id: "stage:2".into(),
            snapshot_id: SNAPSHOT_A.into(),
            snapshot_generation: SNAPSHOT_GEN,
            policy_fingerprint: POLICY.into(),
        },
    )
    .unwrap();
    let cases: Vec<SearchChunkInput> = {
        let base = &bad;
        let mut no_text = base.clone();
        no_text.text.clear();
        let mut bad_span = base.clone();
        bad_span.span_end = bad_span.span_start - 1;
        let mut empty_doc = base.clone();
        empty_doc.document_id.clear();
        let mut bad_record_gen = base.clone();
        bad_record_gen.record_generation = 0;
        let mut bad_snapshot_gen = base.clone();
        bad_snapshot_gen.snapshot_generation = 0;
        let mut other_snapshot = base.clone();
        other_snapshot.snapshot_id = SNAPSHOT_B.into();
        vec![
            no_text,
            bad_span,
            empty_doc,
            bad_record_gen,
            bad_snapshot_gen,
            other_snapshot,
        ]
    };
    for case in cases {
        assert!(
            stage_chunk(&mut connection, &case).is_err(),
            "chunk {} must be rejected",
            case.chunk_id
        );
    }
    bad.text = "x".repeat(MAX_CHUNK_BYTES + 1);
    assert!(stage_chunk(&mut connection, &bad).is_err());
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn commit_requires_matching_snapshot_row_and_prune_removes_old_generations() {
    let (mut connection, directory) = setup();
    let orphan = chunk(
        "chunk:orphan",
        "Снимок без строки в snapshots не должен публиковаться.",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    );
    let mut orphan = orphan;
    orphan.snapshot_id = "snapshot:orphan".into();
    assert!(begin_stage(
        &mut connection,
        &SearchStageInput {
            stage_id: "stage:orphan".into(),
            snapshot_id: "snapshot:orphan".into(),
            snapshot_generation: 1,
            policy_fingerprint: POLICY.into(),
        },
    )
    .is_err());
    // A stage declaring a stale generation of an existing snapshot is rejected.
    assert!(begin_stage(
        &mut connection,
        &SearchStageInput {
            stage_id: "stage:wrong-gen".into(),
            snapshot_id: SNAPSHOT_A.into(),
            snapshot_generation: 9,
            policy_fingerprint: POLICY.into(),
        },
    )
    .is_err());
    // Rebuild twice, then prune: only the active generation remains searchable.
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let mut rebuilt = fixture_chunks();
    rebuilt[0].text = "Второе поколение текста про субботу.".into();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:2",
        &rebuilt,
    );
    assert_eq!(
        active_generation(&connection, SNAPSHOT_A).unwrap(),
        Some((2, 1))
    );
    prune_generations(&mut connection, SNAPSHOT_A).unwrap();
    assert_eq!(
        active_generation(&connection, SNAPSHOT_A).unwrap(),
        Some((2, 1))
    );
    let request = SearchRequest {
        scope: scope_a(),
        query_text: "субботу".into(),
        limit: 20,
        offset: 0,
    };
    let response = search(&connection, &request).unwrap();
    assert!(response.hits.iter().all(|hit| hit.index_generation == 2));
    assert!(indexed_text(&connection, SNAPSHOT_A, 1, "chunk:1")
        .unwrap()
        .is_none());
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn concurrent_reads_never_observe_mixed_generations() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let mut rebuilt = fixture_chunks();
    rebuilt[0].text = "Поколение два: Марья перенесла встречу, суббота не подходит.".into();
    let path = directory.join("corpus.db");
    let publish_started = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let barrier = Arc::new(Barrier::new(2));
    let reader_path = path.clone();
    let reader_started = publish_started.clone();
    let reader_stop = stop.clone();
    let reader_barrier = barrier.clone();
    let reader = std::thread::spawn(move || {
        let connection = open_database(&reader_path);
        let request = SearchRequest {
            scope: scope_a(),
            query_text: "суббота".into(),
            limit: 50,
            offset: 0,
        };
        let mut seen = Vec::new();
        reader_barrier.wait();
        while !reader_stop.load(Ordering::Relaxed) {
            let response = search(&connection, &request).unwrap();
            let generation = response.index_generation;
            assert!(
                response
                    .hits
                    .iter()
                    .all(|hit| hit.index_generation == generation),
                "one response must never mix generations"
            );
            if generation == 2 {
                assert!(
                    reader_started.load(Ordering::Relaxed),
                    "new generation before publish started"
                );
            }
            if seen.last() != Some(&generation) {
                seen.push(generation);
            }
        }
        seen
    });
    barrier.wait();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:2",
        &rebuilt,
    );
    publish_started.store(true, Ordering::Relaxed);
    let mut spins = 0;
    loop {
        let request = SearchRequest {
            scope: scope_a(),
            query_text: "суббота".into(),
            limit: 1,
            offset: 0,
        };
        if search(&connection, &request).unwrap().index_generation == 2 {
            break;
        }
        spins += 1;
        assert!(spins < 1000, "reader never converged");
        std::thread::yield_now();
    }
    stop.store(true, Ordering::Relaxed);
    let seen = reader.join().unwrap();
    assert_eq!(seen.first(), Some(&1));
    assert_eq!(seen.last(), Some(&2));
    assert!(
        seen.windows(2).all(|pair| pair[0] <= pair[1]),
        "observed generations must be monotonic: {seen:?}"
    );
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

fn operator_word_chunks() -> Vec<SearchChunkInput> {
    vec![
        chunk(
            "chunk:ops",
            "Марья: это and or not near в обычном предложении без операторов.",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (Some(1672876800), Some(1672876800)),
        ),
        chunk(
            "chunk:url",
            "Ссылка https://example.com/path и snake_case_token здесь.",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (Some(1672876800), Some(1672876800)),
        ),
        chunk(
            "chunk:accent",
            "Марья: café composed и café decomposed.",
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (Some(1672876800), Some(1672876800)),
        ),
    ]
}

#[test]
fn reserved_operator_words_are_legitimate_search_terms() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &operator_word_chunks(),
    );
    let request = |text: &str| SearchRequest {
        scope: scope_a(),
        query_text: text.into(),
        limit: 20,
        offset: 0,
    };
    // FTS5 operator words are natural words in source text; quoted rendering
    // keeps them searchable instead of rejecting a legitimate query.
    for word in ["and", "or", "not", "near", "AND", "Or", "NOT", "Near"] {
        assert_eq!(
            search_ids(&connection, &request(word)),
            ["chunk:ops"],
            "word {word:?} must be searchable"
        );
    }
    // Multiple natural words still AND together, never widening the query.
    assert_eq!(search_ids(&connection, &request("and or")), ["chunk:ops"]);
    assert!(search(&connection, &request("and подпись"))
        .unwrap()
        .hits
        .is_empty());
    // Quoted prefix rendering keeps natural-word prefixes searchable too.
    assert_eq!(search_ids(&connection, &request("and*")), ["chunk:ops"]);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn urls_underscores_and_unicode_accents_are_searchable_terms() {
    let (mut connection, directory) = setup();
    let chunks = operator_word_chunks();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &chunks,
    );
    let request = |text: &str| SearchRequest {
        scope: scope_a(),
        query_text: text.into(),
        limit: 20,
        offset: 0,
    };
    // A full URL is split into ANDed terms by neutral punctuation; this never
    // widens the selection and never injects FTS5 syntax.
    assert_eq!(
        search_ids(&connection, &request("https://example.com/path")),
        ["chunk:url"]
    );
    assert_eq!(
        search_ids(&connection, &request("example.com/path")),
        ["chunk:url"]
    );
    assert_eq!(
        search_ids(&connection, &request("https example com path")),
        ["chunk:url"]
    );
    // Underscore stays inside one term (FTS5 token character).
    assert_eq!(
        search_ids(&connection, &request("snake_case_token")),
        ["chunk:url"]
    );
    assert_eq!(
        search_ids(&connection, &request("snake_case*")),
        ["chunk:url"]
    );
    // Composed and decomposed accent forms fold to the same lexical token.
    assert_eq!(search_ids(&connection, &request("café")), ["chunk:accent"]);
    assert_eq!(search_ids(&connection, &request("café")), ["chunk:accent"]);
    // Exact source text is preserved in the index in both directions.
    let text = indexed_text(&connection, SNAPSHOT_A, 1, "chunk:accent")
        .unwrap()
        .unwrap();
    assert_eq!(text, "Марья: café composed и café decomposed.");
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn search_pins_one_snapshot_and_survives_concurrent_publish_and_prune() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let path = directory.join("corpus.db");
    let reader = open_database(&path);
    // Controlled synchronization: the caller pins a WAL read snapshot at
    // generation 1 before the writer runs. The writer then publishes
    // generation 2 and prunes generation 1 while the reader snapshot is held;
    // the pinned search must still observe generation 1 completely instead of
    // an empty/partial/error response.
    reader.execute_batch("SAVEPOINT reader_pin").unwrap();
    let pinned_rows: i64 = reader
        .query_row("SELECT COUNT(*) FROM search_chunks", [], |row| row.get(0))
        .unwrap();
    assert!(pinned_rows > 0);
    let mut rebuilt = fixture_chunks();
    rebuilt[0].text = "Поколение два: Марья перенесла встречу, субботу не отменяем.".into();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:2",
        &rebuilt,
    );
    prune_generations(&mut connection, SNAPSHOT_A).unwrap();
    let request = |text: &str| SearchRequest {
        scope: scope_a(),
        query_text: text.into(),
        limit: 50,
        offset: 0,
    };
    // The pinned reader must see generation 1 completely, not empty/partial.
    let response = search(&reader, &request("субботу")).unwrap();
    assert_eq!(response.index_generation, 1);
    assert!(!response.hits.is_empty());
    assert!(response.hits.iter().all(|hit| hit.index_generation == 1));
    // After the caller releases the pin, the same search sees generation 2.
    reader
        .execute_batch("RELEASE SAVEPOINT reader_pin")
        .unwrap();
    let response = search(&reader, &request("субботу")).unwrap();
    assert_eq!(response.index_generation, 2);
    assert!(!response.hits.is_empty());
    assert!(response.hits.iter().all(|hit| hit.index_generation == 2));
    drop(reader);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn search_verifies_current_snapshot_row_before_serving_scope_results() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let request = SearchRequest {
        scope: scope_a(),
        query_text: "суббота".into(),
        limit: 10,
        offset: 0,
    };
    // The corpus lifecycle marks snapshots immutable; drop only this fixture
    // trigger so the search-side reaction to a generation bump can be
    // exercised synthetically.
    connection
        .execute_batch("DROP TRIGGER immutable_snapshots")
        .unwrap();
    // The snapshot generation advanced underneath the still-published index:
    // the obsolete index must not serve stale scope results.
    connection
        .execute(
            "UPDATE snapshots SET generation=2 WHERE id=?1",
            [SNAPSHOT_A],
        )
        .unwrap();
    let error = search(&connection, &request).unwrap_err();
    assert!(
        error.contains("Снимок"),
        "unexpected error for advanced snapshot: {error}"
    );
    // The snapshot row was deleted entirely: fail closed, never serve the
    // obsolete index of a deleted snapshot.
    connection
        .execute("DELETE FROM snapshots WHERE id=?1", [SNAPSHOT_A])
        .unwrap();
    let error = search(&connection, &request).unwrap_err();
    assert!(
        error.contains("Снимок"),
        "unexpected error for deleted snapshot: {error}"
    );
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn stage_chunk_enforces_cumulative_cap_before_inserting() {
    let (mut connection, directory) = setup();
    begin_stage(&mut connection, &stage_input("stage:cap")).unwrap();
    // Simulate a checkpoint already holding MAX_STAGE_CHUNKS-1 staged rows.
    // The production invariant keeps search_stages.chunk_count equal to the
    // staged row count; stage_chunk maintains both in the same transaction.
    let fake_digest = "ab".repeat(32);
    connection
        .execute(
            "WITH RECURSIVE cnt(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM cnt WHERE x < ?1)
             INSERT INTO search_stage_chunks(stage_id,chunk_id,content_sha256,snapshot_id,\
             snapshot_generation,partition,document_id,revision_id,span_start,span_end,persona_id,\
             grant_id,binding_id,record_id,record_generation)
             SELECT 'stage:cap','chunk:'||x,?2,'snapshot:corpus-a',1,'reference','document:'||x,\
             'revision:'||x,0,1,'persona:marya','grant:vk-messages','binding:vk-dasha',\
             'record:'||x,1 FROM cnt",
            rusqlite::params![MAX_STAGE_CHUNKS - 1, fake_digest],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE search_stages SET chunk_count=?1 WHERE stage_id='stage:cap'",
            [MAX_STAGE_CHUNKS - 1],
        )
        .unwrap();
    // The MAX_STAGE_CHUNKS-th chunk is still allowed...
    let mut last = chunk(
        "chunk:last",
        "Марья: последний допустимый чанк этапа.",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (Some(1672876800), Some(1672876800)),
    );
    last.stage_id = "stage:cap".into();
    stage_chunk(&mut connection, &last).unwrap();
    // ...and the next one fails before inserting.
    let mut over = last.clone();
    over.chunk_id = "chunk:over".into();
    over.text = "Марья: чанк сверх лимита.".into();
    let error = stage_chunk(&mut connection, &over).unwrap_err();
    assert!(
        error.contains("превышен") || error.contains("максимальн"),
        "unexpected cap error: {error}"
    );
    let staged_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM search_stage_chunks WHERE stage_id='stage:cap'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(staged_count, MAX_STAGE_CHUNKS);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn reversed_chunk_period_and_js_unsafe_numbers_are_rejected() {
    let (mut connection, directory) = setup();
    publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    begin_stage(&mut connection, &stage_input("stage:2")).unwrap();
    let base = {
        let mut input = fixture_chunks().remove(0);
        input.stage_id = "stage:2".into();
        input
    };
    let mut reversed = base.clone();
    reversed.period_from = Some(200);
    reversed.period_to = Some(100);
    assert!(stage_chunk(&mut connection, &reversed).is_err());
    let mut huge_period = base.clone();
    huge_period.period_from = Some(MAX_JS_SAFE_INTEGER + 1);
    assert!(stage_chunk(&mut connection, &huge_period).is_err());
    let mut negative_huge_period = base.clone();
    negative_huge_period.period_to = Some(-MAX_JS_SAFE_INTEGER - 1);
    assert!(stage_chunk(&mut connection, &negative_huge_period).is_err());
    let mut huge_record_generation = base.clone();
    huge_record_generation.record_generation = MAX_JS_SAFE_INTEGER + 1;
    assert!(stage_chunk(&mut connection, &huge_record_generation).is_err());
    let mut huge_snapshot_generation = base.clone();
    huge_snapshot_generation.snapshot_generation = MAX_JS_SAFE_INTEGER + 1;
    assert!(stage_chunk(&mut connection, &huge_snapshot_generation).is_err());
    // Numbers near the JS-safe boundary stay valid.
    let mut boundary = base.clone();
    boundary.chunk_id = "chunk:boundary".into();
    boundary.period_from = Some(MAX_JS_SAFE_INTEGER);
    boundary.period_to = Some(MAX_JS_SAFE_INTEGER);
    boundary.record_generation = MAX_JS_SAFE_INTEGER;
    stage_chunk(&mut connection, &boundary).unwrap();
    // Scope periods are validated to the same JS-safe boundary.
    let mut scope = scope_a();
    scope.period = SearchPeriod {
        from: Some(MAX_JS_SAFE_INTEGER + 1),
        to: None,
        include_undated: true,
    };
    let request = SearchRequest {
        scope,
        query_text: "суббота".into(),
        limit: 10,
        offset: 0,
    };
    assert!(search(&connection, &request).is_err());
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn empty_reference_corpus_publishes_and_searches_truthfully() {
    let (mut connection, directory) = setup();
    begin_stage(&mut connection, &stage_input("stage:empty")).unwrap();
    // A reference-empty corpus finishes publication with a zero-chunk
    // manifest: count 0 and the sha256 of the empty digest stream. No fake
    // chunks are synthesized.
    let receipt = commit_stage(
        &mut connection,
        "stage:empty",
        &content_manifest::<&str, &str>(&[]),
    )
    .unwrap();
    assert_eq!(receipt.chunk_count, 0);
    assert_eq!(receipt.content_sha256, EMPTY_CONTENT_SHA256);
    assert_eq!(
        active_generation(&connection, SNAPSHOT_A).unwrap(),
        Some((1, SNAPSHOT_GEN))
    );
    let response = search(
        &connection,
        &SearchRequest {
            scope: scope_a(),
            query_text: "суббота".into(),
            limit: 10,
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(response.index_generation, 1);
    assert!(response.hits.is_empty());
    assert!(!response.has_more);
    // A zero-chunk manifest with a wrong digest is rejected.
    begin_stage(&mut connection, &stage_input("stage:bad-digest")).unwrap();
    let wrong_digest = StageManifest {
        chunk_count: 0,
        content_sha256: "0".repeat(64),
    };
    assert!(commit_stage(&mut connection, "stage:bad-digest", &wrong_digest).is_err());
    // A nonzero manifest over an empty stage is rejected.
    begin_stage(&mut connection, &stage_input("stage:bad-count")).unwrap();
    let nonempty = content_manifest(&[("chunk:x", fake_digest())]);
    assert!(commit_stage(&mut connection, "stage:bad-count", &nonempty).is_err());
    // The failed stages can be aborted cleanly.
    abort_stage(&mut connection, "stage:bad-digest").unwrap();
    abort_stage(&mut connection, "stage:bad-count").unwrap();
    assert_eq!(
        active_generation(&connection, SNAPSHOT_A).unwrap(),
        Some((1, SNAPSHOT_GEN))
    );
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

fn fake_digest() -> String {
    "ab".repeat(32)
}

#[test]
fn stage_and_scope_generations_must_be_exact_at_the_javascript_boundary() {
    let mut stage = stage_input("safe-generation");
    stage.snapshot_generation = MAX_JS_SAFE_INTEGER;
    assert!(super::types::validate_stage(&stage).is_ok());
    stage.snapshot_generation += 1;
    assert!(super::types::validate_stage(&stage).is_err());
    let mut scope = scope_a();
    scope.snapshot_generation = MAX_JS_SAFE_INTEGER;
    assert!(super::types::validate_scope(&scope).is_ok());
    scope.snapshot_generation += 1;
    assert!(super::types::validate_scope(&scope).is_err());
}

#[test]
fn combining_marks_inside_words_remain_searchable_without_rewriting_originals() {
    let (mut connection, directory) = setup();
    let composed = "  naïve Amélie piñata\r\n";
    let decomposed = "  nai\u{0308}ve Ame\u{0301}lie pin\u{0303}ata\r\n";
    let chunks = [
        chunk(
            "composed",
            composed,
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (None, None),
        ),
        chunk(
            "decomposed",
            decomposed,
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
        "stage:marks",
        &chunks,
    );
    for query in [
        composed.trim(),
        decomposed.trim(),
        "nai\u{0308}v* Ame\u{0301}li* pin\u{0303}at*",
    ] {
        let response = search(
            &connection,
            &SearchRequest {
                scope: scope_a(),
                query_text: query.into(),
                limit: 10,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(response.hits.len(), 2, "query {query:?}");
    }
    for (id, original) in [("composed", composed), ("decomposed", decomposed)] {
        assert_eq!(
            indexed_text(&connection, SNAPSHOT_A, receipt.index_generation, id)
                .unwrap()
                .as_deref(),
            Some(original)
        );
    }
    assert!(super::query::parse_query("\u{0301}").is_err());
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn nonempty_search_text_cannot_claim_an_empty_original_span() {
    let (mut connection, directory) = setup();
    let stage = stage_input("stage:empty-span");
    begin_stage(&mut connection, &stage).unwrap();
    let mut input = chunk(
        "empty-span",
        "точные слова",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    );
    input.stage_id = stage.stage_id.clone();
    input.span_start = 17;
    input.span_end = 17;
    assert!(stage_chunk(&mut connection, &input).is_err());
    assert_eq!(
        connection
            .query_row(
                "SELECT chunk_count FROM search_stages WHERE stage_id=?1",
                [&stage.stage_id],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn shared_spans_keep_independent_scopes_and_excluded_rows_never_consume_a_page() {
    let (mut connection, directory) = setup();
    let text = "  Общий договор é\r\n👩‍💻  ";
    let mut chunks = vec![
        chunk(
            "0foreign",
            text,
            PERSONA_B,
            GRANT_B,
            BINDING_B,
            (Some(1), Some(1)),
        ),
        chunk(
            "1allowed",
            text,
            PERSONA_A,
            GRANT_A,
            BINDING_A,
            (Some(1), Some(1)),
        ),
        chunk(
            "2allowed",
            text,
            PERSONA_A,
            GRANT_C,
            BINDING_C,
            (Some(1), Some(1)),
        ),
    ];
    for chunk in &mut chunks {
        chunk.document_id = "shared:document".into();
        chunk.revision_id = "shared:revision".into();
    }
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "shared:stage",
        &chunks,
    );
    let mut request = SearchRequest {
        scope: scope_a(),
        query_text: "договор".into(),
        limit: 1,
        offset: 0,
    };
    request.scope.grant_ids = vec![GRANT_A.into(), GRANT_B.into(), GRANT_C.into()];
    request.scope.binding_ids = vec![BINDING_A.into(), BINDING_B.into(), BINDING_C.into()];
    let first = search(&connection, &request).unwrap();
    assert_eq!(first.hits[0].chunk_id, "1allowed");
    assert!(first.has_more);
    request.offset = 1;
    let second = search(&connection, &request).unwrap();
    assert_eq!(second.hits[0].chunk_id, "2allowed");
    assert!(!second.has_more);
    for hit in first.hits.iter().chain(&second.hits) {
        assert_eq!(hit.document_id, "shared:document");
        assert_eq!(hit.revision_id, "shared:revision");
        assert_eq!((hit.span_start, hit.span_end), (0, text.len() as i64));
        assert_eq!(
            indexed_text(
                &connection,
                SNAPSHOT_A,
                receipt.index_generation,
                &hit.chunk_id
            )
            .unwrap()
            .as_deref(),
            Some(text)
        );
    }
    request.offset = 0;
    request.scope = scope_a();
    let narrow = search(&connection, &request).unwrap();
    assert_eq!(narrow.hits[0].chunk_id, "1allowed");
    assert!(!narrow.has_more);
    request.scope.persona_id = PERSONA_B.into();
    request.scope.grant_ids = vec![GRANT_B.into()];
    request.scope.binding_ids = vec![BINDING_B.into()];
    assert_eq!(search_ids(&connection, &request), vec!["0foreign"]);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn shared_span_scope_cannot_replace_an_existing_chunk_identity() {
    let (mut connection, directory) = setup();
    begin_stage(&mut connection, &stage_input("scope-collision")).unwrap();
    let mut original = chunk(
        "shared",
        "Общий договор",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    );
    original.stage_id = "scope-collision".into();
    stage_chunk(&mut connection, &original).unwrap();
    for field in ["persona", "grant", "binding"] {
        let mut changed = original.clone();
        match field {
            "persona" => changed.persona_id = PERSONA_B.into(),
            "grant" => changed.grant_id = GRANT_B.into(),
            _ => changed.binding_id = BINDING_B.into(),
        }
        assert!(stage_chunk(&mut connection, &changed).is_err(), "{field}");
    }
    let receipt = commit_stage(
        &mut connection,
        "scope-collision",
        &content_manifest(&[(original.chunk_id.as_str(), chunk_content_hash(&original))]),
    )
    .unwrap();
    assert_eq!(receipt.chunk_count, 1);
    let response = search(
        &connection,
        &SearchRequest {
            scope: scope_a(),
            query_text: "договор".into(),
            limit: 1,
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(response.hits[0].persona_id, PERSONA_A);
    assert_eq!(response.hits[0].grant_id, GRANT_A);
    assert_eq!(response.hits[0].binding_id, BINDING_A);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn checked_index_commit_rolls_back_when_access_expires_inside_the_transaction() {
    use std::cell::Cell;
    let (connection, dir) = setup();
    let mut db = crate::corpus_database::CorpusDatabase { connection };
    let original = publish(
        &mut db.connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "old",
        &fixture_chunks(),
    );
    db.begin_search_stage(&stage_input("checked")).unwrap();
    let mut input = fixture_chunks().remove(0);
    input.stage_id = "checked".into();
    db.stage_search_chunk(&input).unwrap();
    let manifest = content_manifest(&[(input.chunk_id.clone(), chunk_content_hash(&input))]);
    let calls = Cell::new(0);
    let result = db.commit_search_stage("checked", &manifest, &|| {
        calls.set(calls.get() + 1);
        if calls.get() > 1 {
            Err("expired during commit".into())
        } else {
            Ok(())
        }
    });
    assert_eq!(result.err().as_deref(), Some("expired during commit"));
    assert_eq!(
        active_generation(&db.connection, SNAPSHOT_A)
            .unwrap()
            .unwrap()
            .0,
        original.index_generation
    );
    assert_eq!(
        db.connection
            .query_row(
                "SELECT count(*) FROM search_stage_chunks WHERE stage_id='checked'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert!(db.connection.is_autocommit());
    assert_eq!(
        db.commit_search_stage("checked", &manifest, &|| Ok(()))
            .unwrap()
            .index_generation,
        original.index_generation + 1
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn russian_numero_yo_and_complex_urls_have_explicit_lexical_results() {
    let (mut db, dir) = setup();
    let input = chunk(
        "query-review",
        "Договор № 7: Алёна встретит 田中太郎. https://example.test:8080/a?b=c&d=e",
        PERSONA_A,
        GRANT_A,
        BINDING_A,
        (None, None),
    );
    publish(&mut db, SNAPSHOT_A, SNAPSHOT_GEN, "query-review", &[input]);
    let request = |query: &str| SearchRequest {
        scope: scope_a(),
        query_text: query.into(),
        limit: 10,
        offset: 0,
    };
    assert!(
        !'№'.is_alphanumeric(),
        "verify the review's Unicode premise against this compiler"
    );
    for query in [
        "договор №",
        "договор №7",
        "алёна",
        "田中太郎",
        "https://example.test:8080/a?b=c&d=e",
    ] {
        assert_eq!(
            search_ids(&db, &request(query)),
            vec!["query-review"],
            "{query}"
        );
    }
    for query in ["алена", "田中", "договр"] {
        assert!(search_ids(&db, &request(query)).is_empty(), "{query}");
    }
    let error = search(&db, &request("№")).unwrap_err();
    assert_ne!(error, super::sql_error(rusqlite::Error::InvalidQuery));
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn final_index_commit_guard_restores_old_rows_pointer_and_staging_after_all_writes() {
    use std::cell::Cell;
    let (connection, dir) = setup();
    let mut db = crate::corpus_database::CorpusDatabase { connection };
    let mut input = fixture_chunks().remove(0);
    let mut last = 0;
    let total = Cell::new(0);
    for (stage, deny) in [("first-checked", false), ("late-denied", true)] {
        db.begin_search_stage(&stage_input(stage)).unwrap();
        input.stage_id = stage.into();
        db.stage_search_chunk(&input).unwrap();
        let manifest = content_manifest(&[(input.chunk_id.clone(), chunk_content_hash(&input))]);
        let calls = Cell::new(0);
        let result = db.commit_search_stage(stage, &manifest, &|| {
            calls.set(calls.get() + 1);
            if deny && calls.get() == total.get() {
                Err("expired immediately before commit".into())
            } else {
                Ok(())
            }
        });
        if deny {
            assert_eq!(
                result.err().as_deref(),
                Some("expired immediately before commit")
            );
            assert_eq!(
                active_generation(&db.connection, SNAPSHOT_A)
                    .unwrap()
                    .unwrap()
                    .0,
                last
            );
            assert_eq!(
                db.connection
                    .query_row(
                        "SELECT count(*) FROM search_generations WHERE snapshot_id=?1",
                        [SNAPSHOT_A],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                1
            );
            assert_eq!(
                db.connection
                    .query_row(
                        "SELECT count(*) FROM search_stage_chunks WHERE stage_id=?1",
                        [stage],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                1
            );
            assert_eq!(
                db.commit_search_stage(stage, &manifest, &|| Ok(()))
                    .unwrap()
                    .index_generation,
                last + 1
            );
        } else {
            last = result.unwrap().index_generation;
            total.set(calls.get());
            assert!(total.get() > 5);
        }
    }
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

// ==== Abandoned staging recovery (proteus-tip.9) ====
// `staging_ids`/`discard_staging` are low-level storage APIs, not an
// authorization or abandonment authority: the trusted caller identifies
// abandoned stage IDs and coordinates current owners. The fixtures stage
// synthetic IDs on the shared snapshot rows.

#[path = "corpus-search-recovery-tests.rs"]
mod recovery_tests;

// ==== Expected-publication receipt search (proteus-tip.9) ====
// `active_receipt`/`search_with_receipt` bind one search to the full
// publication identity pinned at commit time. A receipt is an identity pin,
// never an access token: the caller-authorized scope still bounds every hit.
// The fixtures reuse the shared private helpers above via `super`.

#[path = "corpus-search-receipt-tests.rs"]
mod receipt_tests;

#[path = "corpus-search-scan-tests.rs"]
mod scan_tests;

#[path = "corpus-search-selected-tests.rs"]
mod selected_tests;
