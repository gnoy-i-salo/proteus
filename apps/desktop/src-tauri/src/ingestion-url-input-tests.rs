use super::*;
fn document(index: usize, original: &str) -> PageDocument {
    let url = format!("https://example.com/{index}");
    super::super::page_document::normalize(
        PageOrigin::SelectedUrl {
            selected_url: url.clone(),
            final_url: url,
            redirects: vec![],
            retrieved_at: "2026-11-08T09:00:00Z".into(),
        },
        "text/plain".into(),
        original.into(),
        &mut || Ok(()),
    )
    .unwrap()
}
#[test]
fn url_capability_keeps_exact_original_and_receipt_and_release_removes_access() {
    let inputs = UrlInputs::default();
    let doc = document(1, "  Точно 🦉\r\n");
    let expected = doc.clone();
    let handle = inputs.insert(doc).unwrap();
    assert!(!handle.contains("example.com"));
    inputs
        .read(&handle, |input| {
            assert_eq!(input.original, expected.original);
            assert_eq!(input.sha256, expected.sha256);
            assert_eq!(input.origin, expected.origin);
            assert_eq!(input.content_type, "text/plain");
            Ok(())
        })
        .unwrap();
    inputs
        .associate_preview(&handle, "preview:fixture")
        .unwrap();
    assert_eq!(
        inputs
            .read_preview("preview:fixture", |input| Ok(input.original.clone()))
            .unwrap(),
        expected.original
    );
    inputs.release(&handle).unwrap();
    assert!(inputs.read(&handle, |_| Ok(())).is_err());
    assert!(inputs.read_preview("preview:fixture", |_| Ok(())).is_err());
    inputs.release(&handle).unwrap();
}
#[test]
fn url_receipt_cannot_move_between_capabilities_and_has_bounded_associations() {
    let inputs = UrlInputs::default();
    let first = inputs.insert(document(1, "First")).unwrap();
    let second = inputs.insert(document(2, "Second")).unwrap();
    inputs.associate_preview(&first, "preview:shared").unwrap();
    inputs.associate_preview(&first, "preview:shared").unwrap();
    assert!(inputs.associate_preview(&second, "preview:shared").is_err());
    assert_eq!(
        inputs
            .read_preview("preview:shared", |input| Ok(input.original.clone()))
            .unwrap(),
        "First"
    );
    for i in 1..16 {
        inputs
            .associate_preview(&first, &format!("preview:{i}"))
            .unwrap();
    }
    assert!(inputs
        .associate_preview(&first, "preview:overflow")
        .is_err());
    assert!(inputs.associate_preview(&second, "").is_err());
    assert!(inputs
        .associate_preview("unknown", "preview:unknown")
        .is_err());
}
#[test]
fn url_cache_capacity_releases_slots_and_rejects_forged_page_envelopes() {
    let inputs = UrlInputs::default();
    let mut handles = Vec::new();
    for i in 0..8 {
        handles.push(inputs.insert(document(i, "Original")).unwrap());
    }
    assert!(inputs.insert(document(8, "One too many")).is_err());
    inputs.release(&handles[0]).unwrap();
    assert!(inputs.insert(document(8, "Replacement")).is_ok());
    let empty = UrlInputs::default();
    let mut bad = document(1, "Original");
    bad.sha256 = "a".repeat(64);
    assert!(empty.insert(bad).is_err());
    let saved = super::super::page_document::normalize(
        PageOrigin::SavedPage {
            source_origin: format!("localArchivePath:1:sha256:{}", "a".repeat(64)),
            label: "local.txt".into(),
        },
        "text/plain".into(),
        "Local".into(),
        &mut || Ok(()),
    )
    .unwrap();
    assert!(empty.insert(saved).is_err());
}
#[test]
fn url_cache_bounds_retained_bytes_even_before_selection_count_is_reached() {
    let inputs = UrlInputs::default();
    let large = "x".repeat(MAX_PAGE_BYTES);
    let mut handles = Vec::new();
    for i in 0..7 {
        handles.push(inputs.insert(document(i, &large)).unwrap());
    }
    assert!(inputs.insert(document(7, &large)).is_err());
    inputs.release(&handles[0]).unwrap();
    assert!(inputs.insert(document(7, &large)).is_ok());
}
