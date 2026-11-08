//! Structural validation tests for the persisted memory DTOs. Fixtures mirror
//! `packages/contracts/src/memory.ts` and the bounded shapes proven by the
//! STALKER memory-analysis and Fabric skill parsers.
use super::*;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

const TS: &str = "2026-11-08T20:00:00Z";

fn hex64(fill: char) -> String {
    fill.to_string().repeat(64)
}

fn span(start: usize, end: usize) -> ChunkSpan {
    ChunkSpan {
        document_id: "doc-1".into(),
        revision_id: "revision-1".into(),
        unit: SpanUnit::Utf8Bytes,
        start,
        end,
    }
}

fn fingerprint() -> EmbeddingFingerprint {
    EmbeddingFingerprint {
        model_id: "intfloat/multilingual-e5-small".into(),
        model_revision: "ab12cd34ef56ab78cd90ef12ab34cd56ef78ab90".into(),
        model_sha256: hex64('a'),
        tokenizer_sha256: hex64('b'),
        dimensions: 384,
        pooling: Pooling::Mean,
        normalization: Normalization::L2,
        query_template: "query: ".into(),
        passage_template: "passage: ".into(),
        chunking_version: "corpus-chunking-v1".into(),
    }
}

fn recall_budget() -> RecallBudget {
    RecallBudget {
        max_calls: 10,
        max_candidates: 500,
        max_episodes: 20,
        max_source_bytes: 65536,
        max_context_tokens: 4096,
        max_duration_ms: 60_000,
    }
}

fn model_call_budget() -> ModelCallBudget {
    ModelCallBudget {
        max_calls: 4,
        max_input_bytes: 32768,
        max_duration_ms: 300_000,
    }
}

fn settings_entry(
    component: ModelComponent,
    provider: ModelProvider,
    requested_model: Option<String>,
    requested_effort: ModelEffort,
) -> ModelSettingsEntry {
    ModelSettingsEntry {
        component,
        provider,
        requested_model,
        requested_effort,
    }
}

fn model_settings() -> Vec<ModelSettingsEntry> {
    vec![
        settings_entry(
            ModelComponent::Stalker,
            ModelProvider::Codex,
            Some("gpt-5.2".into()),
            ModelEffort::Medium,
        ),
        settings_entry(
            ModelComponent::PersonFabric,
            ModelProvider::Kimi,
            None,
            ModelEffort::Low,
        ),
        settings_entry(
            ModelComponent::Person,
            ModelProvider::Kimi,
            Some("kimi-k2".into()),
            ModelEffort::High,
        ),
        settings_entry(
            ModelComponent::BrainAnalyzator,
            ModelProvider::Codex,
            Some("o4-mini".into()),
            ModelEffort::Medium,
        ),
    ]
}

fn memory_item() -> MemoryItem {
    MemoryItem {
        schema_version: 1,
        id: "item-1".into(),
        snapshot_id: "snapshot-1".into(),
        kind: MemoryItemKind::Episode,
        text: "Exact quoted words".into(),
        source_spans: vec![span(0, 18)],
        episode_ids: vec!["episode-1".into()],
        event_at: None,
        observed_at: TS.into(),
        support: MemoryItemSupport::Observed,
        contradiction_ids: vec![],
        limitations: vec!["Один источник.".into()],
        extraction_version: "memory-extraction-v1".into(),
        policy_version: "policy-v1".into(),
    }
}

fn skill() -> SkillDefinition {
    SkillDefinition {
        schema_version: 1,
        id: "skill-1".into(),
        trigger: "Когда спрашивают о проекте".into(),
        inputs: vec!["переписка".into()],
        steps: vec!["Открыть архив.".into(), "Сверить дату.".into()],
        outcome_check: "Ответ совпадает с источником.".into(),
        support: SkillSupport::Observed,
        memory_item_ids: vec!["item-1".into()],
        source_spans: vec![span(0, 18)],
        limitations: vec!["Наблюдалось один раз.".into()],
    }
}

fn snapshot() -> MemorySnapshot {
    MemorySnapshot {
        schema_version: 1,
        id: "memory-snapshot-1".into(),
        corpus_snapshot_id: "corpus-1".into(),
        item_ids: vec!["item-1".into()],
        extraction_version: "memory-extraction-v1".into(),
        created_at: TS.into(),
    }
}

fn manifest() -> PersonaManifest {
    PersonaManifest {
        schema_version: 1,
        id: "manifest-1".into(),
        persona_id: "persona-1".into(),
        profile_version: "profile-v1".into(),
        skill_ids: vec!["skill-1".into()],
        exemplar_ids: vec!["exemplar-1".into()],
        corpus_snapshot_id: "corpus-1".into(),
        memory_snapshot_id: "memory-snapshot-1".into(),
        source_refs: vec![RevisionReference {
            document_id: "doc-1".into(),
            revision_id: "revision-1".into(),
        }],
        split_version: "split-v1".into(),
        preparation_version: "prep-v1".into(),
        retrieval_version: "retrieval-v1".into(),
        prompt_version: "prompt-v1".into(),
        embedding_fingerprint: Some(fingerprint()),
        mode: RecallMode::FixedContext,
        recall_budget: recall_budget(),
        model_call_budget: model_call_budget(),
        model_settings: model_settings(),
        created_at: TS.into(),
    }
}

fn to_json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("fixture serializes")
}

fn as_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("fixture serializes")
}

fn without(value: &Value, key: &str) -> String {
    let mut map = value.as_object().expect("object").clone();
    assert!(map.remove(key).is_some(), "fixture must contain {key}");
    serde_json::to_string(&map).expect("object serializes")
}

fn with_unknown(value: &Value) -> String {
    let mut map = value.as_object().expect("object").clone();
    map.insert("mysteryField".into(), json!("x"));
    serde_json::to_string(&map).expect("object serializes")
}

