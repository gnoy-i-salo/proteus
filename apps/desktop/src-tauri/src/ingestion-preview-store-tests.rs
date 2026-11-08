use super::*;
use crate::corpus_database::{RecordInput, RecordNamespace};
fn value() -> StoredPreview {
    let v = StoredPreview {
        schema_version: 1,
        id: "preview:one".into(),
        origin: None,
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
            0 => v.schema_version = 4,
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
            0 => json["schemaVersion"] = serde_json::json!(4),
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
#[test]
fn literal_v2_preview_keeps_exact_bytes_and_generation_when_read_or_retried() {
    let (mut db, dir) = setup();
    let raw = include_str!("fixtures/ingestion-preview-v2.json");
    let legacy: StoredPreview = serde_json::from_str(raw).unwrap();
    assert_eq!(legacy.schema_version, 2);
    assert!(legacy.origin.is_none());
    db.save_ingestion(&RecordInput {
        id: legacy.id.clone(),
        kind: "import".into(),
        provider: legacy.provider.clone(),
        state_json: raw.into(),
        documents: vec![],
    })
    .unwrap();
    let before = db.read(&legacy.id).unwrap().unwrap();
    assert_eq!(read_preview(&db, &legacy.id).unwrap(), legacy);
    save_preview(&mut db, &legacy).unwrap();
    assert_eq!(db.read(&legacy.id).unwrap().unwrap(), before);
    assert!(!serde_json::to_value(&legacy)
        .unwrap()
        .as_object()
        .unwrap()
        .contains_key("origin"));
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn page_metadata_rejects_mismatched_origin_format_identity_and_invented_authors() {
    let (mut db, dir) = setup();
    for case in 0..7 {
        let mut page = page_value();
        match case {
            0 => page.origin = None,
            1 => page.provider = "telegram".into(),
            2 => page.source_format = "unknown".into(),
            3 => page.adapter_version = "future".into(),
            4 => page.resources[0].author_ids.push("invented".into()),
            5 => page.resources[0].id = "other-page".into(),
            _ => page.resources[0].estimated_documents = Some(2),
        }
        page.id = preview_id(&page).unwrap();
        assert!(save_preview(&mut db, &page).is_err(), "case {case}");
        assert!(db.read(&page.id).unwrap().is_none());
    }
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

fn page_value() -> StoredPreview {
    let origin = PageOrigin::SavedPage {
        source_origin: format!("localArchivePath:1:sha256:{}", "a".repeat(64)),
        label: "  Сохранённое имя 🦉\r\n.txt".into(),
    };
    let doc = crate::ingestion::page_document::normalize(
        origin.clone(),
        "text/plain".into(),
        "abc".into(),
        &mut || Ok(()),
    )
    .unwrap();
    issue_page_preview(
        origin,
        "savedPageText".into(),
        crate::ingestion::page_preview::ADAPTER_VERSION.into(),
        InputFingerprint {
            sha256: doc.sha256,
            byte_length: 3,
        },
        vec![PreviewResource {
            id: doc.source_key,
            label: "  Сохранённое имя 🦉\r\n.txt".into(),
            author_ids: vec![],
            estimated_documents: Some(1),
        }],
    )
    .unwrap()
}
#[test]
fn page_preview_v3_retains_native_origin_and_original_label_after_restart() {
    let (mut db, dir) = setup();
    let page = page_value();
    assert_eq!(page.schema_version, 3);
    assert!(matches!(page.origin, Some(PageOrigin::SavedPage { .. })));
    save_preview(&mut db, &page).unwrap();
    let before = db.read(&page.id).unwrap().unwrap();
    drop(db);
    let mut db = CorpusDatabase::open(&dir.join("corpus.db"), &[32; 32]).unwrap();
    assert_eq!(read_preview(&db, &page.id).unwrap(), page);
    save_preview(&mut db, &page).unwrap();
    assert_eq!(db.read(&page.id).unwrap().unwrap(), before);
    assert!(before.input.documents.is_empty());
    assert!(!before.input.state_json.contains("source_origin"));
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn legacy_preview_rejects_even_null_origin_without_modifying_its_record() {
    let (mut db, dir) = setup();
    let old = value();
    let mut raw = serde_json::to_value(&old).unwrap();
    raw["origin"] = serde_json::Value::Null;
    let input = RecordInput {
        id: old.id.clone(),
        kind: "import".into(),
        provider: old.provider.clone(),
        state_json: raw.to_string(),
        documents: vec![],
    };
    db.save_ingestion(&input).unwrap();
    assert!(read_preview(&db, &old.id).is_err());
    assert_eq!(db.read(&old.id).unwrap().unwrap().input, input);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn page_origin_is_part_of_receipt_identity_and_cannot_be_injected_into_legacy_records() {
    let (mut db, dir) = setup();
    let page = page_value();
    save_preview(&mut db, &page).unwrap();
    let mut tampered = page.clone();
    tampered.origin = Some(PageOrigin::SavedPage {
        source_origin: format!("localArchivePath:1:sha256:{}", "b".repeat(64)),
        label: "forged".into(),
    });
    assert_ne!(preview_id(&tampered).unwrap(), page.id);
    assert!(save_preview(&mut db, &tampered).is_err());
    assert_eq!(read_preview(&db, &page.id).unwrap(), page);
    let mut old = value();
    old.origin = tampered.origin;
    old.id = preview_id(&old).unwrap();
    assert!(save_preview(&mut db, &old).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
