//! Exact task-free profile body. This new schema is independent of legacy results.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonaProfileSnapshot {
    pub schema_version: u8,
    pub label: String,
    pub palette: Palette,
    pub archetype: String,
    pub rules: Vec<ProfileRule>,
    pub source_status: SourceStatus,
    pub style_profile: StyleProfile,
    pub style_exemplars: Vec<StyleExemplar>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileRule {
    pub id: String,
    pub instruction: String,
    pub evidence_ids: Vec<String>,
    pub status: RuleStatus,
    pub limitations: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StyleProfile {
    pub version: u8,
    pub reasoning_patterns: Vec<StyleRule>,
    pub voice_rules: Vec<StyleRule>,
    pub format_rules: Vec<StyleRule>,
    pub context_modes: Vec<ContextMode>,
    pub style_exemplar_ids: Vec<String>,
    pub limitations: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StyleRule {
    pub id: String,
    pub instruction: String,
    pub exemplar_ids: Vec<String>,
    pub status: StyleStatus,
    pub limitations: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextMode {
    pub mode: CommunicationMode,
    pub status: ModeStatus,
    pub limitations: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StyleExemplar {
    pub id: String,
    pub document_id: String,
    pub mode: CommunicationMode,
    pub text: String,
    pub context: String,
    pub redactions: Vec<Redaction>,
    pub export_allowed: bool,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Redaction {
    pub start: usize,
    pub end: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Palette {
    Turquoise,
    Violet,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceStatus {
    EvidenceInformed,
    Synthetic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleStatus {
    Supported,
    Hypothesis,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StyleStatus {
    Observed,
    Hypothesis,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModeStatus {
    Observed,
    Insufficient,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CommunicationMode {
    Dialogue,
    Comment,
    Formal,
}

impl PersonaProfileSnapshot {
    pub fn validate(&self) -> Result<(), String> {
        const INVALID: &str = "Неверное тело профиля Person.";
        if self.schema_version != 1
            || self.label.len() > 512
            || self.rules.len() > 10
            || self.style_exemplars.len() > 100
            || serde_json::to_vec(self).map_err(|_| INVALID)?.len() > 524288
        {
            return Err(INVALID.into());
        }
        unique_ids(self.rules.iter().map(|rule| &rule.id), 10)?;
        unique_ids(self.style_exemplars.iter().map(|sample| &sample.id), 100)?;
        for rule in &self.rules {
            unique_ids(rule.evidence_ids.iter(), 1000)?;
            if rule.evidence_ids.is_empty() {
                return Err(INVALID.into());
            }
        }
        if (self.source_status == SourceStatus::EvidenceInformed)
            != self
                .rules
                .iter()
                .any(|rule| rule.status == RuleStatus::Supported)
        {
            return Err(INVALID.into());
        }
        for sample in &self.style_exemplars {
            super::types::validate_host_id(&sample.document_id, "документ образца")?;
            if sample.export_allowed || !sample.redactions.is_empty() {
                return Err(INVALID.into());
            }
        }
        self.style_profile.validate(&self.style_exemplars)
    }
}
fn unique_ids<'a>(ids: impl Iterator<Item = &'a String>, maximum: usize) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        super::types::validate_host_id(id, "ссылка профиля")?;
        if !seen.insert(id) || seen.len() > maximum {
            return Err("Повторная ссылка профиля.".into());
        }
    }
    Ok(())
}
impl StyleProfile {
    fn validate(&self, samples: &[StyleExemplar]) -> Result<(), String> {
        const INVALID: &str = "Стиль не соответствует образцам профиля.";
        if self.version != 2
            || self.context_modes.len() != 3
            || self.reasoning_patterns.len() > 10
            || self.voice_rules.len() > 10
            || self.format_rules.len() > 10
        {
            return Err(INVALID.into());
        }
        let rules = || {
            self.reasoning_patterns
                .iter()
                .chain(&self.voice_rules)
                .chain(&self.format_rules)
        };
        unique_ids(rules().map(|rule| &rule.id), 30)?;
        for rule in rules() {
            unique_ids(rule.exemplar_ids.iter(), 1000)?;
            if rule.exemplar_ids.is_empty()
                || rule
                    .exemplar_ids
                    .iter()
                    .any(|id| !samples.iter().any(|sample| sample.id == *id))
            {
                return Err(INVALID.into());
            }
        }
        if self
            .reasoning_patterns
            .iter()
            .any(|rule| rule.status != StyleStatus::Hypothesis)
            || !self
                .style_exemplar_ids
                .iter()
                .eq(samples.iter().map(|sample| &sample.id))
        {
            return Err(INVALID.into());
        }
        let mut modes = std::collections::HashSet::new();
        for mode in &self.context_modes {
            if !modes.insert(mode.mode)
                || (mode.status == ModeStatus::Observed)
                    != samples.iter().any(|sample| sample.mode == mode.mode)
            {
                return Err(INVALID.into());
            }
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "memory-profile-snapshot-tests.rs"]
mod tests;
