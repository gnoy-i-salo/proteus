use super::*;
use serde_json::{json, Value};
fn web() -> PageOrigin {
    PageOrigin::SelectedUrl {
        selected_url: "https://Example.com/post#part".into(),
        final_url: "https://example.com/post".into(),
        redirects: vec![],
        retrieved_at: "2026-11-08T06:00:00Z".into(),
    }
}
fn saved() -> PageOrigin {
    PageOrigin::SavedPage {
        source_origin: format!("localArchivePath:1:sha256:{}", "a".repeat(64)),
        label: "  Анна 🦉.html".into(),
    }
}
fn document(origin: PageOrigin, text: &str) -> PageDocument {
    normalize(origin, "text/plain".into(), text.into(), &mut || Ok(())).unwrap()
}
#[test]
fn exact_original_and_projection_survive_canonical_roundtrip() {
    let raw = "  Имя 🦉\r\n\tword\n";
    let doc = document(web(), raw);
    use sha2::{Digest, Sha256};
    assert_eq!(doc.sha256, format!("{:x}", Sha256::digest(raw.as_bytes())));
    assert_eq!(doc.original.as_bytes(), raw.as_bytes());
    assert_eq!(doc.text, raw);
    let wire = serde_json::to_string(&doc).unwrap();
    assert_eq!(decode(&wire, &mut || Ok(())).unwrap(), doc);
    assert!(wire.contains("\"sourceKey\"") && wire.contains("\"selectedUrl\""));
    assert!(!wire.contains("\"source_key\""));
}
#[test]
fn source_identity_is_stable_across_edits_but_separate_for_distinct_sources() {
    let one = document(web(), "one");
    let two = document(web(), "two");
    assert_eq!(one.id, two.id);
    assert_eq!(one.source_key, two.source_key);
    assert_ne!(one.sha256, two.sha256);
    let mut canonical = web();
    if let PageOrigin::SelectedUrl { selected_url, .. } = &mut canonical {
        *selected_url = "https://example.com/post#other".into();
    }
    assert_eq!(one.source_key, document(canonical, "one").source_key);
    let mut other = web();
    if let PageOrigin::SelectedUrl {
        selected_url,
        final_url,
        ..
    } = &mut other
    {
        *selected_url = "https://example.com/other".into();
        *final_url = selected_url.clone();
    }
    assert_ne!(one.source_key, document(other, "one").source_key);
    assert_ne!(one.source_key, document(saved(), "one").source_key);
}
#[test]
fn saved_page_keeps_native_origin_and_exact_label_without_claiming_fetch_time() {
    let origin = saved();
    let doc = document(origin.clone(), "one");
    assert_eq!(doc.origin, origin);
    let mut renamed = saved();
    if let PageOrigin::SavedPage { label, .. } = &mut renamed {
        *label = "display label".into();
    }
    assert_eq!(doc.source_key, document(renamed, "one").source_key);
    let mut other = saved();
    if let PageOrigin::SavedPage { source_origin, .. } = &mut other {
        *source_origin = source_origin.replace('a', "b");
    }
    assert!(normalize(other, "text/plain".into(), "one".into(), &mut || Ok(())).is_err());
}
#[test]
fn corrupt_derived_fields_original_hash_and_unknown_versions_are_rejected() {
    let doc = document(web(), "real words");
    let original = serde_json::to_value(doc).unwrap();
    for (key, value) in [
        ("schemaVersion", json!(2)),
        ("extractionVersion", json!("future")),
        ("id", json!("other")),
        ("sourceKey", json!("other")),
        ("sha256", json!("0".repeat(64))),
        ("original", json!("changed")),
        ("text", json!("invented")),
        ("title", json!("invented")),
        ("authorLabels", json!(["invented person"])),
        ("publishedAt", json!("2000-01-01T00:00:00Z")),
        ("limitations", json!([])),
    ] {
        let mut bad = original.clone();
        bad[key] = value;
        assert!(decode(&bad.to_string(), &mut || Ok(())).is_err(), "{key}");
    }
}
#[test]
fn missing_nullable_fields_unknown_aliases_and_duplicate_keys_are_rejected() {
    let original = serde_json::to_value(document(web(), "one")).unwrap();
    for key in ["title", "publishedAt"] {
        let mut bad = original.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert!(decode(&bad.to_string(), &mut || Ok(())).is_err(), "{key}");
    }
    let mut bad = original.clone();
    bad["source_key"] = json!("legacy alias");
    assert!(decode(&bad.to_string(), &mut || Ok(())).is_err());
    let wire = original
        .to_string()
        .replacen('{', "{\"schemaVersion\":1,", 1);
    assert!(decode(&wire, &mut || Ok(())).is_err());
}
#[test]
fn origin_shape_redirect_chain_and_retrieval_time_are_validated_without_network() {
    let good = serde_json::to_value(document(web(), "one")).unwrap();
    for (key, value) in [
        ("selectedUrl", json!("file:///private")),
        ("finalUrl", json!("https://other.example/")),
        ("redirects", json!(["https://other.example/"])),
        ("retrievedAt", Value::Null),
        ("retrievedAt", json!("yesterday")),
    ] {
        let mut bad = good.clone();
        bad["origin"][key] = value;
        assert!(decode(&bad.to_string(), &mut || Ok(())).is_err(), "{key}");
    }
    let invalid = PageOrigin::SavedPage {
        source_origin: "../../private".into(),
        label: "file".into(),
    };
    assert!(normalize(invalid, "text/plain".into(), "one".into(), &mut || Ok(())).is_err());
}
#[test]
fn cancellation_precedes_decoding_and_large_metadata_projection() {
    assert_eq!(
        decode("{}", &mut || Err("cancelled".into()))
            .err()
            .as_deref(),
        Some("cancelled")
    );
    let mut origin = web();
    if let PageOrigin::SelectedUrl { selected_url, .. } = &mut origin {
        *selected_url = format!("https://example.com/{}", "x".repeat(8192));
    }
    assert!(normalize(origin, "text/plain".into(), "one".into(), &mut || Ok(())).is_err());
}

