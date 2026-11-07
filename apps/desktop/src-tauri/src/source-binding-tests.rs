use super::*;
use crate::connections::{
    storage::{random_id, MemorySecrets},
    Connections, Provider,
};
use crate::sources::{
    binding_policy::{SourceRole, SpeakerRelation},
    model::{SourceBudget, SourceGrantRequest},
};
use std::{path::PathBuf, sync::Arc};
struct NoNetwork;
impl crate::connections::http::Http for NoNetwork {
    fn get(
        &self,
        _: &str,
        _: &[(&str, String)],
        _: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        panic!("binding import must never use network")
    }
    fn post(&self, _: &str, _: &[(&str, String)]) -> Result<serde_json::Value, String> {
        panic!("binding import must never use network")
    }
}
fn selected() -> (Sources, PathBuf, CreatedSourceBinding) {
    let directory =
        std::env::temp_dir().join(format!("proteus-binding-{}", random_id(18).unwrap()));
    std::fs::create_dir_all(&directory).unwrap();
    let sources = Sources::new(
        directory.join("corpus"),
        Connections::test(Arc::new(MemorySecrets::default()), Arc::new(NoNetwork)),
    );
    let path = directory.join("telegram.json");
    let data = serde_json::json!({"messages":[
 {"id":1,"type":"message","from_id":"target","from":"  Same name é 😀  ","text":"  exact\r\n😀\t"},
 {"id":2,"type":"message","from_id":"peer","from":"  Same name é 😀  ","text":"  exact\r\n😀\t","reply_to_message_id":1},
 {"id":3,"type":"message","from_id":"stranger","from":"  Same name é 😀  ","text":"excluded"},
 {"id":4,"type":"message","text":"unknown text"},
 {"id":5,"type":"message","from_id":"target","text":"forwarded words","forwarded_from":"Original"}]});
    std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
    let imported = sources.import(Provider::Telegram, &path).unwrap();
    let source = SourceGrantRequest {
        provider: Provider::Telegram,
        resource_id: imported.id,
        author_id: Some("target".into()),
        from: None,
        to: None,
        allow_model_processing: true,
        operations: vec![
            "read".into(),
            "collect".into(),
            "coverage".into(),
            "search".into(),
        ],
        budget: SourceBudget {
            max_steps: 64,
            max_documents: 100,
            max_bytes: 1048576,
            max_duration_ms: 300000,
        },
    };
    let binding = sources
        .create_binding(SourceBindingRequest {
            source,
            persona_id: "persona-a".into(),
            role: SourceRole::Own,
            allowed_context_author_ids: vec!["peer".into()],
            allow_unknown_context: true,
            include_undated: true,
        })
        .unwrap();
    (sources, directory, binding)
}
fn collect(sources: &Sources, binding: &CreatedSourceBinding) -> SourceBindingResult {
    sources
        .binding_action(
            &binding.binding.id,
            SourceAction::Collect {
                cursor: None,
                limit: 20,
            },
        )
        .unwrap()
}
#[test]
fn collection_preserves_selected_context_duplicate_words_and_exact_metadata() {
    let (sources, directory, binding) = selected();
    let result = collect(&sources, &binding);
    assert_eq!(
        result.documents.len(),
        4,
        "equal wording from different people must not collapse identity"
    );
    let target = &result.documents[0];
    assert_eq!(target.speaker_relation, SpeakerRelation::Target);
    assert!(!target.can_support_own_words);
    assert_eq!(target.document.authorship, "other");
    let peer = &result.documents[1];
    assert_eq!(peer.speaker_relation, SpeakerRelation::Context);
    assert!(!peer.can_support_own_words);
    assert_eq!(peer.document.text, "  exact\r\n😀\t");
    assert_eq!(
        peer.document.author_label.as_deref(),
        Some("  Same name é 😀  ")
    );
    assert_eq!(
        peer.relations[0].original.as_ref().unwrap().document_id,
        target.document.id
    );
    assert_eq!(
        peer.relations[0].original_author.id.as_deref(),
        Some("target")
    );
    assert_eq!(
        result.documents[2].speaker_relation,
        SpeakerRelation::Unknown
    );
    assert!(!result.documents[2].can_support_own_words);
    assert!(!result.documents[3].can_support_own_words);
    assert_eq!(result.documents[3].relations.len(), 1);
    let forward = &result.documents[3].relations[0];
    assert!(forward.original_author.id.is_none());
    assert!(forward.original_author.label.is_none());
    let read = sources
        .binding_action(
            &binding.binding.id,
            SourceAction::Read {
                document_ids: vec![peer.document.id.clone()],
            },
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(&read.documents[0]).unwrap(),
        serde_json::to_value(peer).unwrap()
    );
    drop(sources);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn legacy_action_cannot_bypass_binding_and_revoked_or_rewritten_scope_fails_closed() {
    let (sources, directory, binding) = selected();
    collect(&sources, &binding);
    assert!(sources
        .execute(&binding.grant.id, SourceAction::Coverage)
        .is_err());
    sources.cancel(&binding.grant.id, true).unwrap();
    assert!(sources
        .binding_action(&binding.binding.id, SourceAction::Coverage)
        .is_err());
    sources.cancel(&binding.grant.id, false).unwrap();
    let path = sources.path(&binding.grant.id).unwrap();
    let mut record: Record = sources.0.store.read(&path).unwrap();
    record.binding.as_mut().unwrap().resource_id = "import:foreign-resource".into();
    sources.0.store.write(&path, &record).unwrap();
    assert!(sources
        .binding_action(&binding.binding.id, SourceAction::Coverage)
        .is_err());
    record.binding = Some(binding.binding.clone());
    record.grant.expires_at = "2000-01-01T00:00:00Z".into();
    sources.0.store.write(&path, &record).unwrap();
    assert!(sources
        .binding_action(&binding.binding.id, SourceAction::Coverage)
        .is_err());
    record.grant = binding.grant.clone();
    record.grant.request.allow_model_processing = false;
    sources.0.store.write(&path, &record).unwrap();
    assert!(sources
        .binding_action(&binding.binding.id, SourceAction::Coverage)
        .is_err());
    drop(sources);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn deleting_selected_import_revokes_bound_reads_and_clear_removes_all_generations() {
    let (sources, directory, binding) = selected();
    let result = collect(&sources, &binding);
    let imported = binding.binding.resource_id.strip_prefix("import:").unwrap();
    sources
        .0
        .store
        .remove(&sources.path(imported).unwrap())
        .unwrap();
    assert!(sources
        .binding_action(
            &binding.binding.id,
            SourceAction::Read {
                document_ids: vec![result.documents[0].document.id.clone()]
            }
        )
        .is_err());
    sources.clear(Provider::Telegram).unwrap();
    assert!(sources
        .binding_action(&binding.binding.id, SourceAction::Coverage)
        .is_err());
    drop(sources);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn revision_identity_uses_full_original_and_authorship_metadata() {
    let (sources, directory, binding) = selected();
    let result = collect(&sources, &binding);
    let path = sources.path(&binding.grant.id).unwrap();
    let mut record: Record = sources.0.store.read(&path).unwrap();
    record.documents[0].text = "😀".repeat(7000);
    record.documents[0].content_hash = "test-full-original-hash".into();
    let original = record.documents[0].clone();
    sources.0.store.write(&path, &record).unwrap();
    let read = sources
        .binding_action(
            &binding.binding.id,
            SourceAction::Read {
                document_ids: vec![result.documents[0].document.id.clone()],
            },
        )
        .unwrap();
    assert_eq!(
        read.documents[0].revision_id,
        sources
            .0
            .store
            .references(&path)
            .unwrap()
            .iter()
            .find(|reference| reference.document_id == original.id)
            .unwrap()
            .revision_id
    );
    assert_eq!(read.documents[0].document.text.len(), 24000);
    assert!(read.documents[0].document.truncated);
    let mut altered = original;
    altered.author_label = Some("Different label".into());
    record.documents[0] = altered;
    sources.0.store.write(&path, &record).unwrap();
    let changed = sources
        .binding_action(
            &binding.binding.id,
            SourceAction::Read {
                document_ids: vec![record.documents[0].id.clone()],
            },
        )
        .unwrap();
    assert_ne!(
        changed.documents[0].revision_id,
        read.documents[0].revision_id
    );
    drop(sources);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn shared_binding_wire_fixture_matches_rust_serde_without_aliases() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../src/shared/lib/fixtures/source-binding-wire.json"
    ))
    .unwrap();
    let request: SourceBindingRequest =
        serde_json::from_value(fixture["createRequest"].clone()).unwrap();
    assert_eq!(
        serde_json::to_value(request).unwrap(),
        fixture["createRequest"]
    );
    let created: CreatedSourceBinding = serde_json::from_value(fixture["created"].clone()).unwrap();
    assert_eq!(serde_json::to_value(created).unwrap(), fixture["created"]);
    let result: SourceBindingResult = serde_json::from_value(fixture["result"].clone()).unwrap();
    assert_eq!(serde_json::to_value(result).unwrap(), fixture["result"]);
    let mut unknown = fixture["createRequest"].clone();
    unknown["allowed-context-author-ids"] = serde_json::json!([]);
    assert!(serde_json::from_value::<SourceBindingRequest>(unknown).is_err());
}
#[test]
fn connected_account_change_and_token_removal_revoke_binding_actions() {
    let (sources, directory, local) = selected();
    let secrets = sources.0.connections.secrets();
    let token = serde_json::json!({"format":"proteus-connection-secret","version":2,"kind":"token","payload":{"access":"synthetic-only","refresh":null,"deviceId":null,"expiresAt":null,"refreshedAt":0,"accountId":"account-a","label":"Synthetic account"}});
    secrets.set("github-token", &token.to_string()).unwrap();
    let mut source = local.grant.request.clone();
    source.provider = Provider::Github;
    source.resource_id = "https://github.com/owner/repository".into();
    let binding = sources
        .create_binding(SourceBindingRequest {
            source,
            persona_id: "persona-b".into(),
            role: SourceRole::Own,
            allowed_context_author_ids: vec![],
            allow_unknown_context: false,
            include_undated: false,
        })
        .unwrap();
    assert_eq!(binding.binding.resource_id, "owner/repository");
    assert_eq!(binding.binding.account_id.as_deref(), Some("account-a"));
    assert!(sources
        .binding_action(&binding.binding.id, SourceAction::Coverage)
        .is_ok());
    let mut changed = token.clone();
    changed["payload"]["accountId"] = serde_json::json!("account-b");
    secrets.set("github-token", &changed.to_string()).unwrap();
    assert!(sources
        .binding_action(&binding.binding.id, SourceAction::Coverage)
        .is_err());
    secrets.set("github-token", &token.to_string()).unwrap();
    assert!(sources
        .binding_action(&binding.binding.id, SourceAction::Coverage)
        .is_ok());
    secrets.remove("github-token").unwrap();
    assert!(sources
        .binding_action(&binding.binding.id, SourceAction::Coverage)
        .is_err());
    drop(sources);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn bound_import_does_not_upgrade_authorship_or_rewrite_source_metadata() {
    let (sources, directory, binding) = selected();
    let resource = &binding.binding.resource_id;
    let imported_id = resource.strip_prefix("import:").unwrap();
    let mut imported: crate::sources::model::ImportedRecord = sources.imported(resource).unwrap();
    imported.documents[0].authorship = "unknown".into();
    let original = serde_json::to_value(&imported.documents[0]).unwrap();
    sources
        .0
        .store
        .write(&sources.path(imported_id).unwrap(), &imported)
        .unwrap();
    let result = collect(&sources, &binding);
    assert_eq!(
        serde_json::to_value(&result.documents[0].document).unwrap(),
        original,
        "binding must not rewrite original metadata"
    );
    assert_eq!(
        result.documents[0].speaker_relation,
        SpeakerRelation::Target
    );
    assert!(!result.documents[0].can_support_own_words);
    drop(sources);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn projected_revision_refs_match_the_canonical_database_including_reply_targets() {
    let (sources, directory, binding) = selected();
    let result = collect(&sources, &binding);
    let references = sources
        .0
        .store
        .references(&sources.path(&binding.grant.id).unwrap())
        .unwrap();
    for projected in &result.documents {
        let canonical = references
            .iter()
            .find(|reference| reference.document_id == projected.document.id)
            .unwrap();
        assert_eq!(
            projected.revision_id, canonical.revision_id,
            "projection must name the canonical source-key-scoped revision"
        );
    }
    let target = &result.documents[0];
    assert_eq!(
        result.documents[1].relations[0]
            .original
            .as_ref()
            .unwrap()
            .revision_id,
        target.revision_id
    );
    drop(sources);
    std::fs::remove_dir_all(directory).unwrap();
}
