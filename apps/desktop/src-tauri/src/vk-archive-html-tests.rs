//! Focused RED/GREEN tests for the bounded VK HTML archive page parser.
//!
//! Positive fixtures live in `src/fixtures/vk-archive/`; adversarial inputs
//! are generated inline. All content is synthetic.
use super::*;
use sha2::{Digest, Sha256};
use std::cell::Cell;

fn fixture(name: &str) -> String {
    let dir = std::env::var("PROTEUS_VK_FIXTURES").unwrap_or_else(|_| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/src/fixtures/vk-archive").to_string()
    });
    let path = format!("{dir}/{name}");
    String::from_utf8(std::fs::read(path).expect("fixture must be readable"))
        .expect("UTF-8 fixture")
}

fn ok_checkpoint() -> impl Fn() -> Result<(), String> {
    || Ok(())
}

fn parse_ok(original: &str) -> VkArchivePage {
    parse(original, &ok_checkpoint()).expect("parse must succeed")
}

fn parse_err(original: &str) -> String {
    parse(original, &ok_checkpoint()).expect_err("parse must fail")
}

const BASIC: &str = "messages-basic.txt";
const DATA_ID: &str = "messages-data-id.txt";

#[test]
fn preserves_original_html_bytes_exactly() {
    let raw = fixture(BASIC);
    let page = parse_ok(&raw);
    assert_eq!(page.original_html.as_bytes(), raw.as_bytes());
    assert!(page.original_html.contains("\r\n"));
    assert!(page.original_html.contains("fish &amp; chips"));
    let expected = format!("{:x}", Sha256::digest(raw.as_bytes()));
    assert_eq!(page.sha256, expected);
}

#[test]
fn parses_multiple_authors_with_ordinals() {
    let page = parse_ok(&fixture(BASIC));
    assert_eq!(page.messages.len(), 4);
    assert_eq!(
        page.messages[0].author_label.as_deref(),
        Some("Алиса Синтетикова")
    );
    assert_eq!(
        page.messages[0].author_url.as_deref(),
        Some("https://vk.com/synthetic.alice")
    );
    assert_eq!(page.messages[0].ordinal, 1);
    assert_eq!(
        page.messages[1].author_label.as_deref(),
        Some("Боб Манекен")
    );
    assert_eq!(page.messages[1].ordinal, 2);
    assert_eq!(
        page.messages[3].author_label.as_deref(),
        Some("Алиса Синтетикова")
    );
    assert_eq!(page.messages[3].ordinal, 4);
}

#[test]
fn decodes_entities_only_in_projection() {
    let raw = fixture(BASIC);
    let page = parse_ok(&raw);
    assert!(raw.contains("fish &amp; chips"));
    assert!(page.messages[0].body_text.contains("fish & chips"));
    assert!(!page.original_html.contains("fish & chips"));
}

#[test]
fn preserves_combining_marks_and_emoji() {
    let page = parse_ok(&fixture(BASIC));
    assert!(page.messages[1].body_text.contains('🎉'));
    assert!(page.messages[1].body_text.contains('\u{0301}'));
}

#[test]
fn keeps_distinct_identical_messages() {
    let page = parse_ok(&fixture(BASIC));
    let second = &page.messages[1];
    let third = &page.messages[2];
    assert_eq!(second.body_text, third.body_text);
    assert_eq!(second.author_label, third.author_label);
    assert_eq!(second.date_text, third.date_text);
    assert_ne!(second.ordinal, third.ordinal);
}

#[test]
fn repeated_parse_is_deterministic() {
    let raw = fixture(BASIC);
    let first = parse_ok(&raw);
    let second = parse_ok(&raw);
    assert_eq!(first, second);
}

#[test]
fn edited_marker_and_raw_edit_label() {
    let page = parse_ok(&fixture(BASIC));
    let edited = &page.messages[1];
    assert!(edited.edited);
    assert_eq!(edited.edit_label.as_deref(), Some("(ред.)"));
    assert_eq!(edited.date_text, "3 мар 2021 в 14:26:11");
    assert!(edited.header_text.contains("(ред.)"));
    assert!(!edited.date_text.contains("(ред.)"));
    assert!(!page.messages[0].edited);
    assert_eq!(page.messages[0].edit_label, None);
}

