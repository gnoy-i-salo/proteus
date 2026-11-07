use super::ingestion_fixture::{archive, prepare};
use super::*;
use crate::corpus_database::{JobProvider, JobStatus};
use crate::ingestion::{
    job_workflow::{self, Restart},
    restore_workflow::{restore, RestoreRequest},
    SelectedInputs,
};
use std::io::{self, Read};
struct Interrupted<'a> {
    bytes: &'a [u8],
    remaining: usize,
}
impl Read for Interrupted<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::other("interrupted"));
        }
        let n = out.len().min(self.remaining).min(16);
        let count = self.bytes.read(&mut out[..n])?;
        self.remaining -= count;
        Ok(count)
    }
}
fn request() -> RestoreRequest {
    RestoreRequest {
        operation_id: "restore".into(),
        job_id: "job".into(),
        source_handle: "new".into(),
    }
}
#[test]
fn restarted_job_can_reselect_a_moved_exact_archive_and_resume_without_duplicate_documents() {
    let (sources, dir) = local();
    let raw = archive();
    let (inputs, preview, origin) = prepare(&sources, &dir, &raw);
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    assert!(crate::ingestion::telegram_worker::run_reader(
        &sources,
        &op,
        "job",
        Interrupted {
            bytes: raw.as_bytes(),
            remaining: raw.find(r#"{"id":3"#).unwrap()
        },
        &origin,
        || 1001
    )
    .is_err());
    let before = job_workflow::inspect(&sources, "job").unwrap();
    assert_eq!(before.state.coverage.loaded_documents, 2);
    let connections = sources.0.connections.clone();
    drop(op);
    drop(inputs);
    drop(sources);
    let moved = dir.join("moved.json");
    std::fs::rename(dir.join("archive.json"), &moved).unwrap();
    let sources = Sources::new(dir.clone(), connections);
    let inputs = SelectedInputs::default();
    inputs.insert("new".into(), &moved).unwrap();
    let op = sources
        .begin_ingestion_operation("restore", JobProvider::Telegram)
        .unwrap();
    let restored = restore(&sources, &inputs, &op, request(), || 1002).unwrap();
    assert_eq!(restored.status, JobStatus::Paused);
    assert_eq!(restored.state.checkpoint, before.state.checkpoint);
    assert_eq!(
        inputs
            .read_preview(&preview, |_, key| Ok(key.to_string()))
            .unwrap(),
        origin
    );
    let (_, worker) =
        job_workflow::restart(&sources, &inputs, "job", Restart::Resume, 1003).unwrap();
    let done = job_workflow::execute(&sources, &inputs, &worker, || 1004).unwrap();
    assert_eq!(done.status, JobStatus::Completed);
    assert_eq!(done.state.coverage.loaded_documents, 3);
    sources
        .with_ingestion_database(&op, |db| {
            assert_eq!(
                db.snapshot(done.state.published_snapshot_id.as_deref().unwrap())?
                    .documents
                    .len(),
                3
            );
            Ok(())
        })
        .unwrap();
    drop(worker);
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn queued_restore_preserves_job_state_and_failed_reverification_preserves_existing_receipt() {
    let (sources, dir) = local();
    let (old, preview, _) = prepare(&sources, &dir, &archive());
    drop(old);
    let inputs = SelectedInputs::default();
    inputs
        .insert("new".into(), &dir.join("archive.json"))
        .unwrap();
    let op = sources
        .begin_ingestion_operation("restore", JobProvider::Telegram)
        .unwrap();
    let before = job_workflow::inspect(&sources, "job").unwrap();
    assert_eq!(
        restore(&sources, &inputs, &op, request(), || 1001).unwrap(),
        before
    );
    std::fs::write(dir.join("archive.json"), b"changed").unwrap();
    assert!(restore(&sources, &inputs, &op, request(), || 1002).is_err());
    assert!(inputs.read_preview(&preview, |_, _| Ok(())).is_ok());
    assert_eq!(job_workflow::inspect(&sources, "job").unwrap(), before);
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn restore_rejects_missing_capabilities_changed_bytes_revoked_authority_and_terminal_jobs() {
    for case in 0..8 {
        let (sources, dir) = local();
        let (old, preview, _) = prepare(&sources, &dir, &archive());
        drop(old);
        let inputs = SelectedInputs::default();
        inputs
            .insert("new".into(), &dir.join("archive.json"))
            .unwrap();
        let op = sources
            .begin_ingestion_operation("restore", JobProvider::Telegram)
            .unwrap();
        let mut req = request();
        let mut now = 1001;
        match case {
            0 => req.source_handle = dir.join("archive.json").to_string_lossy().into_owned(),
            1 => req.operation_id = "forged".into(),
            2 => std::fs::write(dir.join("archive.json"), archive().replace("ok", "no")).unwrap(),
            3 => sources
                .with_ingestion_database(&op, |db| db.remove(&preview))
                .unwrap(),
            4 => {
                job_workflow::control(&sources, "job", job_workflow::Control::Cancel).unwrap();
            }
            5 => {
                sources
                    .with_ingestion_database(&op, |db| db.start_job("job"))
                    .unwrap();
            }
            6 => now = 1000 + 86_400_001,
            _ => sources.cancel_ingestion_operation("restore").unwrap(),
        }
        let before = job_workflow::inspect(&sources, "job").unwrap();
        assert!(
            restore(&sources, &inputs, &op, req, || now).is_err(),
            "case {case}"
        );
        assert!(inputs.read_preview(&preview, |_, _| Ok(())).is_err());
        assert_eq!(job_workflow::inspect(&sources, "job").unwrap(), before);
        drop(op);
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn restore_wire_rejects_client_supplied_paths_origins_and_authority_fields() {
    let valid = serde_json::json!({"operationId":"restore", "jobId":"job", "sourceHandle":"new"});
    assert!(serde_json::from_value::<RestoreRequest>(valid.clone()).is_ok());
    for key in ["path", "origin", "grantId", "budget", "source_handle"] {
        let mut forged = valid.clone();
        forged[key] = serde_json::json!("forged");
        assert!(serde_json::from_value::<RestoreRequest>(forged).is_err());
    }
}
