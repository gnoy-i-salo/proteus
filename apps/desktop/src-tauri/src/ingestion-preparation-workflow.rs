//! Application policy for local preparation; IPC never supplies original text,
//! filesystem paths, tokenizer implementations or episode assignments.
use super::{
    operations::Operation,
    preparation::{prepare, PrepareInput},
    publication::SnapshotView,
};
use crate::sources::Sources;
use crate::{
    corpus_database::episodes::{ChunkPolicy, EpisodePolicy},
    local_embedding::tokenizer::{E5Tokenizer, MAX_SEQUENCE_TOKENS},
};
use serde::Deserialize;
use std::path::Path;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrepareRequest {
    pub schema_version: u8,
    pub operation_id: String,
    pub job_id: String,
    pub max_duration_ms: i64,
}
impl PrepareRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !(1..=3_600_000).contains(&self.max_duration_ms)
            || [&self.operation_id, &self.job_id].iter().any(|id| {
                id.trim().is_empty() || id.len() > 128 || id.chars().any(char::is_control)
            })
        {
            return Err("Неверный запрос подготовки корпуса.".into());
        }
        Ok(())
    }
}
pub fn execute(
    sources: &Sources,
    operation: &Operation,
    request: &PrepareRequest,
    tokenizer_path: &Path,
    now_ms: impl Fn() -> i64,
) -> Result<SnapshotView, String> {
    request.validate()?;
    if operation.id() != request.operation_id {
        return Err("Запрос относится к другой операции подготовки.".into());
    }
    operation.check()?;
    let started = now_ms();
    if started < 0 {
        return Err("Неверное время подготовки.".into());
    }
    let tokenizer = E5Tokenizer::load(tokenizer_path)?;
    operation.check()?;
    let elapsed = now_ms()
        .checked_sub(started)
        .filter(|elapsed| *elapsed >= 0)
        .ok_or("Неверное время подготовки.")?;
    let remaining = request
        .max_duration_ms
        .checked_sub(elapsed)
        .filter(|remaining| *remaining > 0)
        .ok_or("Превышено время подготовки.")?;
    // Application-owned, versioned policy. The core fingerprints every value.
    let split = EpisodePolicy {
        time_gap_ms: 30 * 60 * 1000,
        holdout_basis_points: 2000,
        seed: "proteusReferenceHoldout:1".into(),
    };
    let chunks = ChunkPolicy {
        max_tokens: MAX_SEQUENCE_TOKENS,
        max_bytes: 64 * 1024,
    };
    prepare(
        sources,
        operation,
        &PrepareInput {
            job_id: &request.job_id,
            split: &split,
            chunks: &chunks,
            tokenizer: &tokenizer,
            max_duration_ms: remaining,
        },
        now_ms,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_request_accepts_only_versioned_metadata_and_bounded_budget() {
        let value = serde_json::json!({
            "schemaVersion":1,"operationId":"prepare:1","jobId":"job:1","maxDurationMs":60000
        });
        let request: PrepareRequest = serde_json::from_value(value.clone()).unwrap();
        assert!(request.validate().is_ok());
        for (key, wrong) in [
            ("schemaVersion", serde_json::json!(2)),
            ("maxDurationMs", serde_json::json!(0)),
            ("maxDurationMs", serde_json::json!(-1)),
            ("maxDurationMs", serde_json::json!(3600001)),
            ("operationId", serde_json::json!("")),
            ("jobId", serde_json::json!("\n")),
            ("jobId", serde_json::json!("я".repeat(65))),
        ] {
            let mut invalid = value.clone();
            invalid[key] = wrong;
            assert!(serde_json::from_value::<PrepareRequest>(invalid)
                .unwrap()
                .validate()
                .is_err());
        }
        for field in ["text", "chunks", "tokenizerPath", "job_id"] {
            let mut invalid = value.clone();
            invalid[field] = serde_json::json!("not accepted");
            assert!(serde_json::from_value::<PrepareRequest>(invalid).is_err());
        }
        assert!(serde_json::from_str::<PrepareRequest>(
            r#"{"schemaVersion":1,"operationId":"a","jobId":"b","jobId":"c","maxDurationMs":1}"#
        )
        .is_err());
    }
}
