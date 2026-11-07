use super::preview_store::read_preview;
use crate::corpus_database::{
    validate_source_binding, CorpusDatabase, JobBinding, JobPeriod, JobProvider, JobRole,
    RecordInput, RecordNamespace,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindingRequest {
    pub preview_id: String,
    pub resource_id: String,
    pub persona_id: String,
    pub role: JobRole,
    pub target_author_id: Option<String>,
    pub allowed_context_author_ids: Vec<String>,
    pub allow_unknown_context: bool,
    pub period: JobPeriod,
    pub allow_model_processing: bool,
    pub allow_export: bool,
}
#[derive(Clone, Copy)]
pub enum GrantUse {
    Corpus,
    Model,
    Export,
}
const POLICY: &str = "ingestionBinding:1";
const MAX_SAFE: i64 = 9_007_199_254_740_991;
const LIFETIME_MS: i64 = 86_400_000;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GrantRecord {
    schema_version: u8,
    preview_id: String,
    binding: JobBinding,
    allow_model_processing: bool,
    allow_export: bool,
    created_at_ms: i64,
    expires_at_ms: i64,
}
pub fn create_binding(
    db: &mut CorpusDatabase,
    request: BindingRequest,
    now_ms: i64,
) -> Result<JobBinding, String> {
    if !(0..=MAX_SAFE - LIFETIME_MS).contains(&now_ms) {
        return Err("Неверное время разрешения.".into());
    }
    let preview = read_preview(db, &request.preview_id)?;
    let resource = preview
        .resources
        .iter()
        .find(|r| r.id == request.resource_id)
        .ok_or("Источник отсутствует в проверенном предпросмотре.")?;
    if request
        .target_author_id
        .as_ref()
        .is_some_and(|id| !resource.author_ids.contains(id))
        || request
            .allowed_context_author_ids
            .iter()
            .any(|id| !resource.author_ids.contains(id))
    {
        return Err("Автор отсутствует в выбранном источнике.".into());
    }
    let provider = match preview.provider.as_str() {
        "telegram" => JobProvider::Telegram,
        "vk" => JobProvider::Vk,
        "github" => JobProvider::Github,
        "instagram" => JobProvider::Instagram,
        "web" => JobProvider::Web,
        "savedPage" => JobProvider::SavedPage,
        _ => return Err("Неизвестный сервис источника.".into()),
    };
    let grant_id = format!("grant:{}", crate::connections::storage::random_id(24)?);
    let binding = JobBinding {
        schema_version: 1,
        id: format!("binding:{}", crate::connections::storage::random_id(24)?),
        persona_id: request.persona_id,
        provider,
        account_id: None,
        resource_id: request.resource_id,
        grant_id: grant_id.clone(),
        policy_version: POLICY.into(),
        role: request.role,
        target_author_id: request.target_author_id,
        allowed_context_author_ids: request.allowed_context_author_ids,
        allow_unknown_context: request.allow_unknown_context,
        period: request.period,
    };
    validate_source_binding(&binding)?;
    let grant = GrantRecord {
        schema_version: 1,
        preview_id: request.preview_id,
        binding: binding.clone(),
        allow_model_processing: request.allow_model_processing,
        allow_export: request.allow_export,
        created_at_ms: now_ms,
        expires_at_ms: now_ms + LIFETIME_MS,
    };
    let state_json =
        serde_json::to_string(&grant).map_err(|_| "Не удалось сохранить разрешение.")?;
    db.save_ingestion(&RecordInput {
        id: grant_id,
        kind: "grant".into(),
        provider: preview.provider,
        state_json,
        documents: vec![],
    })?;
    Ok(binding)
}
pub fn authorize_binding(
    db: &CorpusDatabase,
    binding: &JobBinding,
    purpose: GrantUse,
    now_ms: i64,
) -> Result<(), String> {
    validate_source_binding(binding)?;
    let stored = db
        .read(&binding.grant_id)?
        .ok_or("Разрешение источника не найдено.")?;
    if stored.namespace != RecordNamespace::Ingestion
        || stored.input.kind != "grant"
        || !stored.input.documents.is_empty()
    {
        return Err("Разрешение отозвано или принадлежит другому формату.".into());
    }
    let grant: GrantRecord = serde_json::from_str(&stored.input.state_json)
        .map_err(|_| "Неподдерживаемый формат разрешения.")?;
    if grant.schema_version != 1
        || grant.binding != *binding
        || binding.policy_version != POLICY
        || grant.created_at_ms < 0
        || grant.created_at_ms > MAX_SAFE - LIFETIME_MS
        || grant.expires_at_ms != grant.created_at_ms + LIFETIME_MS
        || now_ms < grant.created_at_ms
        || now_ms >= grant.expires_at_ms
    {
        return Err("Разрешение не соответствует области или истекло.".into());
    }
    let expected = match binding.provider {
        JobProvider::Telegram => "telegram",
        JobProvider::Vk => "vk",
        JobProvider::Github => "github",
        JobProvider::Instagram => "instagram",
        JobProvider::Web => "web",
        JobProvider::SavedPage => "savedPage",
    };
    if stored.input.provider != expected {
        return Err("Разрешение принадлежит другому сервису.".into());
    }
    match purpose {
        GrantUse::Model if !grant.allow_model_processing => {
            Err("Модельная обработка источника не разрешена.".into())
        }
        GrantUse::Export if !grant.allow_export => Err("Экспорт источника не разрешён.".into()),
        _ => Ok(()),
    }
}
#[cfg(test)]
#[path = "ingestion-grants-tests.rs"]
mod tests;
