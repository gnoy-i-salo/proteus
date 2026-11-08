use super::{
    corpus::Sources,
    document::document,
    document::in_scope,
    grant::{timestamp, validate_request},
    model::{ImportedRecord, RawItem, SourceAction, SourceBudget, SourceGrant, SourceGrantRequest},
    store,
};
use crate::connections::{storage::random_id, Connections, Provider};
use std::{path::PathBuf, sync::Arc};
fn request() -> SourceGrantRequest {
    SourceGrantRequest {
        provider: Provider::Github,
        resource_id: "https://github.com/owner/repository".into(),
        author_id: Some("author".into()),
        from: Some("2024-01-01".into()),
        to: Some("2025-01-01".into()),
        allow_model_processing: true,
        operations: vec!["collect".into(), "read".into()],
        budget: SourceBudget {
            max_steps: 16,
            max_documents: 1000,
            max_bytes: 1048576,
            max_duration_ms: 300000,
        },
    }
}
#[test]
fn native_grant_canonicalizes_selected_resource_and_rejects_scope_escalation() {
    let mut valid = request();
    validate_request(&mut valid).unwrap();
    assert_eq!(valid.resource_id, "owner/repository");
    for resource in [
        "https://github.com.evil/owner/repo",
        "https://user:pass@github.com/owner/repo",
        "https://github.com/owner/repo?next=other",
        "../../secrets",
        "owner/repo/../../other",
        "https://127.0.0.1/repo",
    ] {
        let mut invalid = request();
        invalid.resource_id = resource.into();
        assert!(validate_request(&mut invalid).is_err());
    }
}
#[test]
fn grants_require_consent_bounded_budgets_and_ordered_dates() {
    let mut invalid = request();
    invalid.allow_model_processing = false;
    assert!(validate_request(&mut invalid).is_err());
    let mut invalid = request();
    invalid.budget.max_documents = 10000000;
    assert!(validate_request(&mut invalid).is_err());
    let mut invalid = request();
    invalid.operations.push("send-message".into());
    assert!(validate_request(&mut invalid).is_err());
    let mut invalid = request();
    invalid.from = Some("2026-01-01".into());
    assert!(validate_request(&mut invalid).is_err());
}
struct NoNetwork;
impl crate::connections::http::Http for NoNetwork {
    fn get(
        &self,
        _: &str,
        _: &[(&str, String)],
        _: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        panic!("local source must never access network")
    }
    fn post(&self, _: &str, _: &[(&str, String)]) -> Result<serde_json::Value, String> {
        panic!("local source must never access network")
    }
}
fn local() -> (Sources, PathBuf) {
    let directory =
        std::env::temp_dir().join(format!("proteus-source-test-{}", random_id(18).unwrap()));
    let hub = Connections::test(
        Arc::new(crate::connections::storage::MemorySecrets::default()),
        Arc::new(NoNetwork),
    );
    (Sources::new(directory.clone(), hub), directory)
}
fn local_grant(sources: &Sources, provider: Provider, max_steps: u32) -> SourceGrant {
    let id = random_id(18).unwrap();
    let mut request = request();
    request.provider = provider;
    request.resource_id = format!("import:{id}");
    request.author_id = None;
    request.from = None;
    request.to = None;
    request.budget.max_steps = max_steps;
    let doc = document(
        &request,
        RawItem {
            id: "one".into(),
            text: "  exact\ntext".into(),
            ..RawItem::default()
        },
        false,
    );
    sources
        .0
        .store
        .write(
            &sources.path(&id).unwrap(),
            &ImportedRecord {
                provider,
                documents: vec![doc],
                warnings: vec![],
            },
        )
        .unwrap();
    sources.create(request).unwrap()
}
#[test]
fn import_collection_preserves_undated_text_and_resume_does_not_reset_shared_budget() {
    let (sources, directory) = local();
    let grant = local_grant(&sources, Provider::Instagram, 2);
    sources.reserve_model_step(&grant.id).unwrap();
    let page = sources
        .execute(
            &grant.id,
            SourceAction::Collect {
                cursor: None,
                limit: 20,
            },
        )
        .unwrap();
    assert_eq!(page.documents[0].text, "  exact\ntext");
    assert!(page.coverage.complete);
    assert!(page.cursor.is_none());
    sources.cancel(&grant.id, true).unwrap();
    sources.cancel(&grant.id, false).unwrap();
    assert_eq!(
        sources.reserve_model_step(&grant.id).unwrap_err(),
        "source-budget-exceeded"
    );
    drop(sources);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn interrupted_selected_delete_can_be_retried_without_reactivating_old_data() {
    let (sources, directory) = local();
    let grant = local_grant(&sources, Provider::Telegram, 10);
    let legacy = sources.path(&grant.id).unwrap();
    std::fs::create_dir(&legacy).unwrap();
    assert!(sources.delete(&grant.id).is_err());
    assert!(sources.execute(&grant.id, SourceAction::Coverage).is_err());
    std::fs::remove_dir(&legacy).unwrap();
    assert!(sources.delete(&grant.id).is_ok());
    assert!(!store::current_path(&legacy).unwrap().exists());
    drop(sources);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn deletion_only_removes_selected_provider_owned_ciphertext() {
    let (sources, directory) = local();
    let tg = local_grant(&sources, Provider::Telegram, 10);
    let ig = local_grant(&sources, Provider::Instagram, 10);
    std::fs::write(directory.join("unrelated.txt"), "keep").unwrap();
    sources.clear(Provider::Telegram).unwrap();
    assert!(!sources.path(&tg.id).unwrap().exists());
    assert!(!sources
        .path(tg.request.resource_id.trim_start_matches("import:"))
        .unwrap()
        .exists());
    assert!(!sources
        .0
        .store
        .is_deleted(&sources.path(&ig.id).unwrap())
        .unwrap());
    assert!(sources
        .0
        .store
        .is_deleted(&sources.path(&tg.id).unwrap())
        .unwrap());
    assert!(sources
        .execute(
            &ig.id,
            SourceAction::Collect {
                cursor: None,
                limit: 1
            }
        )
        .is_ok());
    assert!(directory.join("unrelated.txt").exists());
    assert!(sources.delete("../../other").is_err());
    sources.delete(&ig.id).unwrap();
    assert!(!sources.path(&ig.id).unwrap().exists());
    drop(sources);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn opaque_reply_ids_remain_linked_after_import_and_collection() {
    let mut selected = request();
    selected.resource_id = "import:abcdefghijklmnop".into();
    let parent = document(
        &selected,
        RawItem {
            id: "chat:1".into(),
            text: "parent".into(),
            ..RawItem::default()
        },
        false,
    );
    let child = document(
        &selected,
        RawItem {
            id: "chat:2".into(),
            text: "child".into(),
            reply_to: Some("chat:1".into()),
            ..RawItem::default()
        },
        false,
    );
    assert_eq!(child.reply_to.as_deref(), Some(parent.id.as_str()));
    let collected_parent = document(
        &selected,
        RawItem {
            id: parent.id,
            text: parent.text,
            ..RawItem::default()
        },
        false,
    );
    let collected_child = document(
        &selected,
        RawItem {
            id: child.id,
            text: child.text,
            reply_to: child.reply_to,
            ..RawItem::default()
        },
        false,
    );
    assert_eq!(collected_child.reply_to, Some(collected_parent.id));
}
#[test]
fn provider_iso_offsets_are_normalized_before_time_scope_checks() {
    assert_eq!(
        timestamp("2024-01-01T00:00:00+0000").unwrap(),
        timestamp("2024-01-01T00:00:00Z").unwrap()
    );
    let mut selected = request();
    selected.from = Some("2024-01-01T00:00:00Z".into());
    selected.to = None;
    selected.author_id = None;
    let item = RawItem {
        id: "1".into(),
        created: Some("2024-01-01T03:00:00+0300".into()),
        ..RawItem::default()
    };
    assert!(in_scope(&selected, &item));
}

#[path = "source-ingestion-tests.rs"]
mod ingestion_tests;

#[path = "ingestion-page-preview-tests.rs"]
mod page_preview_tests;
#[path = "ingestion-page-worker-tests.rs"]
mod page_worker_tests;
#[path = "ingestion-preview-workflow-tests.rs"]
mod preview_workflow_tests;

#[path = "ingestion-telegram-worker-tests.rs"]
mod telegram_worker_tests;

#[path = "ingestion-test-fixture.rs"]
mod ingestion_fixture;

#[path = "ingestion-job-workflow-tests.rs"]
mod job_workflow_tests;

#[path = "ingestion-restore-workflow-tests.rs"]
mod restore_workflow_tests;

#[path = "ingestion-publication-tests.rs"]
mod publication_tests;

#[path = "ingestion-reselection-tests.rs"]
mod reselection_tests;

#[path = "ingestion-peer-identity-tests.rs"]
mod peer_identity_tests;

#[path = "ingestion-preparation-tests.rs"]
mod preparation_tests;

#[path = "ingestion-prepared-publication-tests.rs"]
mod prepared_publication_tests;

#[path = "ingestion-search-tests.rs"]
mod ingestion_search;

#[path = "ingestion-page-projection-tests.rs"]
mod page_projection_tests;
