//! Persisted memory DTOs for the native storage boundary: MemoryItem,
//! SkillDefinition, MemorySnapshot, PersonaManifest, EmbeddingFingerprint,
//! RecallBudget and ModelCallBudget, mirroring `packages/contracts/src/memory.ts`.
//!
//! These are NEW native persisted DTOs (schemaVersion 1, camelCase wire,
//! `deny_unknown_fields`, exact TS enum values). This slice provides
//! structural storage-boundary validation only: it does not prove source
//! truth, own authorship, source-span UTF8 alignment, grants, snapshot
//! membership, contradiction-graph semantics, policy or frozen scope. Those
//! stay with the root publication scenario, which validates them from
//! originals and consumed leases.
//!
//! Text, timestamps and IDs are preserved byte-for-byte; validation never
//! trims or normalizes them. Required nullable fields (`eventAt`,
//! `embeddingFingerprint`, `requestedModel`) reject omitted keys and accept
//! explicit `null`.
use crate::corpus_database::episodes::{ChunkSpan, SpanUnit};
use crate::corpus_database::RevisionReference;
use serde::{Deserialize, Serialize};

const MAX_ID_BYTES: usize = 256;
const MAX_TIMESTAMP_BYTES: usize = 64;
const MAX_TEXT_BYTES: usize = 64 * 1024;
const MAX_LIMITATION_BYTES: usize = 2048;
const MAX_SKILL_FIELD_BYTES: usize = 4096;
const MAX_TEMPLATE_BYTES: usize = 4096;
const MAX_REQUESTED_MODEL_BYTES: usize = 120;
const MAX_SPANS: usize = 64;
const MAX_LIMITATIONS: usize = 16;
const MAX_EPISODE_IDS: usize = 64;
const MAX_CONTRADICTION_IDS: usize = 64;
const MAX_MEMORY_ITEM_REFS: usize = 16;
const MAX_LIST_ENTRIES: usize = 16;
const MAX_SKILL_IDS: usize = 64;
const MAX_EXEMPLAR_IDS: usize = 100;
const MAX_SOURCE_REFS: usize = 10_000;
const MAX_SNAPSHOT_ITEMS: usize = 1000;
const MAX_DIMENSIONS: u64 = 4096;
const SHA256_HEX_BYTES: usize = 64;
const JS_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MODEL_COMPONENT_COUNT: usize = 4;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum MemoryItemKind {
    Episode,
    ReportedAssertion,
    Procedure,
    StyleExample,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum MemoryItemSupport {
    Observed,
    Reported,
    Hypothesis,
    Unknown,
    Conflicting,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum SkillSupport {
    Observed,
    Proposed,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum RecallMode {
    FixedContext,
    AgenticRecall,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum ModelComponent {
    #[serde(rename = "stalker")]
    Stalker,
    #[serde(rename = "person-fabric")]
    PersonFabric,
    #[serde(rename = "person")]
    Person,
    #[serde(rename = "brain-analyzator")]
    BrainAnalyzator,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum ModelProvider {
    Codex,
    Kimi,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum ModelEffort {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum Pooling {
    Mean,
    Cls,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum Normalization {
    L2,
    None,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddingFingerprint {
    pub model_id: String,
    pub model_revision: String,
    pub model_sha256: String,
    pub tokenizer_sha256: String,
    pub dimensions: u64,
    pub pooling: Pooling,
    pub normalization: Normalization,
    pub query_template: String,
    pub passage_template: String,
    pub chunking_version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecallBudget {
    pub max_calls: u64,
    pub max_candidates: u64,
    pub max_episodes: u64,
    pub max_source_bytes: u64,
    pub max_context_tokens: u64,
    pub max_duration_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelCallBudget {
    pub max_calls: u64,
    pub max_input_bytes: u64,
    pub max_duration_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelSettingsEntry {
    pub component: ModelComponent,
    pub provider: ModelProvider,
    #[serde(deserialize_with = "required_nullable")]
    pub requested_model: Option<String>,
    pub requested_effort: ModelEffort,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryItem {
    pub schema_version: u8,
    pub id: String,
    pub snapshot_id: String,
    pub kind: MemoryItemKind,
    pub text: String,
    pub source_spans: Vec<ChunkSpan>,
    pub episode_ids: Vec<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub event_at: Option<String>,
    pub observed_at: String,
    pub support: MemoryItemSupport,
    pub contradiction_ids: Vec<String>,
    pub limitations: Vec<String>,
    pub extraction_version: String,
    pub policy_version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillDefinition {
    pub schema_version: u8,
    pub id: String,
    pub trigger: String,
    pub inputs: Vec<String>,
    pub steps: Vec<String>,
    pub outcome_check: String,
    pub support: SkillSupport,
    pub memory_item_ids: Vec<String>,
    pub source_spans: Vec<ChunkSpan>,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemorySnapshot {
    pub schema_version: u8,
    pub id: String,
    pub corpus_snapshot_id: String,
    pub item_ids: Vec<String>,
    pub extraction_version: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonaManifest {
    pub schema_version: u8,
    pub id: String,
    pub persona_id: String,
    pub profile_version: String,
    pub skill_ids: Vec<String>,
    pub exemplar_ids: Vec<String>,
    pub corpus_snapshot_id: String,
    pub memory_snapshot_id: String,
    pub source_refs: Vec<RevisionReference>,
    pub split_version: String,
    pub preparation_version: String,
    pub retrieval_version: String,
    pub prompt_version: String,
    #[serde(deserialize_with = "required_nullable")]
    pub embedding_fingerprint: Option<EmbeddingFingerprint>,
    pub mode: RecallMode,
    pub recall_budget: RecallBudget,
    pub model_call_budget: ModelCallBudget,
    pub model_settings: Vec<ModelSettingsEntry>,
    pub created_at: String,
}

pub(super) fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    // `deserialize_with` disables serde's missing-`Option`-defaults-to-`None`
    // special case, so an omitted key is a `missing_field` error while an
    // explicit `null` still decodes to `None`.
    Option::<T>::deserialize(deserializer)
}

fn is_blank(value: &str) -> bool {
    value.is_empty() || value.chars().all(char::is_whitespace)
}

/// Host id/version policy, mirroring the bounded host ids of the STALKER and
/// Fabric parsers: nonblank, no control characters, at most 256 UTF8 bytes.
/// Surrounding whitespace is preserved, never trimmed or normalized.
pub(super) fn validate_host_id(value: &str, label: &str) -> Result<(), String> {
    if is_blank(value) {
        return Err(format!("{label} не может быть пустым."));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label} содержит управляющие символы."));
    }
    if value.len() > MAX_ID_BYTES {
        return Err(format!("{label} превышает {MAX_ID_BYTES} байт UTF8."));
    }
    Ok(())
}

pub(super) fn validate_timestamp(value: &str, label: &str) -> Result<(), String> {
    if value.len() > MAX_TIMESTAMP_BYTES {
        return Err(format!("{label} превышает {MAX_TIMESTAMP_BYTES} байт."));
    }
    if chrono::DateTime::parse_from_rfc3339(value).is_err() {
        return Err(format!("{label} должен быть реальной датой RFC3339."));
    }
    Ok(())
}

pub(super) fn validate_text(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > MAX_TEXT_BYTES {
        return Err(format!(
            "{label} должен занимать 1..={MAX_TEXT_BYTES} байт UTF8."
        ));
    }
    Ok(())
}

fn validate_nonblank_entry(value: &str, label: &str, max_bytes: usize) -> Result<(), String> {
    if is_blank(value) {
        return Err(format!("{label} не может быть пустым."));
    }
    if value.len() > max_bytes {
        return Err(format!("{label} превышает {max_bytes} байт UTF8."));
    }
    Ok(())
}

pub(super) fn validate_unique_host_ids(
    values: &[String],
    label: &str,
    max_entries: usize,
) -> Result<(), String> {
    if values.len() > max_entries {
        return Err(format!("{label}: превышен лимит в {max_entries} записей."));
    }
    let mut seen = std::collections::HashSet::new();
    for value in values {
        validate_host_id(value, label)?;
        if !seen.insert(value.as_str()) {
            return Err(format!("{label}: повторяющийся идентификатор."));
        }
    }
    Ok(())
}

/// Structural span check only. The unit is `utf8Bytes` at the type level and
/// no character alignment is inferred here; UTF8 boundary alignment against
/// original source bytes stays with the native authority.
fn validate_span(span: &ChunkSpan, label: &str) -> Result<(), String> {
    validate_host_id(&span.document_id, label)?;
    validate_host_id(&span.revision_id, label)?;
    match span.unit {
        SpanUnit::Utf8Bytes => {}
    }
    if span.start >= span.end {
        return Err(format!(
            "{label}: начало интервала должно быть меньше конца."
        ));
    }
    if span.end as u64 > JS_SAFE_INTEGER {
        return Err(format!(
            "{label}: конец интервала превышает безопасное целое JavaScript."
        ));
    }
    Ok(())
}

pub(super) fn validate_spans(spans: &[ChunkSpan], label: &str) -> Result<(), String> {
    if spans.is_empty() || spans.len() > MAX_SPANS {
        return Err(format!(
            "{label}: интервалов источника должно быть 1..={MAX_SPANS}."
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for span in spans {
        validate_span(span, label)?;
        let key = (
            span.document_id.as_str(),
            span.revision_id.as_str(),
            span.start,
            span.end,
        );
        if !seen.insert(key) {
            return Err(format!("{label}: повторяющийся интервал источника."));
        }
    }
    Ok(())
}

pub(super) fn validate_limitations(values: &[String], label: &str) -> Result<(), String> {
    if values.len() > MAX_LIMITATIONS {
        return Err(format!(
            "{label}: превышен лимит в {MAX_LIMITATIONS} записей."
        ));
    }
    for value in values {
        validate_nonblank_entry(value, label, MAX_LIMITATION_BYTES)?;
    }
    Ok(())
}

impl EmbeddingFingerprint {
    pub fn validate(&self) -> Result<(), String> {
        validate_host_id(&self.model_id, "идентификатор модели эмбеддингов")?;
        validate_host_id(&self.model_revision, "ревизия модели эмбеддингов")?;
        validate_host_id(&self.chunking_version, "версия разбиения на фрагменты")?;
        for (value, label) in [
            (&self.model_sha256, "хеш модели"),
            (&self.tokenizer_sha256, "хеш токенизатора"),
        ] {
            let lowercase_hex = value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
            if value.len() != SHA256_HEX_BYTES || !lowercase_hex {
                return Err(format!(
                    "{label} должен быть SHA256 из {SHA256_HEX_BYTES} hex-символов в нижнем регистре."
                ));
            }
        }
        if !(1..=MAX_DIMENSIONS).contains(&self.dimensions) {
            return Err(format!(
                "число измерений эмбеддингов должно быть в пределах 1..={MAX_DIMENSIONS}."
            ));
        }
        if self.query_template.is_empty() || self.query_template.len() > MAX_TEMPLATE_BYTES {
            return Err(format!(
                "шаблон запроса должен занимать 1..={MAX_TEMPLATE_BYTES} байт UTF8."
            ));
        }
        if self.passage_template.is_empty() || self.passage_template.len() > MAX_TEMPLATE_BYTES {
            return Err(format!(
                "шаблон фрагмента должен занимать 1..={MAX_TEMPLATE_BYTES} байт UTF8."
            ));
        }
        Ok(())
    }
}

impl RecallBudget {
    /// Limits mirror `memory::budget::RecallLedger::new` (memory-budget.rs):
    /// the persisted DTO and the runtime ledger stay on one host policy.
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=1000).contains(&self.max_calls) {
            return Err("бюджет памяти: maxCalls должен быть в пределах 1..=1000.".into());
        }
        if !(1..=100_000).contains(&self.max_candidates) {
            return Err("бюджет памяти: maxCandidates должен быть в пределах 1..=100000.".into());
        }
        if !(1..=1000).contains(&self.max_episodes) {
            return Err("бюджет памяти: maxEpisodes должен быть в пределах 1..=1000.".into());
        }
        if !(1..=64 * 1024 * 1024).contains(&self.max_source_bytes) {
            return Err("бюджет памяти: maxSourceBytes должен быть в пределах 1..=64MiB.".into());
        }
        if !(1..=1_000_000).contains(&self.max_context_tokens) {
            return Err(
                "бюджет памяти: maxContextTokens должен быть в пределах 1..=1000000.".into(),
            );
        }
        if !(1..=3_600_000).contains(&self.max_duration_ms) {
            return Err("бюджет памяти: maxDurationMs должен быть в пределах 1..=3600000.".into());
        }
        Ok(())
    }
}

impl ModelCallBudget {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=1000).contains(&self.max_calls) {
            return Err("бюджет вызовов модели: maxCalls должен быть в пределах 1..=1000.".into());
        }
        if !(1..=64 * 1024 * 1024).contains(&self.max_input_bytes) {
            return Err(
                "бюджет вызовов модели: maxInputBytes должен быть в пределах 1..=64MiB.".into(),
            );
        }
        if !(1..=3_600_000).contains(&self.max_duration_ms) {
            return Err(
                "бюджет вызовов модели: maxDurationMs должен быть в пределах 1..=3600000.".into(),
            );
        }
        Ok(())
    }
}

impl ModelSettingsEntry {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(requested_model) = &self.requested_model {
            validate_nonblank_entry(
                requested_model,
                "запрошенная модель",
                MAX_REQUESTED_MODEL_BYTES,
            )?;
        }
        Ok(())
    }
}

impl MemoryItem {
    /// Kind/support values are exact enum members; there are no semantic
    /// promotion rules at this storage boundary.
    pub fn validate(&self) -> Result<(), String> {
        validate_version(self.schema_version)?;
        validate_host_id(&self.id, "идентификатор элемента памяти")?;
        validate_host_id(&self.snapshot_id, "идентификатор снимка памяти")?;
        validate_text(&self.text, "текст элемента памяти")?;
        validate_spans(&self.source_spans, "интервал источника элемента памяти")?;
        validate_unique_host_ids(&self.episode_ids, "идентификатор эпизода", MAX_EPISODE_IDS)?;
        if self.episode_ids.is_empty() {
            return Err("идентификаторов эпизодов должно быть 1..=64.".into());
        }
        if let Some(event_at) = &self.event_at {
            validate_timestamp(event_at, "время события")?;
        }
        validate_timestamp(&self.observed_at, "время наблюдения")?;
        validate_unique_host_ids(
            &self.contradiction_ids,
            "идентификатор противоречия",
            MAX_CONTRADICTION_IDS,
        )?;
        validate_limitations(&self.limitations, "ограничение элемента памяти")?;
        validate_host_id(&self.extraction_version, "версия извлечения")?;
        validate_host_id(&self.policy_version, "версия политики")?;
        Ok(())
    }
}

impl SkillDefinition {
    /// `observed` source-procedure truth is root-owned and deliberately not
    /// enforced here; `proposed` skills require at least one explicit
    /// limitation. List membership against a snapshot stays with the host.
    pub fn validate(&self) -> Result<(), String> {
        validate_version(self.schema_version)?;
        validate_host_id(&self.id, "идентификатор навыка")?;
        validate_nonblank_entry(&self.trigger, "условие навыка", MAX_SKILL_FIELD_BYTES)?;
        if self.inputs.len() > MAX_LIST_ENTRIES {
            return Err("входы навыка: превышен лимит в 16 записей.".into());
        }
        for input in &self.inputs {
            validate_nonblank_entry(input, "вход навыка", MAX_LIMITATION_BYTES)?;
        }
        if self.steps.is_empty() || self.steps.len() > MAX_LIST_ENTRIES {
            return Err("шаги навыка должны занимать 1..=16 записей.".into());
        }
        for step in &self.steps {
            validate_nonblank_entry(step, "шаг навыка", MAX_LIMITATION_BYTES)?;
        }
        validate_nonblank_entry(
            &self.outcome_check,
            "проверка результата навыка",
            MAX_SKILL_FIELD_BYTES,
        )?;
        validate_unique_host_ids(
            &self.memory_item_ids,
            "идентификатор элемента памяти",
            MAX_MEMORY_ITEM_REFS,
        )?;
        if self.memory_item_ids.is_empty() {
            return Err("навык должен ссылаться на 1..=16 элементов памяти.".into());
        }
        validate_spans(&self.source_spans, "интервал источника навыка")?;
        validate_limitations(&self.limitations, "ограничение навыка")?;
        if self.support == SkillSupport::Proposed && self.limitations.is_empty() {
            return Err("предлагаемый навык требует хотя бы одного ограничения.".into());
        }
        Ok(())
    }
}

impl MemorySnapshot {
    pub fn validate(&self) -> Result<(), String> {
        validate_version(self.schema_version)?;
        validate_host_id(&self.id, "идентификатор снимка памяти")?;
        validate_host_id(&self.corpus_snapshot_id, "идентификатор снимка корпуса")?;
        validate_unique_host_ids(
            &self.item_ids,
            "идентификатор элемента памяти",
            MAX_SNAPSHOT_ITEMS,
        )?;
        validate_host_id(&self.extraction_version, "версия извлечения")?;
        validate_timestamp(&self.created_at, "время создания снимка")?;
        Ok(())
    }
}

impl PersonaManifest {
    /// A null `embeddingFingerprint` is structurally valid for any mode; this
    /// boundary never equates `fixedContext`/`agenticRecall` with
    /// lexical/hybrid modes, and pinned-profile equality stays root-owned.
    pub fn validate(&self) -> Result<(), String> {
        validate_version(self.schema_version)?;
        validate_host_id(&self.id, "идентификатор манифеста")?;
        validate_host_id(&self.persona_id, "идентификатор персоны")?;
        for (value, label) in [
            (&self.profile_version, "версия профиля"),
            (&self.split_version, "версия разделения"),
            (&self.preparation_version, "версия подготовки"),
            (&self.retrieval_version, "версия поиска"),
            (&self.prompt_version, "версия промптов"),
        ] {
            validate_host_id(value, label)?;
        }
        validate_host_id(&self.corpus_snapshot_id, "идентификатор снимка корпуса")?;
        validate_host_id(&self.memory_snapshot_id, "идентификатор снимка памяти")?;
        validate_unique_host_ids(&self.skill_ids, "идентификатор навыка", MAX_SKILL_IDS)?;
        validate_unique_host_ids(
            &self.exemplar_ids,
            "идентификатор образца",
            MAX_EXEMPLAR_IDS,
        )?;
        if self.source_refs.len() > MAX_SOURCE_REFS {
            return Err(format!(
                "ссылки источников манифеста превышают лимит в {MAX_SOURCE_REFS} записей."
            ));
        }
        let mut seen_refs = std::collections::HashSet::new();
        for reference in &self.source_refs {
            validate_host_id(&reference.document_id, "идентификатор документа источника")?;
            validate_host_id(&reference.revision_id, "идентификатор ревизии источника")?;
            if !seen_refs.insert((
                reference.document_id.as_str(),
                reference.revision_id.as_str(),
            )) {
                return Err("манифест содержит повторяющуюся пару ссылок источника.".into());
            }
        }
        if let Some(fingerprint) = &self.embedding_fingerprint {
            fingerprint.validate()?;
        }
        self.recall_budget.validate()?;
        self.model_call_budget.validate()?;
        if self.model_settings.len() != MODEL_COMPONENT_COUNT {
            return Err(format!(
                "настройки моделей должны покрывать ровно {MODEL_COMPONENT_COUNT} компонента."
            ));
        }
        let mut components = std::collections::BTreeSet::new();
        for entry in &self.model_settings {
            entry.validate()?;
            if !components.insert(entry.component) {
                return Err("настройки моделей содержат повторяющийся компонент.".into());
            }
        }
        validate_timestamp(&self.created_at, "время создания манифеста")?;
        Ok(())
    }
}

fn validate_version(schema_version: u8) -> Result<(), String> {
    if schema_version == 1 {
        Ok(())
    } else {
        Err("Неподдерживаемая версия схемы памяти.".into())
    }
}

pub fn decode_embedding_fingerprint(json: &str) -> Result<EmbeddingFingerprint, String> {
    let value: EmbeddingFingerprint = decode_bounded(json, 64 * 1024)?;
    value.validate()?;
    Ok(value)
}

pub fn decode_recall_budget(json: &str) -> Result<RecallBudget, String> {
    let value: RecallBudget = decode_bounded(json, 4096)?;
    value.validate()?;
    Ok(value)
}

pub fn decode_model_call_budget(json: &str) -> Result<ModelCallBudget, String> {
    let value: ModelCallBudget = decode_bounded(json, 4096)?;
    value.validate()?;
    Ok(value)
}

pub fn decode_memory_item(json: &str) -> Result<MemoryItem, String> {
    let value: MemoryItem = decode_bounded(json, 512 * 1024)?;
    value.validate()?;
    Ok(value)
}

pub fn decode_skill_definition(json: &str) -> Result<SkillDefinition, String> {
    let value: SkillDefinition = decode_bounded(json, 512 * 1024)?;
    value.validate()?;
    Ok(value)
}

pub fn decode_memory_snapshot(json: &str) -> Result<MemorySnapshot, String> {
    let value: MemorySnapshot = decode_bounded(json, 1024 * 1024)?;
    value.validate()?;
    Ok(value)
}

pub fn decode_persona_manifest(json: &str) -> Result<PersonaManifest, String> {
    let value: PersonaManifest = decode_bounded(json, 8 * 1024 * 1024)?;
    value.validate()?;
    Ok(value)
}

// Match the storage payload caps before serde allocates strings or lists.
fn decode_bounded<T: serde::de::DeserializeOwned>(
    json: &str,
    max_bytes: usize,
) -> Result<T, String> {
    if json.len() > max_bytes {
        return Err(INVALID.into());
    }
    serde_json::from_str(json).map_err(|_| INVALID.to_string())
}

const INVALID: &str = "Неверные данные памяти.";

#[cfg(test)]
#[path = "memory-types-tests.rs"]
mod tests;