#[test]
fn edited_marker_matches_class_token() {
    let html = concat!(
        "<div class=\"message\"><div class=\"message__header\">",
        "<a href=\"https://vk.com/x\">A</a>, 1 янв 2020 в 10:00:00",
        "<span class=\"badge message-edited x\">(изм.)</span></div><div>тело</div></div>"
    );
    let page = parse_ok(html);
    assert!(page.messages[0].edited);
    assert_eq!(page.messages[0].edit_label.as_deref(), Some("(изм.)"));
}

#[test]
fn header_link_is_decoded_presentation() {
    let raw = fixture(BASIC);
    let page = parse_ok(&raw);
    assert_eq!(
        page.messages[1].author_url.as_deref(),
        Some("https://vk.com/id100000001&ref=syn")
    );
    assert!(raw.contains("id100000001&amp;ref=syn"));
}

#[test]
fn attachment_metadata_is_bounded_projection() {
    let page = parse_ok(&fixture(BASIC));
    let message = &page.messages[1];
    assert_eq!(message.attachments.len(), 1);
    let attachment = &message.attachments[0];
    assert_eq!(
        attachment.link.as_deref(),
        Some("https://vk.com/doc100000001_456239017?hash=ab12&dl=cd34")
    );
    assert_eq!(
        attachment.description.as_deref(),
        Some("Прикреплённый файл, не читается парсером")
    );
    assert!(!message.body_text.contains("Прикреплённый файл"));
}

#[test]
fn attachment_without_link_keeps_description() {
    let page = parse_ok(&fixture(DATA_ID));
    let message = &page.messages[1];
    assert_eq!(message.attachments.len(), 1);
    assert_eq!(message.attachments[0].link, None);
    assert_eq!(
        message.attachments[0].description.as_deref(),
        Some("Файл без ссылки: файл не читается")
    );
}

#[test]
fn kludge_metadata_is_captured_separately() {
    let page = parse_ok(&fixture(BASIC));
    let message = &page.messages[1];
    assert_eq!(
        message.kludges,
        vec!["Служебная пометка экспорта".to_string()]
    );
    assert!(!message.body_text.contains("Служебная пометка"));
}

#[test]
fn empty_body_is_allowed() {
    let page = parse_ok(&fixture(BASIC));
    assert_eq!(page.messages[3].body_text, "");
    assert!(page.messages[3].attachments.is_empty());
}

#[test]
fn br_becomes_newline_in_body() {
    let page = parse_ok(&fixture(DATA_ID));
    assert_eq!(
        page.messages[0].body_text,
        "Первое сообщение нового формата\nсо строкой ниже"
    );
}

#[test]
fn legacy_messages_have_no_external_id() {
    let page = parse_ok(&fixture(BASIC));
    assert!(page
        .messages
        .iter()
        .all(|message| message.external_id.is_none()));
}

#[test]
fn external_id_keeps_exact_digits_beyond_js_safe() {
    let page = parse_ok(&fixture(DATA_ID));
    assert_eq!(
        page.messages[0].external_id.as_deref(),
        Some("9007199254740993")
    );
    assert_eq!(
        page.messages[1].external_id.as_deref(),
        Some("9007199254740994")
    );
}

#[test]
fn forward_is_separate_context_not_attributed() {
    let page = parse_ok(&fixture(DATA_ID));
    let message = &page.messages[1];
    assert!(!message.body_text.contains("Пересланный текст"));
    assert_eq!(message.forwarded_contexts.len(), 1);
    let forward = &message.forwarded_contexts[0];
    assert_eq!(forward.author_label.as_deref(), Some("Оригинальный Автор"));
    assert_eq!(
        forward.author_url.as_deref(),
        Some("https://vk.com/id300000003")
    );
    assert_eq!(forward.body_text, "Пересланный текст, не приписывать Дэйву");
    assert!(message.relations.has_forward_or_quote);
    assert!(message.relations.unresolved);
    assert!(!message.relations.reasons.is_empty());
}

#[test]
fn page_limitations_expose_projection_semantics() {
    let page = parse_ok(&fixture(BASIC));
    assert_eq!(page.schema_version, 1);
    assert_eq!(page.projection_version, PROJECTION_VERSION);
    assert!(page.limitations.contains(&"htmlTextProjection".to_string()));
    assert!(page
        .limitations
        .contains(&"dateTimeRawNoTimezone".to_string()));
    assert!(page
        .limitations
        .contains(&"conversationIdentityExternal".to_string()));
    assert!(!page
        .limitations
        .contains(&"forwardedRelationsUnresolved".to_string()));
    let forwarded = parse_ok(&fixture(DATA_ID));
    assert!(forwarded
        .limitations
        .contains(&"forwardedRelationsUnresolved".to_string()));
}

