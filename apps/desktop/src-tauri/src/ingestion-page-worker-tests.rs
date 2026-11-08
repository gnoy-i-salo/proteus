use super::*;
use crate::corpus_database::{JobBudget, JobPeriod, JobProvider, JobRole, JobSelection, JobStatus};
use crate::ingestion::{
    grants::{create_binding, BindingRequest},
    job_workflow::{self, StartRequest},
    page_document,
    page_preview::{preview_saved, PagePreviewRequest},
    SelectedInputs,
};

pub(super) fn prepare_page(
    sources: &Sources,
    dir: &std::path::Path,
    original: &str,
) -> (SelectedInputs, JobSelection, JobBudget) {
    prepare_page_with_policy(sources, dir, original, |request| request)
}
fn prepare_page_with_policy(
    sources: &Sources,
    dir: &std::path::Path,
    original: &str,
    policy: impl FnOnce(BindingRequest) -> BindingRequest,
) -> (SelectedInputs, JobSelection, JobBudget) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("  Страница 🦉.html");
    std::fs::write(&path, original).unwrap();
    let inputs = SelectedInputs::default();
    inputs.insert("selected-page".into(), &path).unwrap();
    let budget = JobBudget {
        max_input_bytes: 1 << 20,
        max_expanded_bytes: 1 << 20,
        max_documents: 10,
        max_batch_documents: 1,
        max_batch_bytes: 1 << 20,
        max_duration_ms: 60000,
    };
    let operation = sources
        .begin_ingestion_operation("preview-page", JobProvider::SavedPage)
        .unwrap();
    let preview = preview_saved(
        sources,
        &inputs,
        &operation,
        PagePreviewRequest {
            operation_id: "preview-page".into(),
            source_handle: "selected-page".into(),
            content_type: "text/html".into(),
            budget: budget.clone(),
        },
    )
    .unwrap();
    let binding = sources
        .with_ingestion_database(&operation, |db| {
            create_binding(
                db,
                policy(BindingRequest {
                    preview_id: preview.id.clone(),
                    resource_id: preview.resources[0].id.clone(),
                    persona_id: "personA".into(),
                    role: JobRole::Reference,
                    target_author_id: None,
                    allowed_context_author_ids: vec![],
                    allow_unknown_context: true,
                    period: JobPeriod {
                        from: None,
                        to: None,
                        include_undated: true,
                    },
                    allow_model_processing: false,
                    allow_export: false,
                }),
                1000,
            )
        })
        .unwrap();
    let selection = JobSelection {
        preview_id: preview.id,
        resource_ids: vec![binding.resource_id.clone()],
        bindings: vec![binding],
    };
    (inputs, selection, budget)
}

