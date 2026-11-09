use super::*;
#[test]
fn research_catalog_uses_native_preparation_without_charging_analyzed_source_reads() {
    let (sources, dir) = local();
    let (op, snapshot, receipt) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let response = bound.research_catalog(100, || 70003).unwrap();
    assert_eq!(response.persona_id, "personA");
    let catalog = response.catalog;
    assert_eq!(catalog.snapshot_id, snapshot.id);
    assert_eq!(catalog.index_generation, receipt.index_generation);
    assert!(!catalog.samples.is_empty());
    for sample in &catalog.samples {
        assert!(snapshot.episode_ids.contains(&sample.episode_id));
        assert!(snapshot.binding_ids.contains(&sample.source_key));
        assert_eq!(
            sample.period_key,
            format!("window30d:{}", 1_704_067_200_000i64 / 2_592_000_000)
        );
    }
    assert_eq!(bound.usage().0, 0);
    assert_eq!(bound.usage().1.source_bytes, 0);
    let id = &catalog.samples[0].episode_id;
    assert_eq!(bound.read_episode(id, || 70004).unwrap().episode.id, *id);
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn research_catalog_never_includes_holdout_or_survives_revocation() {
    for holdout in [0, 10000] {
        let (sources, dir) = local();
        let (op, snapshot, _) = indexed(&sources, &dir, holdout);
        let grant = enable_model(&sources, &op);
        let mut bound = session(&sources, &snapshot, limits()).unwrap();
        let response = bound.research_catalog(100, || 70003).unwrap();
        assert_eq!(response.persona_id, "personA");
        let catalog = response.catalog;
        assert_eq!(catalog.samples.is_empty(), holdout == 10000);
        sources
            .with_ingestion_database(&op, |db| db.remove(&grant))
            .unwrap();
        assert!(bound.research_catalog(100, || 70004).is_err());
        drop(bound);
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn research_catalog_drops_results_when_cancelled_at_its_final_check() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let calls = Cell::new(0);
    bound
        .research_catalog(100, || {
            calls.set(calls.get() + 1);
            70003
        })
        .unwrap();
    let last = calls.get();
    calls.set(0);
    assert!(bound
        .research_catalog(100, || {
            calls.set(calls.get() + 1);
            if calls.get() == last {
                sources.cancel_ingestion_operation("memory").unwrap();
            }
            70003
        })
        .is_err());
    assert_eq!(bound.usage().0, 0);
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
