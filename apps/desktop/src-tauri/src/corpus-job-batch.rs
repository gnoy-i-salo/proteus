use super::job_budget::validate_payloads;
use super::job_types::*;
use super::job_validation::validate_checkpoint;
use super::record_store::read_record;
use super::types::RecordGeneration;
use super::{sql_error, INVALID};
use rusqlite::{params, Connection};
use std::collections::HashSet;
pub(super) fn apply_batch(
    connection: &Connection,
    job: &mut StoredJob,
    batch: &JobBatch,
) -> Result<(), String> {
    if job.state.coverage.complete {
        return Err("Завершённое покрытие не может изменяться.".into());
    }
    let budget = &job.state.budget;
    // Explicit per-batch backpressure before anything is staged.
    if batch.record_ids.len() as i64 > budget.max_batch_documents {
        return Err("Превышен лимит документов в порции.".into());
    }
    if batch.loaded_bytes > budget.max_batch_bytes {
        return Err("Превышен лимит размера порции.".into());
    }
    if batch.limitations.len() > 256
        || batch
            .limitations
            .iter()
            .any(|limitation| limitation.len() > 2048)
    {
        return Err(INVALID.into());
    }
    for increment in [
        batch.loaded_documents,
        batch.loaded_bytes,
        batch.indexed_documents,
        batch.analyzed_documents,
        batch.used_documents,
    ] {
        if increment < 0 {
            return Err("Счётчики покрытия не могут уменьшаться.".into());
        }
    }
    validate_payloads(connection, job, &batch.record_ids)?;
    let (document_count, records) = batch_records(connection, &job.id, &batch.record_ids)?;
    if batch.loaded_documents != document_count || document_count > budget.max_batch_documents {
        return Err("Число загруженных документов не соответствует порции корпуса.".into());
    }
    let coverage = &mut job.state.coverage;
    // Loaded content is never automatically analyzed: the cumulative counters
    // must keep the loaded ≥ indexed ≥ analyzed ≥ used pipeline order.
    let loaded = coverage
        .loaded_documents
        .checked_add(batch.loaded_documents)
        .ok_or(INVALID)?;
    let loaded_bytes = coverage
        .loaded_bytes
        .checked_add(batch.loaded_bytes)
        .ok_or(INVALID)?;
    let indexed = coverage
        .indexed_documents
        .checked_add(batch.indexed_documents)
        .ok_or(INVALID)?;
    let analyzed = coverage
        .analyzed_documents
        .checked_add(batch.analyzed_documents)
        .ok_or(INVALID)?;
    let used = coverage
        .used_documents
        .checked_add(batch.used_documents)
        .ok_or(INVALID)?;
    if used > analyzed || analyzed > indexed || indexed > loaded {
        return Err("Покрытие нарушает порядок этапов loaded ≥ indexed ≥ analyzed ≥ used.".into());
    }
    if loaded > budget.max_documents {
        return Err("Превышен лимит документов импорта.".into());
    }
    if loaded_bytes > budget.max_expanded_bytes {
        return Err("Превышен лимит размера импорта.".into());
    }
    if let Some(total) = coverage.total_documents {
        if batch.total_documents.is_some_and(|next| next != total) {
            return Err("Итоговое число документов уже зафиксировано.".into());
        }
    } else {
        coverage.total_documents = batch.total_documents;
    }
    if coverage
        .total_documents
        .is_some_and(|total| total < loaded || total > 9_007_199_254_740_991)
    {
        return Err("Неверное общее число документов.".into());
    }
    if batch.complete && coverage.total_documents != Some(loaded) {
        return Err("Неполное покрытие не может быть отмечено завершённым.".into());
    }
    if let Some(phase) = batch.phase {
        if phase.order() < job.phase.order() {
            return Err("Фазы задачи не могут переходить назад.".into());
        }
        job.phase = phase;
    }
    if batch.checkpoint.adapter_version != job.state.checkpoint.adapter_version {
        return Err("Версия адаптера изменилась во время импорта.".into());
    }
    validate_checkpoint(
        &batch.checkpoint,
        job.state.checkpoint.committed_batch,
        job.state.checkpoint.staging_generation,
    )?;
    let attached: i64 = connection
        .query_row(
            "SELECT count(*) FROM job_records WHERE job_id=?1",
            [&job.id],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    for (offset, record) in records.iter().enumerate() {
        let record_id = &record.id;
        let duplicate: i64 = connection
            .query_row(
                "SELECT count(*) FROM job_records WHERE job_id=?1 AND record_id=?2",
                params![job.id, record_id],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if duplicate != 0 {
            return Err("Запись уже прикреплена к этой задаче импорта.".into());
        }
        connection
            .execute(
                "INSERT INTO job_records(job_id,record_id,position,record_generation) VALUES(?1,?2,?3,?4)",
                params![job.id, record_id, attached + offset as i64, record.generation],
            )
            .map_err(sql_error)?;
    }
    let staged: i64 = connection
        .query_row(
            "SELECT count(*) FROM job_records WHERE job_id=?1",
            [&job.id],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    if staged != attached + batch.record_ids.len() as i64 {
        return Err(INVALID.into());
    }
    coverage.loaded_documents = loaded;
    coverage.loaded_bytes = loaded_bytes;
    coverage.indexed_documents = indexed;
    coverage.analyzed_documents = analyzed;
    coverage.used_documents = used;
    coverage.complete = coverage.complete || batch.complete;
    for limitation in &batch.limitations {
        if !coverage.limitations.contains(limitation) {
            coverage.limitations.push(limitation.clone());
        }
    }
    if coverage.limitations.len() > 256 {
        return Err("Превышен лимит предупреждений импорта.".into());
    }
    job.state.checkpoint = batch.checkpoint.clone();
    Ok(())
}
fn batch_records(
    connection: &Connection,
    job_id: &str,
    record_ids: &[String],
) -> Result<(i64, Vec<RecordGeneration>), String> {
    let mut records = Vec::new();
    let mut seen = HashSet::new();
    for id in record_ids {
        let record =
            read_record(connection, id)?.ok_or("Запись задачи отсутствует или отозвана.")?;
        if record.input.kind == "deleted" {
            return Err("Запись задачи отсутствует или отозвана.".into());
        }
        records.push(RecordGeneration {
            id: id.clone(),
            generation: record.generation,
        });
        for reference in record.references {
            if !seen.insert(reference.revision_id.clone()) {
                return Err("Документ повторяется в порции.".into());
            }
        }
    }
    if !seen.is_empty() {
        // Scan retained IDs once per batch, not once per new document. Keep
        // only the already-bounded incoming set in memory; stream old IDs and
        // stop at the first duplicate. No payload or persistent cache is added.
        let mut query = connection.prepare("SELECT record_revisions.revision_id FROM job_records JOIN record_revisions ON record_revisions.record_id=job_records.record_id WHERE job_records.job_id=?1").map_err(sql_error)?;
        let mut retained = query.query([job_id]).map_err(sql_error)?;
        while let Some(row) = retained.next().map_err(sql_error)? {
            let id: String = row.get(0).map_err(sql_error)?;
            if seen.contains(&id) {
                return Err("Документ уже загружен этой задачей.".into());
            }
        }
    }
    Ok((i64::try_from(seen.len()).map_err(|_| INVALID)?, records))
}
