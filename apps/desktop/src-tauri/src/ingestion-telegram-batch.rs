//! Durable bounded batches for the Telegram worker. Record data is written before
//! its checkpoint; a retry can replace only its uncommitted deterministic batch.
use super::{authorize, remaining_ms, Location, Pending, Session, ADAPTER, INVALID};
use crate::corpus_database::{
    Acquisition, HashAlgorithm, JobBatch, JobCheckpoint, JobPhase, JobStatus, ProvenanceLimitation,
    RecordInput, RevisionInput, SourceProvenance,
};
use crate::ingestion::telegram_document::TelegramDocument;
use sha2::{Digest, Sha256};
pub(super) fn pending(
    document: TelegramDocument,
    mut location: Location,
) -> Result<Pending, String> {
    location.original_id = document.original_id.clone();
    let raw_bytes = document.raw_json.len() as i64;
    let provenance = SourceProvenance {
        acquisition: Acquisition::Archive,
        adapter_version: ADAPTER.into(),
        source_format: "telegramDesktopJson".into(),
        original_object_id: Some(document.original_id.clone()),
        original_byte_length: Some(raw_bytes as u64),
        hash_algorithm: HashAlgorithm::Sha256,
        limitations: vec![
            ProvenanceLimitation::AttachmentsUnavailable,
            ProvenanceLimitation::RelationsIncomplete,
        ],
    };
    let revision = RevisionInput {
        source_key: document.source_key.clone(),
        document_id: document.id.clone(),
        document_json: serde_json::to_string(&document).map_err(|_| INVALID)?,
        provenance,
    };
    Ok(Pending {
        revision,
        location,
        raw_bytes,
    })
}
impl<F: Fn() -> i64> Session<'_, F> {
    fn record_id(&self) -> String {
        format!(
            "import:{:x}:{}",
            Sha256::digest(self.job.id.as_bytes()),
            self.job.state.checkpoint.committed_batch + 1
        )
    }
    fn metadata(&self, extra: Option<&Pending>) -> Result<String, String> {
        let locations = self
            .pending
            .iter()
            .chain(extra)
            .map(|item| &item.location)
            .collect::<Vec<_>>();
        serde_json::to_string(&serde_json::json!({ "schemaVersion":1, "codec":"telegramArchiveBatch:1", "origin":self.cursor.origin, "acquiredAtMs":self.cursor.started_at_ms, "locations":locations })).map_err(|_| INVALID.into())
    }
    pub(super) fn batch_size(&self, extra: Option<&Pending>) -> Result<i64, String> {
        let mut bytes = self.metadata(extra)?.len()
            + self.record_id().len()
            + "telegram".len()
            + "ingestion".len();
        for item in self.pending.iter().chain(extra) {
            bytes += item.revision.document_json.len()
                + item.revision.source_key.len()
                + item.revision.document_id.len()
                + serde_json::to_string(&item.revision.provenance)
                    .map_err(|_| INVALID)?
                    .len();
        }
        i64::try_from(bytes).map_err(|_| INVALID.into())
    }
    pub(super) fn flush(&mut self, complete: bool) -> Result<(), String> {
        remaining_ms(&self.job, &self.cursor, (self.now_ms)())?;
        let count = self.pending.len() as i64;
        let bytes = self.pending.iter().map(|item| item.raw_bytes).sum();
        let mut ids = Vec::new();
        if !self.pending.is_empty() {
            let record = RecordInput {
                id: self.record_id(),
                kind: "import".into(),
                provider: "telegram".into(),
                state_json: self.metadata(None)?,
                documents: std::mem::take(&mut self.pending)
                    .into_iter()
                    .map(|item| item.revision)
                    .collect(),
            };
            self.sources.with_ingestion_database(self.operation, |db| {
                authorize(db, &self.job, (self.now_ms)())?;
                let current = db.job(&self.job.id)?.ok_or(INVALID)?;
                if current.status != JobStatus::Running || current.hash != self.job.hash {
                    return Err("Задача импорта изменилась.".into());
                }
                db.save_ingestion(&record)?;
                Ok(())
            })?;
            ids.push(record.id);
        }
        self.commit(ids, count, bytes, complete)
    }
    pub(super) fn commit(
        &mut self,
        record_ids: Vec<String>,
        loaded_documents: i64,
        loaded_bytes: i64,
        complete: bool,
    ) -> Result<(), String> {
        remaining_ms(&self.job, &self.cursor, (self.now_ms)())?;
        let cursor = serde_json::to_string(&self.cursor).map_err(|_| INVALID)?;
        let batch = JobBatch {
            phase: Some(if complete { JobPhase::Publishing } else { JobPhase::Reading }),
            checkpoint: JobCheckpoint { adapter_version: ADAPTER.into(), cursor: Some(cursor), committed_batch: self.job.state.checkpoint.committed_batch + 1, staging_generation: self.job.state.checkpoint.staging_generation + 1 },
            loaded_documents, loaded_bytes, indexed_documents: 0, analyzed_documents: 0, used_documents: 0,
            total_documents: if complete { Some(self.job.state.coverage.loaded_documents + loaded_documents) } else { None },
            complete, limitations: vec!["Загружены выбранные сообщения архива; индексация и модельное изучение ещё не выполнены. Вложения и отсутствующие сообщения не восстановлены.".into()], record_ids,
        };
        self.job = self.sources.with_ingestion_database(self.operation, |db| {
            authorize(db, &self.job, (self.now_ms)())?;
            db.commit_job_batch(&self.job.id, &self.job.hash, &batch)
        })?;
        Ok(())
    }
}
