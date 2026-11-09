//! Bind every profile-derived prompt field to the immutable native bundle.
use super::{
    context_wire::ModelContextRequest,
    profile_snapshot::{ProfileRule, SourceStatus, StyleExemplar, StyleProfile},
    types::{PersonaManifest, RecallMode, SkillDefinition},
};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersonaInput {
    task: String,
    #[serde(deserialize_with = "super::types::required_nullable")]
    persona_label: Option<String>,
    archetype: String,
    skills: Vec<String>,
    rules: Vec<ProfileRule>,
    task_skills: Vec<ProfileRule>,
    source_status: SourceStatus,
    style_profile: StyleProfile,
    style_exemplars: Vec<StyleExemplar>,
    procedural_skills: Vec<SkillDefinition>,
    retrieval_allowed: bool,
    #[serde(default, deserialize_with = "optional_text")]
    format_feedback: Option<String>,
}
fn optional_text<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PromptSpec {
    schema_version: u8,
    persona_perspective: String,
    neutral_perspective: String,
    body: String,
    repair: String,
}
pub(crate) fn system_prompt(status: SourceStatus, attempt: u8) -> Result<String, String> {
    let spec: PromptSpec = serde_json::from_str(include_str!(
        "../../../../packages/contracts/src/persona-memory-prompt.json"
    ))
    .map_err(|_| INVALID)?;
    if spec.schema_version != 1 || !matches!(attempt, 1 | 2) {
        return Err(INVALID.into());
    }
    let perspective = if status == SourceStatus::Neutral {
        &spec.neutral_perspective
    } else {
        &spec.persona_perspective
    };
    Ok(format!(
        "PERSON memory\n{perspective}{}{}",
        spec.body,
        if attempt == 2 { &spec.repair } else { "" }
    ))
}
const INVALID: &str = "Вход Person изменяет закреплённое содержимое профиля.";
pub(super) fn validate(
    manifest: &PersonaManifest,
    skills: &[SkillDefinition],
    request: &ModelContextRequest,
) -> Result<(), String> {
    let profile = manifest.profile_snapshot.as_ref().ok_or(INVALID)?;
    if request.system_prompt != system_prompt(profile.source_status, request.format_attempt)? {
        return Err(INVALID.into());
    }
    let input: PersonaInput = serde_json::from_str(&request.task).map_err(|_| INVALID)?;
    let label = if profile.source_status == SourceStatus::Neutral {
        None
    } else {
        Some(profile.label.as_str())
    };
    if (request.format_attempt == 2) != input.format_feedback.is_some()
        || input
            .format_feedback
            .as_ref()
            .is_some_and(|feedback| feedback.is_empty() || feedback.len() > 4000)
        || input.task.len() > 100000
        || input.persona_label.as_deref() != label
        || input.archetype != profile.archetype
        || !input.skills.is_empty()
        || !input.task_skills.is_empty()
        || input.rules != profile.rules
        || input.source_status != profile.source_status
        || input.style_profile != profile.style_profile
        || input.style_exemplars != profile.style_exemplars
        || input.procedural_skills != skills
        || (manifest.mode == RecallMode::FixedContext && input.retrieval_allowed)
    {
        return Err(INVALID.into());
    }
    let mut seen = HashSet::new();
    let ids: Vec<_> = profile
        .rules
        .iter()
        .flat_map(|rule| &rule.evidence_ids)
        .chain(skills.iter().flat_map(|skill| &skill.memory_item_ids))
        .chain(profile.style_exemplars.iter().map(|sample| &sample.id))
        .filter(|id| seen.insert(*id))
        .collect();
    if !ids.into_iter().eq(request.memory_item_ids.iter()) {
        return Err(INVALID.into());
    }
    Ok(())
}
