//! Prepared publication over native-authorized originals. This is not an IPC
//! accepting frontend-authored chunks or assignments. Tokenization holds no DB gate.
use super::{
    grants::preview_deadline,
    job_workflow::{read_job, validate_selection},
    operations::Operation,
    projection::{access, project_checked},
    publication::SnapshotView,
};
use crate::{
    corpus_database::{
        episodes::{chunk_policy_fingerprint, ChunkPolicy, EpisodePolicy, EpisodeTokenizer},
        preparation::PreparationRequest,
        Snapshot,
    },
    sources::Sources,
};
use sha2::{Digest, Sha256};
pub struct PrepareInput<'a> {
    pub job_id: &'a str,
    pub split: &'a EpisodePolicy,
    pub chunks: &'a ChunkPolicy,
    pub tokenizer: &'a dyn EpisodeTokenizer,
    /// Independent CPU preparation budget, not the acquisition deadline.
    pub max_duration_ms: i64,
}
pub fn prepare(
    sources: &Sources,
    operation: &Operation,
    input: &PrepareInput<'_>,
    now_ms: impl Fn() -> i64,
) -> Result<SnapshotView, String> {
    let started = now_ms();
    if !(1..=3_600_000).contains(&input.max_duration_ms) || started < 0 {
        return Err("Неверный бюджет времени подготовки.".into());
    }
    let end = started
        .checked_add(input.max_duration_ms)
        .ok_or("Неверное время подготовки.")?;
    let tokenizer_version = input.tokenizer.version().to_owned();
    let check_budget = || {
        operation.check()?;
        if input.tokenizer.version() != tokenizer_version {
            return Err("Версия токенизатора изменилась во время подготовки.".into());
        }
        let now = now_ms();
        if now < started || now >= end {
            return Err("Превышено время подготовки.".into());
        }
        Ok(())
    };
    check_budget()?;
    let projection = project_checked(sources, operation, input.job_id, &now_ms, &check_budget)?;
    let publication = publication_id(&projection.source, input)?;
    let request = PreparationRequest {
        source_snapshot_id: &projection.source.id,
        publication_id: &publication,
        documents: &projection.documents,
        split: input.split,
        tokenizer: input.tokenizer,
        chunks: input.chunks,
    };
    let (job, deadline, verified) = sources.with_ingestion_database(operation, |db| {
        check_budget()?;
        let job = read_job(db, input.job_id)?;
        let now = now_ms();
        validate_selection(db, &job.state.selection, now)?;
        if job.state.published_snapshot_id.as_deref() != Some(projection.source.id.as_str()) {
            return Err("Исходный снимок задачи изменился.".into());
        }
        let deadline = job
            .state
            .selection
            .bindings
            .iter()
            .map(|binding| preview_deadline(db, binding, &job.state.selection.preview_id, now))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .min()
            .ok_or("Нет разрешённых привязок.")?;
        let check = || {
            check_budget()?;
            access(operation, started, deadline, now_ms())
        };
        let verified = db.verify_preparation_checked(&request, &check)?;
        Ok((job, deadline, verified))
    })?;
    let check = || {
        check_budget()?;
        access(operation, started, deadline, now_ms())
    };
    // The verified value borrows immutable input documents, never the connection.
    let compiled = verified.compile_checked(input.tokenizer, input.chunks, &check)?;
    let prepared = sources.with_ingestion_database(operation, |db| {
        check()?;
        let current = read_job(db, input.job_id)?;
        if current != job {
            return Err("Исходная задача изменилась.".into());
        }
        validate_selection(db, &current.state.selection, now_ms())?;
        db.commit_prepared_checked(&compiled, &check)
    })?;
    Ok(SnapshotView {
        schema_version: 1,
        id: prepared.snapshot.id,
        corpus_id: prepared.snapshot.corpus_id,
        parent_snapshot_id: prepared.snapshot.parent_id,
        created_at: prepared.snapshot.created_at,
        binding_ids: job
            .state
            .selection
            .bindings
            .into_iter()
            .map(|b| b.id)
            .collect(),
        document_refs: prepared.snapshot.documents,
        episode_ids: prepared.episode_ids,
        split_version: prepared.split_version,
        generation: prepared.snapshot.generation,
        coverage: job.state.coverage,
    })
}
fn publication_id(source: &Snapshot, input: &PrepareInput<'_>) -> Result<String, String> {
    // Bump this scenario version if projection semantics change. Time budget
    // affects whether work completes, never the identity of the completed result.
    let bytes = serde_json::to_vec(&(
        "telegramPreparation:1",
        &source.id,
        input.split.time_gap_ms,
        input.split.holdout_basis_points,
        &input.split.seed,
        input.tokenizer.version(),
        chunk_policy_fingerprint(input.tokenizer, input.chunks)?,
    ))
    .map_err(|_| "Не удалось определить версию подготовки.")?;
    Ok(format!("telegramPreparation:1:{:x}", Sha256::digest(bytes)))
}
