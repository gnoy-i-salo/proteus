use super::grant::timestamp;
use super::model::{SourceDocument, SourceGrant, SourceGrantRequest};
use crate::connections::Provider;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub(super) const POLICY_VERSION: &str = "source-policy-v1";
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SourceRole {
    Own,
    Context,
    About,
    Reference,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindingPeriod {
    pub from: Option<String>,
    pub to: Option<String>,
    pub include_undated: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceBinding {
    pub schema_version: u8,
    pub id: String,
    pub persona_id: String,
    pub provider: Provider,
    pub account_id: Option<String>,
    pub resource_id: String,
    pub grant_id: String,
    pub policy_version: String,
    pub role: SourceRole,
    pub target_author_id: Option<String>,
    pub allowed_context_author_ids: Vec<String>,
    pub allow_unknown_context: bool,
    pub period: BindingPeriod,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceBindingRequest {
    pub source: SourceGrantRequest,
    pub persona_id: String,
    pub role: SourceRole,
    pub allowed_context_author_ids: Vec<String>,
    pub allow_unknown_context: bool,
    pub include_undated: bool,
}
#[cfg_attr(test, derive(Deserialize))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedSourceBinding {
    pub binding: SourceBinding,
    pub grant: SourceGrant,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SpeakerRelation {
    Target,
    Context,
    Unknown,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevisionReference {
    pub document_id: String,
    pub revision_id: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SpanUnit {
    Utf8Bytes,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceSpan {
    pub document_id: String,
    pub revision_id: String,
    pub unit: SpanUnit,
    pub start: usize,
    pub end: usize,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceAuthor {
    pub provider: Option<Provider>,
    pub id: Option<String>,
    pub label: Option<String>,
}
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RelationKind {
    Reply,
    Quote,
    Forward,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentRelation {
    pub kind: RelationKind,
    pub original: Option<RevisionReference>,
    pub original_author: SourceAuthor,
    pub span: Option<SourceSpan>,
}
#[cfg_attr(test, derive(Deserialize))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSourceDocument {
    pub document: SourceDocument,
    pub revision_id: String,
    pub role: SourceRole,
    pub speaker_relation: SpeakerRelation,
    pub can_support_own_words: bool,
    pub relations: Vec<DocumentRelation>,
}

const INVALID: &str = "Выбранная область источника недействительна.";
fn valid_identity(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}
fn validate_authors(
    persona: &str,
    role: SourceRole,
    target: Option<&str>,
    context: &[String],
    unknown: bool,
) -> Result<(), String> {
    if !valid_identity(persona)
        || target.is_some_and(|id| !valid_identity(id))
        || context.len() > 128
        || context
            .iter()
            .any(|id| !valid_identity(id) || Some(id.as_str()) == target)
        || context.iter().collect::<HashSet<_>>().len() != context.len()
        || (role == SourceRole::Own && target.is_none())
        || (target.is_none() && context.is_empty() && !unknown)
    {
        return Err(INVALID.into());
    }
    Ok(())
}
pub(super) fn label_only_archive(provider: Provider, resource: &str) -> bool {
    provider == Provider::Instagram && resource.starts_with("import:")
}
pub(super) fn validate_selection(request: &SourceBindingRequest) -> Result<(), String> {
    validate_authors(
        &request.persona_id,
        request.role,
        request.source.author_id.as_deref(),
        &request.allowed_context_author_ids,
        request.allow_unknown_context,
    )?;
    if label_only_archive(request.source.provider, &request.source.resource_id)
        && (request.source.author_id.is_some() || !request.allowed_context_author_ids.is_empty())
    {
        return Err("Экспорт Instagram не подтверждает ID авторов. Используйте явно разрешённый неизвестный контекст.".into());
    }
    Ok(())
}
pub(super) fn validate_authority(
    binding: &SourceBinding,
    grant: &SourceGrant,
    now_ms: i64,
) -> Result<(), String> {
    validate_authors(
        &binding.persona_id,
        binding.role,
        binding.target_author_id.as_deref(),
        &binding.allowed_context_author_ids,
        binding.allow_unknown_context,
    )?;
    if binding.schema_version != 1
        || binding.id != format!("binding:{}", grant.id)
        || binding.grant_id != grant.id
        || binding.policy_version != POLICY_VERSION
        || binding.provider != grant.request.provider
        || binding.account_id.as_deref() != Some(grant.account_id.as_str())
        || binding.resource_id != grant.request.resource_id
        || binding.target_author_id != grant.request.author_id
        || binding.period.from != grant.request.from
        || binding.period.to != grant.request.to
        || !grant.request.allow_model_processing
        || timestamp(&grant.expires_at)? <= now_ms / 1000
    {
        return Err(INVALID.into());
    }
    let from = binding.period.from.as_deref().map(timestamp).transpose()?;
    let to = binding.period.to.as_deref().map(timestamp).transpose()?;
    if from.zip(to).is_some_and(|(from, to)| from > to) {
        return Err(INVALID.into());
    }
    if label_only_archive(binding.provider, &binding.resource_id)
        && (binding.target_author_id.is_some() || !binding.allowed_context_author_ids.is_empty())
    {
        return Err(INVALID.into());
    }
    Ok(())
}
pub(super) fn speaker(
    binding: &SourceBinding,
    author: Option<&str>,
) -> Result<SpeakerRelation, String> {
    // Legacy Instagram archive IDs were display names. Preserve raw metadata but
    // never treat those labels as a verified person identity in a new binding.
    let author = if label_only_archive(binding.provider, &binding.resource_id) {
        None
    } else {
        author
    };
    match author {
        Some(id) if Some(id) == binding.target_author_id.as_deref() => Ok(SpeakerRelation::Target),
        Some(id)
            if binding
                .allowed_context_author_ids
                .iter()
                .any(|value| value == id) =>
        {
            Ok(SpeakerRelation::Context)
        }
        None if binding.allow_unknown_context => Ok(SpeakerRelation::Unknown),
        _ => Err("Автор не включён в разрешённый контекст.".into()),
    }
}
pub(super) fn in_period(binding: &SourceBinding, created: Option<&str>) -> Result<(), String> {
    let Some(created) = created else {
        return if binding.period.include_undated {
            Ok(())
        } else {
            Err("Документ вне выбранного периода.".into())
        };
    };
    let created = timestamp(created)?;
    if binding
        .period
        .from
        .as_deref()
        .map(timestamp)
        .transpose()?
        .is_some_and(|from| created < from)
        || binding
            .period
            .to
            .as_deref()
            .map(timestamp)
            .transpose()?
            .is_some_and(|to| created > to)
    {
        return Err("Документ вне выбранного периода.".into());
    }
    Ok(())
}
pub(super) fn project(
    binding: &SourceBinding,
    grant: &SourceGrant,
    document: &SourceDocument,
    revision_id: &str,
    relations: Vec<DocumentRelation>,
    now_ms: i64,
) -> Result<ModelSourceDocument, String> {
    validate_authority(binding, grant, now_ms)?;
    if document.provider != binding.provider
        || document.resource_id != binding.resource_id
        || document.partition != "reference"
    {
        return Err(INVALID.into());
    }
    in_period(binding, document.created_at.as_deref())?;
    let speaker_relation = speaker(binding, document.author_id.as_deref())?;
    let can_support_own_words = binding.role == SourceRole::Own
        && speaker_relation == SpeakerRelation::Target
        && document.authorship == "owner"
        && !document.is_forwarded
        && !relations
            .iter()
            .any(|item| matches!(item.kind, RelationKind::Forward | RelationKind::Quote));
    Ok(ModelSourceDocument {
        document: document.clone(),
        revision_id: revision_id.into(),
        role: binding.role,
        speaker_relation,
        can_support_own_words,
        relations,
    })
}

#[cfg(test)]
#[path = "source-binding-policy-tests.rs"]
mod tests;
