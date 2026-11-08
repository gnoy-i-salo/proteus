//! Read the exact stored archive generation before native episode projection.
//! Authorization belongs to the caller and must precede reading the revision.
//! CorpusDatabase verifies revision hashes; this codec verifies source semantics.
use super::{normalize_with_version, ArchiveVersion, TelegramDocument};
use crate::{
    corpus_database::{Acquisition, ProvenanceLimitation, StoredRevision},
    ingestion::telegram_archive_json::{TelegramChatHeader, TelegramMessageFrame},
};
const INVALID: &str = "Сохранённая ревизия Telegram не соответствует исходному сообщению.";

pub fn decode_revision(
    version: ArchiveVersion,
    origin: &str,
    revision: &StoredRevision,
) -> Result<TelegramDocument, String> {
    // The record store's maximum envelope applies before materializing owned JSON.
    if revision.document_json.len() > 32 * 1024 * 1024 {
        return Err(INVALID.into());
    }
    let document: TelegramDocument =
        serde_json::from_str(&revision.document_json).map_err(|_| INVALID)?;
    let provenance = &revision.provenance;
    provenance.validate()?;
    if document.schema_version != version.document_version()
        || document.id != revision.reference.document_id
        || document.source_key != revision.source_key
        || document.resource_kind.len() > 65536
        || document
            .resource_label
            .as_ref()
            .is_some_and(|label| label.len() > 65536)
        || provenance.acquisition != Acquisition::Archive
        || provenance.adapter_version != version.adapter()
        || provenance.source_format != "telegramDesktopJson"
        || provenance.original_object_id.as_deref() != Some(document.original_id.as_str())
        || provenance.original_byte_length != Some(document.raw_json.len() as u64)
        || !provenance
            .limitations
            .contains(&ProvenanceLimitation::AttachmentsUnavailable)
        || !provenance
            .limitations
            .contains(&ProvenanceLimitation::RelationsIncomplete)
    {
        return Err(INVALID.into());
    }
    // Resource labels/kinds came from the frozen chat header, not message JSON.
    // The identity reconstruction below checks their namespace, without inventing
    // a label or attempting to derive it from the sender's display name.
    let id = document.resource_id.rsplit(':').next().ok_or(INVALID)?;
    if !integer_token(id) {
        return Err(INVALID.into());
    }
    let normalized = normalize_with_version(
        version,
        origin,
        TelegramMessageFrame {
            chat: TelegramChatHeader {
                id: id.into(),
                name: document.resource_label.clone(),
                kind: document.resource_kind.clone(),
            },
            raw_json: document.raw_json.as_bytes().to_vec(),
            byte_start: 0,
            byte_end: document.raw_json.len() as u64,
        },
    )?;
    // Recompute text/authorship/time/reply markers from the exact original.
    // Comparing the envelope also preserves required null fields: serde Option
    // alone accepts their absence, which is not this writer's historical schema.
    if normalized != document
        || serde_json::from_str::<serde_json::Value>(&revision.document_json)
            .map_err(|_| INVALID)?
            != serde_json::to_value(&normalized).map_err(|_| INVALID)?
    {
        return Err(INVALID.into());
    }
    Ok(document)
}

fn integer_token(value: &str) -> bool {
    if value.len() > 128 {
        return false;
    }
    let digits = value.strip_prefix('-').unwrap_or(value);
    digits == "0"
        || (digits
            .as_bytes()
            .first()
            .is_some_and(|n| (b'1'..=b'9').contains(n))
            && digits.bytes().all(|n| n.is_ascii_digit()))
}

#[cfg(test)]
#[path = "telegram-document-read-tests.rs"]
mod tests;
