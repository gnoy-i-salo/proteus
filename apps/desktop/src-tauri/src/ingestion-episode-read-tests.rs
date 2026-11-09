use super::*;
use crate::ingestion::episode_read::{read, EpisodeReadInput};
fn episode_input<'a>(
    snapshot: &'a crate::ingestion::publication::SnapshotView,
    episode: &'a str,
) -> EpisodeReadInput<'a> {
    EpisodeReadInput {
        job_id: "job",
        snapshot_id: &snapshot.id,
        persona_id: "personA",
        episode_id: episode,
        period: input(snapshot).period,
        expected_index_generation: None,
        max_chunks: 100,
        max_source_bytes: 1024 * 1024,
        max_stored_bytes: 1024 * 1024,
        max_context_tokens: 10000,
        max_duration_ms: 60000,
    }
}
fn episode_ids(sources: &Sources, snapshot: &str) -> Vec<String> {
    sources
        .with_ingestion_job_state(|db| Ok(db.prepared_snapshot(snapshot)?.episode_ids))
        .unwrap()
}

#[test]
fn authorized_episode_expansion_preserves_real_originals_and_exact_chunk_accounting() {
    let (sources, dir) = local();
    let (op, snapshot, index) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut count = 0;
    let mut context_seen = false;
    for id in episode_ids(&sources, &snapshot.id) {
        let result = read(&sources, &op, &episode_input(&snapshot, &id), || 70002).unwrap();
        assert_eq!(result.source, index);
        assert_eq!(result.episode.id, id);
        assert_eq!(result.excerpts.len(), result.entries.len());
        for (excerpt, entry) in result.excerpts.iter().zip(&result.entries) {
            assert_eq!(excerpt.text, entry.chunk.text);
            assert_eq!(excerpt.span, entry.chunk.spans[0]);
            assert_eq!(excerpt.document.id, entry.original.reference.document_id);
        }
        assert_eq!(
            result.source_bytes,
            result
                .entries
                .iter()
                .map(|e| e.chunk.text.len())
                .sum::<usize>()
        );
        assert_eq!(
            result.context_tokens,
            result
                .entries
                .iter()
                .map(|e| e.chunk.token_count)
                .sum::<usize>()
        );
        for entry in &result.entries {
            assert_eq!(entry.chunk.episode_id, id);
            context_seen |= entry.chunk.text == "  контекст\r\n";
            sources
                .with_ingestion_database(&op, |db| {
                    let original = db.revision(&entry.original.reference)?;
                    assert_eq!(entry.original.document_json, original.document_json);
                    assert_eq!(entry.original.source_key, original.source_key);
                    Ok(())
                })
                .unwrap();
        }
        count += result.entries.len();
    }
    assert_eq!(count, index.chunk_count as usize);
    assert!(context_seen);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn episode_ids_do_not_bypass_model_permission_persona_generation_or_holdout() {
    for holdout in [0, 10000] {
        let (sources, dir) = local();
        let (op, snapshot, index) = indexed(&sources, &dir, holdout);
        let ids = episode_ids(&sources, &snapshot.id);
        assert!(!ids.is_empty());
        let mut input = episode_input(&snapshot, &ids[0]);
        assert!(read(&sources, &op, &input, || 70002).is_err());
        enable_model(&sources, &op);
        if holdout != 0 {
            for id in &ids {
                assert!(read(&sources, &op, &episode_input(&snapshot, id), || 70002).is_err());
            }
        } else {
            assert!(read(&sources, &op, &input, || 70002).is_ok());
            input.persona_id = "personB";
            assert!(read(&sources, &op, &input, || 70002).is_err());
            input.persona_id = "personA";
            input.expected_index_generation = Some(index.index_generation + 1);
            assert!(read(&sources, &op, &input, || 70002).is_err());
            input.expected_index_generation = None;
            input.episode_id = "arbitrary-episode";
            assert!(read(&sources, &op, &input, || 70002).is_err());
        }
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn episode_expansion_refuses_insufficient_bytes_tokens_time_or_period_without_partial_output() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let ids = episode_ids(&sources, &snapshot.id);
    for variant in 0..5 {
        let mut input = episode_input(&snapshot, &ids[0]);
        match variant {
            0 => input.max_source_bytes = 1,
            1 => input.max_stored_bytes = 1,
            2 => input.max_context_tokens = 1,
            3 => input.period.from = Some(1704067200001),
            _ => input.max_duration_ms = 1,
        }
        let calls = Cell::new(0);
        assert!(read(&sources, &op, &input, || {
            calls.set(calls.get() + 1);
            if variant == 4 {
                70002 + calls.get()
            } else {
                70002
            }
        })
        .is_err());
    }
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn episode_expansion_observes_revocation_and_final_clock_cancellation() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let ids = episode_ids(&sources, &snapshot.id);
    let input = episode_input(&snapshot, &ids[0]);
    let calls = Cell::new(0);
    read(&sources, &op, &input, || {
        calls.set(calls.get() + 1);
        70002
    })
    .unwrap();
    let final_call = calls.get();
    calls.set(0);
    assert!(read(&sources, &op, &input, || {
        calls.set(calls.get() + 1);
        if calls.get() == final_call {
            sources.cancel_ingestion_operation("job").unwrap();
        }
        70002
    })
    .is_err());
    drop(op);
    let op = sources
        .begin_ingestion_operation(
            "episode-read",
            crate::corpus_database::JobProvider::Telegram,
        )
        .unwrap();
    sources
        .with_ingestion_database(&op, |db| db.remove(&grant))
        .unwrap();
    assert!(read(&sources, &op, &input, || 70002).is_err());
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_episode_budgets_and_scope_fail_before_clock_or_storage() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    for variant in 0..8 {
        let mut input = episode_input(&snapshot, "episode");
        match variant {
            0 => input.max_chunks = 101,
            1 => input.max_source_bytes = 0,
            2 => input.max_stored_bytes = 64 * 1024 * 1024 + 1,
            3 => input.max_context_tokens = 0,
            4 => input.max_duration_ms = 60001,
            5 => input.expected_index_generation = Some(0),
            6 => input.episode_id = "",
            _ => {
                input.period.from = Some(2);
                input.period.to = Some(1);
            }
        }
        assert!(read(&sources, &op, &input, || panic!(
            "invalid input reached clock"
        ))
        .is_err());
    }
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn source_inspection_uses_corpus_permission_without_granting_model_permission() {
    let (sources, dir) = local();
    let (op, snapshot, index) = indexed(&sources, &dir, 0);
    let ids = episode_ids(&sources, &snapshot.id);
    let input = episode_input(&snapshot, &ids[0]);
    assert!(read(&sources, &op, &input, || 70002).is_err());
    let view =
        crate::ingestion::episode_read::read_for_display(&sources, &op, &input, || 70002).unwrap();
    assert_eq!(view.source, index);
    assert_eq!(view.episode.id, ids[0]);
    assert!(!view.excerpts.is_empty());
    for (excerpt, entry) in view.excerpts.iter().zip(&view.entries) {
        assert_eq!(excerpt.text, entry.chunk.text);
        assert_eq!(excerpt.span, entry.chunk.spans[0]);
        assert_eq!(excerpt.document.id, entry.original.reference.document_id);
    }
    assert!(read(&sources, &op, &input, || 70002).is_err());
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn inspection_wire_uses_exact_pinned_scope_and_only_verified_source_excerpts() {
    use crate::ingestion::inspection::{inspect, EpisodeInspectRequest};
    let (sources, dir) = local();
    let (op, snapshot, index) = indexed(&sources, &dir, 0);
    let ids = episode_ids(&sources, &snapshot.id);
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../packages/contracts/src/fixtures/corpus-inspection-wire.json"
    ))
    .unwrap();
    let mut wire = fixture["query"].clone();
    wire["schemaVersion"] = 1.into();
    wire["operationId"] = op.id().into();
    wire["snapshotId"] = snapshot.id.clone().into();
    wire["personaId"] = "personA".into();
    wire["episodeId"] = ids[0].clone().into();
    wire["expectedIndexGeneration"] = index.index_generation.into();
    let request: EpisodeInspectRequest = serde_json::from_value(wire.clone()).unwrap();
    request.validate().unwrap();
    let result = inspect(&sources, &op, &request, || 70002).unwrap();
    assert_eq!(result.episode_id, ids[0]);
    assert_eq!(result.index_generation, index.index_generation);
    let json = serde_json::to_value(&result).unwrap();
    let keys = |value: &serde_json::Value| {
        value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(keys(&json), keys(&fixture["result"]));
    assert_eq!(
        keys(&json["chunks"][0]),
        keys(&fixture["result"]["chunks"][0])
    );
    assert_eq!(
        keys(&json["chunks"][0]["excerpt"]),
        keys(&fixture["result"]["chunks"][0]["excerpt"])
    );
    assert!(!json.to_string().contains("documentJson"));
    for field in ["grantId", "allowModelProcessing", "operation_id"] {
        let mut invalid = wire.clone();
        invalid[field] = "foreign".into();
        assert!(serde_json::from_value::<EpisodeInspectRequest>(invalid).is_err());
    }
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn display_inspection_does_not_bypass_holdout_persona_period_or_index_pins() {
    use crate::ingestion::episode_read::read_for_display;
    for holdout in [0, 10000] {
        let (sources, dir) = local();
        let (op, snapshot, index) = indexed(&sources, &dir, holdout);
        let ids = episode_ids(&sources, &snapshot.id);
        let mut selected = episode_input(&snapshot, &ids[0]);
        if holdout != 0 {
            assert!(read_for_display(&sources, &op, &selected, || 70002).is_err());
        } else {
            assert!(read_for_display(&sources, &op, &selected, || 70002).is_ok());
            selected.persona_id = "personB";
            assert!(read_for_display(&sources, &op, &selected, || 70002).is_err());
            selected.persona_id = "personA";
            selected.expected_index_generation = Some(index.index_generation + 1);
            assert!(read_for_display(&sources, &op, &selected, || 70002).is_err());
            selected.expected_index_generation = Some(index.index_generation);
            selected.period.from = Some(1704067200001);
            assert!(read_for_display(&sources, &op, &selected, || 70002).is_err());
            selected.period.from = None;
            selected.max_source_bytes = 1;
            assert!(read_for_display(&sources, &op, &selected, || 70002).is_err());
        }
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn display_inspection_rechecks_revocation_expiry_and_final_cancellation() {
    use crate::ingestion::episode_read::read_for_display;
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let ids = episode_ids(&sources, &snapshot.id);
    let selected = episode_input(&snapshot, &ids[0]);
    assert!(read_for_display(&sources, &op, &selected, || 100000000).is_err());
    let calls = Cell::new(0);
    read_for_display(&sources, &op, &selected, || {
        calls.set(calls.get() + 1);
        70002
    })
    .unwrap();
    let final_call = calls.get();
    calls.set(0);
    assert!(read_for_display(&sources, &op, &selected, || {
        calls.set(calls.get() + 1);
        if calls.get() == final_call {
            sources.cancel_ingestion_operation("job").unwrap();
        }
        70002
    })
    .is_err());
    drop(op);
    let op = sources
        .begin_ingestion_operation("display", crate::corpus_database::JobProvider::Telegram)
        .unwrap();
    sources
        .with_ingestion_database(&op, |db| {
            let id = db.job("job")?.unwrap().state.selection.bindings[0]
                .grant_id
                .clone();
            db.remove(&id)
        })
        .unwrap();
    assert!(read_for_display(&sources, &op, &selected, || 70002).is_err());
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn inspection_scope_wire_rejects_invalid_budgets_and_operations_before_clock() {
    use crate::ingestion::inspection::{inspect, EpisodeInspectRequest};
    let (sources, dir) = local();
    let (op, snapshot, index) = indexed(&sources, &dir, 0);
    let ids = episode_ids(&sources, &snapshot.id);
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../packages/contracts/src/fixtures/corpus-inspection-wire.json"
    ))
    .unwrap();
    let mut wire = fixture["query"].clone();
    wire["schemaVersion"] = 1.into();
    wire["operationId"] = op.id().into();
    wire["snapshotId"] = snapshot.id.clone().into();
    wire["personaId"] = "personA".into();
    wire["episodeId"] = ids[0].clone().into();
    wire["expectedIndexGeneration"] = index.index_generation.into();
    for (key, bad) in [
        ("schemaVersion", serde_json::json!(99)),
        ("operationId", serde_json::json!("foreign")),
        ("expectedIndexGeneration", serde_json::json!(0)),
        ("maxDurationMs", serde_json::json!(60001)),
        ("personaId", serde_json::json!("person\n")),
    ] {
        let mut invalid = wire.clone();
        invalid[key] = bad;
        let request: EpisodeInspectRequest = serde_json::from_value(invalid).unwrap();
        assert!(inspect(&sources, &op, &request, || panic!(
            "invalid request reached clock"
        ))
        .is_err());
    }
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
