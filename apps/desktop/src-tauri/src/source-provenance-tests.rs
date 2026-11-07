use super::*;
use serde_json::{json, Value};

fn known() -> SourceProvenance {
    SourceProvenance {
        acquisition: Acquisition::Api,
        adapter_version: "  adapter é 😀\r\n  ".into(),
        source_format: "provider-object-v1".into(),
        original_object_id: Some("  exact ID é 😀\r\n  ".into()),
        original_byte_length: Some(111),
        hash_algorithm: HashAlgorithm::Sha256,
        limitations: vec![],
    }
}
fn validate(value: Value) -> Result<(), String> {
    let decoded: SourceProvenance =
        serde_json::from_value(value).map_err(|_| INVALID.to_string())?;
    decoded.validate()
}
#[test]
fn exact_strings_and_known_empty_attachment_metadata_survive_roundtrip() {
    let original = known();
    original.validate().unwrap();
    let encoded = serde_json::to_value(&original).unwrap();
    assert_eq!(encoded.as_object().unwrap().len(), 7);
    assert_eq!(
        encoded["originalObjectId"],
        original.original_object_id.as_ref().unwrap().as_str()
    );
    assert_eq!(encoded["adapterVersion"], original.adapter_version);
    let restored: SourceProvenance = serde_json::from_value(encoded).unwrap();
    assert_eq!(restored, original);
    restored.validate().unwrap();
}
#[test]
fn original_nullable_keys_are_required_not_implicit_none() {
    for field in ["originalObjectId", "originalByteLength"] {
        let mut value =
            serde_json::to_value(SourceProvenance::source_service(Acquisition::Api)).unwrap();
        value.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<SourceProvenance>(value).is_err(),
            "{field}"
        );
    }
}
#[test]
fn own_schema_rejects_unknown_and_legacy_alias_fields() {
    for field in ["original-object-id", "unexpected"] {
        let mut value = serde_json::to_value(known()).unwrap();
        value[field] = json!("unexpected");
        assert!(validate(value).is_err(), "{field}");
    }
}
#[test]
fn enums_and_required_metadata_are_exact() {
    for (field, bad) in [
        ("acquisition", json!("inferred")),
        ("hashAlgorithm", json!("md5")),
        ("adapterVersion", json!("")),
        ("sourceFormat", json!("")),
        ("adapterVersion", Value::Null),
    ] {
        let mut value = serde_json::to_value(known()).unwrap();
        value[field] = bad;
        assert!(validate(value).is_err(), "{field}");
    }
}
#[test]
fn null_original_id_requires_its_marker() {
    let mut value = known();
    value.original_object_id = None;
    assert!(value.validate().is_err());
}
#[test]
fn null_original_bytes_require_their_marker() {
    let mut value = known();
    value.original_byte_length = None;
    assert!(value.validate().is_err());
}
#[test]
fn known_original_id_rejects_unavailable_marker() {
    let mut value = known();
    value
        .limitations
        .push(ProvenanceLimitation::OriginalObjectIdUnavailable);
    assert!(value.validate().is_err());
}
#[test]
fn known_original_bytes_reject_unavailable_marker() {
    let mut value = known();
    value
        .limitations
        .push(ProvenanceLimitation::OriginalBytesUnavailable);
    assert!(value.validate().is_err());
}
#[test]
fn empty_original_id_is_invalid_without_normalizing_whitespace() {
    let mut value = known();
    value.original_object_id = Some(String::new());
    assert!(value.validate().is_err());
    value.original_object_id = Some("   ".into());
    value.validate().unwrap();
}
#[test]
fn byte_length_is_a_nonnegative_safe_integer() {
    for number in [
        json!(-1),
        json!(1.5),
        json!(MAX_SAFE_INTEGER + 1),
        json!("111"),
    ] {
        let mut value = serde_json::to_value(known()).unwrap();
        value["originalByteLength"] = number;
        assert!(validate(value).is_err());
    }
    for number in [0, MAX_SAFE_INTEGER] {
        let mut value = known();
        value.original_byte_length = Some(number);
        value.validate().unwrap();
    }
    let mut value = known();
    value.original_byte_length = Some(MAX_SAFE_INTEGER + 1);
    assert!(value.validate().is_err());
}
#[test]
fn json_numeric_lexemes_match_typescript_integer_semantics() {
    for spelling in ["0.0", "-0", "1e2"] {
        let encoded = serde_json::to_string(&known()).unwrap().replace(
            "\"originalByteLength\":111",
            &format!("\"originalByteLength\":{spelling}"),
        );
        let value: SourceProvenance = serde_json::from_str(&encoded).unwrap();
        value.validate().unwrap();
    }
}
#[test]
fn limitation_list_is_known_unique_and_mandatory() {
    for limitations in [
        json!(["inferredComplete"]),
        json!(["attachmentsUnavailable", "attachmentsUnavailable"]),
        Value::Null,
    ] {
        let mut value = serde_json::to_value(known()).unwrap();
        value["limitations"] = limitations;
        assert!(validate(value).is_err());
    }
    let mut value = serde_json::to_value(known()).unwrap();
    value.as_object_mut().unwrap().remove("limitations");
    assert!(validate(value).is_err());
}
#[test]
fn every_supported_legacy_generation_is_explicitly_incomplete() {
    for generation in 0..=3 {
        let value = SourceProvenance::legacy(Acquisition::Archive, generation).unwrap();
        value.validate().unwrap();
        assert_eq!(value.original_object_id, None);
        assert_eq!(value.original_byte_length, None);
        assert_eq!(value.limitations, ALL_LIMITATIONS);
        let encoded = serde_json::to_value(value).unwrap();
        assert!(encoded["originalObjectId"].is_null());
        assert!(encoded["originalByteLength"].is_null());
    }
    assert!(SourceProvenance::legacy(Acquisition::Api, 4).is_err());
    assert!(SourceProvenance::legacy(Acquisition::Api, 255).is_err());
}
#[test]
fn legacy_format_or_prefix_requires_both_matching_sentinels() {
    let mut value = known();
    value.source_format = LEGACY_FORMAT.into();
    assert!(value.validate().is_err());
    value = SourceProvenance::source_service(Acquisition::Api);
    value.adapter_version = "legacy-source-record-v1".into();
    assert!(value.validate().is_err());
    for adapter in [
        "legacy-source-record-v4",
        "legacy-source-record-v01",
        "legacy-source-record-unknown",
    ] {
        value = SourceProvenance::legacy(Acquisition::Api, 1).unwrap();
        value.adapter_version = adapter.into();
        assert!(value.validate().is_err());
    }
}
#[test]
fn legacy_never_invents_original_ids_or_sizes() {
    let mut value = SourceProvenance::legacy(Acquisition::Archive, 0).unwrap();
    value.original_object_id = Some("source-document-id".into());
    value
        .limitations
        .retain(|item| *item != ProvenanceLimitation::OriginalObjectIdUnavailable);
    assert!(value.validate().is_err());
    value = SourceProvenance::legacy(Acquisition::Api, 3).unwrap();
    value.original_byte_length = Some(100);
    value
        .limitations
        .retain(|item| *item != ProvenanceLimitation::OriginalBytesUnavailable);
    assert!(value.validate().is_err());
}
#[test]
fn legacy_requires_attachment_and_relation_limitations_even_for_empty_arrays() {
    for flag in [
        ProvenanceLimitation::AttachmentsUnavailable,
        ProvenanceLimitation::RelationsIncomplete,
    ] {
        let mut value = SourceProvenance::legacy(Acquisition::Api, 1).unwrap();
        value.limitations.retain(|item| *item != flag);
        assert!(value.validate().is_err());
    }
}
#[test]
fn source_service_constructor_does_not_claim_unavailable_metadata() {
    for acquisition in [
        Acquisition::Api,
        Acquisition::Archive,
        Acquisition::SelectedUrl,
        Acquisition::SavedPage,
    ] {
        let value = SourceProvenance::source_service(acquisition);
        value.validate().unwrap();
        assert_eq!(value.acquisition, acquisition);
        assert_eq!(value.adapter_version, "source-service-v1");
        assert_eq!(value.source_format, "proteusSourceServiceRecord");
        assert_eq!(value.original_object_id, None);
        assert_eq!(value.original_byte_length, None);
        assert_eq!(value.limitations, ALL_LIMITATIONS);
    }
}