fn with<F>(value: &Value, f: F) -> String
where
    F: FnOnce(&mut Map<String, Value>),
{
    let mut map = value.as_object().expect("object").clone();
    f(&mut map);
    serde_json::to_string(&map).expect("object serializes")
}

fn keys(value: &Value) -> BTreeSet<String> {
    value.as_object().expect("object").keys().cloned().collect()
}

#[test]
fn valid_fixtures_validate() {
    memory_item().validate().expect("valid memory item");
    skill().validate().expect("valid skill");
    snapshot().validate().expect("valid snapshot");
    manifest().validate().expect("valid manifest");
    fingerprint().validate().expect("valid fingerprint");
    recall_budget().validate().expect("valid recall budget");
    model_call_budget()
        .validate()
        .expect("valid model call budget");
    for entry in model_settings() {
        entry.validate().expect("valid settings entry");
    }
}

#[test]
fn valid_fixtures_decode_from_json() {
    decode_memory_item(&as_json(&memory_item())).expect("decode memory item");
    decode_skill_definition(&as_json(&skill())).expect("decode skill");
    decode_memory_snapshot(&as_json(&snapshot())).expect("decode snapshot");
    decode_persona_manifest(&as_json(&manifest())).expect("decode manifest");
    decode_embedding_fingerprint(&as_json(&fingerprint())).expect("decode fingerprint");
    decode_recall_budget(&as_json(&recall_budget())).expect("decode recall budget");
    decode_model_call_budget(&as_json(&model_call_budget())).expect("decode call budget");
}

#[test]
fn serialized_keys_match_ts_contracts_exactly() {
    let cases: Vec<(Value, &[&str])> = vec![
        (
            to_json(&memory_item()),
            &[
                "schemaVersion",
                "id",
                "snapshotId",
                "kind",
                "text",
                "sourceSpans",
                "episodeIds",
                "eventAt",
                "observedAt",
                "support",
                "contradictionIds",
                "limitations",
                "extractionVersion",
                "policyVersion",
            ],
        ),
        (
            to_json(&skill()),
            &[
                "schemaVersion",
                "id",
                "trigger",
                "inputs",
                "steps",
                "outcomeCheck",
                "support",
                "memoryItemIds",
                "sourceSpans",
                "limitations",
            ],
        ),
        (
            to_json(&snapshot()),
            &[
                "schemaVersion",
                "id",
                "corpusSnapshotId",
                "itemIds",
                "extractionVersion",
                "createdAt",
            ],
        ),
        (
            to_json(&manifest()),
            &[
                "schemaVersion",
                "id",
                "personaId",
                "profileVersion",
                "skillIds",
                "exemplarIds",
                "corpusSnapshotId",
                "memorySnapshotId",
                "sourceRefs",
                "splitVersion",
                "preparationVersion",
                "retrievalVersion",
                "promptVersion",
                "embeddingFingerprint",
                "mode",
                "recallBudget",
                "modelCallBudget",
                "modelSettings",
                "createdAt",
            ],
        ),
        (
            to_json(&fingerprint()),
            &[
                "modelId",
                "modelRevision",
                "modelSha256",
                "tokenizerSha256",
                "dimensions",
                "pooling",
                "normalization",
                "queryTemplate",
                "passageTemplate",
                "chunkingVersion",
            ],
        ),
        (
            to_json(&recall_budget()),
            &[
                "maxCalls",
                "maxCandidates",
                "maxEpisodes",
                "maxSourceBytes",
                "maxContextTokens",
                "maxDurationMs",
            ],
        ),
        (
            to_json(&model_call_budget()),
            &["maxCalls", "maxInputBytes", "maxDurationMs"],
        ),
        (
            to_json(&settings_entry(
                ModelComponent::Person,
                ModelProvider::Kimi,
                None,
                ModelEffort::Low,
            )),
            &["component", "provider", "requestedModel", "requestedEffort"],
        ),
        (
            to_json(&span(0, 18)),
            &["documentId", "revisionId", "unit", "start", "end"],
        ),
    ];
    for (value, expected) in cases {
        assert_eq!(
            keys(&value),
            expected.iter().map(|key| (*key).to_string()).collect()
        );
    }
}

#[test]
fn enum_wire_values_match_ts_contracts_exactly() {
    assert_eq!(json!("episode"), to_json(&MemoryItemKind::Episode));
    assert_eq!(
        json!("reportedAssertion"),
        to_json(&MemoryItemKind::ReportedAssertion)
    );
    assert_eq!(json!("procedure"), to_json(&MemoryItemKind::Procedure));
    assert_eq!(
        json!("styleExample"),
        to_json(&MemoryItemKind::StyleExample)
    );
    assert_eq!(
        json!("conflicting"),
        to_json(&MemoryItemSupport::Conflicting)
    );
    assert_eq!(json!("proposed"), to_json(&SkillSupport::Proposed));
    assert_eq!(json!("fixedContext"), to_json(&RecallMode::FixedContext));
    assert_eq!(json!("agenticRecall"), to_json(&RecallMode::AgenticRecall));
    assert_eq!(json!("stalker"), to_json(&ModelComponent::Stalker));
    assert_eq!(
        json!("person-fabric"),
        to_json(&ModelComponent::PersonFabric)
    );
    assert_eq!(json!("person"), to_json(&ModelComponent::Person));
    assert_eq!(
        json!("brain-analyzator"),
        to_json(&ModelComponent::BrainAnalyzator)
    );
    assert_eq!(json!("codex"), to_json(&ModelProvider::Codex));
    assert_eq!(json!("kimi"), to_json(&ModelProvider::Kimi));
    assert_eq!(json!("high"), to_json(&ModelEffort::High));
    assert_eq!(json!("mean"), to_json(&Pooling::Mean));
    assert_eq!(json!("cls"), to_json(&Pooling::Cls));
    assert_eq!(json!("l2"), to_json(&Normalization::L2));
    assert_eq!(json!("none"), to_json(&Normalization::None));
    assert_eq!(json!("utf8Bytes"), to_json(&SpanUnit::Utf8Bytes));
}

