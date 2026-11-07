//! Acceptance across fresh picker capabilities, previews, grants and jobs.
//! This uses the same registry entry point as the native picker, never a saved path.
use super::ingestion_fixture::{archive, prepare};
use super::*;
use crate::corpus_database::{JobProvider, JobSelection};
use crate::ingestion::{
    grants::{create_binding, BindingRequest},
    job_workflow::{create, execute, inspect, StartRequest},
    preview_workflow::{preview, PreviewRequest},
    publication::{publish, SnapshotView},
    SelectedInputs,
};
fn fresh_import(sources: &Sources, path: &std::path::Path, id: &str) -> SnapshotView {
    let inputs = SelectedInputs::default();
    inputs.insert("fresh-selection".into(), path).unwrap();
    let operation = sources
        .begin_ingestion_operation(id, JobProvider::Telegram)
        .unwrap();
    let seed = inspect(sources, "job").unwrap();
    let selected = preview(
        sources,
        &inputs,
        &operation,
        PreviewRequest {
            operation_id: id.into(),
            source_handle: "fresh-selection".into(),
            budget: seed.state.budget.clone(),
        },
    )
    .unwrap();
    assert_ne!(selected.id, seed.state.selection.preview_id);
    let prior = &seed.state.selection.bindings[0];
    let binding = sources
        .with_ingestion_database(&operation, |db| {
            create_binding(
                db,
                BindingRequest {
                    preview_id: selected.id.clone(),
                    resource_id: prior.resource_id.clone(),
                    persona_id: prior.persona_id.clone(),
                    role: prior.role,
                    target_author_id: prior.target_author_id.clone(),
                    allowed_context_author_ids: prior.allowed_context_author_ids.clone(),
                    allow_unknown_context: prior.allow_unknown_context,
                    period: prior.period.clone(),
                    allow_model_processing: false,
                    allow_export: false,
                },
                1002,
            )
        })
        .unwrap();
    assert_ne!(binding.grant_id, prior.grant_id);
    create(
        sources,
        &inputs,
        &operation,
        StartRequest {
            operation_id: id.into(),
            budget: seed.state.budget,
            selection: JobSelection {
                preview_id: selected.id,
                resource_ids: vec![binding.resource_id.clone()],
                bindings: vec![binding],
            },
        },
        1003,
    )
    .unwrap();
    execute(sources, &inputs, &operation, || 1004).unwrap();
    publish(sources, &operation, id, 1005).unwrap()
}

#[test]
fn fresh_file_selection_reuses_revisions_and_edits_preserve_prior_snapshots() {
    let (sources, dir) = local();
    let raw = archive();
    let path = dir.join("archive.json");
    let (inputs, _, _) = prepare(&sources, &dir, &raw);
    let operation = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    execute(&sources, &inputs, &operation, || 1001).unwrap();
    let first = publish(&sources, &operation, "job", 1001).unwrap();
    drop(inputs);
    drop(operation);

    let repeated = fresh_import(&sources, &path, "reselected");
    assert_eq!(first.document_refs, repeated.document_refs);
    assert_ne!(
        first.id, repeated.id,
        "a new grant/job has its own immutable receipt"
    );
    assert_ne!(first.binding_ids, repeated.binding_ids);

    let edited_raw = raw.replace(r#""text":"ok""#, r#""text":"  edited é 👩🏽‍🔬\r\n""#);
    std::fs::write(&path, edited_raw).unwrap();
    let edited = fresh_import(&sources, &path, "edited-selection");
    assert_eq!(first.document_refs.len(), 3);
    for (index, (old, new)) in first
        .document_refs
        .iter()
        .zip(&edited.document_refs)
        .enumerate()
    {
        assert_eq!(old.document_id, new.document_id);
        if index < 2 {
            assert_ne!(old.revision_id, new.revision_id);
        } else {
            assert_eq!(old.revision_id, new.revision_id);
        }
    }
    sources
        .with_ingestion_job_state(|db| {
            assert_eq!(db.snapshot(&first.id)?.documents, first.document_refs);
            let old = db.revision(&first.document_refs[0])?;
            let new = db.revision(&edited.document_refs[0])?;
            let old: serde_json::Value = serde_json::from_str(&old.document_json).unwrap();
            let new: serde_json::Value = serde_json::from_str(&new.document_json).unwrap();
            assert_eq!(old["text"], "ok");
            assert_eq!(new["text"], "  edited é 👩🏽‍🔬\r\n");
            Ok(())
        })
        .unwrap();
    let other_path = dir.join("independent-archive.json");
    std::fs::copy(&path, &other_path).unwrap();
    let independent = fresh_import(&sources, &other_path, "independent-selection");
    assert!(edited
        .document_refs
        .iter()
        .zip(&independent.document_refs)
        .all(|(a, b)| a.document_id != b.document_id));
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
