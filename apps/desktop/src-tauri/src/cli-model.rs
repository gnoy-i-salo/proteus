use crate::cli_policy::validate_isolation;
use crate::cli_protocol::{Reasoning, ReasoningBlock};
use serde::{Deserialize, Serialize};

#[cfg_attr(test, derive(Serialize))]
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub(super) id: String,
    pub(super) provider: String,
    pub(super) model: String,
    pub(super) task: String,
    pub(super) system_prompt: String,
    pub(super) effort: String,
    #[serde(default = "default_operation")]
    pub(super) operation: String,
    #[serde(default)]
    pub(super) source_context: bool,
    #[serde(
        default,
        deserialize_with = "present_memory_context",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) memory_context: Option<ModelContextReference>,
}
#[cfg_attr(test, derive(Serialize))]
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ModelContextReference {
    pub(super) session_id: String,
    pub(super) lease_id: String,
}
fn present_memory_context<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ModelContextReference>, D::Error> {
    ModelContextReference::deserialize(deserializer).map(Some)
}
fn default_operation() -> String {
    "generation".into()
}
#[cfg_attr(test, derive(Deserialize))]
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) reasoning: Option<ReasoningBlock>,
}
#[cfg_attr(test, derive(Deserialize))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trace {
    pub(super) provider: String,
    pub(super) wall_time_ms: u64,
    // Deprecated compatibility field: this is wall time, never measured active reasoning.
    pub(super) reasoning_ms: u64,
    pub(super) active_reasoning_ms: Option<u64>,
    pub(super) requested_model: String,
    pub(super) resolved_model: Option<String>,
    pub(super) requested_effort: String,
    pub(super) applied_effort: Option<String>,
    pub(super) cli_version: Option<String>,
    pub(super) terminal_ms: u64,
    pub(super) summary: String,
    pub(super) reasoning: Vec<ReasoningBlock>,
    pub(super) reasoning_truncated: bool,
}
#[cfg_attr(test, derive(Deserialize))]
#[derive(Serialize)]
pub struct Response {
    pub(super) answer: String,
    pub(super) trace: Trace,
}
#[derive(Serialize)]
pub struct Availability {
    pub(super) provider: String,
    pub(super) available: bool,
    pub(super) path: Option<String>,
}

pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
pub(super) fn validate(request: &Request) -> Result<(), String> {
    if let Some(context) = &request.memory_context {
        let valid_handle = |id: &str| {
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b":_-".contains(&b))
        };
        if !request.source_context
            || !["corpus-analysis", "persona-reply"].contains(&request.operation.as_str())
            || !valid_handle(&context.session_id)
            || !valid_handle(&context.lease_id)
        {
            return Err("Invalid memory context reference".into());
        }
    }
    if !valid_id(&request.id)
        || !["codex", "kimi"].contains(&request.provider.as_str())
        || !["low", "medium", "high"].contains(&request.effort.as_str())
        || request.task.len() > 200_000
        || request.system_prompt.len() > 40_000
        || request.model.len() > 120
        || request.model.contains(['\n', '\r', '\0'])
    {
        return Err("Invalid CLI request".into());
    }
    validate_isolation(
        &request.provider,
        &request.operation,
        request.source_context,
    )?;
    Ok(())
}
pub(super) fn response(
    request: &Request,
    answer: String,
    elapsed: u64,
    reasoning: Reasoning,
) -> Response {
    Response {
        answer,
        trace: Trace {
            provider: request.provider.clone(),
            wall_time_ms: elapsed,
            reasoning_ms: elapsed,
            active_reasoning_ms: None,
            requested_model: request.model.clone(),
            resolved_model: None,
            requested_effort: request.effort.clone(),
            // Codex receives an explicit CLI setting. Kimi prompt mode has no effort setting here.
            applied_effort: (request.provider == "codex").then(|| request.effort.clone()),
            cli_version: None,
            terminal_ms: 0,
            summary: "CLI returned an answer".into(),
            reasoning: reasoning.blocks,
            reasoning_truncated: reasoning.truncated,
        },
    }
}