#[test]
fn html_projection_and_reported_authors_are_recomputed_from_exact_original() {
    let original = "<meta name='author' content='Анна'><p>  имя &amp; 🦉</p>\r\n";
    let doc = normalize(saved(), "text/html".into(), original.into(), &mut || Ok(())).unwrap();
    assert_eq!(doc.original, original);
    assert_eq!(doc.author_labels, ["Анна"]);
    assert_eq!(
        decode(&serde_json::to_string(&doc).unwrap(), &mut || Ok(())).unwrap(),
        doc
    );
    let mut other = saved();
    if let PageOrigin::SavedPage { source_origin, .. } = &mut other {
        *source_origin = format!("localArchivePath:1:sha256:{}", "b".repeat(64));
    }
    assert_ne!(doc.source_key, document(other, "one").source_key);
}
#[test]
fn valid_redirects_preserve_selected_identity_but_https_downgrade_is_rejected() {
    let initial = document(web(), "one");
    let mut origin = web();
    if let PageOrigin::SelectedUrl {
        final_url,
        redirects,
        ..
    } = &mut origin
    {
        *final_url = "https://other.example/final".into();
        *redirects = vec!["https://example.com/intermediate".into(), final_url.clone()];
    }
    let redirected = document(origin.clone(), "one");
    assert_eq!(redirected.source_key, initial.source_key);
    assert_eq!(
        decode(&serde_json::to_string(&redirected).unwrap(), &mut || Ok(())).unwrap(),
        redirected
    );
    if let PageOrigin::SelectedUrl {
        final_url,
        redirects,
        ..
    } = &mut origin
    {
        *final_url = "http://other.example/final".into();
        *redirects = vec![final_url.clone()];
    }
    assert!(normalize(origin, "text/plain".into(), "one".into(), &mut || Ok(())).is_err());
}
