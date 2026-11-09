use super::*;

fn catalog_fixture(connection: &Connection, chunks: &[SearchChunkInput], episodes: &[&str]) {
    connection
        .execute_batch(include_str!("corpus-preparation-schema.sql"))
        .unwrap();
    connection
        .execute(
            "INSERT INTO corpus_preparations VALUES(?1,?2,'split','tokenizer','chunks','hash')",
            [SNAPSHOT_A, SNAPSHOT_B],
        )
        .unwrap();
    let mut seen = std::collections::HashSet::new();
    for (position, (chunk, episode)) in chunks.iter().zip(episodes).enumerate() {
        if seen.insert(episode) {
            connection.execute("INSERT INTO corpus_episodes VALUES(?1,?2,?3,'not read by metadata catalog','hash')", rusqlite::params![SNAPSHOT_A, episode, position as i64]).unwrap();
        }
        connection.execute("INSERT INTO corpus_prepared_chunks VALUES(?1,?2,?3,?4,'not read by metadata catalog','hash')", rusqlite::params![SNAPSHOT_A, chunk.chunk_id, episode, position as i64]).unwrap();
    }
}
fn sample_chunk(id: &str, binding: &str, time: Option<i64>) -> SearchChunkInput {
    chunk(
        id,
        "Exact original\r\n",
        PERSONA_A,
        GRANT_A,
        binding,
        (time, time),
    )
}
#[test]
fn catalog_round_robins_sources_and_periods_before_repeating_a_group() {
    let (mut db, dir) = setup();
    let chunks = vec![
        sample_chunk("1", BINDING_A, Some(0)),
        sample_chunk("2", BINDING_A, Some(0)),
        sample_chunk("3", BINDING_A, Some(2_592_000_000)),
        sample_chunk("4", BINDING_B, None),
        sample_chunk("5", BINDING_C, Some(-1)),
    ];
    catalog_fixture(
        &db,
        &chunks,
        &[
            "episode:1",
            "episode:2",
            "episode:3",
            "episode:4",
            "episode:5",
        ],
    );
    let receipt = publish(&mut db, SNAPSHOT_A, SNAPSHOT_GEN, "catalog", &chunks);
    let mut scope = scope_a();
    scope.binding_ids = vec![BINDING_A.into(), BINDING_B.into(), BINDING_C.into()];
    let all = research_catalog(&db, &scope, &receipt, 10, &|| Ok(())).unwrap();
    assert_eq!(all.eligible_episodes, 5);
    // Binding lexical order is archive, tg, vk; each gets its first sample
    // before vk gets its second period, then the repeat of its first period.
    assert_eq!(
        all.samples
            .iter()
            .map(|s| s.episode_id.as_str())
            .collect::<Vec<_>>(),
        [
            "episode:5",
            "episode:4",
            "episode:1",
            "episode:3",
            "episode:2"
        ]
    );
    assert_eq!(all.samples[0].period_key, "window30d:-1");
    assert_eq!(all.samples[1].period_key, "undated");
    let short = research_catalog(&db, &scope, &receipt, 2, &|| Ok(())).unwrap();
    assert_eq!(short.eligible_episodes, 5);
    assert_eq!(short.samples, all.samples[..2]);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn catalog_omits_out_of_scope_and_partial_episodes_without_reading_text() {
    let (mut db, dir) = setup();
    let mut foreign = sample_chunk("foreign", BINDING_A, Some(1));
    foreign.persona_id = PERSONA_B.into();
    let chunks = vec![
        sample_chunk("one", BINDING_A, Some(1)),
        sample_chunk("two", BINDING_A, Some(20)),
        sample_chunk("whole", BINDING_A, Some(1)),
        foreign,
    ];
    catalog_fixture(&db, &chunks, &["partial", "partial", "whole", "foreign"]);
    let receipt = publish(&mut db, SNAPSHOT_A, SNAPSHOT_GEN, "catalog", &chunks);
    db.execute("UPDATE search_fts SET text='unrelated corrupt text'", [])
        .unwrap();
    let mut scope = scope_a();
    scope.period.to = Some(10);
    let catalog = research_catalog(&db, &scope, &receipt, 100, &|| Ok(())).unwrap();
    assert_eq!(
        catalog
            .samples
            .iter()
            .map(|s| s.episode_id.as_str())
            .collect::<Vec<_>>(),
        ["whole"]
    );
    scope.grant_ids = vec![GRANT_B.into()];
    let empty = research_catalog(&db, &scope, &receipt, 100, &|| Ok(())).unwrap();
    assert_eq!(empty.eligible_episodes, 0);
    assert!(empty.samples.is_empty());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn catalog_checks_identity_limits_and_cancellation_before_disclosure() {
    let (mut db, dir) = setup();
    let chunks = vec![sample_chunk("one", BINDING_A, Some(1))];
    catalog_fixture(&db, &chunks, &["episode:1"]);
    let receipt = publish(&mut db, SNAPSHOT_A, SNAPSHOT_GEN, "catalog", &chunks);
    assert!(research_catalog(&db, &scope_a(), &receipt, 0, &|| Ok(())).is_err());
    assert!(research_catalog(&db, &scope_a(), &receipt, 10001, &|| Ok(())).is_err());
    let mut changed = receipt.clone();
    changed.content_sha256 = "a".repeat(64);
    assert!(research_catalog(&db, &scope_a(), &changed, 1, &|| Ok(())).is_err());
    let checks = std::cell::Cell::new(0);
    let result = research_catalog(&db, &scope_a(), &receipt, 1, &|| {
        checks.set(checks.get() + 1);
        if checks.get() >= 2 {
            Err("cancel catalog".into())
        } else {
            Ok(())
        }
    });
    assert_eq!(result.unwrap_err(), "cancel catalog");
    assert!(db.is_autocommit());
    assert_eq!(
        research_catalog(&db, &scope_a(), &receipt, 1, &|| Ok(()))
            .unwrap()
            .samples
            .len(),
        1
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn catalog_includes_episode_at_chunk_cap_and_excludes_episode_over_cap() {
    let (mut db, dir) = setup();
    let mut chunks = Vec::new();
    let mut episodes = Vec::new();
    // Whole-episode cap boundary: exactly 100 prepared chunks stays eligible,
    // 101 is excluded even though every chunk is inside the authorized scope.
    for index in 0..100 {
        chunks.push(sample_chunk(
            &format!("cap-ok-{index:03}"),
            BINDING_A,
            Some(0),
        ));
        episodes.push("episode:at-cap");
    }
    for index in 0..=100 {
        chunks.push(sample_chunk(
            &format!("cap-over-{index:03}"),
            BINDING_A,
            Some(0),
        ));
        episodes.push("episode:over-cap");
    }
    catalog_fixture(&db, &chunks, &episodes);
    let receipt = publish(&mut db, SNAPSHOT_A, SNAPSHOT_GEN, "catalog", &chunks);
    let catalog = research_catalog(&db, &scope_a(), &receipt, 100, &|| Ok(())).unwrap();
    assert_eq!(catalog.eligible_episodes, 1);
    assert_eq!(
        catalog
            .samples
            .iter()
            .map(|s| s.episode_id.as_str())
            .collect::<Vec<_>>(),
        ["episode:at-cap"]
    );
    assert_eq!(catalog.samples[0].source_key, BINDING_A);
    assert_eq!(catalog.samples[0].period_key, "window30d:0");
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