#[test]
fn every_dto_rejects_unknown_fields() {
    let item = to_json(&memory_item());
    assert!(serde_json::from_str::<MemoryItem>(&with_unknown(&item)).is_err());
    let skill = to_json(&skill());
    assert!(serde_json::from_str::<SkillDefinition>(&with_unknown(&skill)).is_err());
    let snapshot = to_json(&snapshot());
    assert!(serde_json::from_str::<MemorySnapshot>(&with_unknown(&snapshot)).is_err());
    let manifest = to_json(&manifest());
    assert!(serde_json::from_str::<PersonaManifest>(&with_unknown(&manifest)).is_err());
    let fp = to_json(&fingerprint());
    assert!(serde_json::from_str::<EmbeddingFingerprint>(&with_unknown(&fp)).is_err());
    let budget = to_json(&recall_budget());
    assert!(serde_json::from_str::<RecallBudget>(&with_unknown(&budget)).is_err());
    let call_budget = to_json(&model_call_budget());
    assert!(serde_json::from_str::<ModelCallBudget>(&with_unknown(&call_budget)).is_err());
}

#[test]
fn every_dto_rejects_wrong_schema_version() {
    let mut item = memory_item();
    item.schema_version = 2;
    assert!(item.validate().is_err());
    let mut item = memory_item();
    item.schema_version = 0;
    assert!(item.validate().is_err());
    let mut skill = skill();
    skill.schema_version = 2;
    assert!(skill.validate().is_err());
    let mut snapshot = snapshot();
    snapshot.schema_version = 2;
    assert!(snapshot.validate().is_err());
    let mut manifest = manifest();
    manifest.schema_version = 2;
    assert!(manifest.validate().is_err());
    let item_json = to_json(&memory_item());
    assert!(decode_memory_item(&without(&item_json, "schemaVersion")).is_err());
}

#[test]
fn required_nullable_keys_reject_omission_but_accept_explicit_null() {
    let item = to_json(&memory_item());
    let missing_event_at = without(&item, "eventAt");
    assert!(serde_json::from_str::<MemoryItem>(&missing_event_at).is_err());
    let null_event_at = with(&item, |map| {
        map.insert("eventAt".into(), Value::Null);
    });
    assert!(serde_json::from_str::<MemoryItem>(&null_event_at).is_ok());

    let manifest_json = to_json(&manifest());
    let missing_fingerprint = without(&manifest_json, "embeddingFingerprint");
    assert!(serde_json::from_str::<PersonaManifest>(&missing_fingerprint).is_err());
    let null_fingerprint = with(&manifest_json, |map| {
        map.insert("embeddingFingerprint".into(), Value::Null);
    });
    assert!(serde_json::from_str::<PersonaManifest>(&null_fingerprint).is_ok());

    let entry = to_json(&settings_entry(
        ModelComponent::Person,
        ModelProvider::Kimi,
        Some("kimi-k2".into()),
        ModelEffort::High,
    ));
    let missing_model = without(&entry, "requestedModel");
    assert!(serde_json::from_str::<ModelSettingsEntry>(&missing_model).is_err());
    let null_model = with(&entry, |map| {
        map.insert("requestedModel".into(), Value::Null);
    });
    assert!(serde_json::from_str::<ModelSettingsEntry>(&null_model).is_ok());
}

#[test]
fn ids_preserve_surrounding_whitespace_and_unicode() {
    let mut item = memory_item();
    item.id = "  item-привет  ".into();
    item.validate()
        .expect("surrounding whitespace is preserved, not normalized");
    let decoded = decode_memory_item(&as_json(&item)).expect("decode");
    assert_eq!(decoded.id, "  item-привет  ");
}

#[test]
fn ids_reject_blank_oversize_and_control_characters() {
    let mut item = memory_item();
    item.id = String::new();
    assert!(item.validate().is_err());
    item.id = "   ".into();
    assert!(item.validate().is_err());
    item.id = "\t\n".into();
    assert!(item.validate().is_err());
    item.id = "a\nb".into();
    assert!(item.validate().is_err());
    item.id = "a".repeat(257);
    assert!(item.validate().is_err());
    item.id = "a".repeat(256);
    item.validate()
        .expect("256 UTF8 bytes is the inclusive bound");
    item.id = "й".repeat(128);
    item.validate().expect("256 UTF8 bytes of multibyte text");
    let mut manifest = manifest();
    manifest.profile_version = " ".repeat(300);
    assert!(manifest.validate().is_err());
    manifest.profile_version = "x".repeat(257);
    assert!(manifest.validate().is_err());
}

#[test]
fn text_bounds_and_exact_preservation() {
    let mut item = memory_item();
    item.text = String::new();
    assert!(item.validate().is_err());
    item.text = " ".into();
    item.validate()
        .expect("whitespace-only quote is valid text");
    item.text = "я".repeat(32768);
    item.validate().expect("exactly 64 KiB of multibyte UTF8");
    item.text = format!("{}a", "я".repeat(32768));
    assert!(item.validate().is_err());
    item.text = "\u{FEFF}quoted words\r\nsecond line   ".into();
    item.validate()
        .expect("BOM, CRLF and trailing spaces are valid");
    let roundtrip: MemoryItem = serde_json::from_str(&as_json(&item)).expect("roundtrip");
    assert_eq!(roundtrip.text, "\u{FEFF}quoted words\r\nsecond line   ");
}

