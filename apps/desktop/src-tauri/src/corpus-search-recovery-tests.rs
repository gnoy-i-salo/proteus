//! Recovery characterization; shared fixtures remain owned by the search tests.
use super::*;

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
    // so recovery must fail closed instead of lazy-migrating.
    connection
        .execute_batch(include_str!("corpus-schema.sql"))
        .unwrap();
    connection
}

#[test]
fn staging_ids_lists_literal_prefix_pages_and_never_lists_published_stages() {
    let (mut connection, directory) = setup();
    for stage_id in [
        "stage:alpha-%_wild",
        "stage:alpha-1",
        "stage:alpha-2",
        "stage:alphabet",
        "stage:beta-1",
    ] {
        begin_stage(&mut connection, &stage_input(stage_id)).unwrap();
    }
    // Publish one stage: its receipt row stays durable, but it must never be
    // listed as staging again.
    commit_stage(
        &mut connection,
        "stage:alpha-2",
        &content_manifest::<&str, &str>(&[]),
    )
    .unwrap();
    // Literal prefix: "stage:alpha-" matches the dash-separated IDs only, not
    // "stage:alphabet"; the published stage is excluded.
    assert_eq!(
        staging_ids(&connection, "stage:alpha-", None, 100).unwrap(),
        vec![
            "stage:alpha-%_wild".to_string(),
            "stage:alpha-1".to_string()
        ]
    );
    // Wildcard-looking characters are literal, not LIKE patterns: only the ID
    // starting with the exact bytes "stage:alpha-%" is returned.
    assert_eq!(
        staging_ids(&connection, "stage:alpha-%", None, 100).unwrap(),
        vec!["stage:alpha-%_wild".to_string()]
    );
    // The match is case-sensitive (binary collation), never ASCII-folded.
    assert!(staging_ids(&connection, "stage:ALPHA-", None, 100)
        .unwrap()
        .is_empty());
    // Keyset paging over the staging set: ordered pages, no overlap, and the
    // published "stage:alpha-2" never appears.
    let page_one = staging_ids(&connection, "stage:", None, 2).unwrap();
    assert_eq!(
        page_one,
        vec![
            "stage:alpha-%_wild".to_string(),
            "stage:alpha-1".to_string()
        ]
    );
    let page_two = staging_ids(&connection, "stage:", Some(page_one.last().unwrap()), 2).unwrap();
    assert_eq!(
        page_two,
        vec!["stage:alphabet".to_string(), "stage:beta-1".to_string()]
    );
    assert!(
        staging_ids(&connection, "stage:", Some(page_two.last().unwrap()), 2)
            .unwrap()
            .is_empty()
    );
    // The published stage still replays its exact publication receipt even
    // though listing never returns it.
    let replay = commit_stage(
        &mut connection,
        "stage:alpha-2",
        &content_manifest::<&str, &str>(&[]),
    )
    .unwrap();
    assert_eq!(replay.chunk_count, 0);
    assert_eq!(replay.content_sha256, EMPTY_CONTENT_SHA256);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn discard_staging_removes_requested_staging_rows_with_their_fts_rows_and_is_idempotent() {
    let (mut connection, directory) = setup();
    for stage_id in ["stage:r-1", "stage:r-2"] {
        begin_stage(&mut connection, &stage_input(stage_id)).unwrap();
    }
    let mut input = fixture_chunks().remove(0);
    input.stage_id = "stage:r-1".into();
    stage_chunk(&mut connection, &input).unwrap();
    let staged_rowid: i64 = connection
        .query_row(
            "SELECT rowid FROM search_stage_chunks WHERE stage_id='stage:r-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    // Missing IDs are harmless skips and do not affect the removed count.
    let removed = discard_staging(
        &mut connection,
        &[
            "stage:r-1".to_string(),
            "stage:r-2".to_string(),
            "stage:missing".to_string(),
        ],
    )
    .unwrap();
    assert_eq!(removed, 2);
    for stage_id in ["stage:r-1", "stage:r-2"] {
        let stage_rows: i64 = connection
            .query_row(
                "SELECT count(*) FROM search_stages WHERE stage_id=?1",
                [stage_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stage_rows, 0);
    }
    let chunk_rows: i64 = connection
        .query_row(
            "SELECT count(*) FROM search_stage_chunks WHERE stage_id='stage:r-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(chunk_rows, 0);
    let fts_rows: i64 = connection
        .query_row(
            "SELECT count(*) FROM search_stage_fts WHERE rowid=?1",
            [staged_rowid],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(fts_rows, 0);
    // Retries are idempotent: the same IDs now remove nothing.
    let retry = discard_staging(
        &mut connection,
        &[
            "stage:r-1".to_string(),
            "stage:r-2".to_string(),
            "stage:missing".to_string(),
        ],
    )
    .unwrap();
    assert_eq!(retry, 0);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn discard_staging_skips_published_stages_and_preserves_search_receipt_and_active_pointer() {
    let (mut connection, directory) = setup();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:pub",
        &fixture_chunks(),
    );
    let request = SearchRequest {
        scope: scope_a(),
        query_text: "субботу".into(),
        limit: 10,
        offset: 0,
    };
    let before = search(&connection, &request).unwrap();
    assert!(!before.hits.is_empty());
    // Nothing is staging after the publication, so listing is empty...
    assert!(staging_ids(&connection, "stage:", None, 100)
        .unwrap()
        .is_empty());
    // ...and discarding the published stage ID plus an unknown ID is a
    // harmless double skip, never a deleted publication.
    let removed = discard_staging(
        &mut connection,
        &["stage:pub".to_string(), "stage:never".to_string()],
    )
    .unwrap();
    assert_eq!(removed, 0);
    let state: String = connection
        .query_row(
            "SELECT state FROM search_stages WHERE stage_id='stage:pub'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "published");
    // Published search, receipt replay, generation row and the active pointer
    // are byte-for-byte unchanged.
    let after = search(&connection, &request).unwrap();
    assert_eq!(before, after);
    let replay = commit_stage(
        &mut connection,
        "stage:pub",
        &manifest_for("stage:pub", &fixture_chunks()),
    )
    .unwrap();
    assert_eq!(replay, receipt);
    assert_eq!(
        active_generation(&connection, SNAPSHOT_A).unwrap(),
        Some((receipt.index_generation, receipt.snapshot_generation))
    );
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
    let published_chunks: i64 = connection
        .query_row(
            "SELECT count(*) FROM search_chunks WHERE snapshot_id=?1 AND index_generation=?2",
            rusqlite::params![SNAPSHOT_A, receipt.index_generation],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(published_chunks, receipt.chunk_count);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn discard_staging_skips_a_stage_published_between_listing_and_deletion() {
    let (mut connection, directory) = setup();
    let race_input = SearchStageInput {
        stage_id: "stage:race".into(),
        snapshot_id: SNAPSHOT_B.into(),
        snapshot_generation: 1,
        policy_fingerprint: POLICY.into(),
    };
    begin_stage(&mut connection, &race_input).unwrap();
    let listed = staging_ids(&connection, "stage:race", None, 10).unwrap();
    assert_eq!(listed, vec!["stage:race".to_string()]);
    // The stage is published after listing but before the discard runs.
    let receipt = commit_stage(
        &mut connection,
        "stage:race",
        &content_manifest::<&str, &str>(&[]),
    )
    .unwrap();
    let removed = discard_staging(&mut connection, &listed).unwrap();
    assert_eq!(removed, 0);
    let state: String = connection
        .query_row(
            "SELECT state FROM search_stages WHERE stage_id='stage:race'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "published");
    let replay = commit_stage(
        &mut connection,
        "stage:race",
        &content_manifest::<&str, &str>(&[]),
    )
    .unwrap();
    assert_eq!(replay, receipt);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn discard_staging_rolls_back_all_cleanup_when_a_write_fails() {
    let (mut connection, directory) = setup();
    for stage_id in ["stage:poison", "stage:ok"] {
        begin_stage(&mut connection, &stage_input(stage_id)).unwrap();
    }
    let mut poison = fixture_chunks().remove(0);
    poison.stage_id = "stage:poison".into();
    stage_chunk(&mut connection, &poison).unwrap();
    let mut ok = fixture_chunks().remove(1);
    ok.stage_id = "stage:ok".into();
    stage_chunk(&mut connection, &ok).unwrap();
    // A fixture trigger fails the second stage's chunk delete, after the
    // first stage was already cleaned up inside the same transaction.
    connection
        .execute(
            "CREATE TRIGGER fixture_fail_discard BEFORE DELETE ON search_stage_chunks \
             WHEN OLD.stage_id='stage:ok' BEGIN SELECT RAISE(ABORT,'fixture discard failure'); END",
            [],
        )
        .unwrap();
    let result = discard_staging(
        &mut connection,
        &["stage:poison".to_string(), "stage:ok".to_string()],
    );
    assert!(result.is_err());
    // Every cleanup is rolled back: both stages are still staging, and both
    // content and FTS rows survive untouched.
    for stage_id in ["stage:poison", "stage:ok"] {
        let state: String = connection
            .query_row(
                "SELECT state FROM search_stages WHERE stage_id=?1",
                [stage_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "staging");
        let chunks: i64 = connection
            .query_row(
                "SELECT count(*) FROM search_stage_chunks WHERE stage_id=?1",
                [stage_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(chunks, 1);
    }
    let fts_rows: i64 = connection
        .query_row(
            "SELECT count(*) FROM search_stage_fts WHERE rowid IN \
             (SELECT rowid FROM search_stage_chunks)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(fts_rows, 2);
    // Recovery succeeds once the failing write is gone.
    connection
        .execute("DROP TRIGGER fixture_fail_discard", [])
        .unwrap();
    assert_eq!(
        discard_staging(
            &mut connection,
            &["stage:poison".to_string(), "stage:ok".to_string()],
        )
        .unwrap(),
        2
    );
    let remaining: i64 = connection
        .query_row(
            "SELECT count(*) FROM search_stages WHERE stage_id IN ('stage:poison','stage:ok')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 0);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn staging_recovery_rejects_invalid_input_and_fails_closed_on_unmigrated_schema() {
    let mut unmigrated = open_unmigrated_database();
    assert!(staging_ids(&unmigrated, "stage:", None, 10).is_err());
    assert!(discard_staging(&mut unmigrated, &["stage:a".to_string()]).is_err());
    drop(unmigrated);

    let (mut connection, directory) = setup();
    assert!(staging_ids(&connection, "", None, 10).is_err());
    assert!(staging_ids(&connection, "stage:", Some(""), 10).is_err());
    assert!(staging_ids(&connection, "stage:\u{0007}bell", None, 10).is_err());
    assert!(staging_ids(&connection, &format!("stage:{}", "a".repeat(300)), None, 10).is_err());
    assert!(staging_ids(&connection, "stage:", Some("stage:\u{0007}"), 10).is_err());
    assert!(staging_ids(&connection, "stage:", None, 0).is_err());
    assert!(staging_ids(&connection, "stage:", None, 101).is_err());
    assert!(staging_ids(&connection, "stage:", None, 100).is_ok());
    assert!(staging_ids(&connection, "stage:missing", None, 10)
        .unwrap()
        .is_empty());

    assert!(discard_staging(&mut connection, &[]).is_err());
    let oversized: Vec<String> = (0..101).map(|index| format!("stage:{index}")).collect();
    assert!(discard_staging(&mut connection, &oversized).is_err());
    assert!(discard_staging(
        &mut connection,
        &["stage:a".to_string(), "stage:a".to_string()]
    )
    .is_err());
    let long_id = "s".repeat(257);
    for invalid in ["", "stage:\u{0007}bell", long_id.as_str()] {
        assert!(discard_staging(&mut connection, &[invalid.to_string()]).is_err());
    }
    // The upper bound itself is accepted; unknown IDs just remove nothing.
    let hundred: Vec<String> = (0..100)
        .map(|index| format!("stage:missing-{index}"))
        .collect();
    assert_eq!(discard_staging(&mut connection, &hundred).unwrap(), 0);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}