#[test]
fn script_and_style_are_not_message_text() {
    let page = parse_ok(&fixture(BASIC));
    assert_eq!(page.messages.len(), 4);
    let bodies: String = page
        .messages
        .iter()
        .map(|message| message.body_text.as_str())
        .collect();
    assert!(!bodies.contains("decoy"));
    assert!(!bodies.contains("color"));
}

#[test]
fn duplicate_data_ids_are_rejected() {
    let html = concat!(
        "<div class=\"message\" data-id=\"42\"><div class=\"message__header\">",
        "<a href=\"https://vk.com/a\">A</a>, 1 янв 2020 в 10:00:00</div><div>один</div></div>",
        "<div class=\"message\" data-id=\"42\"><div class=\"message__header\">",
        "<a href=\"https://vk.com/b\">B</a>, 1 янв 2020 в 10:01:00</div><div>два</div></div>"
    );
    let error = parse_err(html);
    assert!(error.contains("data-id"), "unexpected error: {error}");
}

#[test]
fn empty_data_id_is_rejected() {
    let html = concat!(
        "<div class=\"message\" data-id=\"\"><div class=\"message__header\">",
        "<a href=\"https://vk.com/a\">A</a>, 1 янв 2020 в 10:00:00</div><div>тело</div></div>"
    );
    parse_err(html);
}

#[test]
fn missing_author_link_is_rejected() {
    let html = concat!(
        "<div class=\"message\"><div class=\"message__header\">только текст, 1 янв 2020 в 10:00:00",
        "</div><div>тело</div></div>"
    );
    let error = parse_err(html);
    assert!(error.contains("автор"), "unexpected error: {error}");
}

#[test]
fn author_link_without_href_is_rejected() {
    let html = concat!(
        "<div class=\"message\"><div class=\"message__header\"><a>A</a>, 1 янв 2020 в 10:00:00",
        "</div><div>тело</div></div>"
    );
    parse_err(html);
}

#[test]
fn missing_date_is_rejected() {
    let html = concat!(
        "<div class=\"message\"><div class=\"message__header\"><a href=\"https://vk.com/a\">A</a>",
        "</div><div>тело</div></div>"
    );
    let error = parse_err(html);
    assert!(error.contains("дат"), "unexpected error: {error}");
}

#[test]
fn text_before_author_is_rejected() {
    let html = concat!(
        "<div class=\"message\"><div class=\"message__header\">шум <a href=\"https://vk.com/a\">A</a>,",
        " 1 янв 2020 в 10:00:00</div><div>тело</div></div>"
    );
    parse_err(html);
}

#[test]
fn second_author_link_is_rejected() {
    let html = concat!(
        "<div class=\"message\"><div class=\"message__header\"><a href=\"https://vk.com/a\">A</a>,",
        " 1 янв 2020 в 10:00:00 <a href=\"https://vk.com/b\">B</a></div><div>тело</div></div>"
    );
    parse_err(html);
}

#[test]
fn missing_header_div_is_rejected() {
    let html = "<div class=\"message\"><div>тело без заголовка</div></div>";
    parse_err(html);
}

#[test]
fn missing_body_div_is_rejected() {
    let html = concat!(
        "<div class=\"message\"><div class=\"message__header\">",
        "<a href=\"https://vk.com/a\">A</a>, 1 янв 2020 в 10:00:00</div></div>"
    );
    parse_err(html);
}

#[test]
fn truncated_message_is_rejected() {
    let html = concat!(
        "<div class=\"message\"><div class=\"message__header\">",
        "<a href=\"https://vk.com/a\">A</a>, 1 янв 2020 в 10:00:00</div><div>не закрыто"
    );
    let error = parse_err(html);
    assert!(
        error.to_lowercase().contains("неполн"),
        "unexpected error: {error}"
    );
}

#[test]
fn unsupported_element_in_body_is_rejected() {
    let html = concat!(
        "<div class=\"message\"><div class=\"message__header\">",
        "<a href=\"https://vk.com/a\">A</a>, 1 янв 2020 в 10:00:00</div>",
        "<div>текст<img src=\"https://vk.com/x.png\"></div></div>"
    );
    parse_err(html);
}

