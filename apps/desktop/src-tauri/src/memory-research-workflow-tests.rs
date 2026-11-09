use super::*;
use crate::memory::{
    context_wire::ModelContextRequest,
    registry::MemorySessions,
    research_draft::ResearchPublicationDraft,
    research_workflow::{self, OpenResearchRequest},
    session::{DispatchInput, InvocationBinding},
};
use serde_json::json;

fn request(snapshot_id: &str) -> OpenResearchRequest {
    serde_json::from_value(json!({
        "schemaVersion":1,"operationId":"research-operation","jobId":"job",
        "snapshotId":snapshot_id,"personaId":"personA","mode":"lexical",
        "period":{"from":null,"to":null,"includeUndated":true},
        "recallBudget":{"maxCalls":3,"maxCandidates":100,"maxEpisodes":10,
            "maxSourceBytes":1048576,"maxContextTokens":10000,"maxDurationMs":60000},
        "modelBudget":{"maxCalls":2,"maxInputBytes":100000,"maxDurationMs":1000},
        "model":{"provider":"codex","model":"fixture-model","effort":"medium"}
    }))
    .unwrap()
}

#[test]
fn research_admission_rejects_invalid_versions_ids_and_budgets_before_io() {
    let mut input = request("snapshot:1");
    input.schema_version = 2;
    assert!(input.validate().is_err());
    input = request("snapshot:1");
    input.operation_id = "op\n".into();
    assert!(input.validate().is_err());
    input = request("snapshot:1");
    input.job_id = " ".into();
    assert!(input.validate().is_err());
    input = request("snapshot:1");
    input.persona_id = "я".repeat(129);
    assert!(input.validate().is_err());
    input = request("snapshot:1");
    input.recall_budget.max_calls = 0;
    assert!(input.validate().is_err());
    input = request("snapshot:1");
    input.model_budget.max_duration_ms = 0;
    assert!(input.validate().is_err());
    input = request("snapshot:1");
    input.period.from = Some(50);
    input.period.to = Some(20);
    assert!(input.validate().is_err());
    request("snapshot:1").validate().unwrap();
}

