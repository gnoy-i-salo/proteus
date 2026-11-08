use super::*;
use crate::ingestion::search_workflow::{self as workflow, IndexRequest, QueryRequest};
fn index_value() -> serde_json::Value {
    serde_json::json!({"schemaVersion":1,"operationId":"job","jobId":"job","snapshotId":"snapshot","maxDurationMs":60000})
}
fn query_value() -> serde_json::Value {
    serde_json::json!({"schemaVersion":1,"operationId":"job","jobId":"job","snapshotId":"snapshot","personaId":"personA","queryText":"  контекст\r\n",
        "period":{"from":null,"to":null,"includeUndated":false},"limit":10,"offset":0,"expectedIndexGeneration":null,"maxDurationMs":60000})
}
#[test]
fn search_wire_preserves_exact_query_and_rejects_scope_injection_and_missing_nulls() {
    let value = query_value();
    let request: QueryRequest = serde_json::from_value(value.clone()).unwrap();
    assert!(request.validate().is_ok());
    assert_eq!(request.query_text, "  контекст\r\n");
    for field in ["grantIds", "bindingIds", "text", "path", "query_text"] {
        let mut invalid = value.clone();
        invalid[field] = serde_json::json!("injected");
        assert!(serde_json::from_value::<QueryRequest>(invalid).is_err());
    }
    for field in ["from", "to"] {
        let mut invalid = value.clone();
        invalid["period"].as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<QueryRequest>(invalid).is_err());
    }
    let mut invalid = value.clone();
    invalid
        .as_object_mut()
        .unwrap()
        .remove("expectedIndexGeneration");
    assert!(serde_json::from_value::<QueryRequest>(invalid).is_err());
    for (field, invalid_value) in [
        ("schemaVersion", serde_json::json!(2)),
        ("operationId", serde_json::json!("")),
        ("operationId", serde_json::json!("я".repeat(65))),
        ("jobId", serde_json::json!("\n")),
        ("limit", serde_json::json!(0)),
        ("limit", serde_json::json!(101)),
        ("offset", serde_json::json!(1)),
        ("offset", serde_json::json!(10001)),
        ("queryText", serde_json::json!("")),
        ("queryText", serde_json::json!("я".repeat(2049))),
        ("maxDurationMs", serde_json::json!(60001)),
        (
            "expectedIndexGeneration",
            serde_json::json!(9007199254740992i64),
        ),
    ] {
        let mut invalid = value.clone();
        invalid[field] = invalid_value;
        assert!(
            serde_json::from_value::<QueryRequest>(invalid)
                .unwrap()
                .validate()
                .is_err(),
            "{field}"
        );
    }
    let duplicate =
        serde_json::to_string(&value)
            .unwrap()
            .replacen("{", r#"{"schemaVersion":1,"#, 1);
    let error = serde_json::from_str::<QueryRequest>(&duplicate)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("duplicate field"));
}
#[test]
fn index_wire_accepts_only_bounded_versioned_native_identifiers() {
    let value = index_value();
    assert!(serde_json::from_value::<IndexRequest>(value.clone())
        .unwrap()
        .validate()
        .is_ok());
    for (field, invalid_value) in [
        ("schemaVersion", serde_json::json!(0)),
        ("operationId", serde_json::json!("\t")),
        ("jobId", serde_json::json!("я".repeat(65))),
        ("snapshotId", serde_json::json!("я".repeat(129))),
        ("maxDurationMs", serde_json::json!(0)),
        ("maxDurationMs", serde_json::json!(3600001)),
    ] {
        let mut invalid = value.clone();
        invalid[field] = invalid_value;
        assert!(serde_json::from_value::<IndexRequest>(invalid)
            .unwrap()
            .validate()
            .is_err());
    }
    for field in ["chunks", "grantIds", "policyFingerprint", "tokenizerPath"] {
        let mut invalid = value.clone();
        invalid[field] = serde_json::json!("injected");
        assert!(serde_json::from_value::<IndexRequest>(invalid).is_err());
    }
}
#[test]
fn workflow_uses_authorized_index_and_query_without_accepting_model_authority() {
    let (sources, dir) = local();
    let (operation, snapshot) = prepared(&sources, &dir, 0);
    let mut index: IndexRequest = serde_json::from_value(index_value()).unwrap();
    index.snapshot_id = snapshot.id.clone();
    let receipt = workflow::rebuild(&sources, &operation, &index, || 70001).unwrap();
    let encoded = serde_json::to_value(&receipt).unwrap();
    assert_eq!(encoded["schemaVersion"], 1);
    assert_eq!(encoded["snapshotId"], snapshot.id);
    assert!(encoded.get("index").is_none());
    let mut query: QueryRequest = serde_json::from_value(query_value()).unwrap();
    query.snapshot_id = snapshot.id;
    let result = workflow::query(&sources, &operation, &query, || 70002).unwrap();
    let encoded = serde_json::to_value(&result).unwrap();
    assert_eq!(encoded["schemaVersion"], 1);
    assert_eq!(encoded["indexGeneration"], receipt.index.index_generation);
    assert_eq!(result.result.hits.len(), 1);
    assert!(encoded["hits"][0].get("text").is_none());
    query.persona_id = "foreign-person".into();
    assert!(workflow::query(&sources, &operation, &query, || 70002).is_err());
    query.persona_id = "personA".into();
    query.operation_id = "foreign-operation".into();
    assert!(workflow::query(&sources, &operation, &query, || 70002).is_err());
    index.operation_id = "foreign-operation".into();
    assert!(workflow::rebuild(&sources, &operation, &index, || 70002).is_err());
    let other = sources
        .begin_ingestion_operation("other", JobProvider::SavedPage)
        .unwrap();
    query.operation_id = "other".into();
    assert!(workflow::query(&sources, &other, &query, || 70002).is_err());
    drop(other);
    query.operation_id = "job".into();
    index.operation_id = "job".into();
    sources.cancel_ingestion_operation("job").unwrap();
    assert!(workflow::query(&sources, &operation, &query, || 70002).is_err());
    assert!(workflow::rebuild(&sources, &operation, &index, || 70002).is_err());
    drop(operation);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