#[test]
fn unknown_nested_div_in_body_is_rejected() {
    let html = concat!(
        "<div class=\"message\"><div class=\"message__header\">",
        "<a href=\"https://vk.com/a\">A</a>, 1 янв 2020 в 10:00:00</div>",
        "<div><div class=\"unknown\">метка</div></div></div>"
    );
    parse_err(html);
}

#[test]
fn extra_div_after_body_is_rejected() {
    let html = concat!(
        "<div class=\"message\"><div class=\"message__header\">",
        "<a href=\"https://vk.com/a\">A</a>, 1 янв 2020 в 10:00:00</div>",
        "<div>тело</div><div class=\"attachment\"></div></div>"
    );
    parse_err(html);
}

#[test]
fn nested_forward_without_header_is_rejected() {
    let html = concat!(
        "<div class=\"message\"><div class=\"message__header\">",
        "<a href=\"https://vk.com/a\">A</a>, 1 янв 2020 в 10:00:00</div>",
        "<div><div class=\"message\"><div>пересылка без заголовка</div></div></div></div>"
    );
    parse_err(html);
}

#[test]
fn unknown_page_shape_is_explicit_error() {
    let html = "<html><body><article>очень важный текст страницы целиком</article></body></html>";
    let error = parse_err(html);
    assert!(error.contains("сообщен"), "unexpected error: {error}");
}

#[test]
fn oversize_input_is_rejected() {
    let mut html = String::with_capacity(MAX_INPUT_BYTES + 64);
    html.push_str("<html><body>");
    html.push_str(&"a".repeat(MAX_INPUT_BYTES));
    html.push_str("</body></html>");
    assert!(html.len() > MAX_INPUT_BYTES);
    let error = parse_err(&html);
    assert!(error.contains("размер"), "unexpected error: {error}");
}

#[test]
fn input_at_exact_size_limit_is_accepted() {
    let message = concat!(
        "<div class=\"message\"><div class=\"message__header\">",
        "<a href=\"https://vk.com/a\">A</a>, 1 янв 2020 в 10:00:00</div><div>тело</div></div>"
    );
    let mut html = String::with_capacity(MAX_INPUT_BYTES);
    html.push_str(message);
    html.push_str(&"b".repeat(MAX_INPUT_BYTES - message.len()));
    assert_eq!(html.len(), MAX_INPUT_BYTES);
    let page = parse_ok(&html);
    assert_eq!(page.messages.len(), 1);
}

#[test]
fn message_cap_is_enforced() {
    let one = concat!(
        "<div class=\"message\"><div class=\"message__header\">",
        "<a href=\"https://vk.com/a\">A</a>,d</div><div></div></div>"
    );
    let mut html = String::with_capacity(one.len() * (MAX_MESSAGES + 1));
    for _ in 0..=MAX_MESSAGES {
        html.push_str(one);
    }
    assert!(html.len() <= MAX_INPUT_BYTES);
    let error = parse_err(&html);
    assert!(error.contains("сообщен"), "unexpected error: {error}");
}

#[test]
fn event_cap_is_enforced() {
    let mut html = String::with_capacity(MAX_TOKENIZER_EVENTS * 4);
    html.push_str("<html><body>");
    for _ in 0..=MAX_TOKENIZER_EVENTS {
        html.push_str("<br>");
    }
    html.push_str("</body></html>");
    assert!(html.len() <= MAX_INPUT_BYTES);
    let error = parse_err(&html);
    assert!(error.contains("событ"), "unexpected error: {error}");
}

#[test]
fn depth_cap_is_enforced() {
    let mut html = String::with_capacity(MAX_DEPTH * 6);
    for _ in 0..=MAX_DEPTH {
        html.push_str("<div>");
    }
    let error = parse_err(&html);
    assert!(error.contains("вложен"), "unexpected error: {error}");
}

#[test]
fn late_cancellation_aborts_parse() {
    let raw = fixture(BASIC);
    let calls = Cell::new(0_u32);
    let result = parse(&raw, &|| {
        calls.set(calls.get() + 1);
        if calls.get() >= 3 {
            Err(" cancelled-by-test ".to_string())
        } else {
            Ok(())
        }
    });
    let error = result.expect_err("late cancellation must abort");
    assert_eq!(error, " cancelled-by-test ");
    assert!(calls.get() >= 3);
}

