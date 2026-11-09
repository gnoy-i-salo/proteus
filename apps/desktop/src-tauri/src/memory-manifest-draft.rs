//! Host draft only. Native derives source identity, preparation and stored IDs.
use super::types::{
    self, ModelCallBudget, ModelComponent, ModelSettingsEntry, PersonaManifest, RecallBudget,
    RecallMode, SkillDefinition, SkillSupport,
};
use crate::corpus_database::episodes::ChunkSpan;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonaManifestDraft {
    pub schema_version: u8,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::types::profile_snapshot"
    )]
    pub profile_snapshot: Option<super::profile_snapshot::PersonaProfileSnapshot>,
    pub profile_version: String,
    pub memory_snapshot_id: String,
    pub context_lease_ids: Vec<String>,
    pub exemplar_ids: Vec<String>,
    pub prompt_version: String,
    pub mode: RecallMode,
    pub recall_budget: RecallBudget,
    pub model_call_budget: ModelCallBudget,
    pub model_settings: Vec<ModelSettingsEntry>,
    pub skills: Vec<SkillDefinitionDraft>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillDefinitionDraft {
    pub draft_id: String,
    pub trigger: String,
    pub inputs: Vec<String>,
    pub steps: Vec<String>,
    pub outcome_check: String,
    pub support: SkillSupport,
    pub memory_item_ids: Vec<String>,
    pub source_spans: Vec<ChunkSpan>,
    pub limitations: Vec<String>,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFreezeReceipt {
    pub schema_version: u8,
    pub manifest: PersonaManifest,
    pub skills: Vec<SkillDefinition>,
}
impl PersonaManifestDraft {
    pub fn validate(&self) -> Result<(), String> {
        match (self.schema_version, &self.profile_snapshot) {
            (1, None) => (),
            (2, Some(profile)) => {
                profile.validate()?;
                if !self
                    .exemplar_ids
                    .iter()
                    .eq(profile.style_exemplars.iter().map(|sample| &sample.id))
                {
                    return Err("Черновик изменяет образцы профиля.".into());
                }
            }
            _ => return Err("Неверная версия тела черновика.".into()),
        }
        if self.skills.len() > 64 || self.model_settings.len() != 4 {
            return Err("Неверная схема черновика манифеста.".into());
        }
        for value in [
            &self.profile_version,
            &self.memory_snapshot_id,
            &self.prompt_version,
        ] {
            types::validate_host_id(value, "версия или снимок манифеста")?;
        }
        types::validate_unique_host_ids(&self.context_lease_ids, "контекст Fabric", 1000)?;
        if self.context_lease_ids.is_empty() {
            return Err("Манифест требует завершённого контекста Fabric.".into());
        }
        types::validate_unique_host_ids(&self.exemplar_ids, "образец манифеста", 100)?;
        self.recall_budget.validate()?;
        self.model_call_budget.validate()?;
        let mut components = BTreeSet::<ModelComponent>::new();
        for setting in &self.model_settings {
            setting.validate()?;
            if !components.insert(setting.component) {
                return Err("Повторный компонент модели манифеста.".into());
            }
        }
        let mut ids = BTreeSet::new();
        for skill in &self.skills {
            if !ids.insert(&skill.draft_id) {
                return Err("Повторный черновик навыка.".into());
            }
            skill.materialize(&skill.draft_id).validate()?;
        }
        Ok(())
    }
}
impl SkillDefinitionDraft {
    pub fn materialize(&self, id: &str) -> SkillDefinition {
        SkillDefinition {
            schema_version: 1,
            id: id.into(),
            trigger: self.trigger.clone(),
            inputs: self.inputs.clone(),
            steps: self.steps.clone(),
            outcome_check: self.outcome_check.clone(),
            support: self.support,
            memory_item_ids: self.memory_item_ids.clone(),
            source_spans: self.source_spans.clone(),
            limitations: self.limitations.clone(),
        }
    }
}
