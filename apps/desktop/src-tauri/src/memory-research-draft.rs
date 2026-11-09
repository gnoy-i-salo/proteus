//! Strict host-orchestration draft boundary. Draft identifiers correlate a batch;
//! native identities, observation time and policy never come from model output.
use super::types::{self, MemoryItemKind, MemoryItemSupport};
use crate::corpus_database::{episodes::ChunkSpan, memory_store::MemoryBundle, RevisionReference};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchPublicationDraft {
    pub schema_version: u8,
    pub extraction_version: String,
    pub items: Vec<MemoryItemDraft>,
    pub analyzed_refs: Vec<RevisionReference>,
    pub context_lease_ids: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryItemDraft {
    pub draft_id: String,
    pub kind: MemoryItemKind,
    pub text: String,
    pub source_spans: Vec<ChunkSpan>,
    pub episode_ids: Vec<String>,
    #[serde(deserialize_with = "types::required_nullable")]
    pub event_at: Option<String>,
    pub support: MemoryItemSupport,
    pub contradictions: Vec<Contradiction>,
    pub limitations: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Contradiction {
    Published {
        #[serde(rename = "itemId")]
        item_id: String,
    },
    Draft {
        #[serde(rename = "draftId")]
        draft_id: String,
    },
}
/// Native-only identity chosen once for a publication attempt and retained for retries.
#[derive(Clone)]
pub struct PublicationIdentity {
    pub memory_snapshot_id: String,
    pub corpus_snapshot_id: String,
    pub item_ids: Vec<String>,
    pub policy_version: String,
    pub published_at: String,
}
const INVALID: &str = "Неверный черновик публикации памяти.";
const MAX_DRAFT_BYTES: usize = 16 * 1024 * 1024;
pub fn decode(json: &str) -> Result<ResearchPublicationDraft, String> {
    if json.len() > MAX_DRAFT_BYTES {
        return Err(INVALID.into());
    }
    let draft: ResearchPublicationDraft = serde_json::from_str(json).map_err(|_| INVALID)?;
    draft.validate()?;
    Ok(draft)
}
impl ResearchPublicationDraft {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.items.len() > 1000 || self.analyzed_refs.len() > 10000 {
            return Err(INVALID.into());
        }
        types::validate_host_id(&self.extraction_version, "версия извлечения")?;
        types::validate_unique_host_ids(&self.context_lease_ids, "модельный контекст", 1000)?;
        if self.context_lease_ids.iter().any(|id| {
            id.len() > 128
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b":_-".contains(&b))
        }) {
            return Err(INVALID.into());
        }
        let mut references = HashSet::new();
        for reference in &self.analyzed_refs {
            types::validate_host_id(&reference.document_id, "документ анализа")?;
            types::validate_host_id(&reference.revision_id, "ревизия анализа")?;
            if !references.insert((&reference.document_id, &reference.revision_id)) {
                return Err(INVALID.into());
            }
        }
        let mut draft_ids = HashSet::new();
        for item in &self.items {
            types::validate_host_id(&item.draft_id, "идентификатор черновика")?;
            if !draft_ids.insert(item.draft_id.as_str()) {
                return Err(INVALID.into());
            }
        }
        for item in &self.items {
            item.validate(&draft_ids)?;
        }
        // Also cap native typed callers, without allocating a second serialized draft.
        serde_json::to_writer(DraftByteLimit(MAX_DRAFT_BYTES), self).map_err(|_| INVALID)?;
        Ok(())
    }
    /// Structural assembly only. Live grants, completed context support, original
    /// spans and existence of published contradictions remain the session/DAO's job.
    pub fn materialize(&self, identity: &PublicationIdentity) -> Result<MemoryBundle, String> {
        self.validate()?;
        if identity.item_ids.len() != self.items.len() {
            return Err(INVALID.into());
        }
        types::validate_host_id(&identity.policy_version, "версия политики")?;
        let snapshot = types::MemorySnapshot {
            schema_version: 1,
            id: identity.memory_snapshot_id.clone(),
            corpus_snapshot_id: identity.corpus_snapshot_id.clone(),
            item_ids: identity.item_ids.clone(),
            extraction_version: self.extraction_version.clone(),
            created_at: identity.published_at.clone(),
        };
        snapshot.validate()?;
        let ids: HashMap<_, _> = self
            .items
            .iter()
            .zip(&identity.item_ids)
            .map(|(draft, id)| (draft.draft_id.as_str(), id))
            .collect();
        let native_ids: HashSet<_> = identity.item_ids.iter().map(String::as_str).collect();
        let mut items = Vec::with_capacity(self.items.len());
        for (draft, id) in self.items.iter().zip(&identity.item_ids) {
            let mut contradiction_ids = Vec::with_capacity(draft.contradictions.len());
            for reference in &draft.contradictions {
                contradiction_ids.push(match reference {
                    Contradiction::Draft { draft_id } => {
                        (*ids.get(draft_id.as_str()).ok_or(INVALID)?).clone()
                    }
                    Contradiction::Published { item_id } => {
                        if native_ids.contains(item_id.as_str()) {
                            return Err(INVALID.into());
                        }
                        item_id.clone()
                    }
                });
            }
            let item = types::MemoryItem {
                schema_version: 1,
                id: id.clone(),
                snapshot_id: identity.corpus_snapshot_id.clone(),
                kind: draft.kind,
                text: draft.text.clone(),
                source_spans: draft.source_spans.clone(),
                episode_ids: draft.episode_ids.clone(),
                event_at: draft.event_at.clone(),
                observed_at: identity.published_at.clone(),
                support: draft.support,
                contradiction_ids,
                limitations: draft.limitations.clone(),
                extraction_version: self.extraction_version.clone(),
                policy_version: identity.policy_version.clone(),
            };
            item.validate()?;
            items.push(item);
        }
        Ok(MemoryBundle { snapshot, items })
    }
}
impl MemoryItemDraft {
    fn validate(&self, draft_ids: &HashSet<&str>) -> Result<(), String> {
        types::validate_text(&self.text, "текст памяти")?;
        types::validate_spans(&self.source_spans, "интервалы памяти")?;
        types::validate_unique_host_ids(&self.episode_ids, "эпизод памяти", 64)?;
        if self.episode_ids.is_empty() || self.contradictions.len() > 64 {
            return Err(INVALID.into());
        }
        if let Some(at) = &self.event_at {
            types::validate_timestamp(at, "время события")?;
        }
        types::validate_limitations(&self.limitations, "ограничение памяти")?;
        let mut seen = HashSet::new();
        for reference in &self.contradictions {
            if !seen.insert(reference) {
                return Err(INVALID.into());
            }
            match reference {
                Contradiction::Published { item_id } => {
                    types::validate_host_id(item_id, "опубликованное противоречие")?
                }
                Contradiction::Draft { draft_id } => {
                    if draft_id == &self.draft_id || !draft_ids.contains(draft_id.as_str()) {
                        return Err(INVALID.into());
                    }
                }
            }
        }
        Ok(())
    }
}
struct DraftByteLimit(usize);
impl std::io::Write for DraftByteLimit {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other(INVALID))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[cfg(test)]
#[path = "memory-research-draft-tests.rs"]
mod tests;
