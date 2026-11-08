use super::*;
use std::path::PathBuf;
fn setup() -> (CorpusDatabase, PathBuf) {
    let directory = std::env::temp_dir().join(format!("proteus-jobs-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&directory).unwrap();
    (
        CorpusDatabase::open(&directory.join("corpus.db"), &[31; 32]).unwrap(),
        directory,
    )
}
fn record(id: &str, source: &str, text: &str) -> RecordInput {
    RecordInput{id:id.into(),kind:"grant".into(),provider:"telegram".into(),state_json:"{\"exactState\":\"  é\\r\\n\"}".into(),documents:vec![RevisionInput{source_key:source.into(),document_id:"same-provider-id".into(),provenance:SourceProvenance::source_service(Acquisition::Api),document_json:serde_json::json!({"text":text,"authorLabel":"  Автор é 😀  ","replyTo":"parent","authorship":"unknown"}).to_string()}]}
}
fn binding(id: &str, resource: &str) -> JobBinding {
    JobBinding {
        schema_version: 1,
        id: id.into(),
        persona_id: "persona-é😀".into(),
        provider: JobProvider::Telegram,
        account_id: Some("account-a".into()),
        resource_id: resource.into(),
        grant_id: "grant-1".into(),
        policy_version: "policy-v1".into(),
        role: JobRole::Own,
        target_author_id: Some("author-é 😀".into()),
        allowed_context_author_ids: vec!["ctx-1".into(), "ctx/2".into()],
        allow_unknown_context: false,
        period: JobPeriod {
            from: Some("2020-01-01T00:00:00Z".into()),
            to: None,
            include_undated: false,
        },
    }
}
fn budget() -> JobBudget {
    JobBudget {
        max_input_bytes: 1 << 20,
        max_expanded_bytes: 4 << 20,
        max_documents: 100,
        max_batch_documents: 10,
        max_batch_bytes: 1 << 20,
        max_duration_ms: 60_000,
    }
}
fn job_input(id: &str) -> JobInput {
    JobInput {
        adapter_version: "adapter-v1 é".into(),
        id: id.into(),
        selection: JobSelection {
            preview_id: "preview-1".into(),
            resource_ids: vec!["res/Архив é😀\r\n".into()],
            bindings: vec![binding("binding-1", "res/Архив é😀\r\n")],
        },
        budget: budget(),
    }
}
fn batch(committed_batch: i64, record_ids: &[&str]) -> JobBatch {
    JobBatch {
        phase: None,
        checkpoint: JobCheckpoint {
            adapter_version: "adapter-v1 é".into(),
            cursor: Some(format!("cursor-{committed_batch} é😀\r\n")),
            committed_batch,
            staging_generation: 0,
        },
        loaded_documents: record_ids.len() as i64,
        loaded_bytes: 64,
        indexed_documents: 0,
        analyzed_documents: 0,
        used_documents: 0,
        total_documents: None,
        complete: false,
        limitations: vec![],
        record_ids: record_ids.iter().map(|id| (*id).to_string()).collect(),
    }
}
fn hash_of(db: &CorpusDatabase, id: &str) -> String {
    db.job(id).unwrap().unwrap().hash
}
#[test]
fn job_state_roundtrips_and_survives_restart_with_exact_identifiers() {
    let (mut db, dir) = setup();
    let input = job_input("job-é😀-1");
    let created = db.create_job(&input).unwrap();
    assert_eq!(created.id, input.id);
    assert_eq!(created.status, JobStatus::Queued);
    assert_eq!(created.phase, JobPhase::Reading);
    assert_eq!(created.state.selection, input.selection);
    assert_eq!(created.state.budget, input.budget);
    assert_eq!(created.state.checkpoint.committed_batch, 0);
    assert!(!created.state.coverage.complete);
    assert_eq!(db.jobs().unwrap(), vec![created.clone()]);
    assert_eq!(db.jobs_by_status(JobStatus::Queued).unwrap().len(), 1);
    assert_eq!(db.jobs_by_status(JobStatus::Running).unwrap().len(), 0);
    drop(db);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(db.job(&created.id).unwrap().unwrap(), created);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn wire_dto_uses_camel_case_and_rejects_unknown_fields() {
    let (mut db, dir) = setup();
    let created = db.create_job(&job_input("job-1")).unwrap();
    let value = serde_json::to_value(&created.state).unwrap();
    assert!(value.get("publishedSnapshotId").is_some());
    assert!(value.get("published_snapshot_id").is_none());
    assert!(value.get("selection").unwrap().get("previewId").is_some());
    assert!(value
        .get("checkpoint")
        .unwrap()
        .get("committedBatch")
        .is_some());
    assert!(value
        .get("coverage")
        .unwrap()
        .get("loadedDocuments")
        .is_some());
    assert!(value.get("budget").unwrap().get("maxInputBytes").is_some());
    let mut tampered = value.clone();
    tampered
        .as_object_mut()
        .unwrap()
        .insert("surprise".into(), serde_json::json!(1));
    assert!(serde_json::from_value::<JobState>(tampered).is_err());
    assert!(serde_json::from_str::<JobStatus>("\"frozen\"").is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn create_job_is_idempotent_only_for_identical_input() {
    let (mut db, dir) = setup();
    let input = job_input("job-1");
    let first = db.create_job(&input).unwrap();
    assert_eq!(db.create_job(&input).unwrap(), first);
    let mut changed = input.clone();
    changed.budget.max_documents = 200;
    assert!(db.create_job(&changed).is_err());
    assert_eq!(db.jobs().unwrap().len(), 1);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn lifecycle_rejects_invalid_transitions() {
    let (mut db, dir) = setup();
    db.create_job(&job_input("job-1")).unwrap();
    assert!(db.pause_job("job-1").is_err());
    assert!(db.resume_job("job-1").is_err());
    assert!(db.retry_job("job-1").is_err());
    assert!(db.complete_job("job-1").is_err());
    assert!(db.start_job("job-1").is_ok());
    assert!(db.start_job("job-1").is_err());
    db.cancel_job("job-1").unwrap();
    assert_eq!(
        db.job("job-1").unwrap().unwrap().status,
        JobStatus::Cancelled
    );
    assert!(db.cancel_job("job-1").is_err());
    assert!(db.start_job("job-1").is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn pause_resume_fail_and_retry_follow_the_state_machine() {
    let (mut db, dir) = setup();
    db.create_job(&job_input("job-1")).unwrap();
    assert_eq!(db.start_job("job-1").unwrap().status, JobStatus::Running);
    assert_eq!(db.pause_job("job-1").unwrap().status, JobStatus::Paused);
    assert_eq!(db.resume_job("job-1").unwrap().status, JobStatus::Running);
    let error = JobError {
        code: "rate-limited".into(),
        message: "provider backlog".into(),
        retryable: true,
    };
    let failed = db.fail_job("job-1", &error).unwrap();
    assert_eq!(failed.status, JobStatus::Failed);
    assert_eq!(failed.state.error, Some(error));
    assert!(db.pause_job("job-1").is_err());
    assert!(db.cancel_job("job-1").is_err());
    assert!(db.complete_job("job-1").is_err());
    let queued = db.retry_job("job-1").unwrap();
    assert_eq!(queued.status, JobStatus::Queued);
    assert_eq!(queued.phase, JobPhase::Reading);
    assert_eq!(queued.state.error, None);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn batch_commit_links_records_and_advances_checkpoint_atomically() {
    let (mut db, dir) = setup();
    db.create_job(&job_input("job-1")).unwrap();
    db.start_job("job-1").unwrap();
    db.save(&record("rec-1", "a/1", "one é\r\n😀")).unwrap();
    db.save(&record("rec-2", "a/2", "two")).unwrap();
    let mut progress = batch(1, &["rec-1", "rec-2"]);
    progress.phase = Some(JobPhase::Normalizing);
    progress.indexed_documents = 2;
    progress.total_documents = Some(2);
    let job = db
        .commit_job_batch("job-1", &hash_of(&db, "job-1"), &progress)
        .unwrap();
    assert_eq!(job.phase, JobPhase::Normalizing);
    assert_eq!(job.state.coverage.loaded_documents, 2);
    assert_eq!(job.state.coverage.indexed_documents, 2);
    assert_eq!(job.state.coverage.total_documents, Some(2));
    assert_eq!(job.state.checkpoint.committed_batch, 1);
    assert_eq!(
        db.job_record_ids("job-1").unwrap(),
        vec!["rec-1".to_string(), "rec-2".to_string()]
    );
    drop(db);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(
        db.job("job-1")
            .unwrap()
            .unwrap()
            .state
            .coverage
            .loaded_documents,
        2
    );
    assert_eq!(
        db.job_record_ids("job-1").unwrap(),
        vec!["rec-1".to_string(), "rec-2".to_string()]
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn stale_expected_hash_rejects_commit_and_rolls_back() {
    let (mut db, dir) = setup();
    db.create_job(&job_input("job-1")).unwrap();
    db.start_job("job-1").unwrap();
    db.save(&record("rec-1", "a/1", "one")).unwrap();
    let before = db.job("job-1").unwrap().unwrap();
    let progress = batch(1, &["rec-1"]);
    assert!(db
        .commit_job_batch("job-1", "stale-hash", &progress)
        .is_err());
    let after = db.job("job-1").unwrap().unwrap();
    assert_eq!(after, before, "rejected commit must not change job state");
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn duplicate_record_attachment_is_rejected_even_with_fresh_hash() {
    let (mut db, dir) = setup();
    db.create_job(&job_input("job-1")).unwrap();
    db.start_job("job-1").unwrap();
    db.save(&record("rec-1", "a/1", "one")).unwrap();
    db.commit_job_batch("job-1", &hash_of(&db, "job-1"), &batch(1, &["rec-1"]))
        .unwrap();
    let replay = db.commit_job_batch("job-1", &hash_of(&db, "job-1"), &batch(2, &["rec-1"]));
    assert!(
        replay.is_err(),
        "a record already staged by this job cannot be linked twice"
    );
    let job = db.job("job-1").unwrap().unwrap();
    assert_eq!(job.state.coverage.loaded_documents, 1);
    assert_eq!(job.state.checkpoint.committed_batch, 1);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn per_batch_and_cumulative_budget_limits_are_enforced() {
    let (mut db, dir) = setup();
    let mut input = job_input("job-1");
    input.budget.max_batch_documents = 1;
    input.budget.max_batch_bytes = 1000;
    input.budget.max_documents = 2;
    input.budget.max_expanded_bytes = 1500;
    db.create_job(&input).unwrap();
    db.start_job("job-1").unwrap();
    db.save(&record("rec-1", "a/1", "one")).unwrap();
    db.save(&record("rec-2", "a/2", "two")).unwrap();
    db.save(&record("rec-3", "a/3", "three")).unwrap();
    assert!(db
        .commit_job_batch(
            "job-1",
            &hash_of(&db, "job-1"),
            &batch(1, &["rec-1", "rec-2"])
        )
        .is_err());
    let mut oversized = batch(1, &["rec-1"]);
    oversized.loaded_bytes = 1001;
    assert!(db
        .commit_job_batch("job-1", &hash_of(&db, "job-1"), &oversized)
        .is_err());
    let mut inconsistent = batch(1, &["rec-1"]);
    inconsistent.loaded_documents = 2;
    assert!(db
        .commit_job_batch("job-1", &hash_of(&db, "job-1"), &inconsistent)
        .is_err());
    let mut first = batch(1, &["rec-1"]);
    first.loaded_bytes = 800;
    db.commit_job_batch("job-1", &hash_of(&db, "job-1"), &first)
        .unwrap();
    let mut too_big_total = batch(2, &["rec-2"]);
    too_big_total.loaded_bytes = 800;
    assert!(db
        .commit_job_batch("job-1", &hash_of(&db, "job-1"), &too_big_total)
        .is_err());
    assert!(db
        .commit_job_batch(
            "job-1",
            &hash_of(&db, "job-1"),
            &batch(2, &["rec-2", "rec-3"])
        )
        .is_err());
    let job = db.job("job-1").unwrap().unwrap();
    assert_eq!(job.state.coverage.loaded_documents, 1);
    assert_eq!(job.state.coverage.loaded_bytes, 800);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn coverage_counts_follow_loaded_indexed_analyzed_used_order() {
    let (mut db, dir) = setup();
    db.create_job(&job_input("job-1")).unwrap();
    db.start_job("job-1").unwrap();
    db.save(&record("rec-1", "a/1", "one")).unwrap();
    db.save(&record("rec-2", "a/2", "two")).unwrap();
    let mut analyzed_before_indexed = batch(1, &["rec-1", "rec-2"]);
    analyzed_before_indexed.indexed_documents = 1;
    analyzed_before_indexed.analyzed_documents = 2;
    assert!(db
        .commit_job_batch("job-1", &hash_of(&db, "job-1"), &analyzed_before_indexed)
        .is_err());
    let mut used_before_analyzed = batch(1, &["rec-1", "rec-2"]);
    used_before_analyzed.indexed_documents = 2;
    used_before_analyzed.used_documents = 1;
    assert!(db
        .commit_job_batch("job-1", &hash_of(&db, "job-1"), &used_before_analyzed)
        .is_err());
    let mut valid = batch(1, &["rec-1", "rec-2"]);
    valid.indexed_documents = 2;
    db.commit_job_batch("job-1", &hash_of(&db, "job-1"), &valid)
        .unwrap();
    let mut analyzed = batch(2, &[]);
    analyzed.analyzed_documents = 1;
    assert!(db
        .commit_job_batch("job-1", &hash_of(&db, "job-1"), &analyzed)
        .is_ok());
    let job = db.job("job-1").unwrap().unwrap();
    assert_eq!(job.state.coverage.loaded_documents, 2);
    assert_eq!(job.state.coverage.indexed_documents, 2);
    assert_eq!(job.state.coverage.analyzed_documents, 1);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn only_completed_jobs_publish_and_publication_is_generation_verified() {
    let (mut db, dir) = setup();
    db.create_job(&job_input("job-1")).unwrap();
    db.start_job("job-1").unwrap();
    db.save(&record("rec-1", "a/1", "exact é\r\n😀")).unwrap();
    assert!(db.publish_job("job-1", "corpus", "pub-1").is_err());
    let mut reading = batch(1, &["rec-1"]);
    reading.total_documents = Some(1);
    db.commit_job_batch("job-1", &hash_of(&db, "job-1"), &reading)
        .unwrap();
    assert!(db.complete_job("job-1").is_err());
    let mut publishing = batch(2, &[]);
    publishing.phase = Some(JobPhase::Publishing);
    publishing.complete = true;
    db.commit_job_batch("job-1", &hash_of(&db, "job-1"), &publishing)
        .unwrap();
    assert_eq!(
        db.complete_job("job-1").unwrap().status,
        JobStatus::Completed
    );
    db.create_job(&job_input("job-2")).unwrap();
    db.start_job("job-2").unwrap();
    db.cancel_job("job-2").unwrap();
    assert!(db.publish_job("job-2", "corpus", "pub-2").is_err());
    let snapshot = db.publish_job("job-1", "corpus", "pub-1").unwrap();
    assert_eq!(
        snapshot.records,
        vec![RecordGeneration {
            id: "rec-1".into(),
            generation: 1
        }]
    );
    assert_eq!(
        db.job("job-1")
            .unwrap()
            .unwrap()
            .state
            .published_snapshot_id,
        Some(snapshot.id.clone())
    );
    assert_eq!(
        db.publish_job("job-1", "corpus", "pub-1").unwrap(),
        snapshot
    );
    assert!(db.publish_job("job-1", "corpus", "pub-3").is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn publication_rejects_staged_records_that_changed_generation() {
    let (mut db, dir) = setup();
    db.create_job(&job_input("job-1")).unwrap();
    db.start_job("job-1").unwrap();
    db.save(&record("rec-1", "a/1", "original")).unwrap();
    let mut progress = batch(1, &["rec-1"]);
    progress.phase = Some(JobPhase::Publishing);
    progress.complete = true;
    progress.total_documents = Some(1);
    db.commit_job_batch("job-1", &hash_of(&db, "job-1"), &progress)
        .unwrap();
    db.complete_job("job-1").unwrap();
    db.save(&record("rec-1", "a/1", "edited")).unwrap();
    assert!(db.publish_job("job-1", "corpus", "pub-1").is_err());
    let job = db.job("job-1").unwrap().unwrap();
    assert_eq!(job.state.published_snapshot_id, None);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn job_without_staged_records_cannot_publish_an_empty_snapshot() {
    let (mut db, dir) = setup();
    db.create_job(&job_input("job-1")).unwrap();
    db.start_job("job-1").unwrap();
    let mut progress = batch(1, &[]);
    progress.phase = Some(JobPhase::Publishing);
    progress.total_documents = Some(0);
    progress.complete = true;
    db.commit_job_batch("job-1", &hash_of(&db, "job-1"), &progress)
        .unwrap();
    db.complete_job("job-1").unwrap();
    assert!(db.publish_job("job-1", "corpus", "pub-1").is_err());
    assert_eq!(
        db.job("job-1")
            .unwrap()
            .unwrap()
            .state
            .published_snapshot_id,
        None
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn restart_interrupted_running_job_is_recoverable_without_duplicates() {
    let dir = std::env::temp_dir().join(format!("proteus-jobs-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&dir).unwrap();
    {
        let mut db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
        db.create_job(&job_input("job-1")).unwrap();
        db.start_job("job-1").unwrap();
        db.save(&record("rec-1", "a/1", "one")).unwrap();
        db.commit_job_batch("job-1", &hash_of(&db, "job-1"), &batch(1, &["rec-1"]))
            .unwrap();
    }
    let mut db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(db.recover_interrupted_jobs().unwrap().len(), 1);
    let recovered = db.recover_interrupted_jobs().unwrap();
    assert!(recovered.is_empty(), "recovery is idempotent");
    let job = db.job("job-1").unwrap().unwrap();
    assert_eq!(job.status, JobStatus::Paused);
    let error = job.state.error.clone().unwrap();
    assert_eq!(error.code, "interrupted");
    assert!(error.retryable);
    assert_eq!(job.state.coverage.loaded_documents, 1);
    let running = db.resume_job("job-1").unwrap();
    assert_eq!(running.status, JobStatus::Running);
    assert_eq!(running.state.error, None);
    db.save(&record("rec-2", "a/2", "two")).unwrap();
    let mut next = batch(2, &["rec-2"]);
    next.total_documents = Some(2);
    db.commit_job_batch("job-1", &running.hash, &next).unwrap();
    assert_eq!(
        db.job_record_ids("job-1").unwrap(),
        vec!["rec-1".to_string(), "rec-2".to_string()]
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn failed_job_keeps_checkpoint_and_retry_resumes_from_it() {
    let (mut db, dir) = setup();
    db.create_job(&job_input("job-1")).unwrap();
    db.start_job("job-1").unwrap();
    db.save(&record("rec-1", "a/1", "one")).unwrap();
    db.commit_job_batch("job-1", &hash_of(&db, "job-1"), &batch(1, &["rec-1"]))
        .unwrap();
    let error = JobError {
        code: "timeout".into(),
        message: "batch exceeded".into(),
        retryable: true,
    };
    db.fail_job("job-1", &error).unwrap();
    let queued = db.retry_job("job-1").unwrap();
    assert_eq!(queued.state.checkpoint.committed_batch, 1);
    assert_eq!(queued.state.coverage.loaded_documents, 1);
    db.start_job("job-1").unwrap();
    db.save(&record("rec-2", "a/2", "two")).unwrap();
    db.commit_job_batch("job-1", &hash_of(&db, "job-1"), &batch(2, &["rec-2"]))
        .unwrap();
    assert_eq!(
        db.job("job-1")
            .unwrap()
            .unwrap()
            .state
            .coverage
            .loaded_documents,
        2
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn tampered_job_state_fails_closed_on_read() {
    let (mut db, dir) = setup();
    db.create_job(&job_input("job-1")).unwrap();
    db.start_job("job-1").unwrap();
    db.connection
        .execute("UPDATE jobs SET state_json='{}' WHERE id='job-1'", [])
        .unwrap();
    assert!(db.job("job-1").is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn review_incomplete_coverage_cannot_complete_or_publish() {
    let (mut db, dir) = setup();
    db.create_job(&job_input("review-incomplete")).unwrap();
    db.start_job("review-incomplete").unwrap();
    let mut progress = batch(1, &[]);
    progress.phase = Some(JobPhase::Publishing);
    db.commit_job_batch(
        "review-incomplete",
        &hash_of(&db, "review-incomplete"),
        &progress,
    )
    .unwrap();
    assert!(
        db.complete_job("review-incomplete").is_err(),
        "phase alone must not authorize completion"
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn review_failed_job_acknowledgement_rolls_back_snapshot_and_receipt() {
    let (mut db, dir) = setup();
    db.save(&record("record-1", "source-1", "exact text"))
        .unwrap();
    db.create_job(&job_input("review-atomic")).unwrap();
    db.start_job("review-atomic").unwrap();
    let mut progress = batch(1, &["record-1"]);
    progress.phase = Some(JobPhase::Publishing);
    progress.complete = true;
    progress.total_documents = Some(1);
    db.commit_job_batch("review-atomic", &hash_of(&db, "review-atomic"), &progress)
        .unwrap();
    db.complete_job("review-atomic").unwrap();
    db.connection.execute_batch("CREATE TRIGGER fail_job_publication BEFORE UPDATE ON jobs WHEN json_extract(NEW.state_json,'$.publishedSnapshotId') IS NOT NULL BEGIN SELECT RAISE(ABORT,'injected job acknowledgement failure'); END;").unwrap();
    assert!(db
        .publish_job("review-atomic", "corpus-review", "publication-review")
        .is_err());
    let snapshots: i64 = db
        .connection
        .query_row("SELECT count(*) FROM snapshots", [], |row| row.get(0))
        .unwrap();
    let receipts: i64 = db
        .connection
        .query_row("SELECT count(*) FROM publication_receipts", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        (snapshots, receipts),
        (0, 0),
        "publication and job acknowledgement must be one transaction"
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn review_zero_or_unsafe_budgets_are_rejected_at_native_boundary() {
    let (mut db, dir) = setup();
    let mut input = job_input("review-budget");
    input.budget.max_input_bytes = 0;
    assert!(
        db.create_job(&input).is_err(),
        "zero input budget must fail before queueing"
    );
    input.budget.max_input_bytes = 9_007_199_254_740_992;
    assert!(
        db.create_job(&input).is_err(),
        "native values must roundtrip safely over JavaScript IPC"
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn review_selection_cannot_attach_an_unselected_resource() {
    let (mut db, dir) = setup();
    let mut input = job_input("review-selection");
    input.selection.bindings[0].resource_id = "unselected".into();
    assert!(db.create_job(&input).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn review_staged_records_are_not_document_counts() {
    let (mut db, dir) = setup();
    let mut staged = record("record-1", "source-1", "first");
    let mut second = staged.documents[0].clone();
    second.document_id = "other-document".into();
    second.source_key = "source-2".into();
    staged.documents.push(second);
    db.save(&staged).unwrap();
    db.create_job(&job_input("review-counts")).unwrap();
    db.start_job("review-counts").unwrap();
    let mut progress = batch(1, &["record-1"]);
    progress.loaded_documents = 2;
    let result = db.commit_job_batch("review-counts", &hash_of(&db, "review-counts"), &progress);
    assert!(
        result.is_ok(),
        "one stored record can contain a bounded batch of multiple documents: {result:?}"
    );
    assert_eq!(result.unwrap().state.coverage.loaded_documents, 2);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn review_total_documents_cannot_be_less_than_loaded() {
    let (mut db, dir) = setup();
    db.save(&record("record-1", "source-1", "text")).unwrap();
    db.create_job(&job_input("review-total")).unwrap();
    db.start_job("review-total").unwrap();
    let mut progress = batch(1, &["record-1"]);
    progress.total_documents = Some(0);
    assert!(db
        .commit_job_batch("review-total", &hash_of(&db, "review-total"), &progress)
        .is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn review_version_one_database_upgrades_atomically_and_preserves_exact_records() {
    let dir = std::env::temp_dir().join(format!("proteus-v1-upgrade-{}", rand::random::<u64>()));
    let connection = crypto::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    connection
        .execute_batch(include_str!("fixtures/corpus-v3.sql"))
        .unwrap();
    let mut db = CorpusDatabase { connection };
    db.save(&record("legacy", "legacy/source", "  exact é\r\n😀"))
        .unwrap();
    let before = db.read("legacy").unwrap().unwrap();
    db.connection.execute_batch("DROP TRIGGER immutable_record_namespace; ALTER TABLE records DROP COLUMN namespace; DROP TABLE job_records; CREATE TABLE job_records(job_id TEXT NOT NULL REFERENCES jobs(id),record_id TEXT NOT NULL REFERENCES records(id),position INTEGER NOT NULL CHECK(position>=0),PRIMARY KEY(job_id,position),UNIQUE(job_id,record_id)); PRAGMA user_version=1;").unwrap();
    drop(db);
    let mut db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    let version: i64 = db
        .connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 6);
    assert_eq!(db.read("legacy").unwrap().unwrap(), before);
    db.create_job(&job_input("upgraded")).unwrap();
    db.start_job("upgraded").unwrap();
    db.commit_job_batch(
        "upgraded",
        &hash_of(&db, "upgraded"),
        &batch(1, &["legacy"]),
    )
    .unwrap();
    let saved = db.job("upgraded").unwrap().unwrap();
    drop(db);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(db.job("upgraded").unwrap().unwrap(), saved);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn review_actual_v1_fixture_migrates_but_unsupported_job_data_is_retained_and_rejected() {
    for unsupported_jobs in [false, true] {
        let dir = std::env::temp_dir().join(format!("proteus-v1-jobs-{}", rand::random::<u64>()));
        let connection = crypto::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
        connection
            .execute_batch(include_str!("fixtures/corpus-v1.sql"))
            .unwrap();
        if unsupported_jobs {
            connection.execute("INSERT INTO jobs VALUES('old-job',1,'queued','reading','{}','unrecognized-state')",[]).unwrap();
        }
        drop(connection);
        let result = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]);
        if unsupported_jobs {
            assert!(result.is_err(), "v1 had no job writer; unknown checkpoints cannot be migrated by inventing generations");
            let connection = crypto::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
            let version: i64 = connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap();
            assert_eq!(version, 1);
            let state: String = connection
                .query_row(
                    "SELECT state_json FROM jobs WHERE id='old-job'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(state, "{}");
        } else {
            let db = result.unwrap();
            let version: i64 = db
                .connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap();
            assert_eq!(version, 6);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn bounded_batches_survive_restart_above_old_document_and_byte_limits() {
    let (mut db, dir) = setup();
    let text = format!("  Автор é 👩🏽‍🔬\r\n{}", "x".repeat(2048));
    let mut input = job_input("large-job");
    input.budget.max_input_bytes = 64 * 1024 * 1024;
    input.budget.max_expanded_bytes = 64 * 1024 * 1024;
    input.budget.max_documents = 10400;
    input.budget.max_batch_documents = 80;
    db.create_job(&input).unwrap();
    db.start_job(&input.id).unwrap();
    for index in 0..130 {
        let record_id = format!("batch-{index}");
        let mut staged = record(&record_id, "template", &text);
        let template = staged.documents[0].clone();
        staged.documents = (0..80)
            .map(|offset| {
                let mut document = template.clone();
                document.source_key = format!("selected-resource/{index}/{offset}");
                document.document_id = format!("document-{index}-{offset}");
                document
            })
            .collect();
        db.save(&staged).unwrap();
        let mut progress = batch(index + 1, &[&record_id]);
        progress.loaded_documents = 80;
        progress.loaded_bytes = 80 * text.len() as i64;
        progress.total_documents = Some(10400);
        if index == 129 {
            progress.phase = Some(JobPhase::Publishing);
            progress.complete = true;
        }
        db.commit_job_batch(&input.id, &hash_of(&db, &input.id), &progress)
            .unwrap();
        if index == 64 {
            drop(db);
            db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
            let recovered = db.recover_interrupted_jobs().unwrap();
            assert_eq!(recovered.len(), 1);
            assert_eq!(recovered[0].state.checkpoint.committed_batch, 65);
            db.resume_job(&input.id).unwrap();
        }
    }
    let completed = db.complete_job(&input.id).unwrap();
    assert_eq!(completed.state.coverage.loaded_documents, 10400);
    assert!(completed.state.coverage.loaded_bytes > 16 * 1024 * 1024);
    assert_eq!(
        (
            completed.state.coverage.indexed_documents,
            completed.state.coverage.analyzed_documents,
            completed.state.coverage.used_documents
        ),
        (0, 0, 0)
    );
    let snapshot = db
        .publish_job(&input.id, "large-corpus", "large-publication")
        .unwrap();
    assert_eq!(snapshot.records.len(), 130);
    assert_eq!(snapshot.documents.len(), 10400);
    assert_eq!(
        db.publish_job(&input.id, "large-corpus", "large-publication")
            .unwrap(),
        snapshot
    );
    let revision = db.revision(&snapshot.documents[0]).unwrap();
    let restored: serde_json::Value = serde_json::from_str(&revision.document_json).unwrap();
    assert_eq!(restored["text"], text);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn hardening_byte_cap_uses_actual_utf8_payload_before_materialization() {
    let (mut db, dir) = setup();
    let mut input = job_input("bytes");
    input.budget.max_batch_bytes = 1800;
    db.create_job(&input).unwrap();
    db.start_job("bytes").unwrap();
    db.save(&record("large", "large-source", &"я".repeat(1024)))
        .unwrap();
    let before = hash_of(&db, "bytes");
    assert!(db
        .commit_job_batch("bytes", &before, &batch(1, &["large"]))
        .is_err());
    assert_eq!(hash_of(&db, "bytes"), before);
    assert!(db.job_record_ids("bytes").unwrap().is_empty());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn hardening_cumulative_retained_bytes_cannot_be_underreported() {
    let (mut db, dir) = setup();
    let mut input = job_input("cumulative");
    input.budget.max_batch_bytes = 2000;
    input.budget.max_expanded_bytes = 2500;
    db.create_job(&input).unwrap();
    db.start_job("cumulative").unwrap();
    for id in ["first", "second"] {
        db.save(&record(id, id, &"x".repeat(1000))).unwrap();
    }
    db.commit_job_batch(
        "cumulative",
        &hash_of(&db, "cumulative"),
        &batch(1, &["first"]),
    )
    .unwrap();
    let before = hash_of(&db, "cumulative");
    assert!(db
        .commit_job_batch("cumulative", &before, &batch(2, &["second"]))
        .is_err());
    assert_eq!(hash_of(&db, "cumulative"), before);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn hardening_record_metadata_is_also_subject_to_batch_memory_budget() {
    let (mut db, dir) = setup();
    let mut input = job_input("metadata");
    input.budget.max_batch_bytes = 512;
    db.create_job(&input).unwrap();
    db.start_job("metadata").unwrap();
    let mut stored = record("metadata-record", "metadata-source", "short");
    stored.state_json = serde_json::json!({"metadata":"x".repeat(5000)}).to_string();
    db.save(&stored).unwrap();
    assert!(db
        .commit_job_batch(
            "metadata",
            &hash_of(&db, "metadata"),
            &batch(1, &["metadata-record"])
        )
        .is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn hardening_limitations_remain_bounded_and_do_not_accumulate_duplicates() {
    let (mut db, dir) = setup();
    db.create_job(&job_input("warnings")).unwrap();
    db.start_job("warnings").unwrap();
    let mut progress = batch(1, &[]);
    progress.limitations = (0..128).map(|i| format!("warning-{i}")).collect();
    db.commit_job_batch("warnings", &hash_of(&db, "warnings"), &progress)
        .unwrap();
    progress.checkpoint.committed_batch = 2;
    let saved = db
        .commit_job_batch("warnings", &hash_of(&db, "warnings"), &progress)
        .unwrap();
    assert_eq!(saved.state.coverage.limitations.len(), 128);
    progress.checkpoint.committed_batch = 3;
    progress.limitations = (128..257).map(|i| format!("warning-{i}")).collect();
    assert!(db
        .commit_job_batch("warnings", &saved.hash, &progress)
        .is_err());
    assert_eq!(hash_of(&db, "warnings"), saved.hash);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn hardening_completed_coverage_rejects_zero_increment_checkpoint_changes() {
    let (mut db, dir) = setup();
    db.create_job(&job_input("done")).unwrap();
    db.start_job("done").unwrap();
    let mut progress = batch(1, &[]);
    progress.total_documents = Some(0);
    progress.complete = true;
    progress.phase = Some(JobPhase::Publishing);
    let saved = db
        .commit_job_batch("done", &hash_of(&db, "done"), &progress)
        .unwrap();
    assert!(db
        .commit_job_batch("done", &saved.hash, &batch(2, &[]))
        .is_err());
    assert_eq!(hash_of(&db, "done"), saved.hash);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn hardening_binding_ids_match_the_typescript_boundary_limits() {
    let (mut db, dir) = setup();
    for (index, id) in [
        "я".repeat(65),
        "contains\ncontrol".into(),
        "   ".into(),
        "\u{feff}".into(),
    ]
    .into_iter()
    .enumerate()
    {
        let mut input = job_input(&format!("bad-id-{index}"));
        input.selection.bindings[0].persona_id = id;
        assert!(db.create_job(&input).is_err());
    }
    let mut input = job_input("too-many-context-authors");
    input.selection.bindings[0].allowed_context_author_ids =
        (0..129).map(|i| format!("author-{i}")).collect();
    assert!(db.create_job(&input).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn hardening_retry_preserves_phase_and_requires_retryable_failure() {
    let (mut db, dir) = setup();
    db.create_job(&job_input("retry")).unwrap();
    db.start_job("retry").unwrap();
    let mut progress = batch(1, &[]);
    progress.phase = Some(JobPhase::Indexing);
    let saved = db
        .commit_job_batch("retry", &hash_of(&db, "retry"), &progress)
        .unwrap();
    db.fail_job(
        "retry",
        &JobError {
            code: "temporary".into(),
            message: "exact error".into(),
            retryable: true,
        },
    )
    .unwrap();
    let retried = db.retry_job("retry").unwrap();
    assert_eq!(retried.phase, JobPhase::Indexing);
    assert_eq!(retried.state.checkpoint, saved.state.checkpoint);
    db.start_job("retry").unwrap();
    db.fail_job(
        "retry",
        &JobError {
            code: "invalidSource".into(),
            message: "source cannot be retried".into(),
            retryable: false,
        },
    )
    .unwrap();
    assert!(db.retry_job("retry").is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn hardening_state_size_limit_also_applies_to_updates() {
    let (mut db, dir) = setup();
    let mut job = db.create_job(&job_input("state-limit")).unwrap();
    job.state.coverage.limitations = vec!["x".repeat(32 * 1024 * 1024 + 1)];
    let result = job_store::write_job(&db.connection, &job);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
    assert!(
        result.is_err(),
        "an update must reject oversized state before persisting it"
    );
}

#[test]
fn streamed_job_hash_preserves_the_existing_canonical_bytes() {
    let (mut db, dir) = setup();
    let job = db.create_job(&job_input("hash-é😀")).unwrap();
    let expected =
        types::digest(serde_json::to_vec(&(&job.id, job.status, job.phase, &job.state)).unwrap());
    assert_eq!(job.hash, expected);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[path = "corpus-revocation-tests.rs"]
mod revocation_tests;
