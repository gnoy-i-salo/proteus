use super::*;
fn html(value: &str) -> PageContent {
    extract(value, "text/html", &mut || Ok(())).unwrap()
}
#[test]
fn plain_text_and_markdown_remain_byte_exact_without_markup_interpretation() {
    let original = "  Имя 🦉\r\n\t<script>quoted code</script>\n";
    for kind in ["text/plain", "text/markdown"] {
        let got = extract(original, kind, &mut || Ok(())).unwrap();
        assert_eq!(got.text.as_bytes(), original.as_bytes());
        assert_eq!(got.title, None);
        assert!(got.author_labels.is_empty());
        assert_eq!(got.published_at, None);
    }
}
#[test]
fn html_projection_preserves_words_and_spacing_but_excludes_non_content() {
    let original = "<!doctype html><html><head><title>Заголовок &amp; имя</title><style>secret-style</style></head><body><p>Аня  &amp; 🦉<br>строка</p><pre>  code\t\r\nnext </pre><script>secret-script</script><!-- secret-comment --><template>secret-template</template><iframe>secret-frame</iframe><svg><text>secret-svg</text></svg><div hidden>secret-hidden</div><p aria-hidden='true'>secret-aria</p></body></html>";
    let got = html(original);
    assert_eq!(got.text, "Аня  & 🦉\nстрока\n  code\t\nnext \n");
    assert_eq!(got.title.as_deref(), Some("Заголовок & имя"));
    assert!(!got.text.contains("secret"));
    assert!(got.limitations.iter().any(|s| s == "htmlTextProjection"));
    assert!(original.contains("&amp;") && original.contains("\r\n"));
}
#[test]
fn publisher_metadata_never_becomes_a_confirmed_person_identity() {
    let got = html("<meta name='author' content='Анна &amp; Борис'><meta name='author' content='Анна &amp; Борис'><meta name='author' content='Редакция'><meta property='article:published_time' content='2024-01-02T03:04:05+03:00'><p>Текст</p>");
    assert_eq!(got.author_labels, ["Анна & Борис", "Редакция"]);
    assert_eq!(
        got.published_at.as_deref(),
        Some("2024-01-02T03:04:05+03:00")
    );
    assert!(got
        .limitations
        .iter()
        .any(|s| s == "publisherMetadataUnverified"));
}
#[test]
fn ambiguous_or_invalid_dates_remain_unknown() {
    for dates in ["<meta property='article:published_time' content='yesterday'>", "<meta property='article:published_time' content='2024-01-02T03:04:05Z'><meta property='article:published_time' content='2025-01-02T03:04:05Z'>"] {
        let got = html(&format!("{dates}<p>Text</p>"));
        assert_eq!(got.published_at, None);
        assert!(got.limitations.iter().any(|s| s == "publicationTimeUnavailable"));
    }
}
#[test]
fn empty_script_only_password_pages_and_unsupported_formats_fail_explicitly() {
    for input in [
        "",
        "<script>not content</script>",
        "<form><input type='password'>Sign in</form>",
    ] {
        assert!(
            extract(input, "text/html", &mut || Ok(())).is_err(),
            "{input}"
        );
    }
    assert!(extract("{}", "application/json", &mut || Ok(())).is_err());
}
#[test]
fn input_and_markup_budgets_fail_before_parsing_or_partial_output() {
    for input in ["x".repeat(4 * 1024 * 1024 + 1), "<b>x</b>".repeat(10001)] {
        assert!(extract(&input, "text/html", &mut || Ok(())).is_err());
    }
}
#[test]
fn cancellation_is_checked_during_parsing_and_extraction() {
    let input = format!("<p>{}</p>", "словo🦉 ".repeat(10000));
    let mut calls = 0;
    assert_eq!(
        extract(&input, "text/html", &mut || {
            calls += 1;
            if calls == 3 {
                Err("cancelled".into())
            } else {
                Ok(())
            }
        })
        .err()
        .as_deref(),
        Some("cancelled")
    );
    assert_eq!(calls, 3);
}

#[test]
fn deep_trees_and_excessive_metadata_fail_instead_of_truncating() {
    let nested = format!("{}text{}", "<div>".repeat(300), "</div>".repeat(300));
    assert!(extract(&nested, "text/html", &mut || Ok(())).is_err());
    let title = format!("<title>{}</title><p>text</p>", "x".repeat(2049));
    assert!(extract(&title, "text/html", &mut || Ok(())).is_err());
    let authors = (0..33)
        .map(|i| format!("<meta name='author' content='author{i}'>"))
        .collect::<String>();
    assert!(extract(
        &format!("{authors}<p>text</p>"),
        "text/html",
        &mut || Ok(())
    )
    .is_err());
}

#[test]
fn inline_markup_preserves_spacing_and_never_fetches_links_or_refresh_targets() {
    let got = html("<meta http-equiv='refresh' content='0;url=http://127.0.0.1/'><p>Аня <b>  пишет</b>&nbsp;<a href='file:///secret'>ссылку</a></p><p>next</p>");
    assert_eq!(got.text, "Аня   пишет\u{a0}ссылку\nnext\n");
    assert!(got.author_labels.is_empty());
    assert_eq!(got.published_at, None);
    assert!(got.limitations.iter().any(|s| s == "cssLayoutNotEvaluated"));
}

#[test]
fn repeated_metadata_does_not_consume_a_new_unique_value_slot() {
    let authors = (0..32)
        .map(|i| format!("<meta name='author' content='author{i}'>"))
        .collect::<String>();
    let got = html(&format!(
        "{authors}<meta name='author' content='author0'><p>text</p>"
    ));
    assert_eq!(got.author_labels.len(), 32);
}
