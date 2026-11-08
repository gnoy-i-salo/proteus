//! Native job authority shared by lexical queries and model-backed indexing.
//! Receipts pin immutable derived data; they never replace live source grants.
use super::{
    adapter::CompletedAdapter,
    grants::{authorize_binding, preview_deadline, GrantUse},
    job_workflow::{read_job, validate_selection},
    operations::Operation,
    preview_store::read_preview,
    search::policy,
};
use crate::corpus_database::{
    search::{SearchIndexReceipt, SearchPeriod, SearchScope},
    CorpusDatabase, JobBinding, StoredJob,
};

pub(crate) struct IndexAuthority {
    pub job: StoredJob,
    pub(super) adapter: CompletedAdapter,
    pub bindings: Vec<JobBinding>,
    pub deadline: i64,
}
pub(crate) struct IndexPin {
    pub receipt: SearchIndexReceipt,
    pub tokenizer_version: String,
}
impl IndexAuthority {
    pub fn resolve(
        db: &CorpusDatabase,
        operation: &Operation,
        job_id: &str,
        persona_id: Option<&str>,
        now: i64,
    ) -> Result<Self, String> {
        let job = read_job(db, job_id)?;
        validate_selection(db, &job.state.selection, now)?;
        let preview = read_preview(db, &job.state.selection.preview_id)?;
        let adapter = CompletedAdapter::resolve(&job, &preview, operation.provider(), now)?;
        let bindings: Vec<_> = job
            .state
            .selection
            .bindings
            .iter()
            .filter(|b| persona_id.is_none_or(|id| b.persona_id == id))
            .cloned()
            .collect();
        if bindings.is_empty() {
            return Err("Персона не входит в разрешённую область задачи.".into());
        }
        let deadline = bindings
            .iter()
            .map(|b| preview_deadline(db, b, &preview.id, now))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .min()
            .ok_or("Нет разрешённых привязок.")?;
        Ok(Self {
            job,
            adapter,
            bindings,
            deadline,
        })
    }
    pub fn pin_index(
        &self,
        db: &CorpusDatabase,
        snapshot_id: &str,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<IndexPin, String> {
        let source_id = db.prepared_source_snapshot_id(snapshot_id)?;
        if self.job.state.published_snapshot_id.as_deref() != Some(source_id.as_str()) {
            return Err("Подготовленный снимок не принадлежит задаче.".into());
        }
        let prepared = db.prepared_snapshot_checked(snapshot_id, check)?;
        if prepared.source_snapshot_id != source_id
            || prepared.snapshot.corpus_id != format!("corpus:{}", self.job.id)
        {
            return Err("Подготовленный снимок не принадлежит задаче.".into());
        }
        let receipt = db
            .active_search_receipt(snapshot_id)?
            .ok_or("Индекс для снимка корпуса не опубликован.")?;
        if receipt.snapshot_generation != prepared.snapshot.generation
            || usize::try_from(receipt.chunk_count).ok() != Some(prepared.chunk_count)
            || receipt.policy_fingerprint != policy(&prepared)?
        {
            return Err("Версия индекса изменилась; начните поиск заново.".into());
        }
        Ok(IndexPin {
            receipt,
            tokenizer_version: prepared.tokenizer_version,
        })
    }
    pub fn current(&self, db: &CorpusDatabase, now: i64, purpose: GrantUse) -> Result<(), String> {
        if read_job(db, &self.job.id)? != self.job {
            return Err("Исходная задача изменилась.".into());
        }
        validate_selection(db, &self.job.state.selection, now)?;
        if !matches!(purpose, GrantUse::Corpus) {
            for binding in &self.bindings {
                authorize_binding(db, binding, purpose, now)?;
            }
        }
        Ok(())
    }
    pub fn scope(
        &self,
        pin: &IndexPin,
        persona: &str,
        period: SearchPeriod,
    ) -> Result<SearchScope, String> {
        let bindings: Vec<_> = self
            .bindings
            .iter()
            .filter(|b| b.persona_id == persona)
            .collect();
        if bindings.is_empty() {
            return Err("Персона не входит в разрешённую область задачи.".into());
        }
        Ok(SearchScope {
            snapshot_id: pin.receipt.snapshot_id.clone(),
            snapshot_generation: pin.receipt.snapshot_generation,
            persona_id: persona.into(),
            grant_ids: bindings.iter().map(|b| b.grant_id.clone()).collect(),
            binding_ids: bindings.iter().map(|b| b.id.clone()).collect(),
            period,
        })
    }
}