#[test]
fn timestamps_validate_real_calendar_dates() {
    let mut item = memory_item();
    for valid in [
        "2026-11-08T20:00:00Z",
        "2024-02-29T00:00:00Z",
        "2026-11-08T20:00:00.123456789+23:59",
        "2016-12-31T23:59:60Z",
        "2026-11-08 20:00:00Z",
    ] {
        item.observed_at = valid.into();
        item.validate()
            .unwrap_or_else(|_| panic!("valid timestamp rejected: {valid}"));
    }
    for invalid in [
        "2026-02-30T10:00:00Z",
        "2026-13-01T10:00:00Z",
        "2026-11-08T24:00:00Z",
        "2026-11-08T20:00:00+24:00",
        "2026-11-08",
        "2026-11-08T20:00:00",
        "not-a-timestamp",
    ] {
        item.observed_at = invalid.into();
        assert!(
            item.validate().is_err(),
            "invalid calendar accepted: {invalid}"
        );
    }
}

#[test]
fn timestamps_enforce_64_byte_host_bound() {
    let mut item = memory_item();
    let exact = format!("2026-11-09T10:00:00.{}+23:59", "1".repeat(38));
    assert_eq!(exact.len(), 64);
    item.observed_at = exact.clone();
    item.validate().expect("64-byte RFC3339 timestamp");
    item.observed_at = format!("2026-11-09T10:00:00.{}+23:59", "1".repeat(39));
    assert_eq!(item.observed_at.len(), 65);
    assert!(item.validate().is_err());
}

#[test]
fn event_at_is_nullable_but_validated_when_present() {
    let mut item = memory_item();
    item.event_at = Some("2026-11-01T09:30:00Z".into());
    item.validate().expect("explicit event time");
    item.event_at = Some("2026-02-30T09:30:00Z".into());
    assert!(item.validate().is_err());
    item.event_at = None;
    item.validate().expect("null event time");
}

#[test]
fn snapshot_created_at_is_required() {
    let snapshot_json = to_json(&snapshot());
    let missing = without(&snapshot_json, "createdAt");
    assert!(serde_json::from_str::<MemorySnapshot>(&missing).is_err());
}

#[test]
fn spans_enforce_count_bounds_unit_and_order() {
    let mut item = memory_item();
    item.source_spans = vec![];
    assert!(item.validate().is_err());
    item.source_spans = (0..64).map(|index| span(index, index + 1)).collect();
    item.validate().expect("64 spans is the inclusive bound");
    item.source_spans = (0..65).map(|index| span(index, index + 1)).collect();
    assert!(item.validate().is_err());
    item.source_spans = vec![span(4, 4)];
    assert!(item.validate().is_err());
    item.source_spans = vec![span(9, 4)];
    assert!(item.validate().is_err());

    let item_json = to_json(&memory_item());
    let wrong_unit = with(&item_json, |map| {
        map["sourceSpans"][0]["unit"] = json!("codePoints");
    });
    assert!(serde_json::from_str::<MemoryItem>(&wrong_unit).is_err());
    let extra_field = with(&item_json, |map| {
        map["sourceSpans"][0]["normalized"] = json!(true);
    });
    assert!(serde_json::from_str::<MemoryItem>(&extra_field).is_err());
    let negative = with(&item_json, |map| {
        map["sourceSpans"][0]["start"] = json!(-1);
    });
    assert!(serde_json::from_str::<MemoryItem>(&negative).is_err());
    let float = with(&item_json, |map| {
        map["sourceSpans"][0]["start"] = json!(1.5);
    });
    assert!(serde_json::from_str::<MemoryItem>(&float).is_err());
    let exponent = with(&item_json, |map| {
        map["sourceSpans"][0]["start"] = json!(1e2);
    });
    assert!(serde_json::from_str::<MemoryItem>(&exponent).is_err());
}

#[test]
fn spans_enforce_javascript_safe_integer_bounds() {
    let mut item = memory_item();
    item.source_spans = vec![span(0, JS_SAFE_INTEGER as usize)];
    item.validate().expect("JS safe integer end is allowed");
    item.source_spans = vec![span(0, JS_SAFE_INTEGER as usize + 1)];
    assert!(item.validate().is_err());
    let item_json = to_json(&memory_item());
    let overflowing = item_json
        .to_string()
        .replace("\"end\":18", "\"end\":18446744073709551616");
    assert!(serde_json::from_str::<MemoryItem>(&overflowing).is_err());
}

#[test]
fn spans_validate_embedded_reference_ids() {
    let mut item = memory_item();
    item.source_spans[0].document_id = "x".repeat(257);
    assert!(item.validate().is_err());
    item.source_spans[0].document_id = "   ".into();
    assert!(item.validate().is_err());
    item.source_spans[0].revision_id = "a\tb".into();
    assert!(item.validate().is_err());
}

#[test]
fn spans_reject_exact_duplicates() {
    let mut item = memory_item();
    item.source_spans = vec![span(0, 18), span(0, 18)];
    assert!(item.validate().is_err());
    item.source_spans = vec![span(0, 18), span(0, 19)];
    item.validate()
        .expect("overlapping but distinct spans are kept");
    let mut draft = skill();
    draft.source_spans = vec![span(0, 18), span(0, 18)];
    assert!(draft.validate().is_err());
}

#[test]
fn episode_ids_enforce_nonempty_bounded_unique_lists() {
    let mut item = memory_item();
    item.episode_ids = vec![];
    assert!(item.validate().is_err());
    item.episode_ids = (0..64).map(|index| format!("episode-{index}")).collect();
    item.validate().expect("64 episode ids");
    item.episode_ids = (0..65).map(|index| format!("episode-{index}")).collect();
    assert!(item.validate().is_err());
    item.episode_ids = vec!["episode-1".into(), "episode-1".into()];
    assert!(item.validate().is_err());
    item.episode_ids = vec![String::new()];
    assert!(item.validate().is_err());
}

