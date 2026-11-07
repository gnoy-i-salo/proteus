use super::*;
use crate::connections::storage::{random_id, MemorySecrets};
use serde_json::json;
#[test]
fn source_service_cannot_enumerate_or_decode_ingestion_owned_records() {
    let (store, path) = store();
    let record = Record::decode_legacy(legacy()).unwrap();
    store
        .with_database(path.parent().unwrap(), |database| {
            let input = database_input(record_id(&path)?, encode(&record)?, None, database)?;
            database.save_ingestion(&input)?;
            Ok(())
        })
        .unwrap();
    assert!(store.ids(path.parent().unwrap()).unwrap().is_empty());
    assert!(store.read::<Record>(&path).is_err());
    assert!(store.write(&path, &record).is_err());
    assert!(store.owner(&path).is_err());
    drop(store);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
#[test]
fn upgrading_v2_keeps_ciphertext_and_publishes_sql_generation() {
    let (store, path) = store();
    let record = Record::decode_legacy(legacy()).unwrap();
    let mut v2 = encode(&record).unwrap();
    v2["version"] = json!(2);
    v2["payload"].as_object_mut().unwrap().remove("binding");
    let previous = previous_path(&path).unwrap();
    store.legacy.write(&previous, &v2).unwrap();
    let original = std::fs::read(&previous).unwrap();
    let restored: Record = store.read(&path).unwrap();
    assert!(restored.binding.is_none());
    store.write(&path, &restored).unwrap();
    assert_eq!(std::fs::read(&previous).unwrap(), original);
    assert!(path.parent().unwrap().join("metadata.db").exists());
    assert!(!current_path(&path).unwrap().exists());
    store
        .with_database(path.parent().unwrap(), |database| {
            let audit = database.audit(record_id(&path)?)?.unwrap();
            assert_eq!(audit.source_generation, 2);
            assert_eq!(audit.document_count, 1);
            let saved = database.read(record_id(&path)?)?.unwrap();
            let provenance = &saved.input.documents[0].provenance;
            assert_eq!(
                provenance,
                &crate::corpus_database::SourceProvenance::legacy(
                    crate::corpus_database::Acquisition::Api,
                    2
                )?
            );
            assert_eq!(
                audit.canonical_sha256,
                crate::corpus_database::record_hash(&saved.input)
            );
            Ok(())
        })
        .unwrap();
    store.remove(&path).unwrap();
    assert!(!previous.exists());
    assert!(store.is_deleted(&path).unwrap());
    assert!(store.read::<Record>(&path).is_err());
    drop(store);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
#[test]
fn generation_three_requires_explicit_binding_and_never_falls_back_to_v2() {
    let (store, path) = store();
    let record = Record::decode_legacy(legacy()).unwrap();
    let mut value = encode(&record).unwrap();
    value["payload"].as_object_mut().unwrap().remove("binding");
    assert!(current::<Record>(value.clone()).is_err());
    value["version"] = json!(2);
    store
        .legacy
        .write(&previous_path(&path).unwrap(), &value)
        .unwrap();
    assert!(current::<Record>(value.clone()).is_ok());
    value["payload"]["binding"] = Value::Null;
    assert!(current::<Record>(value).is_err());
    std::fs::create_dir_all(current_path(&path).unwrap().parent().unwrap()).unwrap();
    std::fs::write(current_path(&path).unwrap(), b"corrupt").unwrap();
    assert!(store.read::<Record>(&path).is_err());
    std::fs::remove_file(current_path(&path).unwrap()).unwrap();
    assert!(store.read::<Record>(&path).is_ok());
    drop(store);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
#[test]
fn legacy_source_read_migrates_to_one_sqlcipher_database_without_replacing_ciphertext() {
    let (store, path) = store();
    store.legacy.write(&path, &legacy()).unwrap();
    let original = std::fs::read(&path).unwrap();
    let record: Record = store.read(&path).unwrap();
    assert_eq!(record.documents[0].text, "\r\n  Ответ é 😀\t");
    assert!(
        path.parent().unwrap().join("metadata.db").is_file(),
        "canonical SQLCipher database must own the migrated record"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    drop(store);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
fn legacy() -> Value {
    let text = "\r\n  Ответ é 😀\t";
    json!({"version":1,"grant":{"provider":"telegram","resource-id":"chat:1","author-id":"a:1","from":null,"to":null,
            "allow-model-processing":true,"operations":["read"],"budget":{"max-steps":12,"max-documents":20,"max-bytes":1048576,"max-duration-ms":12000},
            "id":"grant-fixture-1234","account-id":"account:1","snapshot-id":"snap:1","expires-at":"2030-01-01T00:00:00Z"},
            "documents":[{"id":"d:1","provider":"telegram","resource-id":"chat:1","author-id":null,"author-label":"  Автор é  ",
            "authorship":"unknown","kind":"message","mode":"dialogue","text":text,"source-url":null,"created-at":null,"retrieved-at":"2025-01-01T00:00:00Z",
            "content-hash":"unchanged-hash","reply-to":"d:0","is-forwarded":true,"truncated":false,"partition":"reference"}],
            "coverage":{"documents":1,"total-documents":null,"bytes":text.len(),"complete":false,"truncated":false,"not-analyzed":2,"unavailable":["attachment"]},
            "cursor":"opaque==","checkpoint":{"phase":"messages","offset":23,"cursor":"a-b_c","index":4,
            "queue":[{"author_id":7,"reply-to":"raw-external","publicUrls":"opaque","nested":{"max-steps":900},"text":text}]},
            "steps":3,"started_at":12345,"cancelled":true,"limitations":["  unchanged\r\n"]})
}
fn store() -> (SourceStore, PathBuf) {
    let directory =
        std::env::temp_dir().join(format!("proteus-generation-{}", random_id(18).unwrap()));
    (
        SourceStore::new(Arc::new(MemorySecrets::default())),
        directory.join("grant-fixture-1234.enc"),
    )
}
#[test]
fn legacy_to_current_preserves_original_exact_metadata_and_opaque_queue() {
    let (store, path) = store();
    let old = legacy();
    store.legacy.write(&path, &old).unwrap();
    let original = std::fs::read(&path).unwrap();
    let record: Record = store.read(&path).unwrap();
    assert_eq!(record.documents[0].text, "\r\n  Ответ é 😀\t");
    assert_eq!(record.documents[0].authorship, "unknown");
    assert_eq!(record.documents[0].reply_to.as_deref(), Some("d:0"));
    assert_eq!(record.started_at, Some(12345));
    assert_eq!(record.checkpoint.queue[0], old["checkpoint"]["queue"][0]);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    store.write(&path, &record).unwrap();
    store.write(&path, &record).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let current: Value = store
        .with_database(path.parent().unwrap(), |database| {
            database_value(database.read(record_id(&path)?)?.unwrap())
        })
        .unwrap();
    assert_eq!(current["version"], 3);
    assert_eq!(current["kind"], "grant");
    assert_eq!(current["payload"]["startedAt"], 12345);
    assert!(current["payload"].get("started_at").is_none());
    let restored: Record = store.read(&path).unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(record).unwrap()
    );
    drop(store);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
#[test]
fn rejects_corrupt_unknown_or_wrong_kind_current_without_legacy_fallback() {
    let (store, path) = store();
    store.legacy.write(&path, &legacy()).unwrap();
    let record = Record::decode_legacy(legacy()).unwrap();
    let current_path = current_path(&path).unwrap();
    std::fs::create_dir_all(current_path.parent().unwrap()).unwrap();
    std::fs::write(&current_path, b"corrupt").unwrap();
    assert!(store.read::<Record>(&path).is_err());
    for field in ["version", "kind", "format"] {
        let mut value = encode(&record).unwrap();
        value[field] = json!("unknown");
        store.legacy.write(&current_path, &value).unwrap();
        assert!(store.read::<Record>(&path).is_err());
    }
    drop(store);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
#[test]
fn rejects_legacy_mixed_keys_and_unknowns_but_not_external_queue_names() {
    for pointer in [
        "/grant/resourceId",
        "/grant/budget/maxSteps",
        "/documents/0/authorId",
        "/coverage/totalDocuments",
        "/startedAt",
        "/unknown",
    ] {
        let mut value = legacy();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        value
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(key.into(), json!(null));
        assert!(Record::decode_legacy(value).is_err(), "{pointer}");
    }
    assert!(Record::decode_legacy(legacy()).is_ok());
    let mut value = legacy();
    value["version"] = json!(99);
    assert!(Record::decode_legacy(value).is_err());
}
#[test]
fn current_payload_rejects_unknown_and_legacy_nested_fields() {
    let record = Record::decode_legacy(legacy()).unwrap();
    for pointer in [
        "/payload/grant/resource-id",
        "/payload/grant/unknown",
        "/payload/documents/0/author-id",
        "/payload/coverage/not-analyzed",
        "/payload/checkpoint/unknown",
    ] {
        let mut value = encode(&record).unwrap();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        value
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(key.into(), json!(null));
        assert!(current::<Record>(value).is_err(), "{pointer}");
    }
}
#[test]
fn imported_records_have_distinct_kind_and_preserve_documents_without_grants() {
    let (store, path) = store();
    let value = legacy();
    let old =
        json!({"provider":"telegram","documents":value["documents"],"warnings":["  warning\r\n"]});
    store.legacy.write(&path, &old).unwrap();
    let record: ImportedRecord = store.read(&path).unwrap();
    store.write(&path, &record).unwrap();
    assert!(store.read::<Record>(&path).is_err());
    assert_eq!(store.owner(&path).unwrap(), Provider::Telegram);
    let restored: ImportedRecord = store.read(&path).unwrap();
    assert_eq!(restored.documents[0].text, "\r\n  Ответ é 😀\t");
    assert_eq!(restored.warnings, ["  warning\r\n"]);
    store
        .with_database(path.parent().unwrap(), |database| {
            let saved = database.read(record_id(&path)?)?.unwrap();
            assert_eq!(
                saved.input.documents[0].provenance,
                crate::corpus_database::SourceProvenance::legacy(
                    crate::corpus_database::Acquisition::Archive,
                    0
                )?
            );
            Ok(())
        })
        .unwrap();
    drop(store);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
#[test]
fn failed_sql_database_open_preserves_original_and_retry_is_safe() {
    let (store, path) = store();
    store.legacy.write(&path, &legacy()).unwrap();
    let original = std::fs::read(&path).unwrap();
    let record = Record::decode_legacy(legacy()).unwrap();
    let database = path.parent().unwrap().join("metadata.db");
    std::fs::write(&database, b"blocked database").unwrap();
    assert!(store.write(&path, &record).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let old: Value = store.legacy.read(&path).unwrap();
    assert!(Record::decode_legacy(old).is_ok());
    std::fs::remove_file(database).unwrap();
    assert!(store.read::<Record>(&path).is_ok());
    drop(store);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
#[test]
fn delete_removes_both_generations_and_interrupted_delete_stays_revoked() {
    let (store, path) = store();
    store.legacy.write(&path, &legacy()).unwrap();
    let record: Record = store.read(&path).unwrap();
    store.write(&path, &record).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(store.remove(&path).is_err());
    assert!(store.read::<Record>(&path).is_err());
    assert_eq!(store.owner(&path).unwrap(), Provider::Telegram);
    std::fs::remove_dir(&path).unwrap();
    store.legacy.write(&path, &legacy()).unwrap();
    store.remove(&path).unwrap();
    assert!(!path.exists());
    assert!(!current_path(&path).unwrap().exists());
    drop(store);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

mod sql_recovery_tests {

    use super::*;
    use crate::connections::storage::{random_id, MemorySecrets};
    #[test]
    fn corrupted_current_sql_never_falls_back_to_retained_legacy_ciphertext() {
        let secrets = Arc::new(MemorySecrets::default());
        let store = SourceStore::new(secrets.clone());
        let directory =
            std::env::temp_dir().join(format!("proteus-sql-recovery-{}", random_id(18).unwrap()));
        let path = directory.join("import-source-1234.enc");
        let old = serde_json::json!({"provider":"instagram","documents":[],"warnings":["legacy exact é\r\n"]});
        store.legacy.write(&path, &old).unwrap();
        let original = std::fs::read(&path).unwrap();
        let migrated: ImportedRecord = store.read(&path).unwrap();
        assert_eq!(migrated.warnings, ["legacy exact é\r\n"]);
        drop(store);
        std::fs::write(
            directory.join("metadata.db"),
            b"corrupt current SQL database",
        )
        .unwrap();
        let reopened = SourceStore::new(secrets);
        assert!(reopened.read::<ImportedRecord>(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        drop(reopened);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
