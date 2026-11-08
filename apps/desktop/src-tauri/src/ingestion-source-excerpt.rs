//! Bounded display/model projection of verified originals. The caller owns live
//! permissions and reference-partition checks; this projection grants no access.
use super::adapter::{CompletedAdapter, SelectedDocument};
use crate::corpus_database::{
    episodes::ChunkSpan, JobBinding, JobProvider, JobRole, StoredRevision,
};
use crate::sources::{DocumentRelation, RelationKind, SourceAuthor, SpeakerRelation};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceExcerpt {
    pub document: ExcerptDocument,
    pub span: ChunkSpan,
    pub text: String,
    pub role: JobRole,
    pub speaker_relation: crate::sources::SpeakerRelation,
    pub can_support_own_words: bool,
    pub relations: Vec<crate::sources::DocumentRelation>,
    pub source_metadata: ExcerptMetadata,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcerptDocument {
    pub id: String,
    pub provider: JobProvider,
    pub resource_id: String,
    pub author_id: Option<String>,
    pub author_label: Option<String>,
    pub authorship: &'static str,
    pub kind: &'static str,
    pub mode: &'static str,
    pub source_url: Option<String>,
    pub created_at: Option<String>,
    pub retrieved_at: Option<String>,
    pub content_hash: String,
    pub reply_to: Option<String>,
    pub is_forwarded: bool,
    pub truncated: bool,
    pub partition: &'static str,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcerptMetadata {
    pub schema_version: u8,
    pub source_format: String,
    pub resource_label: Option<String>,
    pub title: Option<String>,
    pub author_labels: Vec<String>,
    pub original_object_id: Option<String>,
    pub limitations: Vec<String>,
}
pub(super) fn project(
    adapter: &CompletedAdapter,
    revision: &StoredRevision,
    binding: &JobBinding,
    span: &ChunkSpan,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<SourceExcerpt, String> {
    let selected = adapter.selected_document(revision, binding, check)?;
    project_selected(adapter, revision, binding, span, &selected, check)
}
fn project_selected(
    adapter: &CompletedAdapter,
    revision: &StoredRevision,
    binding: &JobBinding,
    span: &ChunkSpan,
    selected: &SelectedDocument,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<SourceExcerpt, String> {
    check()?;
    if span.document_id != revision.reference.document_id
        || span.revision_id != revision.reference.revision_id
        || span.start >= span.end
    {
        return Err("Фрагмент не соответствует исходной ревизии.".into());
    }
    let text = selected
        .text()
        .get(span.start..span.end)
        .ok_or("Фрагмент выходит за границы исходного UTF-8 текста.")?
        .to_string();
    let mut metadata = ExcerptMetadata {
        schema_version: 1,
        source_format: revision.provenance.source_format.clone(),
        resource_label: None,
        title: None,
        author_labels: vec![],
        original_object_id: revision.provenance.original_object_id.clone(),
        limitations: revision
            .provenance
            .limitations
            .iter()
            .map(|limitation| {
                use crate::corpus_database::ProvenanceLimitation::*;
                match limitation {
                    OriginalObjectIdUnavailable => "originalObjectIdUnavailable",
                    OriginalBytesUnavailable => "originalBytesUnavailable",
                    AttachmentsUnavailable => "attachmentsUnavailable",
                    RelationsIncomplete => "relationsIncomplete",
                }
                .into()
            })
            .collect(),
    };
    let mut document = ExcerptDocument {
        id: span.document_id.clone(),
        provider: binding.provider,
        resource_id: binding.resource_id.clone(),
        author_id: None,
        author_label: None,
        authorship: "unknown",
        kind: "document",
        mode: "formal",
        source_url: None,
        created_at: None,
        retrieved_at: None,
        content_hash: format!("{:x}", Sha256::digest(selected.text().as_bytes())),
        reply_to: None,
        is_forwarded: false,
        truncated: false,
        partition: "reference",
    };
    let mut relations = Vec::new();
    let mut speaker = SpeakerRelation::Unknown;
    let mut own = false;
    match (adapter, selected) {
        (CompletedAdapter::Telegram { version, origin }, SelectedDocument::Telegram(doc)) => {
            let relation = super::telegram_relations::metadata::parse(doc)?;
            speaker = match doc.author_id.as_deref() {
                Some(id) if Some(id) == binding.target_author_id.as_deref() => {
                    SpeakerRelation::Target
                }
                Some(_) => SpeakerRelation::Context,
                None => SpeakerRelation::Unknown,
            };
            document.author_id = doc.author_id.clone();
            document.author_label = doc.author_label.clone();
            document.authorship = match speaker {
                SpeakerRelation::Target => "owner",
                SpeakerRelation::Context => "other",
                SpeakerRelation::Unknown => "unknown",
            };
            document.kind = if doc.kind == "message" {
                "message"
            } else {
                "document"
            };
            document.mode = "dialogue";
            document.created_at = doc.created_at.clone();
            document.is_forwarded = doc.is_forwarded;
            metadata.resource_label = doc.resource_label.clone();
            metadata.author_labels = doc.author_label.iter().cloned().collect();
            if let Some(reply) = &doc.reply_to_original_id {
                let resource = match relation.peer.as_deref() {
                    None => Some(doc.resource_id.clone()),
                    Some(peer) => {
                        super::telegram_relations::peer_resource(*version, peer).map(|p| p.0)
                    }
                };
                document.reply_to = resource
                    .map(|r| version.identifiers(origin, &r, reply).map(|p| p.1))
                    .transpose()?;
                relations.push(unresolved(RelationKind::Reply));
            }
            if !relation.quotes.is_empty() {
                relations.push(unresolved(RelationKind::Quote));
            }
            if doc.is_forwarded {
                #[derive(serde::Deserialize)]
                struct Forward {
                    forwarded_from: Option<String>,
                    forwarded_from_id: Option<String>,
                }
                let raw: Forward = serde_json::from_str(&doc.raw_json)
                    .map_err(|_| "Повреждённый оригинал пересылки.")?;
                let mut forward = unresolved(RelationKind::Forward);
                forward.original_author = SourceAuthor {
                    provider: Some(crate::connections::Provider::Telegram),
                    id: raw.forwarded_from_id,
                    label: raw.forwarded_from,
                };
                relations.push(forward);
            }
            own = relation.complete
                && binding.role == JobRole::Own
                && speaker == SpeakerRelation::Target
                && relation.quotes.is_empty()
                && !doc.is_forwarded;
        }
        (CompletedAdapter::Page(_), SelectedDocument::Page(doc)) => {
            document.created_at = doc.published_at.clone();
            document.author_label = if doc.author_labels.len() == 1 {
                doc.author_labels.first().cloned()
            } else {
                None
            };
            metadata.title = doc.title.clone();
            metadata.author_labels = doc.author_labels.clone();
            metadata.limitations.extend(doc.limitations.iter().cloned());
            match &doc.origin {
                super::page_document::PageOrigin::SavedPage { label, .. } => {
                    metadata.resource_label = Some(label.clone())
                }
                super::page_document::PageOrigin::SelectedUrl {
                    final_url,
                    retrieved_at,
                    ..
                } => {
                    document.source_url = Some(final_url.clone());
                    document.retrieved_at = Some(retrieved_at.clone());
                }
            }
        }
        _ => return Err("Оригинал принадлежит другому адаптеру.".into()),
    }
    check()?;
    Ok(SourceExcerpt {
        document,
        span: span.clone(),
        text,
        role: binding.role,
        speaker_relation: speaker,
        can_support_own_words: own,
        relations,
        source_metadata: metadata,
    })
}
fn unresolved(kind: RelationKind) -> DocumentRelation {
    // Do not invent a revision/author from a reply ID or matching quote text.
    // The exact source and document.replyTo retain the adapter-owned identity.
    DocumentRelation {
        kind,
        original: None,
        original_author: SourceAuthor {
            provider: None,
            id: None,
            label: None,
        },
        span: None,
    }
}

#[cfg(test)]
#[path = "ingestion-source-excerpt-tests.rs"]
mod tests;