#[test]
fn contradiction_ids_enforce_bounded_unique_lists() {
    let mut item = memory_item();
    item.contradiction_ids = vec![];
    item.validate().expect("empty contradiction list is valid");
    item.contradiction_ids = (0..64).map(|index| format!("item-{index}")).collect();
    item.validate().expect("64 contradiction ids");
    item.contradiction_ids = (0..65).map(|index| format!("item-{index}")).collect();
    assert!(item.validate().is_err());
    item.contradiction_ids = vec!["item-2".into(), "item-2".into()];
    assert!(item.validate().is_err());
}

#[test]
fn limitations_enforce_count_byte_and_blank_bounds() {
    let mut item = memory_item();
    item.limitations = (0..16).map(|index| format!("lim-{index}")).collect();
    item.validate().expect("16 limitations");
    item.limitations = (0..17).map(|index| format!("lim-{index}")).collect();
    assert!(item.validate().is_err());
    item.limitations = vec!["a".repeat(2048)];
    item.validate().expect("2048 UTF8 bytes");
    item.limitations = vec!["a".repeat(2049)];
    assert!(item.validate().is_err());
    item.limitations = vec!["   ".into()];
    assert!(item.validate().is_err());
    item.limitations = vec![String::new()];
    assert!(item.validate().is_err());
    item.limitations = vec!["ок".repeat(512)];
    item.validate()
        .expect("2048 UTF8 bytes of multibyte entries");
}

#[test]
fn memory_item_accepts_all_exact_kind_and_support_values() {
    for kind in [
        MemoryItemKind::Episode,
        MemoryItemKind::ReportedAssertion,
        MemoryItemKind::Procedure,
        MemoryItemKind::StyleExample,
    ] {
        let mut item = memory_item();
        item.kind = kind;
        item.validate().expect("exact kind value");
    }
    for support in [
        MemoryItemSupport::Observed,
        MemoryItemSupport::Reported,
        MemoryItemSupport::Hypothesis,
        MemoryItemSupport::Unknown,
        MemoryItemSupport::Conflicting,
    ] {
        let mut item = memory_item();
        item.support = support;
        item.validate()
            .expect("exact support value; no semantic promotion rules here");
    }
    let item_json = to_json(&memory_item());
    for (key, value) in [("kind", json!("rumor")), ("support", json!("proven"))] {
        let broken = with(&item_json, |map| {
            map.insert(key.to_string(), value.clone());
        });
        assert!(serde_json::from_str::<MemoryItem>(&broken).is_err());
    }
}

#[test]
fn skill_fields_enforce_byte_and_blank_bounds() {
    for field in ["trigger", "outcome_check"] {
        let mut draft = skill();
        match field {
            "trigger" => draft.trigger = String::new(),
            _ => draft.outcome_check = String::new(),
        }
        assert!(draft.validate().is_err());
        let mut draft = skill();
        match field {
            "trigger" => draft.trigger = "  ".into(),
            _ => draft.outcome_check = "\t".into(),
        }
        assert!(draft.validate().is_err());
        let mut draft = skill();
        match field {
            "trigger" => draft.trigger = "x".repeat(4097),
            _ => draft.outcome_check = "x".repeat(4097),
        }
        assert!(draft.validate().is_err());
        let mut draft = skill();
        match field {
            "trigger" => draft.trigger = "x".repeat(4096),
            _ => draft.outcome_check = "x".repeat(4096),
        }
        draft.validate().expect("4096 UTF8 bytes");
    }
}

#[test]
fn skill_inputs_steps_memory_refs_and_spans_enforce_bounds() {
    let mut draft = skill();
    draft.inputs = (0..16).map(|index| format!("input-{index}")).collect();
    draft.validate().expect("16 inputs");
    draft.inputs = (0..17).map(|index| format!("input-{index}")).collect();
    assert!(draft.validate().is_err());
    draft.inputs = vec!["   ".into()];
    assert!(draft.validate().is_err());
    draft = skill();
    draft.inputs = vec!["a".repeat(2049)];
    assert!(draft.validate().is_err());
    draft.inputs = vec!["a".repeat(2048)];
    draft.validate().expect("2048-byte entry");

    draft = skill();
    draft.steps = vec![];
    assert!(draft.validate().is_err(), "steps must be nonempty");
    draft.steps = (0..16).map(|index| format!("step-{index}")).collect();
    draft.validate().expect("16 steps");
    draft.steps = (0..17).map(|index| format!("step-{index}")).collect();
    assert!(draft.validate().is_err());

    draft = skill();
    draft.memory_item_ids = vec![];
    assert!(draft.validate().is_err());
    draft.memory_item_ids = (0..16).map(|index| format!("item-{index}")).collect();
    draft.validate().expect("16 memory item refs");
    draft.memory_item_ids = (0..17).map(|index| format!("item-{index}")).collect();
    assert!(draft.validate().is_err());
    draft.memory_item_ids = vec!["item-1".into(), "item-1".into()];
    assert!(draft.validate().is_err());

    draft = skill();
    draft.source_spans = vec![];
    assert!(draft.validate().is_err());
    draft.source_spans = (0..64).map(|index| span(index, index + 1)).collect();
    draft.validate().expect("64 spans");
    draft.source_spans = (0..65).map(|index| span(index, index + 1)).collect();
    assert!(draft.validate().is_err());
}

