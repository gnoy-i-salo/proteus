//! Types, policy caps, content hashing and input validation for the corpus
//! FTS search. DTOs are plain owned data; `serde` boundaries are camelCase
//! with `deny_unknown_fields`, native inputs stay snake_case.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_QUERY_BYTES: usize = 4096;
pub const MAX_TERM_BYTES: usize = 256;
pub const MAX_TERMS: usize = 64;
pub const MAX_LIMIT: i64 = 100;
pub const MAX_OFFSET: i64 = 10_000;
pub const MAX_LIST_ITEMS: usize = super::super::job_types::MAX_JOB_BINDINGS;
pub const MAX_ID_LEN: usize = 256;
pub const MAX_CHUNK_BYTES: usize = 4 * 1024 * 1024;
/// Numbers crossing this boundary lose precision in JavaScript; serialized
/// search DTOs and staged metadata stay within it.
pub const MAX_JS_SAFE_INTEGER: i64 = (1 << 53) - 1;
/// sha256 of the empty digest stream: the content manifest of a
/// reference-empty corpus that legitimately publishes zero chunks.
pub const EMPTY_CONTENT_SHA256: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
pub(super) const MAX_SPAN: i64 = 1_000_000_000_000;
pub const MAX_STAGE_CHUNKS: i64 = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchChunkInput {
    pub stage_id: String,
    pub chunk_id: String,
    pub snapshot_id: String,
    pub snapshot_generation: i64,
    pub partition: String,
    pub text: String,
    pub document_id: String,
    pub revision_id: String,
    pub span_start: i64,
    pub span_end: i64,
    pub persona_id: String,
    pub grant_id: String,
    pub binding_id: String,
    pub record_id: String,
    pub record_generation: i64,
    pub period_from: Option<i64>,
    pub period_to: Option<i64>,
    pub conversation_id: Option<String>,
    pub thread_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchStageInput {
    pub stage_id: String,
    pub snapshot_id: String,
    pub snapshot_generation: i64,
    pub policy_fingerprint: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageManifest {
    pub chunk_count: i64,
    pub content_sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchPeriod {
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub include_undated: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchScope {
    pub snapshot_id: String,
    pub snapshot_generation: i64,
    pub persona_id: String,
    pub grant_ids: Vec<String>,
    pub binding_ids: Vec<String>,
    pub period: SearchPeriod,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SearchRequest {
    pub scope: SearchScope,
    pub query_text: String,
    pub limit: i64,
    pub offset: i64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchHit {
    pub chunk_id: String,
    pub snapshot_id: String,
    pub index_generation: i64,
    pub bm25: f64,
    pub document_id: String,
    pub revision_id: String,
    pub span_start: i64,
    pub span_end: i64,
    pub persona_id: String,
    pub grant_id: String,
    pub binding_id: String,
    pub period_from: Option<i64>,
    pub period_to: Option<i64>,
    pub conversation_id: Option<String>,
    pub thread_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchResponse {
    pub snapshot_id: String,
    pub snapshot_generation: i64,
    pub index_generation: i64,
    pub has_more: bool,
    pub hits: Vec<SearchHit>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchIndexReceipt {
    pub snapshot_id: String,
    pub snapshot_generation: i64,
    pub index_generation: i64,
    pub policy_fingerprint: String,
    pub content_sha256: String,
    pub chunk_count: i64,
    pub created_at: String,
}

/// Deterministic content hash of one chunk over its immutable fields. The
/// parent worker can stream these into `content_manifest` while staging.
pub fn chunk_content_hash(chunk: &SearchChunkInput) -> String {
    let canonical = serde_json::json!({
        "stageId": &chunk.stage_id,
        "chunkId": &chunk.chunk_id,
        "snapshotId": &chunk.snapshot_id,
        "snapshotGeneration": chunk.snapshot_generation,
        "partition": &chunk.partition,
        "text": &chunk.text,
        "documentId": &chunk.document_id,
        "revisionId": &chunk.revision_id,
        "spanStart": chunk.span_start,
        "spanEnd": chunk.span_end,
        "personaId": &chunk.persona_id,
        "grantId": &chunk.grant_id,
        "bindingId": &chunk.binding_id,
        "recordId": &chunk.record_id,
        "recordGeneration": chunk.record_generation,
        "periodFrom": chunk.period_from,
        "periodTo": chunk.period_to,
        "conversationId": &chunk.conversation_id,
        "threadId": &chunk.thread_id,
    });
    let json = serde_json::to_vec(&canonical).expect("serializing chunk fields cannot fail");
    format!("{:x}", Sha256::digest(json))
}
/// Manifest over per-chunk digests, ordered by chunk_id. `commit_stage`
/// recomputes the same value from staged rows and rejects any drift. An empty
/// digest list yields `EMPTY_CONTENT_SHA256` with chunk count 0.
pub fn content_manifest<A: AsRef<str>, B: AsRef<str>>(digests: &[(A, B)]) -> StageManifest {
    let mut ordered = digests.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.0.as_ref().cmp(right.0.as_ref()));
    let mut hasher = Sha256::new();
    for (_, digest) in ordered {
        hasher.update(digest.as_ref().as_bytes());
    }
    StageManifest {
        chunk_count: digests.len() as i64,
        content_sha256: format!("{:x}", hasher.finalize()),
    }
}

pub(super) fn invalid_id(value: &str) -> bool {
    value.is_empty() || value.len() > MAX_ID_LEN || value.chars().any(char::is_control)
}
fn optional_id_valid(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(|id| !invalid_id(id))
}
fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn text_control_character(character: char) -> bool {
    character.is_control() && !matches!(character, '\r' | '\n' | '\t')
}
fn js_safe(value: i64) -> bool {
    (-MAX_JS_SAFE_INTEGER..=MAX_JS_SAFE_INTEGER).contains(&value)
}

pub(super) fn validate_stage(input: &SearchStageInput) -> Result<(), String> {
    if invalid_id(&input.stage_id)
        || invalid_id(&input.snapshot_id)
        || !(1..=MAX_JS_SAFE_INTEGER).contains(&input.snapshot_generation)
        || input.policy_fingerprint.is_empty()
        || input.policy_fingerprint.len() > MAX_ID_LEN
        || input.policy_fingerprint.chars().any(char::is_control)
    {
        return Err(super::invalid_data("Недопустимые параметры этапа индекса."));
    }
    Ok(())
}
pub(super) fn validate_chunk(chunk: &SearchChunkInput) -> Result<(), String> {
    if invalid_id(&chunk.stage_id)
        || invalid_id(&chunk.chunk_id)
        || invalid_id(&chunk.snapshot_id)
        || invalid_id(&chunk.document_id)
        || invalid_id(&chunk.revision_id)
        || invalid_id(&chunk.persona_id)
        || invalid_id(&chunk.grant_id)
        || invalid_id(&chunk.binding_id)
        || invalid_id(&chunk.record_id)
        || !optional_id_valid(&chunk.conversation_id)
        || !optional_id_valid(&chunk.thread_id)
        || chunk.snapshot_generation <= 0
        || chunk.snapshot_generation > MAX_JS_SAFE_INTEGER
        || chunk.record_generation <= 0
        || chunk.record_generation > MAX_JS_SAFE_INTEGER
        || chunk.text.is_empty()
        || chunk.text.len() > MAX_CHUNK_BYTES
        || chunk.text.chars().any(text_control_character)
        || !(0..=MAX_SPAN).contains(&chunk.span_start)
        || chunk.span_end <= chunk.span_start
        || chunk.span_end > MAX_SPAN
    {
        return Err(super::invalid_data("Недопустимый поисковый чанк корпуса."));
    }
    match (chunk.period_from, chunk.period_to) {
        (Some(from), Some(to)) if from > to => {
            return Err(super::invalid_data("Некорректный период чанка корпуса."));
        }
        (from, to)
            if from.is_some_and(|value| !js_safe(value))
                || to.is_some_and(|value| !js_safe(value)) =>
        {
            return Err(super::invalid_data(
                "Период чанка корпуса выходит за пределы точности.",
            ));
        }
        _ => {}
    }
    if chunk.partition != "reference" {
        return if chunk.partition == "holdout" {
            Err(super::invalid_data(
                "Holdout-содержимое не может быть добавлено в поисковый индекс.",
            ))
        } else {
            Err(super::invalid_data(
                "Индексируется только reference-раздел эпизодов.",
            ))
        };
    }
    Ok(())
}
pub(in crate::corpus_database) fn validate_scope(scope: &SearchScope) -> Result<(), String> {
    if invalid_id(&scope.snapshot_id)
        || !(1..=MAX_JS_SAFE_INTEGER).contains(&scope.snapshot_generation)
        || invalid_id(&scope.persona_id)
        || scope.grant_ids.is_empty()
        || scope.grant_ids.len() > MAX_LIST_ITEMS
        || scope.grant_ids.iter().any(|id| invalid_id(id))
        || scope.binding_ids.is_empty()
        || scope.binding_ids.len() > MAX_LIST_ITEMS
        || scope.binding_ids.iter().any(|id| invalid_id(id))
    {
        return Err(super::invalid_data(
            "Область поиска должна явно указывать персону, гранты и привязки.",
        ));
    }
    if let (Some(from), Some(to)) = (scope.period.from, scope.period.to) {
        if from > to {
            return Err(super::invalid_data("Некорректный период области поиска."));
        }
    }
    if scope.period.from.is_some_and(|value| !js_safe(value))
        || scope.period.to.is_some_and(|value| !js_safe(value))
    {
        return Err(super::invalid_data(
            "Период области поиска выходит за пределы точности.",
        ));
    }
    Ok(())
}
pub(super) fn validate_manifest(manifest: &StageManifest) -> Result<(), String> {
    if manifest.chunk_count < 0 || manifest.chunk_count > MAX_STAGE_CHUNKS {
        return Err(super::invalid_data("Недопустимый манифест этапа индекса."));
    }
    if manifest.chunk_count == 0 {
        // A reference-empty corpus finishes publication with a zero-chunk
        // manifest: count 0 and the sha256 of the empty digest stream. No
        // fake chunks are synthesized.
        if manifest.content_sha256 != EMPTY_CONTENT_SHA256 {
            return Err(super::invalid_data("Недопустимый манифест этапа индекса."));
        }
        return Ok(());
    }
    if !is_sha256_hex(&manifest.content_sha256) {
        return Err(super::invalid_data("Недопустимый манифест этапа индекса."));
    }
    Ok(())
}