#[test]
fn research_host_workflow_opens_retrieves_reserves_publishes_and_closes() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let sessions = MemorySessions::default();
    let opened =
        research_workflow::open(&sources, &sessions, &request(&snapshot.id), None, || 70003)
            .unwrap();
    assert_eq!(opened.persona_id, "personA");
    assert_eq!(opened.corpus_snapshot_id, snapshot.id);
    assert_eq!(opened.mode, "lexical");
    let record = sessions
        .with_session(
            &opened.id,
            || 70003,
            |s| s.recall_recorded("контекст", None, || 70003),
        )
        .unwrap();
    let lease = sessions
        .with_session(
            &opened.id,
            || 70003,
            |s| {
                s.reserve_context(
                    &ModelContextRequest {
                        schema_version: 2,
                        purpose: "research".into(),
                        manifest_id: None,
                        system_prompt: "analysis".into(),
                        task: "source only".into(),
                        receipt_ids: vec![record.receipt.id.clone()],
                        memory_item_ids: vec![],
                        format_attempt: 1,
                    },
                    || 70003,
                )
            },
        )
        .unwrap();
    let draft: ResearchPublicationDraft = serde_json::from_value(json!({
        "schemaVersion":1,"extractionVersion":"fixture:1","items":[],
        "analyzedRefs":record.receipt.hits.iter().flat_map(|h| &h.spans).map(|s| json!({"documentId":s.document_id,"revisionId":s.revision_id})).collect::<Vec<_>>(),
        "contextLeaseIds":[lease.id]
    })).unwrap();
    assert!(sessions
        .with_session(
            &opened.id,
            || 70003,
            |s| s.publish_research(&draft, || 70003)
        )
        .is_err());
    sessions
        .with_session(
            &opened.id,
            || 70003,
            |s| {
                s.consume_model_context(
                    &lease.id,
                    DispatchInput {
                        invocation_id: "host-fixture",
                        binding: &InvocationBinding {
                            provider: "codex".into(),
                            model: "fixture-model".into(),
                            effort: "medium".into(),
                            operation: "corpus-analysis".into(),
                        },
                        system_prompt: &lease.system_prompt,
                        task: &lease.task,
                    },
                    || 70003,
                )?;
                s.complete_model_context(&lease.id, "host-fixture", || 70003)
            },
        )
        .unwrap();
    let published = sessions
        .with_session(
            &opened.id,
            || 70003,
            |s| s.publish_research(&draft, || 70003),
        )
        .unwrap();
    assert_eq!(published.coverage.analyzed_documents, 1);
    assert_eq!(published.coverage.used_documents, 0);
    assert!(sessions.close(&opened.id).unwrap());
    assert!(!sessions.close(&opened.id).unwrap());
    assert!(sessions
        .with_session(&opened.id, || 70003, |_| Ok(()))
        .is_err());
    drop(sessions);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn research_open_does_not_replace_native_scope_or_fall_back_from_hybrid() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let sessions = MemorySessions::default();
    let mut input = request(&snapshot.id);
    input.persona_id = "foreign".into();
    assert!(research_workflow::open(&sources, &sessions, &input, None, || 70003).is_err());
    input = request(&snapshot.id);
    input.mode = research_workflow::ResearchMode::Hybrid;
    assert!(research_workflow::open(&sources, &sessions, &input, None, || 70003).is_err());
    input = request(&snapshot.id);
    input.model.provider = "unconfigured".into();
    assert!(research_workflow::open(&sources, &sessions, &input, None, || 70003).is_err());
    input = request(&snapshot.id);
    sources
        .with_ingestion_database(&op, |db| db.remove(&grant))
        .unwrap();
    assert!(research_workflow::open(&sources, &sessions, &input, None, || 70003).is_err());
    drop(sessions);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn hybrid_research_admission_uses_the_pinned_index_and_semantic_only_hits() {
    use super::super::hybrid_tests::QueryModel;
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let sessions = MemorySessions::default();
    let mut input = request(&snapshot.id);
    input.mode = research_workflow::ResearchMode::Hybrid;
    let model = QueryModel::new(&|| Ok(()));
    assert!(research_workflow::open(&sources, &sessions, &input, Some(&model), || 70003).is_err());
    assert_eq!(model.calls.get(), 0);
    build(
        &sources,
        &op,
        &super::super::request(&snapshot),
        &Model::new(&|_| Ok(())),
        || 70002,
    )
    .unwrap();
    let opened =
        research_workflow::open(&sources, &sessions, &input, Some(&model), || 70003).unwrap();
    assert_eq!(opened.mode, "hybrid");
    let result = sessions
        .with_session(
            &opened.id,
            || 70003,
            |session| session.recall("word_absent", Some(&model), || 70003),
        )
        .unwrap();
    assert_eq!(result.hits.len(), 3);
    assert!(result
        .hits
        .iter()
        .all(|hit| hit.lexical_rank.is_none() && hit.semantic_rank.is_some()));
    sessions.close(&opened.id).unwrap();
    drop(sessions);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn late_admission_expiry_never_leaves_an_undisclosed_session_or_operation() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let sessions = MemorySessions::default();
    let input = request(&snapshot.id);
    let count = Cell::new(0usize);
    let clock = || {
        count.set(count.get() + 1);
        70003
    };
    let opened = research_workflow::open(&sources, &sessions, &input, None, clock).unwrap();
    let calls = count.get();
    sessions.close(&opened.id).unwrap();
    // Exercise final checks before/after registry insertion, without changing
    // source data or relying on wall-clock sleeps.
    for late in calls.saturating_sub(8)..=calls {
        count.set(0);
        let clock = || {
            count.set(count.get() + 1);
            if count.get() == late {
                900000
            } else {
                70003
            }
        };
        assert!(
            research_workflow::open(&sources, &sessions, &input, None, clock).is_err(),
            "clock {late}"
        );
        let probe = sources
            .begin_ingestion_operation(
                &input.operation_id,
                crate::corpus_database::JobProvider::Telegram,
            )
            .unwrap();
        drop(probe);
    }
    drop(sessions);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn fabric_admission_binds_native_source_scope_and_separate_model_role() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let mut researcher = session(&sources, &snapshot, limits()).unwrap();
    let draft = super::publication_session_tests::draft(&mut researcher);
    let publication = researcher.publish_research(&draft, || 70004).unwrap();
    drop(researcher);
    let sessions = MemorySessions::default();
    let mut input = request(&snapshot.id);
    input.mode = research_workflow::ResearchMode::Hybrid;
    assert!(
        research_workflow::open_fabric(&sources, &sessions, &input, || panic!(
            "invalid mode touched clock"
        ))
        .is_err()
    );
    input.mode = research_workflow::ResearchMode::Lexical;
    let opened = research_workflow::open_fabric(&sources, &sessions, &input, || 70005).unwrap();
    let json = serde_json::to_value(&opened).unwrap();
    assert_eq!(json["purpose"], "fabric");
    assert_eq!(json["corpusSnapshotId"], snapshot.id);
    assert_eq!(json["personaId"], "personA");
    let lease = sessions
        .with_session(
            &opened.session.id,
            || 70005,
            |s| {
                s.reserve_fabric_context(
                    &crate::memory::session::FabricContextRequest {
                        schema_version: 1,
                        memory_snapshot_id: publication.snapshot.id,
                        item_ids: publication.snapshot.item_ids,
                        system_prompt: "Fabric".into(),
                        task: "Persona context".into(),
                        format_attempt: 1,
                    },
                    || 70005,
                )
            },
        )
        .unwrap();
    assert_eq!(lease.purpose, "fabric");
    assert_eq!(lease.items.len(), 1);
    assert!(sessions
        .with_session(
            &opened.session.id,
            || 70005,
            |s| s.reserve_research_context(&[], "", "", 1, || 70005)
        )
        .is_err());
    sources
        .with_ingestion_database(&op, |db| db.remove(&grant))
        .unwrap();
    assert!(sessions
        .with_session(&opened.session.id, || 70006, |_| Ok(()))
        .is_err());
    sessions.close(&opened.session.id).unwrap();
    assert!(research_workflow::open_fabric(&sources, &sessions, &input, || 70006).is_err());
    drop(sessions);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
