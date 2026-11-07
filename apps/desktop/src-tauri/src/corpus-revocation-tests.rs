use super::*;
#[test]
fn provider_revocation_removes_only_ingestion_snapshots_and_cannot_restore_old_records() {
    let (mut db, dir) = setup();
    let legacy = db.save(&record("legacy", "old/1", "keep legacy")).unwrap();
    let modern = record("modern", "new/1", "  exact\r\n😀");
    let generation = db.save_ingestion(&modern).unwrap();
    let snapshot = db.publish("corpus", "publication", &[generation]).unwrap();
    let mut other = record("other", "other/1", "keep other");
    other.provider = "github".into();
    let other_generation = db.save_ingestion(&other).unwrap();
    let other_snapshot = db
        .publish("other-corpus", "other-publication", &[other_generation])
        .unwrap();
    db.revoke_ingestion_provider(JobProvider::Telegram).unwrap();
    assert!(db.snapshot(&snapshot.id).is_err());
    assert!(db.save_ingestion(&modern).is_err());
    assert_eq!(db.read("modern").unwrap().unwrap().input.kind, "deleted");
    assert_eq!(
        db.read("legacy").unwrap().unwrap().generation,
        legacy.generation
    );
    assert_eq!(db.read("other").unwrap().unwrap().input, other);
    assert_eq!(db.snapshot(&other_snapshot.id).unwrap(), other_snapshot);
    db.revoke_ingestion_provider(JobProvider::Telegram).unwrap();
    drop(db);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert!(db.snapshot(&snapshot.id).is_err());
    assert_eq!(db.read("modern").unwrap().unwrap().input.kind, "deleted");
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn provider_revocation_cancels_all_active_and_retryable_failed_jobs_across_pages() {
    let (mut db, dir) = setup();
    for i in 0..70 {
        let id = format!("job-{i:03}");
        db.create_job(&job_input(&id)).unwrap();
        match i % 4 {
            0 => {}
            1 => {
                db.start_job(&id).unwrap();
            }
            2 => {
                db.start_job(&id).unwrap();
                db.pause_job(&id).unwrap();
            }
            _ => {
                db.start_job(&id).unwrap();
                db.fail_job(
                    &id,
                    &JobError {
                        code: "transient".into(),
                        message: "Retry".into(),
                        retryable: true,
                    },
                )
                .unwrap();
            }
        }
    }
    let mut other = job_input("other");
    other.selection.bindings[0].provider = JobProvider::Github;
    let other = db.create_job(&other).unwrap();
    db.revoke_ingestion_provider(JobProvider::Telegram).unwrap();
    for i in 0..70 {
        let id = format!("job-{i:03}");
        assert_eq!(db.job(&id).unwrap().unwrap().status, JobStatus::Cancelled);
        assert!(db.resume_job(&id).is_err());
        assert!(db.retry_job(&id).is_err());
        assert!(db.start_job(&id).is_err());
    }
    assert_eq!(db.job("other").unwrap().unwrap(), other);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn failed_revocation_rolls_back_jobs_and_records_together_and_can_be_retried() {
    let (mut db, dir) = setup();
    let before = db.create_job(&job_input("job")).unwrap();
    let input = record("modern", "new/1", "keep until commit");
    db.save_ingestion(&input).unwrap();
    db.connection.execute_batch("CREATE TRIGGER reject_revoke BEFORE UPDATE ON records BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert!(db.revoke_ingestion_provider(JobProvider::Telegram).is_err());
    assert_eq!(db.job("job").unwrap().unwrap(), before);
    assert_eq!(db.read("modern").unwrap().unwrap().input, input);
    db.connection
        .execute_batch("DROP TRIGGER reject_revoke;")
        .unwrap();
    db.revoke_ingestion_provider(JobProvider::Telegram).unwrap();
    assert_eq!(db.job("job").unwrap().unwrap().status, JobStatus::Cancelled);
    assert_eq!(db.read("modern").unwrap().unwrap().input.kind, "deleted");
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn paged_revocation_handles_camel_case_provider_keys_without_touching_other_services() {
    let (mut db, dir) = setup();
    for i in 0..70 {
        let mut input = record(&format!("page-{i:03}"), &format!("page/{i}"), "text");
        input.provider = "savedPage".into();
        db.save_ingestion(&input).unwrap();
    }
    let keep = record("keep", "telegram/1", "keep");
    db.save_ingestion(&keep).unwrap();
    db.revoke_ingestion_provider(JobProvider::SavedPage)
        .unwrap();
    for i in 0..70 {
        assert_eq!(
            db.read(&format!("page-{i:03}"))
                .unwrap()
                .unwrap()
                .input
                .kind,
            "deleted"
        );
    }
    assert_eq!(db.read("keep").unwrap().unwrap().input, keep);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
