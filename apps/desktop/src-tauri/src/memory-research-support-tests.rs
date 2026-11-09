use super::*;
use crate::memory::{
    research_support::verify,
    session::RecordedRetrieval,
    types::{MemoryItem, MemoryItemKind, MemoryItemSupport},
};
use std::sync::Arc;
fn evidence() -> (RecordedRetrieval, MemoryItem) {
    evidence_for("контекст")
}
fn evidence_for(query: &str) -> (RecordedRetrieval, MemoryItem) {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let record = bound.recall_recorded(query, None, || 70003).unwrap();
    let item = item_for(&record);
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
    (
        Arc::try_unwrap(record).unwrap_or_else(|_| panic!("receipt still retained")),
        item,
    )
}
fn item_for(record: &RecordedRetrieval) -> MemoryItem {
    MemoryItem {
        schema_version: 1,
        id: "native-item".into(),
        snapshot_id: record.receipt.snapshot_id.clone(),
        kind: MemoryItemKind::ReportedAssertion,
        text: "Сообщённый контекст".into(),
        source_spans: vec![record.sources[0].span.clone()],
        episode_ids: vec![record.receipt.hits[0].episode_id.clone()],
        event_at: None,
        observed_at: "2026-11-09T00:00:00Z".into(),
        support: MemoryItemSupport::Reported,
        contradiction_ids: vec![],
        limitations: vec![],
        extraction_version: "memory-extraction-v1".into(),
        policy_version: record.receipt.policy_version.clone(),
    }
}

#[test]
fn research_support_derives_coverage_from_exact_native_receipts_and_item_spans() {
    let (record, mut item) = evidence();
    let original = record.sources[0].span.clone();
    item.source_spans[0].start += 2; // Preserve UTF8 boundaries; exclude the two leading spaces.
    let record = Arc::new(record);
    let proof = verify(
        std::slice::from_ref(&item),
        &[record.clone(), record],
        &|| Ok(()),
    )
    .unwrap();
    assert_eq!(proof.analyzed_spans, [original]);
    assert_eq!(proof.used_spans, item.source_spans);
    assert_eq!(item.text, "Сообщённый контекст");
}
#[test]
fn research_support_rejects_unseen_foreign_unaligned_and_misattributed_item_spans() {
    let (record, item) = evidence();
    let records = [Arc::new(record)];
    assert!(verify(std::slice::from_ref(&item), &[], &|| Ok(())).is_err());
    let mut changed = item.clone();
    changed.source_spans[0].revision_id = "foreign".into();
    assert!(verify(&[changed], &records, &|| Ok(())).is_err());
    let mut changed = item.clone();
    changed.source_spans[0].start += 3; // Middle of Cyrillic UTF8.
    assert!(verify(&[changed], &records, &|| Ok(())).is_err());
    let mut changed = item.clone();
    changed.source_spans[0].end += 1;
    assert!(verify(&[changed], &records, &|| Ok(())).is_err());
    let mut changed = item.clone();
    changed.episode_ids = vec!["foreign".into()];
    assert!(verify(&[changed], &records, &|| Ok(())).is_err());
    let mut changed = item;
    changed.support = MemoryItemSupport::Observed;
    assert!(verify(&[changed], &records, &|| Ok(())).is_err());
}
#[test]
fn research_support_never_upgrades_context_or_quoted_text_into_own_style() {
    let (mut record, mut item) = evidence();
    item.kind = MemoryItemKind::StyleExample;
    item.text = record.sources[0].text.clone();
    record.sources[0].can_support_own_words = false;
    assert!(verify(std::slice::from_ref(&item), &[Arc::new(record)], &|| Ok(())).is_err());
    let (mut record, _) = evidence();
    record.sources[0].document.is_forwarded = true;
    // The second fixture has different native IDs; use its own span and episode.
    item.source_spans = vec![record.sources[0].span.clone()];
    item.episode_ids = vec![record.receipt.hits[0].episode_id.clone()];
    assert!(verify(&[item], &[Arc::new(record)], &|| Ok(())).is_err());
}
#[test]
fn research_support_preserves_cancellation_and_rejects_corrupt_receipt_geometry() {
    let (mut record, item) = evidence();
    let error = verify(std::slice::from_ref(&item), &[], &|| {
        Err("cancelled exactly".into())
    })
    .err();
    assert_eq!(error.as_deref(), Some("cancelled exactly"));
    record.sources[0].text.push('x');
    assert!(verify(&[item], &[Arc::new(record)], &|| Ok(())).is_err());
}

