//! Bind every profile-derived prompt field to the immutable native bundle.
use super::{
    context_wire::ModelContextRequest,
    profile_snapshot::{ProfileRule, RuleStatus, SourceStatus, StyleExemplar, StyleProfile},
    types::{PersonaManifest, RecallMode, SkillDefinition},
};
use serde::{Deserialize, Serialize};
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
    #[serde(default, deserialize_with = "optional_adaptation")]
    task_adaptation: Option<TaskAdaptation>,
    #[serde(default, deserialize_with = "optional_text")]
    format_feedback: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskAdaptation {
    schema_version: u8,
    manifest_id: String,
    task: String,
    strategies: Vec<ProfileRule>,
}
fn optional_adaptation<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<TaskAdaptation>, D::Error> {
    TaskAdaptation::deserialize(deserializer).map(Some)
}
impl TaskAdaptation {
    fn validate(&self, manifest: &PersonaManifest, task: &str) -> Result<(), String> {
        let profile = manifest.profile_snapshot.as_ref().ok_or(INVALID)?;
        if self.schema_version != 1
            || self.manifest_id != manifest.id
            || self.task != task
            || !(1..=5).contains(&self.strategies.len())
            || serde_json::to_vec(self).map_err(|_| INVALID)?.len() > 150000
        {
            return Err(INVALID.into());
        }
        let mut ids = HashSet::new();
        for strategy in &self.strategies {
            let suffix = strategy.id.strip_prefix('s').ok_or(INVALID)?;
            if suffix.is_empty()
                || !suffix.bytes().all(|byte| byte.is_ascii_digit())
                || strategy.id.len() > 256
                || !ids.insert(&strategy.id)
                || strategy.instruction.trim().is_empty()
                || strategy.instruction.len() > 96000
                || strategy.status != RuleStatus::Hypothesis
                || strategy.evidence_ids.len() > 1000
                || strategy.limitations.len() > 1000
                || strategy.limitations.iter().any(|text| text.len() > 150000)
                || (profile.source_status == SourceStatus::Neutral
                    && !strategy.evidence_ids.is_empty())
            {
                return Err(INVALID.into());
            }
            let mut evidence_ids = HashSet::new();
            for id in &strategy.evidence_ids {
                if id.trim().is_empty()
                    || id.len() > 256
                    || id.chars().any(char::is_control)
                    || !evidence_ids.insert(id)
                {
                    return Err(INVALID.into());
                }
                let rules: Vec<_> = profile
                    .rules
                    .iter()
                    .filter(|rule| rule.evidence_ids.contains(id))
                    .collect();
                if rules.is_empty()
                    || rules.iter().any(|rule| {
                        rule.limitations
                            .iter()
                            .any(|text| !strategy.limitations.contains(text))
                    })
                {
                    return Err(INVALID.into());
                }
            }
        }
        Ok(())
    }
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
    task_guidance: String,
}
#[cfg(test)]
pub(crate) fn system_prompt(status: SourceStatus, attempt: u8) -> Result<String, String> {
    render_prompt(status, attempt, false)
}
fn render_prompt(status: SourceStatus, attempt: u8, adaptation: bool) -> Result<String, String> {
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
        "PERSON memory\n{perspective}{}{}{}",
        spec.body,
        if attempt == 2 { &spec.repair } else { "" },
        if adaptation { &spec.task_guidance } else { "" }
    ))
}
const INVALID: &str = "Вход Person изменяет закреплённое содержимое профиля.";
pub(super) fn validate(
    manifest: &PersonaManifest,
    skills: &[SkillDefinition],
    request: &ModelContextRequest,
) -> Result<(), String> {
    let profile = manifest.profile_snapshot.as_ref().ok_or(INVALID)?;
    let input: PersonaInput = serde_json::from_str(&request.task).map_err(|_| INVALID)?;
    if request.system_prompt
        != render_prompt(
            profile.source_status,
            request.format_attempt,
            input.task_adaptation.is_some(),
        )?
    {
        return Err(INVALID.into());
    }
    if let Some(adaptation) = &input.task_adaptation {
        adaptation.validate(manifest, &input.task)?;
    }
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