#[test]
fn checkpoint_runs_during_work_and_at_end() {
    let raw = fixture(BASIC);
    let calls = Cell::new(0_u32);
    let page = parse(&raw, &|| {
        calls.set(calls.get() + 1);
        Ok(())
    })
    .expect("parse must succeed");
    assert!(
        calls.get() >= 3,
        "checkpoint must run incrementally and at end"
    );
    assert_eq!(page.messages.len(), 4);
}

#[test]
fn per_message_attachment_cap_is_enforced() {
    let mut html = String::new();
    html.push_str("<div class=\"message\"><div class=\"message__header\">");
    html.push_str("<a href=\"https://vk.com/a\">A</a>, 1 янв 2020 в 10:00:00</div><div>");
    for _ in 0..=MAX_ATTACHMENTS_PER_MESSAGE {
        html.push_str(
            "<div class=\"attachment\"><div class=\"attachment__description\">x</div></div>",
        );
    }
    html.push_str("</div></div>");
    let error = parse_err(&html);
    assert!(error.contains("вложен"), "unexpected error: {error}");
}

#[test]
fn projection_needs_no_original_offsets() {
    // The projection is derived presentation only; re-parsing a mutated copy
    // yields the same derived body for unchanged messages.
    let raw = fixture(BASIC);
    let page = parse_ok(&raw);
    let mut padded = raw.clone();
    padded.insert_str(0, "<!-- padding comment -->");
    let reparsed = parse_ok(&padded);
    assert_eq!(page.messages[0].body_text, reparsed.messages[0].body_text);
    assert_ne!(page.original_html, reparsed.original_html);
}

fn message_with(body: &str) -> String {
    format!("<div class=\"message\"><div class=\"message__header\"><a href=\"author\">A</a>, date</div><div>{body}</div></div>")
}

#[test]
fn rejects_ambiguous_duplicate_attributes() {
    for (from, to) in [
        ("href=\"author\"", "href=\"author\" href=\"other\""),
        ("class=\"message\"", "class=\"message\" class=\"other\""),
        (
            "class=\"message\"",
            "class=\"message\" data-id=\"1\" data-id=\"2\"",
        ),
    ] {
        let html = message_with("body").replacen(from, to, 1);
        assert!(
            parse(&html, &ok_checkpoint()).is_err(),
            "ambiguous attributes: {to}"
        );
    }
}

#[test]
fn rejects_null_instead_of_silently_dropping_source_characters() {
    parse_err(&message_with("before\0after"));
}

#[test]
fn preserves_body_trailing_spaces_and_line_breaks() {
    let body = "  é 😀  <br>  <br>";
    assert_eq!(
        parse_ok(&message_with(body)).messages[0].body_text,
        "  é 😀  \n  \n"
    );
}

#[test]
fn preserves_forward_body_trailing_spaces() {
    let nested = message_with("forward  <br> ");
    let page = parse_ok(&message_with(&nested));
    assert_eq!(
        page.messages[0].forwarded_contexts[0].body_text,
        "forward  \n "
    );
}

#[test]
fn header_projection_preserves_token_order() {
    let html = message_with("body").replace(
        ", date",
        "<span class=\"message-edited\"> edit </span>, date",
    );
    assert_eq!(parse_ok(&html).messages[0].header_text, "A edit , date");
}

#[test]
fn separator_obeys_body_budget() {
    let body = "x".repeat(MAX_BODY_BYTES);
    assert_eq!(
        parse_ok(&message_with(&body)).messages[0].body_text.len(),
        MAX_BODY_BYTES
    );
    let body = format!("{body}<div class=\"kludges\">note</div>");
    parse_err(&message_with(&body));
}

#[test]
fn blank_data_id_is_not_an_identity() {
    parse_err(&message_with("body").replacen(
        "class=\"message\"",
        "class=\"message\" data-id=\"   \"",
        1,
    ));
}

#[test]
fn malformed_forward_author_link_is_not_treated_as_absent() {
    let nested = message_with("forward").replace("href=\"author\"", "href=\"\"");
    parse_err(&message_with(&nested));
}

#[test]
fn absent_forward_markup_does_not_claim_complete_reply_coverage() {
    let page = parse_ok(&message_with("body"));
    assert!(page
        .limitations
        .iter()
        .any(|value| value == "replyTargetsNotExposed"));
    assert!(!page.messages[0].relations.has_forward_or_quote);
}
