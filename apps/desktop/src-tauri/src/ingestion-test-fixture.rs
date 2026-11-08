use super::*;
use crate::corpus_database::{JobBudget, JobInput, JobPeriod, JobProvider, JobRole, JobSelection};
use crate::ingestion::telegram_identity::ArchiveVersion;
use crate::ingestion::{
    grants::{create_binding, BindingRequest},
    preview_workflow::{preview_version, PreviewRequest},
    SelectedInputs,
};
pub(super) fn archive() -> String {
    r#"{"name":"Чат","type":"personal_chat","id":1,"messages":[{"id":1,"type":"message","from_id":"user1","text":"ok"},{"id":2,"type":"message","from_id":"user1","text":"ok"},{"id":3,"type":"message","from_id":"user2","text":"  контекст\r\n"},{"id":4,"type":"message","from_id":"outsider","text":"excluded"}]}"#.into()
}
pub(super) fn prepare(
    sources: &Sources,
    dir: &std::path::Path,
    raw: &str,
) -> (SelectedInputs, String, String) {
    let budget = JobBudget {
        max_input_bytes: 1 << 20,
        max_expanded_bytes: 1 << 20,
        max_documents: 100,
        max_batch_documents: 1,
        max_batch_bytes: 65536,
        max_duration_ms: 60000,
    };
    prepare_with_budget(sources, dir, raw, budget)
}
pub(super) fn prepare_with_budget(
    sources: &Sources,
    dir: &std::path::Path,
    raw: &str,
    budget: JobBudget,
) -> (SelectedInputs, String, String) {
    prepare_with_version(sources, dir, raw, budget, ArchiveVersion::CURRENT)
}
pub(super) fn prepare_with_version(
    sources: &Sources,
    dir: &std::path::Path,
    raw: &str,
    budget: JobBudget,
    version: ArchiveVersion,
) -> (SelectedInputs, String, String) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("archive.json");
    std::fs::write(&path, raw).unwrap();
    let inputs = SelectedInputs::default();
    inputs.insert("selected".into(), &path).unwrap();
    let op = sources
        .begin_ingestion_operation("setup", JobProvider::Telegram)
        .unwrap();
    let preview = preview_version(
        sources,
        &inputs,
        &op,
        PreviewRequest {
            operation_id: "setup".into(),
            source_handle: "selected".into(),
            budget: budget.clone(),
        },
        version,
    )
    .unwrap();
    sources
        .with_ingestion_database(&op, |db| {
            let binding = create_binding(
                db,
                BindingRequest {
                    preview_id: preview.id.clone(),
                    resource_id: preview.resources[0].id.clone(),
                    persona_id: "personA".into(),
                    role: JobRole::Own,
                    target_author_id: Some("user1".into()),
                    allowed_context_author_ids: vec!["user2".into()],
                    allow_unknown_context: false,
                    period: JobPeriod {
                        from: None,
                        to: None,
                        include_undated: true,
                    },
                    allow_model_processing: false,
                    allow_export: false,
                },
                1000,
            )?;
            db.create_job(&JobInput {
                id: "job".into(),
                adapter_version: preview.adapter_version.clone(),
                budget,
                selection: JobSelection {
                    preview_id: preview.id.clone(),
                    resource_ids: vec![binding.resource_id.clone()],
                    bindings: vec![binding],
                },
            })?;
            Ok(())
        })
        .unwrap();
    let origin = inputs
        .read_scoped("selected", |_, origin| Ok(origin.to_string()))
        .unwrap();
    (inputs, preview.id, origin)
}