#[test]
fn saved_page_job_publishes_exact_original_and_separate_projection_without_model_usage() {
    let (sources, dir) = local();
    let raw = "<title> Точное название 🦉</title><meta name='author' content='Непроверенное имя'><p>  Слова\r\n&amp; смысл</p>";
    let (inputs, selection, budget) = prepare_page(&sources, &dir, raw);
    let source_key = selection.resource_ids[0].clone();
    let operation = sources
        .begin_ingestion_operation("page-job", JobProvider::SavedPage)
        .unwrap();
    job_workflow::create(
        &sources,
        &inputs,
        &operation,
        StartRequest {
            operation_id: "page-job".into(),
            selection,
            budget,
        },
        1001,
    )
    .unwrap();
    let job = job_workflow::execute(&sources, &inputs, &operation, || 1002).unwrap();
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(job.state.coverage.loaded_documents, 1);
    assert_eq!(job.state.coverage.indexed_documents, 0);
    assert_eq!(job.state.coverage.analyzed_documents, 0);
    assert_eq!(job.state.coverage.used_documents, 0);
    assert!(job.state.coverage.complete);
    sources
        .with_ingestion_database(&operation, |db| {
            let snapshot = db.snapshot(job.state.published_snapshot_id.as_deref().unwrap())?;
            assert_eq!(snapshot.documents.len(), 1);
            let revision = db.revision(&snapshot.documents[0])?;
            let document = page_document::decode(&revision.document_json, &mut || Ok(()))?;
            assert_eq!(document.original, raw);
            assert_eq!(document.source_key, source_key);
            assert!(document.text.contains("& смысл"));
            assert_eq!(document.author_labels, ["Непроверенное имя"]);
            assert!(document.published_at.is_none());
            assert_eq!(
                revision.provenance.original_byte_length,
                Some(raw.len() as u64)
            );
            assert!(revision.provenance.original_object_id.is_none());
            Ok(())
        })
        .unwrap();
    drop(operation);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn saved_page_changed_after_preview_never_publishes_a_partial_snapshot() {
    let (sources, dir) = local();
    let (inputs, selection, budget) = prepare_page(&sources, &dir, "<p>Original</p>");
    let operation = sources
        .begin_ingestion_operation("page-job", JobProvider::SavedPage)
        .unwrap();
    job_workflow::create(
        &sources,
        &inputs,
        &operation,
        StartRequest {
            operation_id: "page-job".into(),
            selection,
            budget,
        },
        1001,
    )
    .unwrap();
    std::fs::write(dir.join("  Страница 🦉.html"), "<p>Changed!</p>").unwrap();
    let job = job_workflow::execute(&sources, &inputs, &operation, || 1002).unwrap();
    assert_eq!(job_workflow::inspect(&sources, "page-job").unwrap(), job);
    assert_eq!(job.status, JobStatus::Failed);
    assert!(!job.state.error.as_ref().unwrap().retryable);
    assert!(job.state.published_snapshot_id.is_none());
    assert_eq!(job.state.coverage.loaded_documents, 0);
    drop(operation);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn saved_page_restore_keeps_original_identity_and_label_after_file_is_moved() {
    use crate::ingestion::{
        job_workflow::{Control, Restart},
        restore_workflow::{restore, RestoreRequest},
    };
    let (sources, dir) = local();
    let (inputs, selection, budget) = prepare_page(&sources, &dir, "<p>Точный текст</p>");
    let preview_id = selection.preview_id.clone();
    let origin = inputs
        .read_preview(&preview_id, |_, origin| Ok(origin.to_string()))
        .unwrap();
    let operation = sources
        .begin_ingestion_operation("page-job", JobProvider::SavedPage)
        .unwrap();
    job_workflow::create(
        &sources,
        &inputs,
        &operation,
        StartRequest {
            operation_id: "page-job".into(),
            selection,
            budget,
        },
        1001,
    )
    .unwrap();
    job_workflow::control(&sources, "page-job", Control::Pause).unwrap();
    let connections = sources.0.connections.clone();
    drop(operation);
    drop(inputs);
    drop(sources);
    let moved = dir.join("Другое имя.html");
    std::fs::rename(dir.join("  Страница 🦉.html"), &moved).unwrap();
    let sources = Sources::new(dir.clone(), connections);
    let inputs = SelectedInputs::default();
    inputs.insert("new".into(), &moved).unwrap();
    let operation = sources
        .begin_ingestion_operation("restore", JobProvider::SavedPage)
        .unwrap();
    restore(
        &sources,
        &inputs,
        &operation,
        RestoreRequest {
            operation_id: "restore".into(),
            job_id: "page-job".into(),
            source_handle: "new".into(),
        },
        || 1002,
    )
    .unwrap();
    assert_eq!(
        inputs
            .read_preview(&preview_id, |_, origin| Ok(origin.to_string()))
            .unwrap(),
        origin
    );
    let (_, worker) =
        job_workflow::restart(&sources, &inputs, "page-job", Restart::Resume, 1003).unwrap();
    assert_eq!(worker.provider(), JobProvider::SavedPage);
    let done = job_workflow::execute(&sources, &inputs, &worker, || 1004).unwrap();
    assert_eq!(done.status, JobStatus::Completed);
    sources
        .with_ingestion_database(&operation, |db| {
            let snapshot = db.snapshot(done.state.published_snapshot_id.as_deref().unwrap())?;
            let document = page_document::decode(
                &db.revision(&snapshot.documents[0])?.document_json,
                &mut || Ok(()),
            )?;
            assert_eq!(
                document.origin,
                page_document::PageOrigin::SavedPage {
                    source_origin: origin,
                    label: "  Страница 🦉.html".into()
                }
            );
            Ok(())
        })
        .unwrap();
    drop(worker);
    drop(operation);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

pub(super) fn start_page(
    sources: &Sources,
    inputs: &SelectedInputs,
    selection: JobSelection,
    budget: JobBudget,
) -> crate::ingestion::operations::Operation {
    let operation = sources
        .begin_ingestion_operation("page-job", JobProvider::SavedPage)
        .unwrap();
    job_workflow::create(
        sources,
        inputs,
        &operation,
        StartRequest {
            operation_id: "page-job".into(),
            selection,
            budget,
        },
        1001,
    )
    .unwrap();
    operation
}
#[test]
fn saved_page_rejects_wrong_provider_and_never_creates_the_job() {
    let (sources, dir) = local();
    let (inputs, selection, budget) = prepare_page(&sources, &dir, "<p>Original</p>");
    let operation = sources
        .begin_ingestion_operation("page-job", JobProvider::Telegram)
        .unwrap();
    assert!(job_workflow::create(
        &sources,
        &inputs,
        &operation,
        StartRequest {
            operation_id: "page-job".into(),
            selection,
            budget
        },
        1001
    )
    .is_err());
    assert!(job_workflow::inspect(&sources, "page-job").is_err());
    drop(operation);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn saved_page_cancel_or_revoked_grant_cannot_publish() {
    for cancel in [true, false] {
        let (sources, dir) = local();
        let (inputs, selection, budget) = prepare_page(&sources, &dir, "<p>Original</p>");
        let grant = selection.bindings[0].grant_id.clone();
        let operation = start_page(&sources, &inputs, selection, budget);
        if cancel {
            job_workflow::control(&sources, "page-job", job_workflow::Control::Cancel).unwrap();
        } else {
            sources
                .with_ingestion_database(&operation, |db| db.revoke(&grant, "savedPage"))
                .unwrap();
        }
        let job = job_workflow::execute(&sources, &inputs, &operation, || 1002).unwrap();
        assert_eq!(
            job.status,
            if cancel {
                JobStatus::Cancelled
            } else {
                JobStatus::Failed
            }
        );
        assert!(job.state.published_snapshot_id.is_none());
        assert_eq!(job.state.coverage.loaded_documents, 0);
        drop(operation);
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn saved_page_period_and_unknown_context_filter_do_not_promote_author_labels() {
    for case in 0..5 {
        let (sources, dir) = local();
        let raw = if case == 0 {
            "<p>Undated</p>"
        } else {
            "<meta property='article:published_time' content='2026-11-01T12:00:00Z'><meta name='author' content='personA'><p>Original</p>"
        };
        let (inputs, selection, budget) =
            prepare_page_with_policy(&sources, &dir, raw, |mut request| {
                request.period.include_undated = false;
                request.period.from = Some(
                    if case == 1 {
                        "2026-11-02T00:00:00Z"
                    } else {
                        "2026-10-01T00:00:00Z"
                    }
                    .into(),
                );
                request.period.to = Some(
                    if case == 2 {
                        "2026-10-31T00:00:00Z"
                    } else {
                        "2026-11-02T00:00:00Z"
                    }
                    .into(),
                );
                if case == 3 {
                    request.allow_unknown_context = false;
                }
                request
            });
        let operation = start_page(&sources, &inputs, selection, budget);
        let job = job_workflow::execute(&sources, &inputs, &operation, || 1002).unwrap();
        assert_eq!(
            job.status,
            JobStatus::Completed,
            "{case}: {:?}",
            job.state.error
        );
        assert_eq!(
            job.state.coverage.loaded_documents,
            i64::from(case == 4),
            "case {case}"
        );
        drop(operation);
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn saved_page_completed_job_is_idempotent_without_rereading_changed_bytes() {
    let (sources, dir) = local();
    let (inputs, selection, budget) = prepare_page(&sources, &dir, "<p>Original</p>");
    let operation = start_page(&sources, &inputs, selection, budget);
    let first = job_workflow::execute(&sources, &inputs, &operation, || 1002).unwrap();
    assert_eq!(first.status, JobStatus::Completed);
    std::fs::write(dir.join("  Страница 🦉.html"), "<p>Changed</p>").unwrap();
    let second = job_workflow::execute(&sources, &inputs, &operation, || 100000).unwrap();
    assert_eq!(first, second);
    drop(operation);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn saved_page_input_and_retained_byte_budgets_fail_before_publication() {
    for limit_input in [true, false] {
        let (sources, dir) = local();
        let (inputs, selection, mut budget) = prepare_page(
            &sources,
            &dir,
            &format!("<p>{}</p>", "Exact original &amp;".repeat(1000)),
        );
        if limit_input {
            budget.max_input_bytes = 32;
        } else {
            budget.max_batch_bytes = 12000;
            budget.max_expanded_bytes = 12000;
        }
        let operation = start_page(&sources, &inputs, selection, budget);
        let job = job_workflow::execute(&sources, &inputs, &operation, || 1002).unwrap();
        assert_eq!(job.status, JobStatus::Failed);
        assert!(!job.state.error.as_ref().unwrap().retryable);
        assert!(job.state.published_snapshot_id.is_none());
        assert_eq!(job.state.coverage.loaded_documents, 0);
        drop(operation);
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn saved_page_retry_after_io_failure_reuses_authority_checkpoints() {
    use crate::ingestion::{
        page_worker,
        restore_workflow::{restore, RestoreRequest},
    };
    struct Unavailable;
    impl std::io::Read for Unavailable {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("fixture read interrupted"))
        }
    }
    let (sources, dir) = local();
    let (inputs, selection, budget) = prepare_page(&sources, &dir, "<p>Original</p>");
    let preview = selection.preview_id.clone();
    let origin = inputs
        .read_preview(&preview, |_, value| Ok(value.to_string()))
        .unwrap();
    let operation = start_page(&sources, &inputs, selection, budget);
    let failure = page_worker::run_reader_outcome(
        &sources,
        &operation,
        "page-job",
        Unavailable,
        &origin,
        || 1002,
    )
    .err()
    .unwrap();
    assert!(failure.retryable);
    let before = job_workflow::finish_failure(&sources, &operation, failure, 1002).unwrap();
    assert_eq!(before.state.checkpoint.committed_batch, 2);
    assert_eq!(before.state.coverage.loaded_documents, 0);
    let connections = sources.0.connections.clone();
    drop(operation);
    drop(inputs);
    drop(sources);
    let sources = Sources::new(dir.clone(), connections);
    let inputs = SelectedInputs::default();
    let moved = dir.join("moved.html");
    std::fs::rename(dir.join("  Страница 🦉.html"), &moved).unwrap();
    inputs.insert("new".into(), &moved).unwrap();
    let operation = sources
        .begin_ingestion_operation("restore", JobProvider::SavedPage)
        .unwrap();
    restore(
        &sources,
        &inputs,
        &operation,
        RestoreRequest {
            operation_id: "restore".into(),
            job_id: "page-job".into(),
            source_handle: "new".into(),
        },
        || 1003,
    )
    .unwrap();
    let (resumed, worker) = job_workflow::restart(
        &sources,
        &inputs,
        "page-job",
        job_workflow::Restart::Retry,
        1004,
    )
    .unwrap();
    assert_eq!(resumed.state.checkpoint, before.state.checkpoint);
    let done = job_workflow::execute(&sources, &inputs, &worker, || 1005).unwrap();
    assert_eq!(done.status, JobStatus::Completed);
    assert_eq!(done.state.checkpoint.committed_batch, 3);
    assert_eq!(done.state.coverage.loaded_documents, 1);
    drop(worker);
    drop(operation);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn saved_page_cancellation_revocation_or_deadline_at_eof_wins_before_any_original_is_committed() {
    struct AtEnd<'a> {
        bytes: &'a [u8],
        action: Option<Box<dyn FnOnce() + 'a>>,
    }
    impl std::io::Read for AtEnd<'_> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let n = std::io::Read::read(&mut self.bytes, out)?;
            if n == 0 {
                if let Some(action) = self.action.take() {
                    action();
                }
            }
            Ok(n)
        }
    }
    for case in 0..3 {
        let (sources, dir) = local();
        let raw = "<p>Original</p>";
        let (inputs, selection, budget) = prepare_page(&sources, &dir, raw);
        let origin = inputs
            .read_preview(&selection.preview_id, |_, value| Ok(value.to_string()))
            .unwrap();
        let grant = selection.bindings[0].grant_id.clone();
        let operation = start_page(&sources, &inputs, selection, budget);
        let now = std::cell::Cell::new(1002);
        let action = || match case {
            0 => {
                sources.cancel_ingestion_operation("page-job").unwrap();
            }
            1 => {
                sources
                    .with_ingestion_database(&operation, |db| db.revoke(&grant, "savedPage"))
                    .unwrap();
            }
            _ => now.set(61002),
        };
        let failure = crate::ingestion::page_worker::run_reader_outcome(
            &sources,
            &operation,
            "page-job",
            AtEnd {
                bytes: raw.as_bytes(),
                action: Some(Box::new(action)),
            },
            &origin,
            || now.get(),
        )
        .err()
        .unwrap();
        let job = job_workflow::finish_failure(&sources, &operation, failure, now.get()).unwrap();
        assert_eq!(
            job.status,
            if case == 0 {
                JobStatus::Cancelled
            } else {
                JobStatus::Failed
            }
        );
        assert_eq!(job.state.coverage.loaded_documents, 0);
        assert!(job.state.published_snapshot_id.is_none());
        if case != 0 {
            assert!(!job.state.error.as_ref().unwrap().retryable);
        }
        drop(operation);
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