#[test]
fn skill_support_values_are_exact_and_no_semantic_rules_apply() {
    let mut draft = skill();
    draft.support = SkillSupport::Observed;
    draft.limitations = vec![];
    draft
        .validate()
        .expect("observed skills carry no limitation requirement here");
    draft.support = SkillSupport::Proposed;
    assert!(
        draft.validate().is_err(),
        "proposed skills require at least one limitation"
    );
    draft.limitations = vec!["Требует проверки.".into()];
    draft
        .validate()
        .expect("proposed skill with explicit limitation");
    let skill_json = to_json(&skill());
    let broken = with(&skill_json, |map| {
        map.insert("support".to_string(), json!("suggested"));
    });
    assert!(serde_json::from_str::<SkillDefinition>(&broken).is_err());
}

#[test]
fn snapshot_item_ids_enforce_bounded_unique_lists_and_empty_is_valid() {
    let mut snap = snapshot();
    snap.item_ids = vec![];
    snap.validate().expect("empty memory snapshot is valid");
    snap.item_ids = (0..1000).map(|index| format!("item-{index}")).collect();
    snap.validate().expect("1000 item ids");
    snap.item_ids = (0..1001).map(|index| format!("item-{index}")).collect();
    assert!(snap.validate().is_err());
    snap.item_ids = vec!["item-1".into(), "item-1".into()];
    assert!(snap.validate().is_err());
    snap.item_ids = vec!["x".repeat(257)];
    assert!(snap.validate().is_err());
}

#[test]
fn manifest_list_ids_and_reference_pairs_are_unique_and_bounded() {
    let mut value = manifest();
    value.skill_ids = (0..64).map(|index| format!("skill-{index}")).collect();
    value.skill_ids.push("skill-0".into());
    assert!(value.validate().is_err(), "duplicate skill id");
    value = manifest();
    value.skill_ids = (0..65).map(|index| format!("skill-{index}")).collect();
    assert!(value.validate().is_err());
    value.skill_ids = (0..64).map(|index| format!("skill-{index}")).collect();
    value.validate().expect("64 skill ids");

    value = manifest();
    value.exemplar_ids = (0..100).map(|index| format!("exemplar-{index}")).collect();
    value.validate().expect("100 exemplar ids");
    value.exemplar_ids = (0..101).map(|index| format!("exemplar-{index}")).collect();
    assert!(value.validate().is_err());

    value = manifest();
    let many_refs: Vec<RevisionReference> = (0..10_000)
        .map(|index| RevisionReference {
            document_id: format!("doc-{index}"),
            revision_id: "revision-1".into(),
        })
        .collect();
    value.source_refs = many_refs.clone();
    value.validate().expect("10000 source refs");
    let mut with_duplicate = many_refs;
    with_duplicate.push(RevisionReference {
        document_id: "doc-1".into(),
        revision_id: "revision-1".into(),
    });
    value.source_refs = with_duplicate;
    assert!(value.validate().is_err(), "duplicate reference pair");
    value.source_refs = (0..10_001)
        .map(|index| RevisionReference {
            document_id: format!("doc-{index}"),
            revision_id: "revision-1".into(),
        })
        .collect();
    assert!(value.validate().is_err());
    value = manifest();
    value.source_refs = vec![
        RevisionReference {
            document_id: "doc-1".into(),
            revision_id: "revision-1".into(),
        },
        RevisionReference {
            document_id: "doc-1".into(),
            revision_id: "revision-2".into(),
        },
    ];
    value
        .validate()
        .expect("distinct revisions of one document are distinct refs");
}

#[test]
fn manifest_requires_exactly_four_distinct_components() {
    let mut value = manifest();
    value.model_settings = model_settings()[..3].to_vec();
    assert!(value.validate().is_err(), "three components");
    value.model_settings = {
        let mut entries = model_settings();
        entries.push(settings_entry(
            ModelComponent::Stalker,
            ModelProvider::Kimi,
            None,
            ModelEffort::Low,
        ));
        entries
    };
    assert!(value.validate().is_err(), "duplicate component");
    let mut settings = model_settings();
    settings[1].component = ModelComponent::PersonFabric;
    value.model_settings = settings;
    value.validate().expect("four distinct components");
    let components: BTreeSet<_> = value
        .model_settings
        .iter()
        .map(|entry| entry.component)
        .collect();
    assert_eq!(components.len(), 4);
}

#[test]
fn manifest_requested_model_is_nullable_and_bounded() {
    let mut entry = settings_entry(
        ModelComponent::Person,
        ModelProvider::Kimi,
        Some("k".repeat(120)),
        ModelEffort::Low,
    );
    entry.validate().expect("120-byte requested model");
    entry.requested_model = Some("k".repeat(121));
    assert!(entry.validate().is_err());
    entry.requested_model = Some(String::new());
    assert!(entry.validate().is_err());
    entry.requested_model = Some("   ".into());
    assert!(entry.validate().is_err());
    entry.requested_model = Some("  alias  ".into());
    entry.validate().expect("surrounding whitespace preserved");
    entry.requested_model = None;
    entry
        .validate()
        .expect("null leaves model choice to the CLI");
}

#[test]
fn manifest_version_strings_are_nonblank_and_bounded() {
    for (target, field) in [
        ("profileVersion", "profile_version"),
        ("splitVersion", "split_version"),
        ("preparationVersion", "preparation_version"),
        ("retrievalVersion", "retrieval_version"),
        ("promptVersion", "prompt_version"),
    ] {
        let mut broken = manifest();
        match field {
            "profile_version" => broken.profile_version = String::new(),
            "split_version" => broken.split_version = "  ".into(),
            "preparation_version" => broken.preparation_version = "x".repeat(257),
            "retrieval_version" => broken.retrieval_version = "x".repeat(257),
            _ => broken.prompt_version = "\u{0007}".into(),
        }
        assert!(
            broken.validate().is_err(),
            "{target} must be a nonblank id <= 256 bytes"
        );
    }
    let mut value = manifest();
    value.corpus_snapshot_id = "x".repeat(257);
    assert!(value.validate().is_err());
    value = manifest();
    value.memory_snapshot_id = "x".repeat(257);
    assert!(value.validate().is_err());
}

