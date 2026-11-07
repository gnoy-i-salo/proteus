use super::*;
use crate::corpus_database::{RecordInput, RecordNamespace};
fn value() -> StoredPreview {
    let v = StoredPreview {
        schema_version: 1,
        id: "preview:one".into(),
        provider: "telegram".into(),
        source_format: "telegramDesktopJson".into(),
        adapter_version: "telegramDesktopJson:1".into(),
        fingerprint: InputFingerprint {
            sha256: "a".repeat(64),
            byte_length: 500,
        },
        resources: vec![PreviewResource {
            id: "telegram:chat:-1".into(),
            label: "  Чат é\r\n".into(),
            author_ids: vec!["user1".into()],
            estimated_documents: Some(1),
        }],
    };
    issue_preview(
        v.provider,
        v.source_format,
        v.adapter_version,
        v.fingerprint,
        v.resources,
    )
    .unwrap()
}
fn setup() -> (CorpusDatabase, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("proteus-preview-store-{}", rand::random::<u64>()));
    (
        CorpusDatabase::open(&dir.join("corpus.db"), &[32; 32]).unwrap(),
        dir,
    )
}
#[test]
fn preview_survives_restart_with_exact_metadata_in_encrypted_owner_namespace() {
    let (mut db, dir) = setup();
    let v = value();
    save_preview(&mut db, &v).unwrap();
    assert_eq!(
        db.read(&v.id).unwrap().unwrap().namespace,
        RecordNamespace::Ingestion
    );
    drop(db);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[32; 32]).unwrap();
    assert_eq!(read_preview(&db, &v.id).unwrap(), v);
    drop(db);
    assert!(!std::fs::read(dir.join("corpus.db"))
        .unwrap()
        .windows("Чат".len())
        .any(|w| w == "Чат".as_bytes()));
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn preview_id_is_immutable_and_retries_do_not_create_generations() {
    let (mut db, dir) = setup();
    let v = value();
    save_preview(&mut db, &v).unwrap();
    let before = db.read(&v.id).unwrap().unwrap();
    save_preview(&mut db, &v).unwrap();
    assert_eq!(db.read(&v.id).unwrap().unwrap(), before);
    let mut changed = v.clone();
    changed.fingerprint.sha256 = "b".repeat(64);
    assert!(save_preview(&mut db, &changed).is_err());
    assert_eq!(read_preview(&db, &v.id).unwrap(), v);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn deleted_previews_and_other_codecs_never_become_valid_previews() {
    let (mut db, dir) = setup();
    let v = value();
    let input = RecordInput {
        id: v.id.clone(),
        kind: "import".into(),
        provider: v.provider.clone(),
        state_json: serde_json::to_string(&v).unwrap(),
        documents: vec![],
    };
    db.save(&input).unwrap();
    assert!(read_preview(&db, &v.id).is_err());
    assert!(save_preview(&mut db, &v).is_err());
    let mut own = v.clone();
    own.resources[0].label = "second".into();
    own.id = preview_id(&own).unwrap();
    save_preview(&mut db, &own).unwrap();
    db.remove(&own.id).unwrap();
    assert!(read_preview(&db, &own.id).is_err());
    assert!(save_preview(&mut db, &own).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn malformed_versions_fingerprints_and_resource_author_metadata_fail_without_writes() {
    let (mut db, dir) = setup();
    for kind in 0..7 {
        let mut v = value();
        match kind {
            0 => v.schema_version = 3,
            1 => v.fingerprint.sha256 = "notHash".into(),
            2 => v.fingerprint.byte_length = 9_007_199_254_740_992,
            3 => v.resources.push(v.resources[0].clone()),
            4 => v.resources[0].author_ids.push("user1".into()),
            5 => v.resources[0].estimated_documents = Some(9_007_199_254_740_992),
            _ => v.provider = "unknown".into(),
        };
        assert!(save_preview(&mut db, &v).is_err());
        assert!(db.read(&v.id).unwrap().is_none());
    }
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn stored_codec_rejects_unknown_versions_fields_and_mismatched_provider_without_overwriting() {
    for mutation in 0..3 {
        let (mut db, dir) = setup();
        let v = value();
        let mut json = serde_json::to_value(&v).unwrap();
        match mutation {
            0 => json["schemaVersion"] = serde_json::json!(3),
            1 => json["sourceHandle"] = serde_json::json!("not-persisted"),
            _ => json["provider"] = serde_json::json!("vk"),
        };
        let input = RecordInput {
            id: v.id.clone(),
            kind: "import".into(),
            provider: "telegram".into(),
            state_json: json.to_string(),
            documents: vec![],
        };
        db.save_ingestion(&input).unwrap();
        assert!(read_preview(&db, &v.id).is_err());
        assert!(save_preview(&mut db, &v).is_err());
        assert_eq!(db.read(&v.id).unwrap().unwrap().input, input);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn persisted_metadata_uses_only_versioned_camel_case_and_never_a_filesystem_path() {
    let v = value();
    let json = serde_json::to_value(&v).unwrap();
    assert_eq!(json["fingerprint"]["byteLength"], 500);
    assert!(json["fingerprint"].get("byte_length").is_none());
    assert!(json.get("sourceHandle").is_none());
    assert!(json.get("path").is_none());
    assert_eq!(serde_json::from_value::<StoredPreview>(json).unwrap(), v);
}
#[test]
fn encoding_refuses_over_budget_before_retaining_oversized_bytes() {
    let mut writer = CappedBytes(vec![]);
    assert!(writer.write(&vec![0; MAX_BYTES + 1]).is_err());
    assert!(writer.0.is_empty());
    let oversized = "\n".repeat(MAX_BYTES / 6 + 1);
    assert!(encode(&oversized).is_ok());
    // JSON control escaping expands one input byte to six persisted bytes.
    let escaped = "\u{0001}".repeat(MAX_BYTES / 6 + 1);
    assert!(encode(&escaped).is_err());
}

fn issued() -> StoredPreview {
    let v = value();
    issue_preview(
        v.provider,
        v.source_format,
        v.adapter_version,
        v.fingerprint,
        v.resources,
    )
    .unwrap()
}
#[test]
fn an_explicit_new_selection_can_import_again_without_reactivating_a_revoked_preview() {
    let (mut db, dir) = setup();
    let first = issued();
    assert_eq!(first.schema_version, 2);
    save_preview(&mut db, &first).unwrap();
    db.remove(&first.id).unwrap();
    let second = issued();
    assert_ne!(first.id, second.id);
    assert_eq!(first.fingerprint, second.fingerprint);
    assert_eq!(first.resources, second.resources);
    save_preview(&mut db, &second).unwrap();
    assert!(read_preview(&db, &first.id).is_err());
    assert_eq!(read_preview(&db, &second.id).unwrap(), second);
    assert_eq!(db.read(&second.id).unwrap().unwrap().generation, 1);
    save_preview(&mut db, &second).unwrap();
    assert_eq!(db.read(&second.id).unwrap().unwrap().generation, 1);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn legacy_preview_is_read_only_and_its_exact_saved_bytes_are_not_migrated_in_place() {
    let (mut db, dir) = setup();
    let raw = include_str!("fixtures/ingestion-preview-v1.json");
    let legacy: StoredPreview = serde_json::from_str(raw).unwrap();
    let input = RecordInput {
        id: legacy.id.clone(),
        kind: "import".into(),
        provider: legacy.provider.clone(),
        state_json: raw.into(),
        documents: vec![],
    };
    db.save_ingestion(&input).unwrap();
    let before = db.read(&legacy.id).unwrap().unwrap();
    assert_eq!(read_preview(&db, &legacy.id).unwrap(), legacy);
    assert!(save_preview(&mut db, &legacy).is_err());
    assert_eq!(db.read(&legacy.id).unwrap().unwrap(), before);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
