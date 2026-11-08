//! Saved-page semantics shared by acquisition, episode projection and indexing.
//! Callers authorize the source before asking this codec to inspect original bytes.
use super::{
    page_document::{self, PageDocument},
    preview_store::StoredPreview,
};
use crate::corpus_database::{
    episodes::EpisodeDocument, Acquisition, HashAlgorithm, JobBinding, ProvenanceLimitation,
    SourceProvenance, StoredJob, StoredRevision,
};
use sha2::{Digest, Sha256};
const INVALID: &str = "Сохранённая ревизия страницы не соответствует проверенному исходнику.";
pub(super) fn provenance(preview: &StoredPreview, bytes: u64) -> SourceProvenance {
    SourceProvenance {
        acquisition: Acquisition::SavedPage,
        adapter_version: preview.adapter_version.clone(),
        source_format: preview.source_format.clone(),
        original_object_id: None,
        original_byte_length: Some(bytes),
        hash_algorithm: HashAlgorithm::Sha256,
        limitations: vec![
            ProvenanceLimitation::OriginalObjectIdUnavailable,
            ProvenanceLimitation::AttachmentsUnavailable,
            ProvenanceLimitation::RelationsIncomplete,
        ],
    }
}
pub(super) fn decode_revision(
    preview: &StoredPreview,
    revision: &StoredRevision,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<PageDocument, String> {
    check()?;
    let document = page_document::decode(&revision.document_json, &mut || check())?;
    if Some(&document.origin) != preview.origin.as_ref()
        || document.content_type != super::adapter::content_type(preview)?
        || document.sha256 != preview.fingerprint.sha256
        || document.original.len() as u64 != preview.fingerprint.byte_length
        || document.id != revision.reference.document_id
        || document.source_key != revision.source_key
        || preview.resources.len() != 1
        || preview.resources[0].id != document.source_key
        || revision.provenance != provenance(preview, document.original.len() as u64)
    {
        return Err(INVALID.into());
    }
    check()?;
    Ok(document)
}
pub(super) fn selected(bindings: &[JobBinding], doc: &PageDocument) -> Result<bool, String> {
    for binding in bindings {
        if binding.resource_id != doc.source_key || !binding.allow_unknown_context {
            continue;
        }
        let Some(created) = &doc.published_at else {
            if binding.period.include_undated {
                return Ok(true);
            }
            continue;
        };
        let date = super::period::date_ms(created)?;
        let from = binding
            .period
            .from
            .as_deref()
            .map(super::period::date_ms)
            .transpose()?;
        let to = binding
            .period
            .to
            .as_deref()
            .map(super::period::date_ms)
            .transpose()?;
        if from.is_none_or(|from| date >= from) && to.is_none_or(|to| date <= to) {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(super) fn project(
    job: &StoredJob,
    preview: &StoredPreview,
    originals: Vec<StoredRevision>,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Vec<EpisodeDocument>, String> {
    let mut documents = Vec::new();
    let mut bytes = 0usize;
    for original in originals {
        let doc = decode_revision(preview, &original, check)?;
        let content =
            super::selected_page_content::extract(&doc.original, &doc.content_type, &mut || {
                check()
            })?;
        let relations_complete = !content.has_relation_markers;
        let mut assigned = false;
        for binding in &job.state.selection.bindings {
            check()?;
            if !selected(std::slice::from_ref(binding), &doc)? {
                continue;
            }
            bytes = bytes.checked_add(doc.text.len()).ok_or(INVALID)?;
            if documents.len() >= 100000 || bytes > 128 * 1024 * 1024 {
                return Err("Превышен бюджет подготовки страниц.".into());
            }
            documents.push(EpisodeDocument {
                reference: original.reference.clone(),
                source_key: doc.source_key.clone(),
                binding_id: binding.id.clone(),
                conversation_id: format!(
                    "pageConversation:1:{:x}",
                    Sha256::digest(doc.source_key.as_bytes())
                ),
                thread_id: None,
                timestamp_ms: doc
                    .published_at
                    .as_deref()
                    .map(super::period::date_ms)
                    .transpose()?,
                text: doc.text.clone(),
                related: Vec::new(),
                relations_complete,
                holdout: false,
            });
            assigned = true;
        }
        if !assigned {
            return Err("Ревизия не входит в разрешённую область задачи.".into());
        }
    }
    check()?;
    Ok(documents)
}

#[cfg(test)]
#[path = "ingestion-page-source-tests.rs"]
mod tests;
