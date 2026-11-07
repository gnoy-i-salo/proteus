use super::*;
#[test]
fn exact_labels_and_author_identity_are_preserved_without_document_text() {
    let mut p = PreviewCollector::new(10, 10000).unwrap();
    p.resource("чат:\r\nID", "  Имя é 👩🏽‍💻  ").unwrap();
    p.resource("empty", "Пустой").unwrap();
    p.document("чат:\r\nID", Some("user1")).unwrap();
    p.document("чат:\r\nID", Some("user1")).unwrap();
    p.document("чат:\r\nID", None).unwrap();
    assert_eq!(
        p.finish().unwrap(),
        vec![
            PreviewResource {
                id: "чат:\r\nID".into(),
                label: "  Имя é 👩🏽‍💻  ".into(),
                author_ids: vec!["user1".into()],
                estimated_documents: Some(3)
            },
            PreviewResource {
                id: "empty".into(),
                label: "Пустой".into(),
                author_ids: vec![],
                estimated_documents: Some(0)
            }
        ]
    );
}
#[test]
fn authors_are_deterministic_and_never_fabricated_from_labels() {
    let mut p = PreviewCollector::new(3, 10000).unwrap();
    p.resource("r", "somebody").unwrap();
    p.document("r", Some("user2")).unwrap();
    p.document("r", Some("user1")).unwrap();
    p.document("r", None).unwrap();
    assert_eq!(p.finish().unwrap()[0].author_ids, vec!["user1", "user2"]);
}
#[test]
fn duplicate_resource_headers_are_ambiguous_even_when_labels_match() {
    let mut p = PreviewCollector::new(10, 10000).unwrap();
    p.resource("r", "same").unwrap();
    assert!(p.resource("r", "same").is_err());
    assert!(p.finish().is_err());
}
#[test]
fn documents_must_have_a_registered_resource() {
    let mut p = PreviewCollector::new(10, 10000).unwrap();
    assert!(p.document("missing", Some("user1")).is_err());
    assert!(p.finish().is_err());
}
#[test]
fn document_budget_is_global_and_never_silently_truncates() {
    let mut p = PreviewCollector::new(1, 10000).unwrap();
    p.resource("a", "a").unwrap();
    p.resource("b", "b").unwrap();
    p.document("a", None).unwrap();
    assert!(p.document("b", None).is_err());
    assert!(p.finish().is_err());
}
#[test]
fn metadata_budget_counts_utf8_bytes_not_characters_and_does_not_allow_partial_publish() {
    let mut p = PreviewCollector::new(10, 1024).unwrap();
    assert!(p.resource("r", &"Я".repeat(600)).is_err());
    assert!(p.finish().is_err());
}
#[test]
fn too_many_empty_resources_are_bounded() {
    let mut p = PreviewCollector::new(1, 10_000_000).unwrap();
    for i in 0..512 {
        p.resource(&i.to_string(), "").unwrap();
    }
    assert!(p.resource("extra", "").is_err());
    assert!(p.finish().is_err());
}
#[test]
fn author_metadata_is_capped_even_in_a_single_chat() {
    let mut p = PreviewCollector::new(10000, 10_000_000).unwrap();
    p.resource("r", "").unwrap();
    for i in 0..4096 {
        p.document("r", Some(&format!("user{i}"))).unwrap();
    }
    assert!(p.document("r", Some("extra")).is_err());
    assert!(p.finish().is_err());
}
#[test]
fn malformed_author_ids_cannot_become_selectable_bindings() {
    for id in ["", " \u{feff}", "x\n", &"Я".repeat(65)] {
        let mut p = PreviewCollector::new(10, 10000).unwrap();
        p.resource("r", "").unwrap();
        assert!(p.document("r", Some(id)).is_err());
        assert!(p.finish().is_err());
    }
}
#[test]
fn unsafe_limits_are_rejected() {
    for count in [0, 9_007_199_254_740_992] {
        assert!(PreviewCollector::new(count, 10000).is_err());
    }
    assert!(PreviewCollector::new(1, 0).is_err());
}
