use super::*;
use crate::memory::{
    research_draft::{Contradiction, MemoryItemDraft, ResearchPublicationDraft},
    session::{DispatchInput, InvocationBinding},
    types::{MemoryItemKind, MemoryItemSupport, ModelCallBudget},
};
fn configure(bound: &mut MemorySession) -> InvocationBinding {
    let binding = InvocationBinding {
        provider: "codex".into(),
        model: "fixture-model".into(),
        effort: "medium".into(),
        operation: "corpus-analysis".into(),
    };
    bound
        .configure_research_model(
            binding.clone(),
            ModelCallBudget {
                max_calls: 2,
                max_input_bytes: 100000,
                max_duration_ms: 1000,
            },
            || 70003,
        )
        .unwrap();
    binding
}
pub(super) fn draft(bound: &mut MemorySession) -> ResearchPublicationDraft {
    let record = bound.recall_recorded("контекст", None, || 70003).unwrap();
    let binding = configure(bound);
    let reserved = bound
        .reserve_research_context(
            std::slice::from_ref(&record.receipt.id),
            "research",
            "task",
            1,
            || 70003,
        )
        .unwrap();
    bound
        .consume_model_context(
            &reserved.id,
            DispatchInput {
                invocation_id: "publication-call",
                binding: &binding,
                system_prompt: &reserved.input.system_prompt,
                task: &reserved.input.task,
            },
            || 70003,
        )
        .unwrap();
    bound
        .complete_model_context(&reserved.id, "publication-call", || 70003)
        .unwrap();
    ResearchPublicationDraft {
        schema_version: 1,
        extraction_version: "research:1".into(),
        items: vec![MemoryItemDraft {
            draft_id: "draft:item".into(),
            kind: MemoryItemKind::ReportedAssertion,
            text: "  контекст\r\n".into(),
            source_spans: vec![record.sources[0].span.clone()],
            episode_ids: vec![record.receipt.hits[0].episode_id.clone()],
            event_at: None,
            support: MemoryItemSupport::Reported,
            contradictions: vec![],
            limitations: vec![],
        }],
        analyzed_refs: vec![crate::corpus_database::RevisionReference {
            document_id: record.sources[0].span.document_id.clone(),
            revision_id: record.sources[0].span.revision_id.clone(),
        }],
        context_lease_ids: vec![reserved.id],
    }
}
#[test]
fn research_publication_derives_native_identity_coverage_and_durable_audit() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let draft = draft(&mut bound);
    let receipt = bound.publish_research(&draft, || 70004).unwrap();
    assert_eq!(receipt.snapshot.corpus_snapshot_id, snapshot.id);
    assert_eq!(receipt.snapshot.extraction_version, "research:1");
    assert_eq!(receipt.published_at, "1970-01-01T00:01:10.004Z");
    assert_eq!(receipt.snapshot.created_at, receipt.published_at);
    assert_eq!(receipt.item_ids[0].draft_id, "draft:item");
    assert_ne!(receipt.item_ids[0].item_id, "draft:item");
    assert_eq!(
        receipt.snapshot.item_ids,
        vec![receipt.item_ids[0].item_id.clone()]
    );
    assert_eq!(
        receipt.coverage.loaded_documents,
        snapshot.coverage.loaded_documents as u64
    );
    assert_eq!(
        receipt.coverage.loaded_bytes,
        snapshot.coverage.loaded_bytes as u64
    );
    assert_eq!(receipt.coverage.indexed_documents, 3);
    assert_eq!(receipt.coverage.analyzed_documents, 1);
    assert_eq!(receipt.coverage.used_documents, 1);
    assert_eq!(receipt.analyzed_spans, draft.items[0].source_spans);
    let stored = sources
        .with_ingestion_database(&op, |db| {
            db.read_research_publication_checked(&receipt.id, "personA", &|| Ok(()))
        })
        .unwrap();
    assert_eq!(stored.bundle.items[0].text, "  контекст\r\n");
    assert_eq!(stored.publication.model_contexts.len(), 1);
    assert_eq!(stored.publication.model_contexts[0].completed_at_ms, 70003);
    assert_eq!(stored.publication.retrievals.len(), 1);
    assert_eq!(stored.publication.receipt, receipt);
    assert_eq!(bound.publish_research(&draft, || 70005).unwrap(), receipt);
    let mut changed = draft.clone();
    changed.items[0].text.push('!');
    assert!(bound.publish_research(&changed, || 70005).is_err());
    assert_eq!(bound.publish_research(&draft, || 70005).unwrap(), receipt);
    sources
        .with_ingestion_database(&op, |db| db.remove(&grant))
        .unwrap();
    assert!(bound.publish_research(&draft, || 70006).is_err());
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn publication_rejects_unconsumed_sources_foreign_claims_and_missing_contradictions() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let original = draft(&mut bound);
    for kind in [
        "lease",
        "reference",
        "missing-reference",
        "support",
        "contradiction",
    ] {
        let mut changed = original.clone();
        match kind {
            "lease" => changed.context_lease_ids.clear(),
            "reference" => changed.analyzed_refs[0].revision_id = "foreign".into(),
            "missing-reference" => changed.analyzed_refs.clear(),
            "support" => changed.items[0].support = MemoryItemSupport::Observed,
            _ => changed.items[0]
                .contradictions
                .push(Contradiction::Published {
                    item_id: "missing".into(),
                }),
        }
        assert!(
            bound.publish_research(&changed, || 70004).is_err(),
            "{kind}"
        );
    }
    // Rejected drafts do not permanently reserve the publication identity.
    bound.publish_research(&original, || 70004).unwrap();
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn publication_rechecks_revocation_model_expiry_and_source_index_generation() {
    for kind in ["revoke", "model-expiry", "rebuild", "cancel"] {
        let (sources, dir) = local();
        let (op, snapshot, _) = indexed(&sources, &dir, 0);
        let grant = enable_model(&sources, &op);
        let mut bound = session(&sources, &snapshot, limits()).unwrap();
        let draft = draft(&mut bound);
        let now = if kind == "model-expiry" { 71003 } else { 70004 };
        match kind {
            "revoke" => {
                sources
                    .with_ingestion_database(&op, |db| db.remove(&grant))
                    .unwrap();
            }
            "rebuild" => {
                rebuild(
                    &sources,
                    &op,
                    &IndexInput {
                        job_id: "job",
                        snapshot_id: &snapshot.id,
                        max_duration_ms: 60000,
                    },
                    || 70004,
                )
                .unwrap();
            }
            "cancel" => bound.cancel(),
            _ => {}
        }
        assert!(bound.publish_research(&draft, || now).is_err(), "{kind}");
        drop(bound);
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn empty_research_keeps_loaded_and_indexed_counts_separate_from_analysis() {
    for holdout in [0, 10000] {
        let (sources, dir) = local();
        let (op, snapshot, _) = indexed(&sources, &dir, holdout);
        enable_model(&sources, &op);
        let mut bound = session(&sources, &snapshot, limits()).unwrap();
        configure(&mut bound);
        let draft = ResearchPublicationDraft {
            schema_version: 1,
            extraction_version: "research:1".into(),
            items: vec![],
            analyzed_refs: vec![],
            context_lease_ids: vec![],
        };
        let receipt = bound.publish_research(&draft, || 70004).unwrap();
        assert!(receipt.coverage.loaded_documents > 0);
        assert_eq!(
            receipt.coverage.indexed_documents,
            if holdout == 0 { 3 } else { 0 }
        );
        assert_eq!(receipt.coverage.analyzed_documents, 0);
        assert_eq!(receipt.coverage.used_documents, 0);
        assert!(receipt.snapshot.item_ids.is_empty());
        assert!(receipt.analyzed_spans.is_empty());
        drop(bound);
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn cancellation_and_model_expiry_during_publication_never_expose_a_receipt() {
    use std::cell::Cell;
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let prepared = draft(&mut bound);
    let ticks = Cell::new(0usize);
    bound
        .publish_research(&prepared, || {
            ticks.set(ticks.get() + 1);
            70004
        })
        .unwrap();
    let total = ticks.get();
    assert!(total > 10);
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
    // Mid-operation and final-disclosure failures. A final failure may follow
    // a commit, but must not disclose a receipt after authority is lost.
    for at in [total / 2, total - 1] {
        for cancel in [false, true] {
            let (sources, dir) = local();
            let (op, snapshot, _) = indexed(&sources, &dir, 0);
            enable_model(&sources, &op);
            let mut bound = session(&sources, &snapshot, limits()).unwrap();
            let prepared = draft(&mut bound);
            ticks.set(0);
            assert!(
                bound
                    .publish_research(&prepared, || {
                        ticks.set(ticks.get() + 1);
                        if cancel && ticks.get() == at {
                            sources.cancel_ingestion_operation("memory").unwrap();
                        }
                        if !cancel && ticks.get() >= at {
                            71003
                        } else {
                            70004
                        }
                    })
                    .is_err(),
                "checkpoint {at}, cancel={cancel}"
            );
            assert!(ticks.get() >= at);
            drop(bound);
            drop(op);
            drop(sources);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}

#[test]
fn publication_timestamp_never_precedes_completed_analysis_when_clock_recovers() {
    // A brief backwards clock reading can recover before later checks. Any
    // returned publication still must follow its completed model context.
    for backwards_at in 1..=8 {
        let (sources, dir) = local();
        let (op, snapshot, _) = indexed(&sources, &dir, 0);
        enable_model(&sources, &op);
        let mut bound = session(&sources, &snapshot, limits()).unwrap();
        let prepared = draft(&mut bound);
        let ticks = std::cell::Cell::new(0);
        let result = bound.publish_research(&prepared, || {
            ticks.set(ticks.get() + 1);
            if ticks.get() == backwards_at {
                69900
            } else {
                70004
            }
        });
        if let Ok(receipt) = result {
            let published = chrono::DateTime::parse_from_rfc3339(&receipt.published_at)
                .unwrap()
                .timestamp_millis();
            assert!(
                published >= 70003,
                "publication predates its model context at clock sample {backwards_at}"
            );
        }
        drop(bound);
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[path = "memory-published-read-tests.rs"]
mod published_read_tests;
