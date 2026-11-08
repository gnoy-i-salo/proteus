//! Native reference-only lexical index, derived solely from a prepared snapshot.
//! The source gate is released between bounded pages; no frontend-authored text,
//! persona/grant pairing or source span is accepted by this scenario.
use super::{
    adapter::CompletedAdapter,
    grants::preview_deadline,
    job_workflow::{read_job, validate_selection},
    operations::Operation,
    preview_store::read_preview,
    projection::access,
};
use crate::{
    corpus_database::{
        preparation::{PreparedEntry, PreparedSnapshot},
        search::{
            chunk_content_hash, content_manifest, SearchChunkInput, SearchIndexReceipt,
            SearchStageInput,
        },
        CorpusDatabase, StoredJob,
    },
    sources::Sources,
};
use sha2::{Digest, Sha256};
pub struct IndexInput<'a> {
    pub job_id: &'a str,
    pub snapshot_id: &'a str,
    pub max_duration_ms: i64,
}
struct IndexSource {
    job: StoredJob,
    adapter: CompletedAdapter,
    deadline: i64,
}
impl IndexSource {
    fn current(&self, db: &CorpusDatabase, now: i64) -> Result<(), String> {
        if read_job(db, &self.job.id)? != self.job {
            return Err("Исходная задача изменилась.".into());
        }
        validate_selection(db, &self.job.state.selection, now)
    }
}
pub fn rebuild(
    sources: &Sources,
    operation: &Operation,
    input: &IndexInput<'_>,
    now_ms: impl Fn() -> i64,
) -> Result<SearchIndexReceipt, String> {
    let started = now_ms();
    if started < 0 || !(1..=3_600_000).contains(&input.max_duration_ms) {
        return Err("Неверный бюджет индексирования.".into());
    }
    let end = started
        .checked_add(input.max_duration_ms)
        .ok_or("Неверное время индексирования.")?;
    let budget = || -> Result<(), String> {
        operation.check()?;
        let now = now_ms();
        if now < started || now >= end {
            return Err("Превышено время индексирования.".into());
        }
        Ok(())
    };
    budget()?;
    let (source, mut cursor) = sources.with_ingestion_database(operation, |db| {
        budget()?;
        let job = read_job(db, input.job_id)?;
        let now = now_ms();
        validate_selection(db, &job.state.selection, now)?;
        let preview = read_preview(db, &job.state.selection.preview_id)?;
        let adapter = CompletedAdapter::resolve(&job, &preview, operation.provider(), now)?;
        let deadline = job
            .state
            .selection
            .bindings
            .iter()
            .map(|b| preview_deadline(db, b, &preview.id, now))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .min()
            .ok_or("Нет разрешённых привязок.")?;
        let check = || {
            budget()?;
            access(operation, started, deadline, now_ms())
        };
        let source_id = db.prepared_source_snapshot_id(input.snapshot_id)?;
        if job.state.published_snapshot_id.as_deref() != Some(source_id.as_str()) {
            return Err("Подготовленный снимок не принадлежит задаче.".into());
        }
        let cursor = db.prepared_cursor(input.snapshot_id, &check)?;
        let prepared = cursor.snapshot();
        if job.state.published_snapshot_id.as_deref() != Some(prepared.source_snapshot_id.as_str())
            || prepared.snapshot.corpus_id != format!("corpus:{}", input.job_id)
        {
            return Err("Подготовленный снимок не принадлежит задаче.".into());
        }
        Ok((
            IndexSource {
                job,
                adapter,
                deadline,
            },
            cursor,
        ))
    })?;
    let check = || {
        budget()?;
        access(operation, started, source.deadline, now_ms())
    };
    // Each rebuild owns a fresh stage, so cancellation cannot delete another
    // attempt's checkpoint or an earlier successful publication receipt.
    let stage = SearchStageInput {
        stage_id: format!("nativeFts:{}", crate::connections::storage::random_id(24)?),
        snapshot_id: input.snapshot_id.into(),
        snapshot_generation: cursor.snapshot().snapshot.generation,
        policy_fingerprint: policy(cursor.snapshot())?,
    };
    let result = (|| {
        sources.with_ingestion_database(operation, |db| {
            check()?;
            source.current(db, now_ms())?;
            db.begin_search_stage(&stage)
        })?;
        let mut hashes = Vec::with_capacity(cursor.snapshot().chunk_count);
        while !cursor.is_complete() {
            let page_hashes = sources.with_ingestion_database(operation, |db| {
                check()?;
                source.current(db, now_ms())?;
                let page = db.prepared_page(&mut cursor, 100, 64 * 1024 * 1024, &check)?;
                let mut hashes = Vec::with_capacity(page.len());
                for entry in page {
                    check()?;
                    let chunk = index_chunk(&source, &stage, entry, &check)?;
                    db.stage_search_chunk(&chunk)?;
                    hashes.push((chunk.chunk_id.clone(), chunk_content_hash(&chunk)));
                    check()?;
                }
                Ok(hashes)
            })?;
            hashes.extend(page_hashes);
        }
        sources.with_ingestion_database(operation, |db| {
            check()?;
            source.current(db, now_ms())?;
            db.validate_prepared_cursor(&cursor, &check)?;
            let manifest = content_manifest(&hashes);
            db.commit_search_stage(&stage.stage_id, &manifest, &check)
        })
    })();
    // Cleanup only deletes this attempt's derived stage, even after cancel.
    finish_rebuild(result, || {
        sources.with_ingestion_job_state(|db| db.abort_search_stage(&stage.stage_id))
    })
}
fn finish_rebuild(
    result: Result<SearchIndexReceipt, String>,
    cleanup: impl FnOnce() -> Result<(), String>,
) -> Result<SearchIndexReceipt, String> {
    match result {
        Ok(receipt) => Ok(receipt),
        Err(error) => match cleanup() {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(format!(
                "{error} Не удалось очистить незавершённый индекс: {cleanup_error}"
            )),
        },
    }
}
pub(super) fn policy(snapshot: &PreparedSnapshot) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(
        "nativeFts:1",
        &snapshot.split_version,
        &snapshot.tokenizer_version,
        &snapshot.chunking_version,
    ))
    .map_err(|_| "Не удалось определить версию индекса.")?;
    Ok(format!("nativeFts:1:{:x}", Sha256::digest(bytes)))
}
fn index_chunk(
    source: &IndexSource,
    stage: &SearchStageInput,
    entry: PreparedEntry,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<SearchChunkInput, String> {
    let binding = source
        .job
        .state
        .selection
        .bindings
        .iter()
        .find(|b| b.id == entry.episode.binding_id)
        .ok_or("Фрагмент не принадлежит разрешённой привязке.")?;
    let timestamp = source
        .adapter
        .selected_timestamp(&entry.original, binding, check)?;
    let span = &entry.chunk.spans[0];
    Ok(SearchChunkInput {
        stage_id: stage.stage_id.clone(),
        chunk_id: entry.chunk.id,
        snapshot_id: stage.snapshot_id.clone(),
        snapshot_generation: stage.snapshot_generation,
        partition: "reference".into(),
        text: entry.chunk.text,
        document_id: span.document_id.clone(),
        revision_id: span.revision_id.clone(),
        span_start: i64::try_from(span.start).map_err(|_| "Неверная граница фрагмента.")?,
        span_end: i64::try_from(span.end).map_err(|_| "Неверная граница фрагмента.")?,
        persona_id: binding.persona_id.clone(),
        grant_id: binding.grant_id.clone(),
        binding_id: binding.id.clone(),
        record_id: entry.source_record.id,
        record_generation: entry.source_record.generation,
        period_from: timestamp,
        period_to: timestamp,
        conversation_id: Some(entry.episode.conversation_id.clone()),
        thread_id: entry.episode.thread_id.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_failure_preserves_the_original_index_failure_and_reports_both() {
        let error = finish_rebuild(Err("Разрешение отозвано.".into()), || {
            Err("Хранилище занято.".into())
        })
        .unwrap_err();
        assert!(
            error.contains("Разрешение отозвано."),
            "Lost the primary error: {error}"
        );
        assert!(error.contains("Хранилище занято."));
    }
    #[test]
    fn cleanup_success_keeps_the_original_error_and_successful_publication_skips_cleanup() {
        assert_eq!(
            finish_rebuild(Err("expired".into()), || Ok(())).unwrap_err(),
            "expired"
        );
        let receipt = SearchIndexReceipt {
            snapshot_id: "snapshot".into(),
            snapshot_generation: 1,
            index_generation: 2,
            policy_fingerprint: "policy".into(),
            content_sha256: "hash".into(),
            chunk_count: 0,
            created_at: "1".into(),
        };
        let result = finish_rebuild(Ok(receipt), || {
            panic!("A successful stage must not be removed")
        })
        .unwrap();
        assert_eq!(result.index_generation, 2);
    }
}
