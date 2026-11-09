//! Native research publication: the session owns identity, source scope,
//! completed-context evidence and coverage. Drafts cannot grant authority.
use super::{MemoryScope, MemorySession};
use crate::{
    corpus_database::{episodes::ChunkSpan, CorpusDatabase},
    ingestion::grants::GrantUse,
    memory::{
        publication::{
            PublicationCoverage, PublishedItemId, ResearchPublication, ResearchPublicationReceipt,
        },
        research_draft::{PublicationIdentity, ResearchPublicationDraft},
    },
};
use sha2::{Digest, Sha256};
use std::{cell::RefCell, collections::HashSet};
#[derive(Clone)]
pub(super) struct PublicationMemo {
    input_sha256: String,
    id: String,
    identity: PublicationIdentity,
}
const INVALID: &str = "Публикация не соответствует завершённому исследованию.";
impl MemorySession {
    pub fn publish_research(
        &mut self,
        draft: &ResearchPublicationDraft,
        now: impl Fn() -> i64,
    ) -> Result<ResearchPublicationReceipt, String> {
        self.verify_current(&now)?;
        draft.validate()?;
        let hash = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(draft).map_err(|_| INVALID)?)
        );
        let memo = match &self.publication {
            Some(value) if value.input_sha256 == hash => value.clone(),
            Some(_) => return Err("Сессия уже опубликовала другой черновик исследования.".into()),
            None => {
                let time = self.ledger.verify(|| checked_time(&self.scope, &now))?;
                identity(&self.scope, draft, hash, time)?
            }
        };
        let bundle = draft.materialize(&memo.identity)?;
        let records = self.completed_context_receipts(&draft.context_lease_ids, &now)?;
        let support = crate::memory::research_support::verify(&bundle.items, &records, &|| {
            self.scope.guard(&now).map(|_| ())
        })?;
        let references: HashSet<_> = support
            .analyzed_spans
            .iter()
            .map(|s| (&s.document_id, &s.revision_id))
            .collect();
        let claimed: HashSet<_> = draft
            .analyzed_refs
            .iter()
            .map(|r| (&r.document_id, &r.revision_id))
            .collect();
        if references != claimed {
            return Err(INVALID.into());
        }
        let contexts = self.completed_context_audit(&draft.context_lease_ids, &now)?;
        self.verify_current(&now)?;
        // The source gate below freezes job/grant/index state during this short
        // database action. Callbacks check clocks and cancellation without trying
        // to reacquire that gate; both budget ledgers remain authoritative.
        let scope = &self.scope;
        let ledger = RefCell::new(&mut self.ledger);
        let models = RefCell::new(self.models.as_mut().ok_or(INVALID)?);
        let stored = scope
            .sources
            .with_ingestion_database(&scope.operation, |db| {
                let clock = || checked_time(scope, &now);
                let check = || {
                    ledger.borrow_mut().verify(clock)?;
                    models.borrow_mut().verify(&clock)?;
                    Ok(())
                };
                check()?;
                scope.authority.current(db, clock()?, GrantUse::Model)?;
                let coverage = coverage(
                    db,
                    scope,
                    &support.analyzed_spans,
                    &support.used_spans,
                    &check,
                )?;
                let publication = ResearchPublication {
                    schema_version: 1,
                    input_sha256: memo.input_sha256.clone(),
                    receipt: ResearchPublicationReceipt {
                        schema_version: 1,
                        id: memo.id.clone(),
                        snapshot: bundle.snapshot.clone(),
                        item_ids: draft
                            .items
                            .iter()
                            .zip(&bundle.items)
                            .map(|(draft, item)| PublishedItemId {
                                draft_id: draft.draft_id.clone(),
                                item_id: item.id.clone(),
                            })
                            .collect(),
                        coverage,
                        analyzed_spans: support.analyzed_spans,
                        published_at: memo.identity.published_at.clone(),
                    },
                    retrievals: records.iter().map(|r| r.receipt.clone()).collect(),
                    model_contexts: contexts,
                };
                check()?;
                db.publish_research_checked(
                    &scope.persona_id,
                    &publication,
                    &bundle.items,
                    &memo.input_sha256,
                    &check,
                )
            })?;
        // Retain only identity, not another copy of source text/audit. Rejected
        // drafts cannot freeze this slot; a committed result keeps retry identity
        // even if the final permission check prevents returning its receipt.
        self.publication = Some(memo);
        self.verify_current(now)?;
        Ok(stored.publication.receipt)
    }
}

fn checked_time(scope: &MemoryScope, now: &dyn Fn() -> i64) -> Result<i64, String> {
    scope.operation.check()?;
    let time = now();
    scope.operation.check()?;
    if time < 0 || time >= scope.deadline {
        return Err("Разрешение источника истекло.".into());
    }
    Ok(time)
}
fn identity(
    scope: &MemoryScope,
    draft: &ResearchPublicationDraft,
    input_sha256: String,
    now: i64,
) -> Result<PublicationMemo, String> {
    let policy = scope.policy_version().map_err(|_| INVALID)?;
    let published_at = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now)
        .ok_or(INVALID)?
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let native_id = |prefix: &str| -> Result<String, String> {
        Ok(format!(
            "{prefix}:{}",
            crate::connections::storage::random_id(24)?
        ))
    };
    Ok(PublicationMemo {
        input_sha256,
        id: native_id("research")?,
        identity: PublicationIdentity {
            memory_snapshot_id: native_id("memory")?,
            corpus_snapshot_id: scope.source.snapshot_id.clone(),
            item_ids: draft
                .items
                .iter()
                .map(|_| native_id("memoryItem"))
                .collect::<Result<_, _>>()?,
            policy_version: policy.into(),
            published_at,
        },
    })
}
fn documents(spans: &[ChunkSpan]) -> u64 {
    spans
        .iter()
        .map(|s| &s.document_id)
        .collect::<HashSet<_>>()
        .len() as u64
}
fn coverage(
    db: &mut CorpusDatabase,
    scope: &MemoryScope,
    analyzed: &[ChunkSpan],
    used: &[ChunkSpan],
    check: &dyn Fn() -> Result<(), String>,
) -> Result<PublicationCoverage, String> {
    let pin = scope
        .authority
        .pin_index(db, &scope.source.snapshot_id, check)?;
    if pin.receipt != scope.source {
        return Err("Индекс сессии изменился.".into());
    }
    if let Some((expected, _)) = &scope.hybrid {
        if db
            .active_vector_receipt(&scope.source.snapshot_id, check)?
            .as_ref()
            != Some(expected)
        {
            return Err("Векторный индекс сессии изменился.".into());
        }
    }
    let allowed = scope
        .authority
        .scope(&pin, &scope.persona_id, scope.period.clone())?;
    let counts = db.count_reference_documents(&allowed, &scope.source, check)?;
    // Loaded bytes/documents, total and completeness describe acquisition of
    // the selected job, exactly as SnapshotView does. Indexed is the current
    // persona/reference/time scope; analyzed/used are actual completed evidence.
    let acquired = &scope.authority.job.state.coverage;
    let count = |n: i64| u64::try_from(n).map_err(|_| INVALID.to_string());
    Ok(PublicationCoverage {
        loaded_documents: count(acquired.loaded_documents)?,
        indexed_documents: count(counts.document_count)?,
        analyzed_documents: documents(analyzed),
        used_documents: documents(used),
        total_documents: acquired.total_documents.map(count).transpose()?,
        loaded_bytes: count(acquired.loaded_bytes)?,
        complete: acquired.complete,
        limitations: acquired.limitations.clone(),
    })
}