#[test]
fn manifest_embedding_fingerprint_is_nullable_without_mode_equivalence() {
    let mut value = manifest();
    value.embedding_fingerprint = None;
    value
        .validate()
        .expect("null fingerprint is structurally valid for any mode");
    value.mode = RecallMode::AgenticRecall;
    value
        .validate()
        .expect("no fixedContext/agenticRecall to lexical/hybrid equivalence here");
    let mut broken = manifest();
    let mut fp = fingerprint();
    fp.dimensions = 0;
    broken.embedding_fingerprint = Some(fp);
    assert!(broken.validate().is_err());
}

#[test]
fn manifest_rejects_unknown_enum_values() {
    let manifest_json = to_json(&manifest());
    let cases = vec![("mode", json!("lexical")), ("mode", json!("hybrid"))];
    for (key, value) in cases {
        let broken = with(&manifest_json, |map| {
            map.insert(key.to_string(), value);
        });
        assert!(serde_json::from_str::<PersonaManifest>(&broken).is_err());
    }
    for (key, value) in [
        ("component", json!("personFabric")),
        ("component", json!("person_fabric")),
        ("component", json!("brainAnalyzator")),
        ("provider", json!("openai")),
        ("requestedEffort", json!("ultra")),
    ] {
        let broken = with(&manifest_json, |map| {
            map["modelSettings"][0][key] = value.clone();
        });
        assert!(serde_json::from_str::<PersonaManifest>(&broken).is_err());
    }
}

#[test]
fn fingerprint_enforces_sha256_dimensions_and_templates() {
    let mut fp = fingerprint();
    fp.model_sha256 = "a".repeat(63);
    assert!(fp.validate().is_err());
    fp.model_sha256 = "a".repeat(65);
    assert!(fp.validate().is_err());
    fp.model_sha256 = "A".repeat(64);
    assert!(fp.validate().is_err(), "uppercase hex is rejected");
    fp.model_sha256 = "g".repeat(64);
    assert!(fp.validate().is_err());
    fp.model_sha256 = hex64('a');
    fp.tokenizer_sha256 = hex64('f');
    fp.validate().expect("64 lowercase hex digits");

    fp = fingerprint();
    fp.dimensions = 0;
    assert!(fp.validate().is_err());
    fp.dimensions = 4096;
    fp.validate().expect("4096 dimensions");
    fp.dimensions = 4097;
    assert!(fp.validate().is_err());

    fp = fingerprint();
    fp.query_template = String::new();
    assert!(fp.validate().is_err());
    fp.query_template = "q".repeat(4097);
    assert!(fp.validate().is_err());
    fp = fingerprint();
    fp.passage_template = " ".into();
    fp.validate().expect("whitespace-only template is nonempty");
    fp = fingerprint();
    fp.model_id = String::new();
    assert!(fp.validate().is_err());
    fp = fingerprint();
    fp.model_revision = "x".repeat(257);
    assert!(fp.validate().is_err());
    fp = fingerprint();
    fp.chunking_version = " ".into();
    assert!(fp.validate().is_err());
    let fp_json = to_json(&fingerprint());
    for (key, value) in [
        ("pooling", json!("max")),
        ("normalization", json!("l2norm")),
    ] {
        let broken = with(&fp_json, |map| {
            map.insert(key.to_string(), value.clone());
        });
        assert!(serde_json::from_str::<EmbeddingFingerprint>(&broken).is_err());
    }
}

#[test]
fn recall_budget_matches_native_memory_budget_limits() {
    // Mirrors `memory::budget::RecallLedger::new` (memory-budget.rs).
    let cases: Vec<(&str, u64, u64)> = vec![
        ("maxCalls", 1000, 1),
        ("maxCandidates", 100_000, 1),
        ("maxEpisodes", 1000, 1),
        ("maxSourceBytes", 64 * 1024 * 1024, 1),
        ("maxContextTokens", 1_000_000, 1),
        ("maxDurationMs", 3_600_000, 1),
    ];
    for (field, max, min) in cases {
        let mut budget = recall_budget();
        let (too_low, at_min, at_max, too_high) = match field {
            "maxCalls" => {
                budget.max_calls = min - 1;
                let a = budget.clone();
                budget.max_calls = min;
                let b = budget.clone();
                budget.max_calls = max;
                let c = budget.clone();
                budget.max_calls = max + 1;
                (a, b, c, budget)
            }
            "maxCandidates" => {
                budget.max_candidates = min - 1;
                let a = budget.clone();
                budget.max_candidates = min;
                let b = budget.clone();
                budget.max_candidates = max;
                let c = budget.clone();
                budget.max_candidates = max + 1;
                (a, b, c, budget)
            }
            "maxEpisodes" => {
                budget.max_episodes = min - 1;
                let a = budget.clone();
                budget.max_episodes = min;
                let b = budget.clone();
                budget.max_episodes = max;
                let c = budget.clone();
                budget.max_episodes = max + 1;
                (a, b, c, budget)
            }
            "maxSourceBytes" => {
                budget.max_source_bytes = min - 1;
                let a = budget.clone();
                budget.max_source_bytes = min;
                let b = budget.clone();
                budget.max_source_bytes = max;
                let c = budget.clone();
                budget.max_source_bytes = max + 1;
                (a, b, c, budget)
            }
            "maxContextTokens" => {
                budget.max_context_tokens = min - 1;
                let a = budget.clone();
                budget.max_context_tokens = min;
                let b = budget.clone();
                budget.max_context_tokens = max;
                let c = budget.clone();
                budget.max_context_tokens = max + 1;
                (a, b, c, budget)
            }
            _ => {
                budget.max_duration_ms = min - 1;
                let a = budget.clone();
                budget.max_duration_ms = min;
                let b = budget.clone();
                budget.max_duration_ms = max;
                let c = budget.clone();
                budget.max_duration_ms = max + 1;
                (a, b, c, budget)
            }
        };
        assert!(too_low.validate().is_err(), "{field} below minimum");
        at_min.validate().expect("{field} at minimum");
        at_max.validate().expect("{field} at maximum");
        assert!(too_high.validate().is_err(), "{field} above maximum");
    }
}

