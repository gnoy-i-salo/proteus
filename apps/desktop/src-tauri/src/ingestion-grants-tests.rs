use super::*;
use crate::corpus_database::RecordNamespace;
use crate::ingestion::{
    fingerprint::InputFingerprint,
    preview::PreviewResource,
    preview_store::{issue_preview, save_preview, StoredPreview},
};
fn setup() -> (CorpusDatabase, std::path::PathBuf, BindingRequest) {
    let dir =
        std::env::temp_dir().join(format!("proteus-ingestion-grant-{}", rand::random::<u64>()));
    let mut db = CorpusDatabase::open(&dir.join("corpus.db"), &[33; 32]).unwrap();
    let p = StoredPreview {
        schema_version: 1,
        id: String::new(),
        provider: "telegram".into(),
        source_format: "telegramDesktopJson".into(),
        adapter_version: "telegramDesktopJson:1".into(),
        fingerprint: InputFingerprint {
            sha256: "a".repeat(64),
            byte_length: 1000,
        },
        resources: vec![PreviewResource {
            id: "chat:1".into(),
            label: "  Имя é\r\n".into(),
            author_ids: vec!["author".into(), "friend".into()],
            estimated_documents: Some(2),
        }],
    };
    let p = issue_preview(
        p.provider,
        p.source_format,
        p.adapter_version,
        p.fingerprint,
        p.resources,
    )
    .unwrap();
    save_preview(&mut db, &p).unwrap();
    let r = BindingRequest {
        preview_id: p.id,
        resource_id: "chat:1".into(),
        persona_id: "personA".into(),
        role: JobRole::Own,
        target_author_id: Some("author".into()),
        allowed_context_author_ids: vec!["friend".into()],
        allow_unknown_context: false,
        period: JobPeriod {
            from: None,
            to: None,
            include_undated: true,
        },
        allow_model_processing: false,
        allow_export: false,
    };
    (db, dir, r)
}
#[test]
fn binding_authority_is_issued_by_native_and_permissions_remain_independent() {
    for model in [true, false] {
        for export in [true, false] {
            let (mut db, dir, mut request) = setup();
            request.allow_model_processing = model;
            request.allow_export = export;
            let b = create_binding(&mut db, request.clone(), 1000).unwrap();
            assert_eq!(b.persona_id, request.persona_id);
            assert_eq!(b.resource_id, request.resource_id);
            assert_eq!(b.target_author_id, request.target_author_id);
            assert_eq!(b.account_id, None);
            assert_eq!(b.schema_version, 1);
            assert!(!b.id.is_empty());
            assert!(!b.grant_id.is_empty());
            assert_eq!(
                db.read(&b.grant_id).unwrap().unwrap().namespace,
                RecordNamespace::Ingestion
            );
            assert!(authorize_binding(&db, &b, GrantUse::Corpus, 1001).is_ok());
            assert_eq!(
                authorize_binding(&db, &b, GrantUse::Model, 1001).is_ok(),
                model
            );
            assert_eq!(
                authorize_binding(&db, &b, GrantUse::Export, 1001).is_ok(),
                export
            );
            drop(db);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
#[test]
fn binds_only_resources_and_known_authors_in_the_verified_preview() {
    for kind in 0..6 {
        let (mut db, dir, mut r) = setup();
        match kind {
            0 => r.resource_id = "foreign".into(),
            1 => r.target_author_id = Some("outsider".into()),
            2 => r.allowed_context_author_ids = vec!["outsider".into()],
            3 => r.target_author_id = None,
            4 => r.persona_id = "\n".into(),
            _ => r.period.from = Some("not a date".into()),
        };
        assert!(create_binding(&mut db, r, 1000).is_err());
        assert_eq!(
            db.ids_in_namespace(RecordNamespace::Ingestion)
                .unwrap()
                .len(),
            1
        );
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn omitted_unknown_author_is_allowed_only_for_explicit_non_own_role() {
    let (mut db, dir, mut r) = setup();
    r.target_author_id = None;
    r.role = JobRole::Reference;
    r.allow_unknown_context = true;
    let b = create_binding(&mut db, r, 1000).unwrap();
    assert_eq!(b.target_author_id, None);
    assert!(b.allow_unknown_context);
    assert_eq!(b.role, JobRole::Reference);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn every_read_rechecks_exact_binding_expiry_and_revocation_after_restart() {
    let (mut db, dir, r) = setup();
    let b = create_binding(&mut db, r, 1000).unwrap();
    drop(db);
    let mut db = CorpusDatabase::open(&dir.join("corpus.db"), &[33; 32]).unwrap();
    assert!(authorize_binding(&db, &b, GrantUse::Corpus, 1001).is_ok());
    for kind in 0..4 {
        let mut fake = b.clone();
        match kind {
            0 => fake.persona_id = "personB".into(),
            1 => fake.resource_id = "chat:2".into(),
            2 => fake.policy_version = "future".into(),
            _ => fake.allowed_context_author_ids.clear(),
        };
        assert!(authorize_binding(&db, &fake, GrantUse::Corpus, 1001).is_err());
    }
    assert!(authorize_binding(&db, &b, GrantUse::Corpus, 1000 + 86_400_000).is_err());
    db.remove(&b.grant_id).unwrap();
    assert!(authorize_binding(&db, &b, GrantUse::Corpus, 1001).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn caller_cannot_inject_native_authority_or_indefinite_lifetime_into_request() {
    let (db, dir, r) = setup();
    for key in [
        "id",
        "grantId",
        "provider",
        "accountId",
        "policyVersion",
        "expiresAt",
    ] {
        let mut value = serde_json::to_value(&r).unwrap();
        value[key] = serde_json::json!("forged");
        assert!(serde_json::from_value::<BindingRequest>(value).is_err());
    }
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
