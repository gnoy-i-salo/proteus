use super::ingestion_fixture::{archive, prepare};
use super::*;
use crate::corpus_database::JobProvider;
use crate::ingestion::{
    preview_store::read_preview,
    telegram_worker::{completed_origin, run_reader},
};

#[test]
fn completed_projection_uses_frozen_origin_after_acquisition_deadline() {
    let (sources, dir) = local();
    let (inputs, preview_id, origin) = prepare(&sources, &dir, &archive());
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    let job = inputs
        .read_preview(&preview_id, |file, origin| {
            run_reader(&sources, &op, "job", file, origin, || 1001)
        })
        .unwrap();
    let preview = sources
        .with_ingestion_database(&op, |db| read_preview(db, &preview_id))
        .unwrap();
    assert_eq!(
        completed_origin(&job, &preview.fingerprint, 70000).unwrap(),
        origin
    );
    let mut wrong = preview.fingerprint.clone();
    wrong.byte_length += 1;
    assert!(completed_origin(&job, &wrong, 70000).is_err());
    let mut incomplete = job;
    incomplete.state.coverage.complete = false;
    assert!(completed_origin(&incomplete, &preview.fingerprint, 70000).is_err());
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_projection_loads_exact_authorized_originals_and_reply_context() {
    let (sources, dir) = local();
    let raw = archive()
        .replace(
            "\"type\":\"message\"",
            "\"type\":\"message\",\"date_unixtime\":\"1704067200\"",
        )
        .replace("\"id\":2,", "\"id\":2,\"reply_to_message_id\":1,");
    let (inputs, preview_id, _) = prepare(&sources, &dir, &raw);
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    let job = inputs
        .read_preview(&preview_id, |file, origin| {
            run_reader(&sources, &op, "job", file, origin, || 1001)
        })
        .unwrap();
    let result = crate::ingestion::projection::project(&sources, &op, "job", || 70000).unwrap();
    assert_eq!(result.source.id, job.state.published_snapshot_id.unwrap());
    assert_eq!(result.documents.len(), 3);
    assert!(result
        .documents
        .iter()
        .all(|d| d.binding_id == job.state.selection.bindings[0].id));
    assert!(result
        .documents
        .iter()
        .all(|d| d.relations_complete && d.timestamp_ms == Some(1704067200000)));
    assert_eq!(
        result.documents[1].related,
        vec![result.documents[0].reference.clone()]
    );
    assert_eq!(result.documents[2].text, "  контекст\r\n");
    assert!(result
        .documents
        .iter()
        .all(|d| !d.holdout && d.thread_id.is_none()));
    assert_eq!(
        result
            .documents
            .iter()
            .map(|d| d.reference.clone())
            .collect::<Vec<_>>(),
        result.source.documents
    );
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_projection_checks_current_grants_before_a_revoked_snapshot() {
    for expired in [false, true] {
        let (sources, dir) = local();
        let (inputs, preview_id, _) = prepare(&sources, &dir, &archive());
        let op = sources
            .begin_ingestion_operation("job", JobProvider::Telegram)
            .unwrap();
        let job = inputs
            .read_preview(&preview_id, |file, origin| {
                run_reader(&sources, &op, "job", file, origin, || 1001)
            })
            .unwrap();
        let now = if expired {
            1000 + 86_400_000
        } else {
            sources
                .with_ingestion_database(&op, |db| {
                    db.remove(&job.state.selection.bindings[0].grant_id)?;
                    Ok(())
                })
                .unwrap();
            1002
        };
        let error = crate::ingestion::projection::project(&sources, &op, "job", || now)
            .err()
            .unwrap();
        assert!(error.starts_with("Разрешение"), "{error}");
        drop(op);
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn native_projection_observes_expiry_before_reading_snapshot_originals() {
    let (sources, dir) = local();
    let (inputs, preview_id, _) = prepare(&sources, &dir, &archive());
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    let job = inputs
        .read_preview(&preview_id, |file, origin| {
            run_reader(&sources, &op, "job", file, origin, || 1001)
        })
        .unwrap();
    sources
        .with_ingestion_database(&op, |db| {
            let snapshot = db.snapshot(job.state.published_snapshot_id.as_deref().unwrap())?;
            let batch_id = snapshot
                .records
                .iter()
                .find_map(|record| {
                    let stored = db.read(&record.id).unwrap().unwrap();
                    (!stored.references.is_empty()).then_some(record.id.clone())
                })
                .unwrap();
            db.remove(&batch_id)?;
            Ok(())
        })
        .unwrap();
    let calls = std::cell::Cell::new(0);
    let error = crate::ingestion::projection::project(&sources, &op, "job", || {
        calls.set(calls.get() + 1);
        if calls.get() == 1 {
            1002
        } else {
            1000 + 86_400_000
        }
    })
    .err()
    .unwrap();
    assert_eq!(error, "Разрешение источника истекло во время подготовки.");
    assert_eq!(calls.get(), 2);
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn native_projection_cancellation_interrupts_preparation_after_initial_authorization() {
    let (sources, dir) = local();
    let (inputs, preview_id, _) = prepare(&sources, &dir, &archive());
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    inputs
        .read_preview(&preview_id, |file, origin| {
            run_reader(&sources, &op, "job", file, origin, || 1001)
        })
        .unwrap();
    let calls = std::cell::Cell::new(0);
    assert!(
        crate::ingestion::projection::project(&sources, &op, "job", || {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                sources.cancel_ingestion_operation("job").unwrap();
            }
            1002
        })
        .is_err()
    );
    assert_eq!(calls.get(), 2);
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_projection_preserves_long_external_chat_ids_with_bounded_episode_keys() {
    let (sources, dir) = local();
    let raw = archive().replace(
        "\"type\":\"personal_chat\",\"id\":1,",
        &format!("\"type\":\"future_peer\",\"id\":{},", "9".repeat(128)),
    );
    let (inputs, preview_id, _) = prepare(&sources, &dir, &raw);
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    inputs
        .read_preview(&preview_id, |file, origin| {
            run_reader(&sources, &op, "job", file, origin, || 1001)
        })
        .unwrap();
    let projected = crate::ingestion::projection::project(&sources, &op, "job", || 1002).unwrap();
    let policy = crate::corpus_database::episodes::EpisodePolicy {
        time_gap_ms: 1000,
        holdout_basis_points: 0,
        seed: "fixture".into(),
    };
    assert!(
        crate::corpus_database::episodes::prepare("prepared", &projected.documents, &policy)
            .is_ok()
    );
    assert!(projected
        .documents
        .iter()
        .all(|d| d.conversation_id.len() <= 256));
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_projection_assigns_shared_originals_to_each_authorized_persona_once() {
    use crate::corpus_database::{JobInput, JobSelection};
    use crate::ingestion::grants::{create_binding, BindingRequest};
    let (sources, dir) = local();
    let (inputs, preview_id, _) = prepare(&sources, &dir, &archive());
    let op = sources
        .begin_ingestion_operation("shared", JobProvider::Telegram)
        .unwrap();
    let bindings = sources
        .with_ingestion_database(&op, |db| {
            let first = db.job("job")?.unwrap();
            let binding = &first.state.selection.bindings[0];
            let second = create_binding(
                db,
                BindingRequest {
                    preview_id: preview_id.clone(),
                    resource_id: binding.resource_id.clone(),
                    persona_id: "personB".into(),
                    role: binding.role,
                    target_author_id: binding.target_author_id.clone(),
                    allowed_context_author_ids: binding.allowed_context_author_ids.clone(),
                    allow_unknown_context: binding.allow_unknown_context,
                    period: binding.period.clone(),
                    allow_model_processing: false,
                    allow_export: false,
                },
                1000,
            )?;
            let bindings = vec![binding.clone(), second];
            db.create_job(&JobInput {
                id: "shared".into(),
                adapter_version: first.state.checkpoint.adapter_version,
                budget: first.state.budget,
                selection: JobSelection {
                    preview_id: preview_id.clone(),
                    resource_ids: first.state.selection.resource_ids,
                    bindings: bindings.clone(),
                },
            })?;
            Ok(bindings)
        })
        .unwrap();
    inputs
        .read_preview(&preview_id, |file, origin| {
            run_reader(&sources, &op, "shared", file, origin, || 1001)
        })
        .unwrap();
    let projected =
        crate::ingestion::projection::project(&sources, &op, "shared", || 70000).unwrap();
    assert_eq!(projected.source.documents.len(), 3);
    assert_eq!(projected.documents.len(), 6);
    for binding in bindings {
        let refs: Vec<_> = projected
            .documents
            .iter()
            .filter(|d| d.binding_id == binding.id)
            .map(|d| d.reference.clone())
            .collect();
        assert_eq!(refs, projected.source.documents);
    }
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