#[test]
fn model_call_budget_limits_every_model_attempt() {
    let mut budget = model_call_budget();
    budget.max_calls = 0;
    assert!(budget.validate().is_err());
    budget.max_calls = 1000;
    budget.max_input_bytes = 64 * 1024 * 1024;
    budget.max_duration_ms = 3_600_000;
    budget.validate().expect("inclusive maximums");
    budget.max_calls = 1001;
    assert!(budget.validate().is_err());
    budget.max_calls = 1;
    budget.max_input_bytes = 0;
    assert!(budget.validate().is_err());
    budget.max_input_bytes = 64 * 1024 * 1024 + 1;
    assert!(budget.validate().is_err());
    budget.max_input_bytes = 1;
    budget.max_duration_ms = 0;
    assert!(budget.validate().is_err());
    budget.max_duration_ms = 3_600_001;
    assert!(budget.validate().is_err());
}

#[test]
fn budgets_reject_overflow_negative_and_float_spellings() {
    for (field, valid) in [
        ("maxCalls", 10),
        ("maxCandidates", 500),
        ("maxEpisodes", 20),
        ("maxSourceBytes", 65536),
        ("maxContextTokens", 4096),
        ("maxDurationMs", 60_000),
    ] {
        let budget_json = to_json(&recall_budget());
        for spelling in ["-1", "1e2", "100.0", "18446744073709551616"] {
            let broken = budget_json.to_string().replace(
                &format!("\"{field}\":{valid}"),
                &format!("\"{field}\":{spelling}"),
            );
            assert!(
                serde_json::from_str::<RecallBudget>(&broken).is_err(),
                "{field} numeric spelling {spelling}"
            );
        }
        let ok = with(&budget_json, |map| {
            map[field] = json!(valid);
        });
        serde_json::from_str::<RecallBudget>(&ok).expect("integer spelling");
    }
    let call_budget_json = to_json(&model_call_budget());
    for spelling in ["-5", "1e3", "100.0", "18446744073709551616"] {
        let broken = call_budget_json
            .to_string()
            .replace("\"maxCalls\":4", &format!("\"maxCalls\":{spelling}"));
        assert!(serde_json::from_str::<ModelCallBudget>(&broken).is_err());
    }
}

#[test]
fn decode_helpers_reject_garbage_and_out_of_bounds_json() {
    assert!(decode_memory_item("not json").is_err());
    assert!(decode_memory_item("{}").is_err());
    let mut item = memory_item();
    item.text = String::new();
    assert!(decode_memory_item(&as_json(&item)).is_err());
    let mut value = manifest();
    value.model_settings = vec![];
    assert!(decode_persona_manifest(&as_json(&value)).is_err());
}

#[test]
fn roundtrip_preserves_every_field_byte_for_byte() {
    let mut item = memory_item();
    item.event_at = Some("2026-11-01T09:30:00+03:00".into());
    item.text = "\u{FEFF}первая строка\r\nвторая   ".into();
    let decoded: MemoryItem = serde_json::from_str(&as_json(&item)).expect("roundtrip");
    assert_eq!(decoded, item);
    let draft = skill();
    let decoded: SkillDefinition = serde_json::from_str(&as_json(&draft)).expect("roundtrip");
    assert_eq!(decoded, draft);
    let value = manifest();
    let decoded: PersonaManifest = serde_json::from_str(&as_json(&value)).expect("roundtrip");
    assert_eq!(decoded, value);
}

// Root review: reject an oversized serialized payload BEFORE deserialization,
// including padding that would disappear from the validated DTO.
macro_rules! serialized_limit_case {
    ($name:ident, $fixture:expr, $decode:ident, $limit:expr) => {
        #[test]
        fn $name() {
            let json = serde_json::to_string(&$fixture).unwrap();
            let at_limit = format!("{json}{}", " ".repeat($limit - json.len()));
            assert!($decode(&at_limit).is_ok(), "exact serialized byte limit");
            assert!(
                $decode(&(at_limit + " ")).is_err(),
                "reject bytes before decoding"
            );
        }
    };
}
serialized_limit_case!(
    item_json_size_is_bounded,
    memory_item(),
    decode_memory_item,
    512 * 1024
);
serialized_limit_case!(
    skill_json_size_is_bounded,
    skill(),
    decode_skill_definition,
    512 * 1024
);
serialized_limit_case!(
    snapshot_json_size_is_bounded,
    snapshot(),
    decode_memory_snapshot,
    1024 * 1024
);
serialized_limit_case!(
    manifest_json_size_is_bounded,
    manifest(),
    decode_persona_manifest,
    8 * 1024 * 1024
);
serialized_limit_case!(
    fingerprint_json_size_is_bounded,
    fingerprint(),
    decode_embedding_fingerprint,
    64 * 1024
);
serialized_limit_case!(
    recall_budget_json_size_is_bounded,
    recall_budget(),
    decode_recall_budget,
    4096
);
serialized_limit_case!(
    model_budget_json_size_is_bounded,
    model_call_budget(),
    decode_model_call_budget,
    4096
);
