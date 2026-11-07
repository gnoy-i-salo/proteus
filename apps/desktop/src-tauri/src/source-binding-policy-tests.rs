use super::*;
use crate::sources::model::SourceBudget;
use serde_json::json;
fn setup() -> (SourceBinding, SourceGrant, SourceDocument) {
    let source = SourceGrantRequest {
        provider: Provider::Telegram,
        resource_id: "-12345".into(),
        author_id: Some("target:1".into()),
        from: None,
        to: None,
        allow_model_processing: true,
        operations: vec!["read".into()],
        budget: SourceBudget {
            max_steps: 10,
            max_documents: 100,
            max_bytes: 4096,
            max_duration_ms: 10000,
        },
    };
    let grant = SourceGrant {
        request: source,
        id: "grant-binding-123456".into(),
        account_id: "account:1".into(),
        snapshot_id: "snapshot:1".into(),
        expires_at: "2040-01-01T00:00:00Z".into(),
    };
    let binding = SourceBinding {
        schema_version: 1,
        id: format!("binding:{}", grant.id),
        persona_id: "persona-a".into(),
        provider: Provider::Telegram,
        account_id: Some(grant.account_id.clone()),
        resource_id: grant.request.resource_id.clone(),
        grant_id: grant.id.clone(),
        policy_version: POLICY_VERSION.into(),
        role: SourceRole::Own,
        target_author_id: grant.request.author_id.clone(),
        allowed_context_author_ids: vec!["peer:2".into()],
        allow_unknown_context: true,
        period: BindingPeriod {
            from: None,
            to: None,
            include_undated: true,
        },
    };
    let document = SourceDocument {
        id: "document:1".into(),
        provider: Provider::Telegram,
        resource_id: "-12345".into(),
        author_id: Some("target:1".into()),
        author_label: Some("  Имя é 👩🏽‍🔬\r\n".into()),
        authorship: "owner".into(),
        kind: "message".into(),
        mode: "dialogue".into(),
        text: "  Слова\r\nhttps://example.test/a-b?x_y=1\t👩🏽‍🔬".into(),
        source_url: Some("https://example.test/a-b?x_y=1".into()),
        created_at: None,
        retrieved_at: None,
        content_hash: "unchanged-hash".into(),
        reply_to: None,
        is_forwarded: false,
        truncated: false,
        partition: "reference".into(),
    };
    (binding, grant, document)
}
#[test]
fn permits_selected_context_without_claiming_its_words_or_changing_text() {
    let (binding, grant, mut doc) = setup();
    doc.author_id = Some("peer:2".into());
    doc.authorship = "other".into();
    let projected = project(&binding, &grant, &doc, vec![], 0).unwrap();
    assert_eq!(projected.speaker_relation, SpeakerRelation::Context);
    assert!(!projected.can_support_own_words);
    assert_eq!(
        serde_json::to_value(&projected.document).unwrap(),
        serde_json::to_value(&doc).unwrap()
    );
    let value = serde_json::to_value(projected).unwrap();
    assert!(value.get("grantId").is_none());
    assert!(value.get("policyVersion").is_none());
}
#[test]
fn unknown_author_remains_unknown_and_is_only_explicit_context() {
    let (mut binding, grant, mut doc) = setup();
    doc.author_id = None;
    doc.authorship = "unknown".into();
    let projected = project(&binding, &grant, &doc, vec![], 0).unwrap();
    assert_eq!(projected.speaker_relation, SpeakerRelation::Unknown);
    assert!(!projected.can_support_own_words);
    binding.allow_unknown_context = false;
    assert!(project(&binding, &grant, &doc, vec![], 0).is_err());
}
#[test]
fn never_merges_people_by_identical_label_or_allows_unselected_author() {
    let (binding, grant, mut doc) = setup();
    doc.author_id = Some("foreign:3".into());
    assert!(project(&binding, &grant, &doc, vec![], 0).is_err());
}
#[test]
fn other_or_unknown_authorship_is_not_upgraded_even_with_matching_target_id() {
    let (binding, grant, mut doc) = setup();
    for value in ["other", "unknown"] {
        doc.authorship = value.into();
        assert!(
            !project(&binding, &grant, &doc, vec![], 0)
                .unwrap()
                .can_support_own_words
        );
    }
}
#[test]
fn non_own_roles_never_turn_matching_author_words_into_persona_evidence() {
    let (mut binding, grant, doc) = setup();
    for role in [
        SourceRole::Context,
        SourceRole::About,
        SourceRole::Reference,
    ] {
        binding.role = role;
        assert!(
            !project(&binding, &grant, &doc, vec![], 0)
                .unwrap()
                .can_support_own_words
        );
    }
}
#[test]
fn forwards_and_quotes_keep_original_attribution_unknown_when_not_available() {
    let (binding, grant, mut doc) = setup();
    doc.is_forwarded = true;
    assert!(
        !project(&binding, &grant, &doc, vec![], 0)
            .unwrap()
            .can_support_own_words
    );
    doc.is_forwarded = false;
    for kind in [RelationKind::Forward, RelationKind::Quote] {
        let relation = DocumentRelation {
            kind,
            original: None,
            original_author: SourceAuthor {
                provider: None,
                id: None,
                label: None,
            },
            span: None,
        };
        let projected = project(&binding, &grant, &doc, vec![relation], 0).unwrap();
        assert!(!projected.can_support_own_words);
        assert!(projected.relations[0].original_author.id.is_none());
    }
}
#[test]
fn rejects_foreign_resources_providers_holdout_or_mismatched_saved_scope() {
    let (binding, grant, doc) = setup();
    let mut foreign = doc.clone();
    foreign.resource_id = "other".into();
    assert!(project(&binding, &grant, &foreign, vec![], 0).is_err());
    foreign = doc.clone();
    foreign.provider = Provider::Vk;
    assert!(project(&binding, &grant, &foreign, vec![], 0).is_err());
    foreign = doc;
    foreign.partition = "holdout".into();
    assert!(project(&binding, &grant, &foreign, vec![], 0).is_err());
    for key in [
        "id",
        "grantId",
        "accountId",
        "resourceId",
        "policyVersion",
        "targetAuthorId",
    ] {
        let mut raw = serde_json::to_value(&binding).unwrap();
        raw[key] = json!("foreign");
        let bad = serde_json::from_value::<SourceBinding>(raw).unwrap();
        assert!(validate_authority(&bad, &grant, 0).is_err(), "{key}");
    }
}
#[test]
fn permission_and_expiry_are_checked_even_for_already_saved_documents() {
    let (binding, mut grant, doc) = setup();
    grant.request.allow_model_processing = false;
    assert!(project(&binding, &grant, &doc, vec![], 0).is_err());
    grant.request.allow_model_processing = true;
    grant.expires_at = "1970-01-01T00:00:00Z".into();
    assert!(project(&binding, &grant, &doc, vec![], 1).is_err());
}
#[test]
fn explicit_period_controls_undated_and_out_of_range_context() {
    let (mut binding, mut grant, mut doc) = setup();
    binding.period.from = Some("2024-01-01T00:00:00Z".into());
    grant.request.from = binding.period.from.clone();
    binding.period.to = Some("2024-12-31T23:59:59Z".into());
    grant.request.to = binding.period.to.clone();
    binding.period.include_undated = false;
    assert!(project(&binding, &grant, &doc, vec![], 0).is_err());
    doc.created_at = Some("2023-12-31T23:59:59Z".into());
    assert!(project(&binding, &grant, &doc, vec![], 0).is_err());
    doc.created_at = Some("2024-01-01T00:00:00Z".into());
    assert!(project(&binding, &grant, &doc, vec![], 0).is_ok());
    binding.period.include_undated = true;
    doc.created_at = None;
    assert!(project(&binding, &grant, &doc, vec![], 0).is_ok());
}
#[test]
fn selection_requires_explicit_target_and_bounded_distinct_context_ids() {
    let (binding, grant, _) = setup();
    let mut request = SourceBindingRequest {
        source: grant.request,
        persona_id: binding.persona_id,
        role: binding.role,
        allowed_context_author_ids: vec!["peer:2".into()],
        allow_unknown_context: false,
        include_undated: true,
    };
    assert!(validate_selection(&request).is_ok());
    request.source.author_id = None;
    assert!(validate_selection(&request).is_err());
    request.source.author_id = Some("target:1".into());
    for ids in [
        vec!["peer:2".into(), "peer:2".into()],
        vec!["target:1".into()],
        vec!["".into()],
        vec!["a".repeat(129)],
        (0..129).map(|n| format!("peer:{n}")).collect(),
    ] {
        request.allowed_context_author_ids = ids;
        assert!(validate_selection(&request).is_err());
    }
}
#[test]
fn legacy_instagram_display_labels_remain_raw_metadata_without_identity_authority() {
    let (mut binding, mut grant, mut doc) = setup();
    binding.provider = Provider::Instagram;
    grant.request.provider = Provider::Instagram;
    doc.provider = Provider::Instagram;
    binding.resource_id = "import:archive".into();
    grant.request.resource_id = binding.resource_id.clone();
    doc.resource_id = binding.resource_id.clone();
    binding.target_author_id = None;
    grant.request.author_id = None;
    binding.allowed_context_author_ids.clear();
    binding.role = SourceRole::Context;
    doc.author_id = Some("  Display Name  ".into());
    doc.authorship = "owner".into();
    let projected = project(&binding, &grant, &doc, vec![], 0).unwrap();
    assert_eq!(projected.speaker_relation, SpeakerRelation::Unknown);
    assert!(!projected.can_support_own_words);
    assert_eq!(projected.document.author_id, doc.author_id);
    assert_eq!(projected.document.authorship, "owner");
    binding.allow_unknown_context = false;
    assert!(project(&binding, &grant, &doc, vec![], 0).is_err());
    binding.allow_unknown_context = true;
    binding.allowed_context_author_ids = vec!["  Display Name  ".into()];
    assert!(project(&binding, &grant, &doc, vec![], 0).is_err());
}