#[test]
fn own_style_requires_exact_words_and_all_matching_sources_to_be_attributable() {
    let (record, mut item) = evidence_for("ok");
    item.kind = MemoryItemKind::StyleExample;
    item.support = MemoryItemSupport::Observed;
    item.text = record.sources[0].text.clone();
    let records = [Arc::new(record)];
    verify(std::slice::from_ref(&item), &records, &|| Ok(())).unwrap();
    item.text.push(' ');
    assert!(verify(&[item], &records, &|| Ok(())).is_err());
    let (mut record, mut item) = evidence_for("ok");
    item.kind = MemoryItemKind::StyleExample;
    item.text = record.sources[0].text.clone();
    assert!(record.sources.len() >= 2);
    record.sources[1].span = record.sources[0].span.clone();
    record.sources[1].document.id = record.sources[0].document.id.clone();
    record.sources[1].can_support_own_words = false;
    record.receipt.hits[1].spans = record.receipt.hits[0].spans.clone();
    record.receipt.hits[1].episode_id = record.receipt.hits[0].episode_id.clone();
    assert!(verify(&[item], &[Arc::new(record)], &|| Ok(())).is_err());
}
#[test]
fn research_support_retains_analyzed_coverage_without_items_and_checks_late_cancellation() {
    let (record, item) = evidence();
    let records = [Arc::new(record)];
    let proof = verify(&[], &records, &|| Ok(())).unwrap();
    assert_eq!(proof.analyzed_spans, [records[0].sources[0].span.clone()]);
    assert!(proof.used_spans.is_empty());
    let calls = Cell::new(0);
    verify(std::slice::from_ref(&item), &records, &|| {
        calls.set(calls.get() + 1);
        Ok(())
    })
    .unwrap();
    let last = calls.get();
    calls.set(0);
    let result = verify(&[item], &records, &|| {
        calls.set(calls.get() + 1);
        if calls.get() == last {
            Err("late cancellation".into())
        } else {
            Ok(())
        }
    });
    assert_eq!(result.err().as_deref(), Some("late cancellation"));
}
#[test]
fn session_support_requires_completed_context_and_live_grants() {
    use crate::memory::{
        session::{DispatchInput, InvocationBinding},
        types::ModelCallBudget,
    };
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let record = bound.recall_recorded("контекст", None, || 70003).unwrap();
    let item = item_for(&record);
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
                max_calls: 1,
                max_input_bytes: 100000,
                max_duration_ms: 1000,
            },
            || 70003,
        )
        .unwrap();
    let reserved = bound
        .reserve_research_context(
            std::slice::from_ref(&record.receipt.id),
            "prompt",
            "task",
            1,
            || 70003,
        )
        .unwrap();
    let ids = [reserved.id.clone()];
    assert!(bound
        .verify_research_support(std::slice::from_ref(&item), &ids, || 70003)
        .is_err());
    bound
        .consume_model_context(
            &reserved.id,
            DispatchInput {
                invocation_id: "call",
                binding: &binding,
                system_prompt: &reserved.input.system_prompt,
                task: &reserved.input.task,
            },
            || 70003,
        )
        .unwrap();
    assert!(bound
        .verify_research_support(std::slice::from_ref(&item), &ids, || 70003)
        .is_err());
    bound
        .complete_model_context(&reserved.id, "call", || 70003)
        .unwrap();
    let proof = bound
        .verify_research_support(std::slice::from_ref(&item), &ids, || 70003)
        .unwrap();
    assert_eq!(proof.used_spans, item.source_spans);
    sources
        .with_ingestion_database(&op, |db| db.remove(&grant))
        .unwrap();
    assert!(bound
        .verify_research_support(&[item], &ids, || 70003)
        .is_err());
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn session_support_rechecks_model_deadline_after_source_verification() {
    use crate::memory::{
        session::{DispatchInput, InvocationBinding},
        types::ModelCallBudget,
    };
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let record = bound.recall_recorded("контекст", None, || 70003).unwrap();
    let item = item_for(&record);
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
                max_calls: 1,
                max_input_bytes: 100000,
                max_duration_ms: 1000,
            },
            || 70003,
        )
        .unwrap();
    let reserved = bound
        .reserve_research_context(
            std::slice::from_ref(&record.receipt.id),
            "prompt",
            "task",
            1,
            || 70003,
        )
        .unwrap();
    let ids = [reserved.id.clone()];
    assert!(bound
        .verify_research_support(std::slice::from_ref(&item), &ids, || 70003)
        .is_err());
    bound
        .consume_model_context(
            &reserved.id,
            DispatchInput {
                invocation_id: "call",
                binding: &binding,
                system_prompt: &reserved.input.system_prompt,
                task: &reserved.input.task,
            },
            || 70003,
        )
        .unwrap();
    assert!(bound
        .verify_research_support(std::slice::from_ref(&item), &ids, || 70003)
        .is_err());
    bound
        .complete_model_context(&reserved.id, "call", || 70003)
        .unwrap();
    let calls = Cell::new(0);
    bound
        .verify_research_support(std::slice::from_ref(&item), &ids, || {
            calls.set(calls.get() + 1);
            70003
        })
        .unwrap();
    let last = calls.get();
    calls.set(0);
    let result = bound.verify_research_support(&[item], &ids, || {
        calls.set(calls.get() + 1);
        if calls.get() == last {
            71003
        } else {
            70003
        }
    });
    assert!(
        result.is_err(),
        "expired model authority escaped after source verification"
    );
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
