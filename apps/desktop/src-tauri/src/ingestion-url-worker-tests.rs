use super::*;
use crate::corpus_database::{
    Acquisition, JobBudget, JobPeriod, JobProvider, JobRole, JobSelection, JobStatus,
};
use crate::ingestion::{
    grants::{create_binding, BindingRequest},
    job_workflow::{self, StartRequest},
    page_document,
    selected_page::PageSnapshot,
    url_preview::{preview_with, UrlPreviewRequest},
    SelectedInputs,
};
use sha2::{Digest, Sha256};
const URL: &str = "https://example.com/article#original";
fn budget() -> JobBudget {
    JobBudget {
        max_input_bytes: 1 << 20,
        max_expanded_bytes: 1 << 20,
        max_documents: 1,
        max_batch_documents: 1,
        max_batch_bytes: 1 << 20,
        max_duration_ms: 60000,
    }
}
fn preview_url(
    sources: &Sources,
    inputs: &SelectedInputs,
    raw: &str,
    url: &str,
    retrieved: &str,
) -> crate::ingestion::preview_workflow::PreviewResult {
    let operation = sources
        .begin_ingestion_operation("preview-url", JobProvider::Web)
        .unwrap();
    preview_with(
        sources,
        inputs,
        &operation,
        UrlPreviewRequest {
            operation_id: "preview-url".into(),
            selected_url: url.into(),
            budget: budget(),
        },
        |_, _, _| {
            Ok(PageSnapshot {
                selected_url: url.into(),
                final_url: url.split('#').next().unwrap().into(),
                redirects: vec![],
                content_type: "text/html".into(),
                original: raw.into(),
                sha256: format!("{:x}", Sha256::digest(raw)),
                retrieved_at: retrieved.into(),
            })
        },
    )
    .unwrap()
}
fn prepare_url(sources: &Sources, inputs: &SelectedInputs, raw: &str) -> (String, JobSelection) {
    let preview = preview_url(sources, inputs, raw, URL, "2026-11-08T09:00:00Z");
    let operation = sources
        .begin_ingestion_operation("binding-url", JobProvider::Web)
        .unwrap();
    let binding = sources
        .with_ingestion_database(&operation, |db| {
            create_binding(
                db,
                BindingRequest {
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
                },
                1000,
            )
        })
        .unwrap();
    (
        preview.source_handle,
        JobSelection {
            preview_id: preview.id,
            resource_ids: vec![binding.resource_id.clone()],
            bindings: vec![binding],
        },
    )
}
fn start_url(
    sources: &Sources,
    inputs: &SelectedInputs,
    selection: JobSelection,
) -> crate::ingestion::operations::Operation {
    let operation = sources
        .begin_ingestion_operation("url-job", JobProvider::Web)
        .unwrap();
    job_workflow::create(
        sources,
        inputs,
        &operation,
        StartRequest {
            operation_id: "url-job".into(),
            selection,
            budget: budget(),
        },
        1001,
    )
    .unwrap();
    operation
}
#[test]
fn selected_url_job_publishes_exact_original_and_url_provenance_without_model_usage() {
    let (sources, dir) = local();
    let inputs = SelectedInputs::default();
    let raw = "<title>  Источник 🦉</title><p>  Исходный текст\r\n&amp; смысл</p>";
    let (_, selection) = prepare_url(&sources, &inputs, raw);
    let operation = start_url(&sources, &inputs, selection);
    let job = job_workflow::execute(&sources, &inputs, &operation, || 1002).unwrap();
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(job.state.coverage.loaded_documents, 1);
    assert_eq!(job.state.coverage.indexed_documents, 0);
    assert_eq!(job.state.coverage.analyzed_documents, 0);
    assert_eq!(job.state.coverage.used_documents, 0);
    sources.with_ingestion_database(&operation, |db| {
        let snapshot = db.snapshot(job.state.published_snapshot_id.as_deref().unwrap())?;
        let revision = db.revision(&snapshot.documents[0])?;
        let document = page_document::decode(&revision.document_json, &mut || Ok(()))?;
        assert_eq!(document.original, raw);
        assert_eq!(revision.provenance.acquisition, Acquisition::SelectedUrl);
        assert!(document.source_key.starts_with("pageSource:1:web:"));
        assert!(matches!(&document.origin, page_document::PageOrigin::SelectedUrl { selected_url, retrieved_at, .. } if selected_url == URL && retrieved_at == "2026-11-08T09:00:00Z"));
        Ok(())
    }).unwrap();
    let projected =
        crate::ingestion::projection::project(&sources, &operation, "url-job", || 70000).unwrap();
    assert_eq!(projected.documents.len(), 1);
    assert!(projected.documents[0].text.contains("Исходный текст"));
    assert_eq!(
        job_workflow::execute(&sources, &inputs, &operation, || 70001).unwrap(),
        job
    );
    drop(operation);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn released_url_input_requires_explicit_refetch_and_matching_origin_before_resume() {
    use crate::ingestion::{
        job_workflow::{Control, Restart},
        restore_workflow::{restore, RestoreRequest},
    };
    let (sources, dir) = local();
    let inputs = SelectedInputs::default();
    let raw = "<p>  Неизменный текст 🦉\r\n</p>";
    let (handle, selection) = prepare_url(&sources, &inputs, raw);
    let operation = start_url(&sources, &inputs, selection);
    job_workflow::control(&sources, "url-job", Control::Pause).unwrap();
    drop(operation);
    inputs.release(&handle).unwrap();
    let connections = sources.0.connections.clone();
    drop(inputs);
    drop(sources);
    let sources = Sources::new(dir.clone(), connections);
    let inputs = SelectedInputs::default();
    assert!(job_workflow::restart(&sources, &inputs, "url-job", Restart::Resume, 1002).is_err());
    for (text, url, valid) in [
        ("<p>Changed</p>", URL, false),
        (raw, "https://example.com/other", false),
        (raw, URL, true),
    ] {
        let fresh = preview_url(&sources, &inputs, text, url, "2026-11-08T09:05:00Z");
        let restore_operation = sources
            .begin_ingestion_operation("restore-url", JobProvider::Web)
            .unwrap();
        let restored = restore(
            &sources,
            &inputs,
            &restore_operation,
            RestoreRequest {
                operation_id: "restore-url".into(),
                job_id: "url-job".into(),
                source_handle: fresh.source_handle.clone(),
            },
            || 1003,
        );
        assert_eq!(restored.is_ok(), valid);
        drop(restore_operation);
        if !valid {
            inputs.release(&fresh.source_handle).unwrap();
        }
    }
    let (_, operation) =
        job_workflow::restart(&sources, &inputs, "url-job", Restart::Resume, 1004).unwrap();
    let job = job_workflow::execute(&sources, &inputs, &operation, || 1005).unwrap();
    assert_eq!(job.status, JobStatus::Completed);
    sources.with_ingestion_database(&operation, |db| {
        let snapshot = db.snapshot(job.state.published_snapshot_id.as_deref().unwrap())?;
        let revision = db.revision(&snapshot.documents[0])?;
        let document = page_document::decode(&revision.document_json, &mut || Ok(()))?;
        assert!(matches!(&document.origin, page_document::PageOrigin::SelectedUrl { retrieved_at, .. } if retrieved_at == "2026-11-08T09:00:00Z"));
        assert_eq!(document.original, raw); Ok(())
    }).unwrap();
    drop(operation);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn selected_url_preparation_and_native_search_keep_exact_spans_and_exclude_holdout() {
    use crate::corpus_database::{
        episodes::{ChunkPolicy, EpisodePolicy, EpisodeTokenizer},
        search::SearchPeriod,
    };
    use crate::ingestion::{
        preparation::{prepare, PrepareInput},
        search::{rebuild, IndexInput},
        search_query::{query, QueryInput},
    };
    struct Tokenizer;
    impl EpisodeTokenizer for Tokenizer {
        fn version(&self) -> &str {
            "syntheticUrl:1"
        }
        fn count(&self, text: &str) -> Result<usize, String> {
            Ok(text.chars().count() + 2)
        }
    }
    for holdout in [0, 10000] {
        let (sources, dir) = local();
        let raw = "<meta property='article:published_time' content='2026-11-01T12:00:00Z'><p>  точная цитата 🦉 &amp; слово</p>";
        let inputs = SelectedInputs::default();
        let (_, selection) = prepare_url(&sources, &inputs, raw);
        let binding = selection.bindings[0].clone();
        let operation = start_url(&sources, &inputs, selection);
        let original = job_workflow::execute(&sources, &inputs, &operation, || 1002).unwrap();
        let prepared = prepare(
            &sources,
            &operation,
            &PrepareInput {
                job_id: "url-job",
                split: &EpisodePolicy {
                    time_gap_ms: 1000,
                    holdout_basis_points: holdout,
                    seed: "pageFixture".into(),
                },
                chunks: &ChunkPolicy {
                    max_tokens: 512,
                    max_bytes: 4096,
                },
                tokenizer: &Tokenizer,
                max_duration_ms: 60000,
            },
            || 70000,
        )
        .unwrap();
        assert_eq!(
            prepared.parent_snapshot_id,
            original.state.published_snapshot_id
        );
        let receipt = rebuild(
            &sources,
            &operation,
            &IndexInput {
                job_id: "url-job",
                snapshot_id: &prepared.id,
                max_duration_ms: 60000,
            },
            || 70001,
        )
        .unwrap();
        let response = query(
            &sources,
            &operation,
            &QueryInput {
                job_id: "url-job",
                snapshot_id: &prepared.id,
                persona_id: "personA",
                query_text: "цитата",
                period: SearchPeriod {
                    from: None,
                    to: None,
                    include_undated: true,
                },
                limit: 10,
                offset: 0,
                expected_index_generation: Some(receipt.index_generation),
                max_duration_ms: 60000,
            },
            || 70002,
        )
        .unwrap();
        assert_eq!(response.hits.len(), usize::from(holdout == 0));
        sources
            .with_ingestion_database(&operation, |db| {
                assert!(db
                    .snapshot(&prepared.id)?
                    .publication_id
                    .starts_with("selectedUrlPreparation:1:"));
                Ok(())
            })
            .unwrap();
        let revoked_query = QueryInput {
            job_id: "url-job",
            snapshot_id: &prepared.id,
            persona_id: "personA",
            query_text: "цитата",
            period: SearchPeriod {
                from: None,
                to: None,
                include_undated: true,
            },
            limit: 10,
            offset: 0,
            expected_index_generation: Some(receipt.index_generation),
            max_duration_ms: 60000,
        };
        let wrong = sources
            .begin_ingestion_operation("wrong-query", crate::corpus_database::JobProvider::Telegram)
            .unwrap();
        assert!(query(&sources, &wrong, &revoked_query, || 70003).is_err());
        drop(wrong);

        for hit in response.hits {
            assert_eq!(hit.binding_id, binding.id);
            assert_eq!(hit.persona_id, "personA");
            sources
                .with_ingestion_database(&operation, |db| {
                    let document = page_document::decode(
                        &db.revision(&crate::corpus_database::RevisionReference {
                            document_id: hit.document_id,
                            revision_id: hit.revision_id,
                        })?
                        .document_json,
                        &mut || Ok(()),
                    )?;
                    assert_eq!(document.original, raw);
                    assert!(
                        document.text[hit.span_start as usize..hit.span_end as usize]
                            .contains("цитата 🦉 & слово")
                    );
                    Ok(())
                })
                .unwrap();
        }
        sources
            .with_ingestion_database(&operation, |db| db.revoke(&binding.grant_id, "web"))
            .unwrap();
        assert!(query(&sources, &operation, &revoked_query, || 70004).is_err());
        drop(operation);
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn selected_url_cancellation_and_revocation_never_publish_a_snapshot() {
    for cancel in [true, false] {
        let (sources, dir) = local();
        let inputs = SelectedInputs::default();
        let (_, selection) = prepare_url(&sources, &inputs, "<p>Original</p>");
        let grant = selection.bindings[0].grant_id.clone();
        let operation = start_url(&sources, &inputs, selection);
        if cancel {
            job_workflow::control(&sources, "url-job", job_workflow::Control::Cancel).unwrap();
        } else {
            sources
                .with_ingestion_database(&operation, |db| db.revoke(&grant, "web"))
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
