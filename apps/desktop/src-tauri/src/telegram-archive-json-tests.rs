use super::*;
use std::fs;
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct ChunkedReader {
    bytes: Vec<u8>,
    pos: usize,
    chunk: usize,
}

impl ChunkedReader {
    fn new(bytes: Vec<u8>, chunk: usize) -> Self {
        Self {
            bytes,
            pos: 0,
            chunk,
        }
    }
}

impl Read for ChunkedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos == self.bytes.len() {
            return Ok(0);
        }
        let take = self.chunk.min(buf.len()).min(self.bytes.len() - self.pos);
        buf[..take].copy_from_slice(&self.bytes[self.pos..self.pos + take]);
        self.pos += take;
        Ok(take)
    }
}

struct SlowReader {
    bytes: Vec<u8>,
    pos: usize,
    delay: Duration,
}

impl Read for SlowReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos == self.bytes.len() {
            return Ok(0);
        }
        std::thread::sleep(self.delay);
        let take = 1.min(buf.len());
        buf[..take].copy_from_slice(&self.bytes[self.pos..self.pos + take]);
        self.pos += take;
        Ok(take)
    }
}

struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("disk failure"))
    }
}

struct CappedFileReader {
    file: fs::File,
    chunk: usize,
}

impl Read for CappedFileReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let cap = self.chunk.min(buf.len());
        self.file.read(&mut buf[..cap])
    }
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/fixtures/telegram-archive")
        .join(name);
    fs::read(&path).unwrap_or_else(|error| panic!("cannot read fixture {path:?}: {error}"))
}

fn default_limits() -> TelegramArchiveLimits {
    TelegramArchiveLimits {
        max_input_bytes: 64 << 20,
        max_message_bytes: 1 << 20,
        max_metadata_bytes: 4096,
        max_messages: 1_000_000,
        max_duration: Duration::from_secs(120),
    }
}

struct CollectedScan {
    summary: TelegramScanSummary,
    chats: Vec<TelegramChatHeader>,
    messages: Vec<TelegramMessageFrame>,
}

fn scan_bytes(input: &[u8]) -> CollectedScan {
    scan_bytes_with_limits(input, &default_limits(), &AtomicBool::new(false))
}

fn scan_bytes_with_limits(
    input: &[u8],
    limits: &TelegramArchiveLimits,
    cancelled: &AtomicBool,
) -> CollectedScan {
    let mut chats = Vec::new();
    let mut messages = Vec::new();
    let summary = read_telegram_json(
        ChunkedReader::new(input.to_vec(), 13),
        limits,
        cancelled,
        |chat| {
            chats.push(chat.clone());
            Ok(())
        },
        |frame| {
            messages.push(frame);
            Ok(ScanControl::Continue)
        },
    )
    .expect("scan should succeed");
    CollectedScan {
        summary,
        chats,
        messages,
    }
}

fn scan_string(input: &str) -> Result<TelegramScanSummary, String> {
    read_telegram_json(
        ChunkedReader::new(input.as_bytes().to_vec(), 5),
        &default_limits(),
        &AtomicBool::new(false),
        |_chat| Ok(()),
        |_frame| Ok(ScanControl::Continue),
    )
}

fn scan_string_collect(input: &str) -> CollectedScan {
    let mut chats = Vec::new();
    let mut messages = Vec::new();
    let summary = read_telegram_json(
        ChunkedReader::new(input.as_bytes().to_vec(), 5),
        &default_limits(),
        &AtomicBool::new(false),
        |chat| {
            chats.push(chat.clone());
            Ok(())
        },
        |frame| {
            messages.push(frame);
            Ok(ScanControl::Continue)
        },
    )
    .expect("scan should succeed");
    CollectedScan {
        summary,
        chats,
        messages,
    }
}

#[test]
fn single_chat_fixture_streams_exact_raw_messages() {
    let input = fixture_bytes("single-chat.json");
    let scan = scan_bytes(&input);
    assert!(scan.summary.complete);
    assert_eq!(scan.summary.scanned_messages, 2);
    assert_eq!(scan.summary.bytes_read, input.len() as u64);
    assert_eq!(scan.chats.len(), 1);
    let chat = &scan.chats[0];
    assert_eq!(chat.id, "123456789");
    assert_eq!(chat.kind, "personal_chat");
    assert_eq!(chat.name.as_deref(), Some("Example Contact"));
    assert_eq!(scan.messages.len(), 2);
    for frame in &scan.messages {
        assert_eq!(frame.chat.id, "123456789");
        assert_eq!(
            frame.byte_end,
            frame.byte_start + frame.raw_json.len() as u64
        );
        let source_slice = &input[frame.byte_start as usize..frame.byte_end as usize];
        assert_eq!(
            source_slice,
            frame.raw_json.as_slice(),
            "raw bytes must be exact"
        );
        let parsed: serde_json::Value =
            serde_json::from_slice(&frame.raw_json).expect("raw frame must be valid JSON");
        assert!(parsed.is_object());
        let reserialized = serde_json::to_vec(&parsed).unwrap();
        assert_ne!(
            reserialized, frame.raw_json,
            "raw bytes must not be re-serialized"
        );
    }
    let first = &scan.messages[0].raw_json;
    let expected_first = "привет\\nemoji 😀\\t\\u2028".as_bytes();
    assert!(
        first
            .windows(expected_first.len())
            .any(|window| window == expected_first),
        "raw bytes must keep escapes exactly: {}",
        String::from_utf8_lossy(first)
    );
    let second = &scan.messages[1].raw_json;
    let expected_second = "CRLF line\\r\\nand \\\"quotes\\\" and \\\\ backslash".as_bytes();
    assert!(
        second
            .windows(expected_second.len())
            .any(|window| window == expected_second),
        "raw bytes must keep escapes exactly: {}",
        String::from_utf8_lossy(second)
    );
}

#[test]
fn account_fixture_skips_sections_and_scans_multiple_chats() {
    let input = fixture_bytes("account-export.json");
    let scan = scan_bytes(&input);
    assert!(scan.summary.complete);
    assert_eq!(scan.summary.scanned_messages, 2);
    assert_eq!(scan.summary.bytes_read, input.len() as u64);
    let ids: Vec<&str> = scan.chats.iter().map(|chat| chat.id.as_str()).collect();
    assert_eq!(ids, ["555000111", "777000222"]);
    assert_eq!(scan.chats[0].name.as_deref(), Some("Group Chat"));
    assert_eq!(scan.chats[0].kind, "private_supergroup");
    assert_eq!(scan.chats[1].name.as_deref(), Some("Empty Channel"));
    assert_eq!(scan.chats[1].kind, "public_channel");
    assert_eq!(scan.messages.len(), 2);
    for frame in &scan.messages {
        assert_eq!(frame.chat.id, "555000111");
        assert!(frame.byte_start > 0);
    }
}

#[test]
fn empty_chat_still_calls_on_chat() {
    let input = fixture_bytes("account-export.json");
    let scan = scan_bytes(&input);
    assert_eq!(scan.chats.len(), 2, "both chats, including the empty one");
    assert_eq!(scan.summary.scanned_messages, 2);
}

#[test]
fn left_chats_section_is_scanned_after_chats() {
    let input = fixture_bytes("account-export-left-chats.json");
    let scan = scan_bytes(&input);
    assert!(scan.summary.complete);
    let ids: Vec<&str> = scan.chats.iter().map(|chat| chat.id.as_str()).collect();
    assert_eq!(ids, ["100001", "100002"]);
    assert_eq!(scan.chats[1].kind, "private_channel");
    assert_eq!(scan.messages.len(), 2);
    assert_eq!(scan.messages[1].chat.id, "100002");
}

#[test]
fn null_name_and_null_type_are_preserved_as_absence_and_empty_kind() {
    let scan = scan_string_collect("{\"name\":null,\"type\":null,\"id\":7,\"messages\":[]}");
    assert_eq!(scan.chats.len(), 1);
    assert_eq!(scan.chats[0].name, None);
    assert_eq!(scan.chats[0].kind, "");
    assert_eq!(scan.chats[0].id, "7");
    assert!(scan.summary.complete);
}

#[test]
fn saved_messages_chat_without_name_key_is_accepted() {
    let scan = scan_string_collect("{\"type\":\"saved_messages\",\"id\":42,\"messages\":[]}");
    assert_eq!(scan.chats[0].name, None);
    assert_eq!(scan.chats[0].kind, "saved_messages");
}

#[test]
fn ids_are_verbatim_number_tokens() {
    let scan =
        scan_string_collect("{\"type\":\"private_channel\",\"id\":-1001234567890,\"messages\":[]}");
    assert_eq!(scan.chats[0].id, "-1001234567890");
    let scan = scan_string_collect(
        "{\"type\":\"private_channel\",\"id\":99999999999999999999999999,\"messages\":[]}",
    );
    assert_eq!(scan.chats[0].id, "99999999999999999999999999");
}

#[test]
fn escaped_name_decodes_unicode_and_surrogate_pairs() {
    let scan = scan_string_collect(
        "{\"name\":\"A\\u043d\\ud83d\\ude00\",\"type\":\"bot_chat\",\"id\":5,\"messages\":[]}",
    );
    assert_eq!(scan.chats[0].name.as_deref(), Some("Aн😀"));
}

#[test]
fn unknown_formats_fail_explicitly() {
    for input in [
        "",
        "not json",
        "[]",
        "[1,2,3]",
        "{}",
        "{\"messages\":[{\"id\":1}]}",
        "{\"data\":{\"messages\":[{\"id\":1}]}}",
        "{\"about\":\"x\"}",
        "{\"about\":\"x\",\"unknown_section\":[]}",
        "{\"chats\":{\"about\":\"a\",\"list\":[]},\"about\":\"wrong order\"}",
        "{\"name\":\"a\",\"type\":\"personal_chat\",\"id\":1,\"messages\":[],\"extra\":1}",
        "123",
        "null",
    ] {
        let result = scan_string(input);
        assert!(result.is_err(), "input must be rejected: {input:?}");
    }
}

#[test]
fn duplicate_structural_keys_fail() {
    for input in [
        "{\"name\":\"a\",\"name\":\"b\",\"type\":\"personal_chat\",\"id\":1,\"messages\":[]}",
        "{\"name\":\"a\",\"type\":\"x\",\"type\":\"y\",\"id\":1,\"messages\":[]}",
        "{\"name\":\"a\",\"type\":\"x\",\"id\":1,\"id\":2,\"messages\":[]}",
        "{\"about\":\"a\",\"about\":\"b\",\"personal_information\":{}}",
        "{\"about\":\"a\",\"chats\":{\"about\":\"a\",\"about\":\"b\",\"list\":[]}}",
        "{\"about\":\"a\",\"chats\":{\"about\":\"a\",\"list\":[],\"list\":[]}}",
    ] {
        let result = scan_string(input);
        assert!(
            result.is_err(),
            "duplicate keys must be rejected: {input:?}"
        );
    }
}

#[test]
fn messages_before_identity_or_after_messages_fails() {
    for input in [
        "{\"type\":\"personal_chat\",\"messages\":[],\"id\":1}",
        "{\"messages\":[],\"type\":\"personal_chat\",\"id\":1}",
        "{\"type\":\"personal_chat\",\"id\":1,\"messages\":[],\"name\":\"late\"}",
        "{\"name\":\"a\",\"id\":1,\"type\":\"personal_chat\",\"messages\":[]}",
        "{\"name\":\"a\",\"type\":\"x\",\"id\":1,\"messages\":[{\"id\":9}],\"id\":2}",
    ] {
        let result = scan_string(input);
        assert!(
            result.is_err(),
            "wrong key ordering must be rejected: {input:?}"
        );
    }
}

#[test]
fn account_chat_list_rejects_wrong_chat_shapes() {
    for input in [
        // chat without id before messages
        "{\"about\":\"a\",\"chats\":{\"about\":\"a\",\"list\":[{\"name\":\"g\",\"type\":\"private_group\",\"messages\":[]}]}}",
        // chat without messages
        "{\"about\":\"a\",\"chats\":{\"about\":\"a\",\"list\":[{\"name\":\"g\",\"type\":\"private_group\",\"id\":1}]}}",
        // chat messages before id
        "{\"about\":\"a\",\"chats\":{\"about\":\"a\",\"list\":[{\"type\":\"private_group\",\"messages\":[],\"id\":1}]}}",
        // non-object chat entry
        "{\"about\":\"a\",\"chats\":{\"about\":\"a\",\"list\":[7]}}",
        // chats section without about
        "{\"about\":\"a\",\"chats\":{\"list\":[]}}",
        // chats section without list
        "{\"about\":\"a\",\"chats\":{\"about\":\"a\"}}",
        // chats section with unknown extra key
        "{\"about\":\"a\",\"chats\":{\"about\":\"a\",\"list\":[],\"extra\":1}}",
        // chats section not an object
        "{\"about\":\"a\",\"chats\":[]}",
        // message entry not an object
        "{\"about\":\"a\",\"chats\":{\"about\":\"a\",\"list\":[{\"name\":\"g\",\"type\":\"private_group\",\"id\":1,\"messages\":[\"x\"]}]}}",
    ] {
        let result = scan_string(input);
        assert!(result.is_err(), "bad chat shape must be rejected: {input:?}");
    }
}

#[test]
fn truncated_inputs_fail() {
    let full = fixture_bytes("single-chat.json");
    let significant = full
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| index + 1)
        .unwrap_or(0);
    for cut in [1_usize, 10, significant / 2, significant - 1] {
        let result = read_telegram_json(
            &full[..cut],
            &default_limits(),
            &AtomicBool::new(false),
            |_chat| Ok(()),
            |_frame| Ok(ScanControl::Continue),
        );
        assert!(result.is_err(), "truncated at {cut} must fail");
    }
    let account = fixture_bytes("account-export.json");
    let account_significant = account
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| index + 1)
        .unwrap_or(0);
    let result = read_telegram_json(
        &account[..account_significant - 2],
        &default_limits(),
        &AtomicBool::new(false),
        |_chat| Ok(()),
        |_frame| Ok(ScanControl::Continue),
    );
    assert!(result.is_err(), "truncated account export must fail");
}

#[test]
fn trailing_garbage_after_root_fails() {
    let mut input = fixture_bytes("single-chat.json");
    input.extend_from_slice(b" \n trailing");
    let result = read_telegram_json(
        &input[..],
        &default_limits(),
        &AtomicBool::new(false),
        |_chat| Ok(()),
        |_frame| Ok(ScanControl::Continue),
    );
    assert!(result.is_err());
}

#[test]
fn invalid_utf8_and_bad_escapes_fail() {
    for input in [
        // invalid UTF-8 inside chat name
        b"{\"name\":\"a\xFFb\",\"type\":\"x\",\"id\":1,\"messages\":[]}".to_vec(),
        // invalid UTF-8 inside a skipped contacts section
        b"{\"about\":\"a\",\"contacts\":{\"about\":\"a\",\"list\":[{\"x\":\"\xC0\xC1\"}]},\"chats\":{\"about\":\"a\",\"list\":[]}}".to_vec(),
        // invalid escape sequence
        b"{\"name\":\"a\\xb\",\"type\":\"x\",\"id\":1,\"messages\":[]}".to_vec(),
        // raw control character inside string
        b"{\"name\":\"a\nb\",\"type\":\"x\",\"id\":1,\"messages\":[]}".to_vec(),
        // unpaired surrogate
        b"{\"name\":\"a\\ud800b\",\"type\":\"x\",\"id\":1,\"messages\":[]}".to_vec(),
        // broken short unicode escape
        b"{\"name\":\"a\\u12\",\"type\":\"x\",\"id\":1,\"messages\":[]}".to_vec(),
        // invalid UTF-8 lead byte inside message text
        b"{\"type\":\"x\",\"id\":1,\"messages\":[{\"id\":1,\"text\":\"\xF8\"}]}".to_vec(),
    ] {
        let result = read_telegram_json(
            &input[..],
            &default_limits(),
            &AtomicBool::new(false),
            |_chat| Ok(()),
            |_frame| Ok(ScanControl::Continue),
        );
        assert!(result.is_err(), "bad text must be rejected");
    }
}

#[test]
fn oversized_message_fails() {
    let mut big_text = String::from("{\"type\":\"x\",\"id\":1,\"messages\":[{\"id\":1,\"text\":\"");
    big_text.push_str(&"x".repeat(2 << 20));
    big_text.push_str("\"}]}");
    let limits = TelegramArchiveLimits {
        max_message_bytes: 1 << 20,
        ..default_limits()
    };
    let result = read_telegram_json(
        big_text.as_bytes(),
        &limits,
        &AtomicBool::new(false),
        |_chat| Ok(()),
        |_frame| Ok(ScanControl::Continue),
    );
    let error = result.expect_err("oversized message must fail");
    assert!(
        error.contains("размера сообщения"),
        "unexpected error: {error}"
    );
}

#[test]
fn message_budget_fails_when_exceeded() {
    let input = fixture_bytes("single-chat.json");
    for (max_messages, should_fail) in [(1_u64, true), (2, false), (3, false)] {
        let limits = TelegramArchiveLimits {
            max_messages,
            ..default_limits()
        };
        let result = read_telegram_json(
            &input[..],
            &limits,
            &AtomicBool::new(false),
            |_chat| Ok(()),
            |_frame| Ok(ScanControl::Continue),
        );
        if should_fail {
            assert!(result.is_err(), "limit {max_messages} must fail");
        } else {
            assert!(result.is_ok(), "limit {max_messages} must pass");
        }
    }
}

#[test]
fn account_export_message_budget_is_global() {
    let input = fixture_bytes("account-export.json");
    let limits = TelegramArchiveLimits {
        max_messages: 1,
        ..default_limits()
    };
    let result = read_telegram_json(
        &input[..],
        &limits,
        &AtomicBool::new(false),
        |_chat| Ok(()),
        |_frame| Ok(ScanControl::Continue),
    );
    assert!(result.is_err());
}

#[test]
fn input_budget_fails() {
    let input = fixture_bytes("single-chat.json");
    let limits = TelegramArchiveLimits {
        max_input_bytes: 64,
        ..default_limits()
    };
    let result = read_telegram_json(
        &input[..],
        &limits,
        &AtomicBool::new(false),
        |_chat| Ok(()),
        |_frame| Ok(ScanControl::Continue),
    );
    let error = result.expect_err("input budget must fail");
    assert!(
        error.contains("входного файла"),
        "unexpected error: {error}"
    );
}

#[test]
fn non_positive_limits_fail() {
    let input = fixture_bytes("single-chat.json");
    for limits in [
        TelegramArchiveLimits {
            max_input_bytes: 0,
            ..default_limits()
        },
        TelegramArchiveLimits {
            max_message_bytes: 0,
            ..default_limits()
        },
        TelegramArchiveLimits {
            max_metadata_bytes: 0,
            ..default_limits()
        },
        TelegramArchiveLimits {
            max_messages: 0,
            ..default_limits()
        },
        TelegramArchiveLimits {
            max_duration: Duration::ZERO,
            ..default_limits()
        },
    ] {
        let result = read_telegram_json(
            &input[..],
            &limits,
            &AtomicBool::new(false),
            |_chat| Ok(()),
            |_frame| Ok(ScanControl::Continue),
        );
        assert!(result.is_err(), "non-positive limits must fail");
    }
}

#[test]
fn metadata_budget_fails() {
    let long_name = format!(
        "{{\"name\":\"{}\",\"type\":\"x\",\"id\":1,\"messages\":[]}}",
        "n".repeat(5000)
    );
    let result = scan_string(&long_name);
    let error = result.expect_err("oversized metadata must fail");
    assert!(error.contains("метаданных"), "unexpected error: {error}");
}

#[test]
fn cancellation_returns_explicit_error() {
    let input = fixture_bytes("single-chat.json");
    let cancelled = AtomicBool::new(false);
    let result = read_telegram_json(
        ChunkedReader::new(input.clone(), 17),
        &default_limits(),
        &cancelled,
        |_chat| Ok(()),
        |frame| {
            assert_eq!(frame.chat.id, "123456789");
            cancelled.store(true, Ordering::SeqCst);
            Ok(ScanControl::Continue)
        },
    );
    let error = result.expect_err("cancelled scan must fail");
    assert!(error.contains("отменено"), "unexpected error: {error}");
}

#[test]
fn cancellation_interrupts_inside_long_value() {
    let mut big_text = String::from("{\"type\":\"x\",\"id\":1,\"messages\":[{\"id\":1,\"text\":\"");
    big_text.push_str(&"x".repeat(8 << 20));
    big_text.push_str("\"}]}");
    let cancelled = AtomicBool::new(true);
    let result = read_telegram_json(
        ChunkedReader::new(big_text.into_bytes(), 8192),
        &default_limits(),
        &cancelled,
        |_chat| Ok(()),
        |_frame| Ok(ScanControl::Continue),
    );
    let error = result.expect_err("pre-cancelled scan must fail");
    assert!(error.contains("отменено"), "unexpected error: {error}");
}

#[test]
fn stop_is_intentional_pause_without_false_eof() {
    let input = fixture_bytes("account-export.json");
    let mut seen = 0_u64;
    let summary = read_telegram_json(
        ChunkedReader::new(input.clone(), 11),
        &default_limits(),
        &AtomicBool::new(false),
        |_chat| Ok(()),
        |_frame| {
            seen += 1;
            if seen == 2 {
                Ok(ScanControl::Stop)
            } else {
                Ok(ScanControl::Continue)
            }
        },
    )
    .expect("pause must not be an error");
    assert!(!summary.complete, "stop is a pause, not EOF");
    assert_eq!(summary.scanned_messages, 2);
    assert!(
        summary.bytes_read < input.len() as u64,
        "paused scan must not claim the whole input was read"
    );
}

#[test]
fn deadline_interrupts_slow_reader() {
    let input = fixture_bytes("account-export.json");
    let started = Instant::now();
    let limits = TelegramArchiveLimits {
        max_duration: Duration::from_millis(60),
        ..default_limits()
    };
    let result = read_telegram_json(
        SlowReader {
            bytes: input,
            pos: 0,
            delay: Duration::from_millis(4),
        },
        &limits,
        &AtomicBool::new(false),
        |_chat| Ok(()),
        |_frame| Ok(ScanControl::Continue),
    );
    let error = result.expect_err("deadline must fail the scan");
    assert!(error.contains("время"), "unexpected error: {error}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "deadline must interrupt promptly, took {:?}",
        started.elapsed()
    );
}

#[test]
fn reader_io_error_fails_explicitly() {
    let result = read_telegram_json(
        FailingReader,
        &default_limits(),
        &AtomicBool::new(false),
        |_chat| Ok(()),
        |_frame| Ok(ScanControl::Continue),
    );
    let error = result.expect_err("io error must surface");
    assert!(error.contains("чтения"), "unexpected error: {error}");
}

#[test]
fn chat_and_message_callback_errors_propagate() {
    let input = fixture_bytes("single-chat.json");
    let result = read_telegram_json(
        &input[..],
        &default_limits(),
        &AtomicBool::new(false),
        |_chat| Err("chat boom".into()),
        |_frame| Ok(ScanControl::Continue),
    );
    assert_eq!(result.expect_err("chat error must propagate"), "chat boom");
    let result = read_telegram_json(
        &input[..],
        &default_limits(),
        &AtomicBool::new(false),
        |_chat| Ok(()),
        |_frame| Err("message boom".into()),
    );
    assert_eq!(
        result.expect_err("message error must propagate"),
        "message boom"
    );
}

#[test]
fn bom_prefixed_export_is_accepted() {
    let mut input = vec![0xEF, 0xBB, 0xBF];
    input.extend_from_slice(&fixture_bytes("single-chat.json"));
    let scan = scan_bytes(&input);
    assert!(scan.summary.complete);
    assert_eq!(scan.summary.scanned_messages, 2);
    assert_eq!(scan.summary.bytes_read, input.len() as u64);
    for frame in &scan.messages {
        let source_slice = &input[frame.byte_start as usize..frame.byte_end as usize];
        assert_eq!(source_slice, frame.raw_json.as_slice());
    }
}

#[test]
fn deep_nesting_inside_messages_is_bounded_and_skipped_sections_validate() {
    let mut deep = String::from("{\"type\":\"x\",\"id\":1,\"messages\":[{\"id\":1,\"text\":");
    deep.push_str(&"[".repeat(600));
    deep.push('1');
    deep.push_str(&"]".repeat(600));
    deep.push_str("}]}");
    let result = scan_string(&deep);
    assert!(result.is_err(), "excessive nesting must be rejected");

    let mut nested = String::from(
        "{\"about\":\"a\",\"other_data\":{\"about\":\"a\",\"list\":[[[[{\"k\":[1,2,{\"s\":\"v\"}]}]]]]",
    );
    nested.push_str("},\"chats\":{\"about\":\"a\",\"list\":[]}}");
    let scan = scan_string_collect(&nested);
    assert!(scan.summary.complete);
}

#[test]
fn large_synthetic_archive_scans_without_whole_file_buffers() {
    let mut archive = String::with_capacity(24 << 20);
    archive.push_str("{\"about\":\"large synthetic export\",");
    archive.push_str("\"contacts\":{\"about\":\"bulk\",\"list\":[");
    for index in 0..1500_u32 {
        if index > 0 {
            archive.push(',');
        }
        archive.push_str(&format!(
            "{{\"first_name\":\"Contact {index}\",\"last_name\":null,\"phone_number\":\"+{index}\",\"date\":\"2020-01-01\",\"date_unixtime\":\"1577836800\"}}"
        ));
    }
    archive.push_str("]},\"chats\":{\"about\":\"large synthetic chats\",\"list\":[");
    let chat_break = 6000_u32;
    for message_id in 1..=10_400_u32 {
        if message_id == 1 {
            archive.push_str(
                "{\"name\":\"Chat A\",\"type\":\"private_supergroup\",\"id\":900001,\"messages\":[",
            );
        }
        if message_id == chat_break + 1 {
            archive.push_str(
                "]},{\"name\":\"Chat B\",\"type\":\"personal_chat\",\"id\":900002,\"messages\":[",
            );
        } else if message_id > 1 {
            archive.push(',');
        }
        archive.push_str(&format!(
            "{{\"id\":{message_id},\"type\":\"message\",\"date\":\"2023-01-01T00:00:00\",\"date_unixtime\":\"1672531200\",\"from\":\"Author\",\"from_id\":\"user900001\",\"text\":"
        ));
        archive.push_str("\"фрагмент 😀 с переносом\\nи кавычкой \\\" и слэшем \\\\\\t");
        archive.push_str(&"x".repeat(1700 + (message_id as usize % 97)));
        archive.push_str("\"}");
    }
    archive.push_str("]}]}}");
    let bytes = archive.into_bytes();
    assert!(
        bytes.len() > 16 << 20,
        "fixture must exceed 16 MiB: {}",
        bytes.len()
    );

    let path = std::env::temp_dir().join(format!(
        "proteus-telegram-archive-large-{}-{}.json",
        std::process::id(),
        bytes.len()
    ));
    fs::write(&path, &bytes).unwrap();
    let file = fs::File::open(&path).unwrap();
    let file_len = bytes.len() as u64;

    struct BoundedSink {
        count: u64,
        last_span: Option<(u64, u64)>,
        previous_end: u64,
        samples: Vec<(u64, String)>,
    }

    let mut chats_seen = Vec::new();
    let mut sink = BoundedSink {
        count: 0,
        last_span: None,
        previous_end: 0,
        samples: Vec::new(),
    };
    let started = Instant::now();
    let summary = read_telegram_json(
        CappedFileReader { file, chunk: 7 },
        &TelegramArchiveLimits {
            max_input_bytes: 64 << 20,
            max_message_bytes: 1 << 20,
            max_metadata_bytes: 4096,
            max_messages: 20_000,
            max_duration: Duration::from_secs(300),
        },
        &AtomicBool::new(false),
        |chat| {
            chats_seen.push(chat.id.clone());
            Ok(())
        },
        |frame| {
            assert!(
                frame.byte_start >= sink.previous_end,
                "spans must be ordered"
            );
            sink.previous_end = frame.byte_end;
            sink.last_span = Some((frame.byte_start, frame.byte_end));
            sink.count += 1;
            if sink.count == 1 || sink.count == 10_400 || sink.count.is_multiple_of(1000) {
                let parsed: serde_json::Value = serde_json::from_slice(&frame.raw_json)
                    .expect("every sampled frame must be valid JSON");
                sink.samples
                    .push((sink.count, parsed["id"].as_u64().unwrap().to_string()));
            }
            assert!(frame.raw_json.len() <= 1 << 20);
            Ok(ScanControl::Continue)
        },
    )
    .expect("large archive must scan to completion");

    fs::remove_file(&path).ok();
    assert!(summary.complete);
    assert_eq!(summary.scanned_messages, 10_400);
    assert_eq!(summary.bytes_read, file_len);
    assert_eq!(chats_seen, ["900001", "900002"]);
    assert_eq!(sink.count, 10_400);
    assert_eq!(sink.last_span.unwrap().1, file_len - "]}]}}".len() as u64);
    assert_eq!(sink.samples[0].1, "1");
    assert_eq!(sink.samples.last().unwrap().1, "10400");
    eprintln!(
        "large scan of {} bytes took {:?}",
        file_len,
        started.elapsed()
    );
}

// ---------------------------------------------------------------------------
// Hardening regressions (parent review round 2).
// ---------------------------------------------------------------------------

/// Regression 1: `scan_number_raw` must write the original exponent marker
/// byte (`E` vs `e`) into the capture and preserve every number spelling byte.
#[test]
fn number_spelling_and_exponent_case_are_preserved_verbatim() {
    let mut doc = String::from("{\"type\":\"x\",\"id\":1,\"messages\":[");
    doc.push_str(
        "{\"id\":1,\"a\":1E+02,\"b\":[2.5E-3, 3e+4 ,-0.5E0],\"c\":{\"d\":6E6,\"e\":-7.25E-1}}",
    );
    doc.push_str(",{\"id\":2,\"n\":0.125,\"m\":1e0,\"k\":-0}");
    doc.push_str("]}");
    let input = doc.into_bytes();
    let scan = scan_bytes(&input);
    assert!(scan.summary.complete);
    assert_eq!(scan.messages.len(), 2);
    for frame in &scan.messages {
        assert_eq!(
            &input[frame.byte_start as usize..frame.byte_end as usize],
            frame.raw_json.as_slice(),
            "raw message must equal the exact input slice"
        );
    }
    let first = String::from_utf8(scan.messages[0].raw_json.clone()).unwrap();
    for spelling in ["1E+02", "2.5E-3", "3e+4", "-0.5E0", "6E6", "-7.25E-1"] {
        assert!(
            first.contains(spelling),
            "number spelling {spelling} must survive verbatim: {first}"
        );
    }
    let parsed: serde_json::Value =
        serde_json::from_slice(&scan.messages[0].raw_json).expect("raw frame must be valid JSON");
    assert_eq!(parsed["a"].as_f64().unwrap(), 100.0);
    let second = String::from_utf8(scan.messages[1].raw_json.clone()).unwrap();
    assert!(second.contains("0.125") && second.contains("1e0") && second.contains("-0"));
}

/// Regression 2: the chat-object loop must not accept a comma before the
/// first key, and the same comma grammar must hold in every envelope.
#[test]
fn chat_objects_reject_bad_comma_grammar_in_every_envelope() {
    let bad = [
        // comma before the first key of a nested chat
        "{\"about\":\"a\",\"chats\":{\"about\":\"a\",\"list\":[{,\"type\":\"private_group\",\"id\":1,\"messages\":[]}]}}",
        // double comma inside a nested chat
        "{\"about\":\"a\",\"chats\":{\"about\":\"a\",\"list\":[{\"type\":\"private_group\",,\"id\":1,\"messages\":[]}]}}",
        // trailing comma inside a nested chat
        "{\"about\":\"a\",\"chats\":{\"about\":\"a\",\"list\":[{\"type\":\"private_group\",\"id\":1,\"messages\":[],}]}}",
        // missing comma inside a nested chat
        "{\"about\":\"a\",\"chats\":{\"about\":\"a\",\"list\":[{\"type\":\"private_group\" \"id\":1,\"messages\":[]}]}}",
        // comma before the first key of the single-peer root
        "{,\"type\":\"private_group\",\"id\":1,\"messages\":[]}",
        // double comma inside the single-peer root
        "{\"name\":\"a\",,\"type\":\"private_group\",\"id\":1,\"messages\":[]}",
        // trailing comma inside the single-peer root
        "{\"type\":\"private_group\",\"id\":1,\"messages\":[],}",
        // double comma between account root sections
        "{\"about\":\"a\",,\"chats\":{\"about\":\"a\",\"list\":[]}}",
        // double comma inside the chats section object
        "{\"about\":\"a\",\"chats\":{\"about\":\"a\",,\"list\":[]}}",
    ];
    for input in bad {
        let result = scan_string(input);
        assert!(
            result.is_err(),
            "bad comma grammar must be rejected: {input:?}"
        );
    }
    // Positive control: well-formed grammar still parses in both envelopes.
    let scan = scan_string_collect(
        "{\"about\":\"a\",\"chats\":{\"about\":\"a\",\"list\":[{\"type\":\"private_group\",\"id\":1,\"messages\":[]}]}}",
    );
    assert!(scan.summary.complete);
    assert_eq!(scan.chats[0].id, "1");
}

/// Regression 3: cancellation or deadline firing during a blocking read must
/// not surface as a clean EOF. `Scanner::fill` is the sound place to enforce
/// this: the first `peek` after the event must be the interrupt error, never
/// `Ok(None)`, and the top-level scan must never report `complete = true`.
#[test]
fn interrupt_during_read_is_not_clean_eof() {
    struct CancelOnEof<'a> {
        data: Vec<u8>,
        pos: usize,
        flag: &'a AtomicBool,
    }
    impl Read for CancelOnEof<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos == self.data.len() {
                self.flag.store(true, Ordering::SeqCst);
                return Ok(0);
            }
            let take = (self.data.len() - self.pos).min(buf.len());
            buf[..take].copy_from_slice(&self.data[self.pos..self.pos + take]);
            self.pos += take;
            Ok(take)
        }
    }
    struct SlowEof {
        data: Vec<u8>,
        pos: usize,
        delay: Duration,
    }
    impl Read for SlowEof {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos == self.data.len() {
                std::thread::sleep(self.delay);
                return Ok(0);
            }
            let take = (self.data.len() - self.pos).min(buf.len());
            buf[..take].copy_from_slice(&self.data[self.pos..self.pos + take]);
            self.pos += take;
            Ok(take)
        }
    }
    // Sets cancellation while returning a data byte, in one-byte chunks, so
    // the event is guaranteed to fire inside a blocking read.
    struct CancelOnDataRead<'a> {
        data: Vec<u8>,
        pos: usize,
        flag: &'a AtomicBool,
    }
    impl Read for CancelOnDataRead<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos == self.data.len() {
                return Ok(0);
            }
            self.flag.store(true, Ordering::SeqCst);
            buf[..1].copy_from_slice(&self.data[self.pos..self.pos + 1]);
            self.pos += 1;
            Ok(1)
        }
    }
    // Sleeps past the deadline while serving the first data byte.
    struct SlowFirstRead {
        data: Vec<u8>,
        pos: usize,
        delay: Duration,
    }
    impl Read for SlowFirstRead {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos == self.data.len() {
                return Ok(0);
            }
            std::thread::sleep(self.delay);
            buf[..1].copy_from_slice(&self.data[self.pos..self.pos + 1]);
            self.pos += 1;
            Ok(1)
        }
    }

    // Scanner level: a peek whose fill performed an eventful read must
    // surface the interrupt error, not the read result.
    let input = fixture_bytes("single-chat.json");
    let flag = AtomicBool::new(false);
    let limits = default_limits();
    let mut scanner = Scanner::new(
        CancelOnDataRead {
            data: input.clone(),
            pos: 0,
            flag: &flag,
        },
        &limits,
        &flag,
    );
    let error = scanner
        .peek()
        .expect_err("cancellation fired during a read must surface immediately");
    assert!(error.contains("отменено"), "unexpected error: {error}");

    let limits = TelegramArchiveLimits {
        max_duration: Duration::from_millis(100),
        ..default_limits()
    };
    let never = AtomicBool::new(false);
    let mut scanner = Scanner::new(
        SlowFirstRead {
            data: input.clone(),
            pos: 0,
            delay: Duration::from_millis(300),
        },
        &limits,
        &never,
    );
    let error = scanner
        .peek()
        .expect_err("deadline expiring during a read must surface immediately");
    assert!(error.contains("время"), "unexpected error: {error}");

    // Top level: whatever the timing, a scan must never end as
    // `Ok(complete = true)` once the event fired during the final read.
    let flag = AtomicBool::new(false);
    let result = read_telegram_json(
        CancelOnEof {
            data: input.clone(),
            pos: 0,
            flag: &flag,
        },
        &default_limits(),
        &flag,
        |_chat| Ok(()),
        |_frame| Ok(ScanControl::Continue),
    );
    let error = result.expect_err("cancellation during the EOF read must fail");
    assert!(error.contains("отменено"), "unexpected error: {error}");

    let result = read_telegram_json(
        SlowEof {
            data: input.clone(),
            pos: 0,
            delay: Duration::from_millis(300),
        },
        &TelegramArchiveLimits {
            max_duration: Duration::from_millis(100),
            ..default_limits()
        },
        &AtomicBool::new(false),
        |_chat| Ok(()),
        |_frame| Ok(ScanControl::Continue),
    );
    let error = result.expect_err("deadline expiring during the EOF read must fail");
    assert!(error.contains("время"), "unexpected error: {error}");
}

/// Regression 4: `fill` must request at most `remaining budget + 1` probe
/// byte, must enforce the budget by bytes loaded, and a malformed `Read`
/// impl claiming more bytes than requested must fail explicitly, not panic.
#[test]
fn fill_requests_are_bounded_by_remaining_budget_plus_probe() {
    struct TrackingReader {
        data: Vec<u8>,
        pos: usize,
        requests: Arc<Mutex<Vec<usize>>>,
    }
    impl Read for TrackingReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.requests.lock().unwrap().push(buf.len());
            if self.pos == self.data.len() {
                return Ok(0);
            }
            let take = (self.data.len() - self.pos).min(buf.len());
            buf[..take].copy_from_slice(&self.data[self.pos..self.pos + take]);
            self.pos += take;
            Ok(take)
        }
    }

    // Over-budget input: requests must never exceed remaining + 1.
    let input = fixture_bytes("single-chat.json");
    assert!(input.len() > 64, "fixture must exceed the test budget");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let result = read_telegram_json(
        TrackingReader {
            data: input.clone(),
            pos: 0,
            requests: Arc::clone(&requests),
        },
        &TelegramArchiveLimits {
            max_input_bytes: 64,
            ..default_limits()
        },
        &AtomicBool::new(false),
        |_chat| Ok(()),
        |_frame| Ok(ScanControl::Continue),
    );
    let error = result.expect_err("over-budget input must fail");
    assert!(
        error.contains("входного файла"),
        "unexpected error: {error}"
    );
    let requested = requests.lock().unwrap();
    assert!(
        requested.iter().all(|&size| size <= 65),
        "requests must be bounded by remaining + 1 probe byte: {requested:?}"
    );

    // Exact-fit input: the scan completes and the final EOF probe requests
    // exactly one byte.
    let exact = b"{\"type\":\"x\",\"id\":1,\"messages\":[]}";
    let requests = Arc::new(Mutex::new(Vec::new()));
    let summary = read_telegram_json(
        TrackingReader {
            data: exact.to_vec(),
            pos: 0,
            requests: Arc::clone(&requests),
        },
        &TelegramArchiveLimits {
            max_input_bytes: exact.len() as u64,
            ..default_limits()
        },
        &AtomicBool::new(false),
        |_chat| Ok(()),
        |_frame| Ok(ScanControl::Continue),
    )
    .expect("exact-fit input must scan to completion");
    assert!(summary.complete);
    assert_eq!(summary.bytes_read, exact.len() as u64);
    let requested = requests.lock().unwrap();
    assert!(
        requested.contains(&1),
        "the EOF probe must request exactly one byte: {requested:?}"
    );
    assert!(
        requested.iter().all(|&size| size <= exact.len() + 1),
        "requests must stay within remaining + 1: {requested:?}"
    );

    // One byte past the budget: rejected instead of delivering the probe byte.
    let result = read_telegram_json(
        TrackingReader {
            data: exact.to_vec(),
            pos: 0,
            requests: Arc::new(Mutex::new(Vec::new())),
        },
        &TelegramArchiveLimits {
            max_input_bytes: exact.len() as u64 - 1,
            ..default_limits()
        },
        &AtomicBool::new(false),
        |_chat| Ok(()),
        |_frame| Ok(ScanControl::Continue),
    );
    let error = result.expect_err("input one byte over budget must fail");
    assert!(
        error.contains("входного файла"),
        "unexpected error: {error}"
    );

    // Malformed Read impl returning more bytes than requested: explicit error.
    struct LyingReader {
        done: bool,
    }
    impl Read for LyingReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.done {
                return Ok(0);
            }
            self.done = true;
            Ok(buf.len() + 7)
        }
    }
    let result = read_telegram_json(
        LyingReader { done: false },
        &default_limits(),
        &AtomicBool::new(false),
        |_chat| Ok(()),
        |_frame| Ok(ScanControl::Continue),
    );
    let error = result.expect_err("lying reader must fail explicitly without panicking");
    assert!(
        error.contains("больше байт, чем запрошено"),
        "unexpected error: {error}"
    );
}

/// Regression 5: leading JSON whitespace (including after a BOM) is accepted
/// while absolute byte offsets stay exact.
#[test]
fn leading_whitespace_is_accepted_with_exact_offsets() {
    let fixture = fixture_bytes("single-chat.json");
    for prefix in [
        b" ".to_vec(),
        b"\t\r\n ".to_vec(),
        b" \n\t  ".to_vec(),
        vec![0xEF, 0xBB, 0xBF, b' ', b'\n'],
    ] {
        let mut input = prefix;
        input.extend_from_slice(&fixture);
        let scan = scan_bytes(&input);
        assert!(
            scan.summary.complete,
            "leading whitespace/BOM must be accepted"
        );
        assert_eq!(scan.summary.scanned_messages, 2);
        for frame in &scan.messages {
            assert_eq!(
                &input[frame.byte_start as usize..frame.byte_end as usize],
                frame.raw_json.as_slice(),
                "offsets must stay absolute with leading whitespace"
            );
        }
    }
}

/// Regression 6: lone UTF-16 surrogates are rejected in message bodies and
/// skipped sections, a valid pair is preserved as escaped bytes, and a small
/// representative escaped-Unicode message scans identically at every byte
/// split point.
#[test]
fn lone_surrogates_are_rejected_and_pairs_survive_every_split() {
    for doc in [
        // lone high surrogate in a message body
        "{\"type\":\"x\",\"id\":1,\"messages\":[{\"id\":1,\"text\":\"a\\ud800b\"}]}",
        // lone low surrogate in a message body
        "{\"type\":\"x\",\"id\":1,\"messages\":[{\"id\":1,\"text\":\"a\\udc00b\"}]}",
        // high surrogate followed by a non-`\u` escape
        "{\"type\":\"x\",\"id\":1,\"messages\":[{\"id\":1,\"text\":\"\\ud83d\\n\"}]}",
        // high surrogate followed by a non-surrogate `\u` escape
        "{\"type\":\"x\",\"id\":1,\"messages\":[{\"id\":1,\"text\":\"\\ud83d\\u0041\"}]}",
        // lone surrogate in a skipped section
        "{\"about\":\"a\",\"contacts\":{\"about\":\"c\",\"list\":[{\"x\":\"\\ud800\"}]},\"chats\":{\"about\":\"a\",\"list\":[]}}",
        // lone surrogate in a chat header (unescape path)
        "{\"name\":\"a\\ud800\",\"type\":\"x\",\"id\":1,\"messages\":[]}",
    ] {
        let result = scan_string(doc);
        assert!(result.is_err(), "lone surrogate must be rejected: {doc:?}");
    }

    // A valid pair is accepted and kept exactly as escaped bytes.
    let doc = "{\"type\":\"x\",\"id\":1,\"messages\":[{\"id\":1,\"text\":\"a\\ud83d\\ude00b\"}]}";
    let scan = scan_string_collect(doc);
    assert!(scan.summary.complete);
    let pair = b"\\ud83d\\ude00";
    assert!(
        scan.messages[0]
            .raw_json
            .windows(pair.len())
            .any(|window| window == pair),
        "valid pair must stay escaped: {}",
        String::from_utf8_lossy(&scan.messages[0].raw_json)
    );

    // Every byte-split point of a representative escaped-Unicode message.
    let doc = "{\"type\":\"x\",\"id\":1,\"messages\":[{\"id\":1,\"text\":\"A\\u043d\\ud83d\\ude00\\u2028\\n\"}]}";
    let input = doc.as_bytes();
    let expected_raw: &[u8] = br#"{"id":1,"text":"A\u043d\ud83d\ude00\u2028\n"}"#;
    for chunk in 1..=input.len() {
        let mut frames = Vec::new();
        let summary = read_telegram_json(
            ChunkedReader::new(input.to_vec(), chunk),
            &default_limits(),
            &AtomicBool::new(false),
            |_chat| Ok(()),
            |frame| {
                frames.push(frame);
                Ok(ScanControl::Continue)
            },
        )
        .unwrap_or_else(|error| panic!("chunk {chunk} must scan: {error}"));
        assert!(summary.complete, "chunk {chunk} must reach EOF");
        assert_eq!(frames.len(), 1, "chunk {chunk} must yield one message");
        assert_eq!(
            frames[0].raw_json, expected_raw,
            "chunk {chunk} must preserve escaped bytes exactly"
        );
        assert_eq!(
            &input[frames[0].byte_start as usize..frames[0].byte_end as usize],
            frames[0].raw_json.as_slice(),
            "chunk {chunk} offsets must be exact"
        );
    }

    // Invalid surrogate documents must fail at every split point too.
    for doc in [
        "{\"type\":\"x\",\"id\":1,\"messages\":[{\"id\":1,\"text\":\"\\ud800\"}]}",
        "{\"type\":\"x\",\"id\":1,\"messages\":[{\"id\":1,\"text\":\"x\\udc00\"}]}",
    ] {
        let input = doc.as_bytes();
        for chunk in 1..=input.len() {
            let result = read_telegram_json(
                ChunkedReader::new(input.to_vec(), chunk),
                &default_limits(),
                &AtomicBool::new(false),
                |_chat| Ok(()),
                |_frame| Ok(ScanControl::Continue),
            );
            assert!(
                result.is_err(),
                "chunk {chunk} must reject lone surrogate in {doc:?}"
            );
        }
    }
}

/// Regression 7: the chat id must be an exact integer token as emitted by
/// the official exporter; fractions and exponents are rejected instead of
/// being coerced into a fabricated identity.
#[test]
fn chat_id_requires_an_exact_integer_token() {
    for id_token in ["1.5", "1e3", "1E3", "-1.0", "0.5", "1e+2", "-2E-1"] {
        let doc = format!("{{\"type\":\"x\",\"id\":{id_token},\"messages\":[]}}");
        let result = scan_string(&doc);
        assert!(
            result.is_err(),
            "fraction/exponent chat id must be rejected: {doc}"
        );
    }
    for (id_token, expected) in [
        (
            "-100123456789012345678901234567890",
            "-100123456789012345678901234567890",
        ),
        (
            "999999999999999999999999999999",
            "999999999999999999999999999999",
        ),
        ("-0", "-0"),
        ("0", "0"),
    ] {
        let doc = format!("{{\"type\":\"x\",\"id\":{id_token},\"messages\":[]}}");
        let scan = scan_string_collect(&doc);
        assert_eq!(scan.chats[0].id, expected, "id digits must stay verbatim");
    }
    for id_token in ["01", "-01", "+1"] {
        let doc = format!("{{\"type\":\"x\",\"id\":{id_token},\"messages\":[]}}");
        assert!(scan_string(&doc).is_err(), "bad integer must fail: {doc}");
    }
    // Message bodies keep the full JSON number grammar.
    let scan =
        scan_string_collect("{\"type\":\"x\",\"id\":7,\"messages\":[{\"id\":1.5,\"v\":1E+02}]}");
    assert!(scan.summary.complete);
    assert_eq!(scan.chats[0].id, "7");
}

/// Differential grammar check against serde_json for a bounded corpus of
/// JSON values embedded as message fields.
#[test]
fn message_value_grammar_matches_serde_json() {
    let valid = [
        "1E+02",
        "1e+02",
        "1E-02",
        "2.5E3",
        "-0",
        "0",
        "-12",
        "0.125",
        "1e0",
        "18446744073709551615",
        "3.402823e38",
        "\"plain\"",
        "\"esc \\\" \\\\ \\/ \\b \\f \\n \\r \\t\"",
        "\"uni \\u00e9\\u4e2d\\ud83d\\ude00\"",
        "true",
        "false",
        "null",
        "[]",
        "{}",
        "[1,2,3]",
        "{\"a\":1}",
        "[[[[{\"k\":[1,2,{\"s\":\"v\"}]}]]]]",
        "[ 1 , { \"a\" : [ true , null ] } , \"x\" ]",
    ];
    let invalid = [
        "01",
        "-01",
        "1.",
        ".5",
        "+1",
        "1e",
        "1e+",
        "--1",
        "1..2",
        "0x10",
        "\"unterminated",
        "\"bad \\x escape\"",
        "\"lone \\ud800 lead\"",
        "\"\\udc00 trail\"",
        "\"raw\ttab\"",
        "tru",
        "True",
        "nul",
        "nulll",
        "[1,]",
        "[,1]",
        "[1 2]",
        "[1,,2]",
        "{\"a\":1,}",
        "{,\"a\":1}",
        "{\"a\" 1}",
        "{\"a\":}",
        "{a:1}",
        "['x']",
        "{\"a\":1 \"b\":2}",
    ];
    for value in valid {
        let doc = format!("{{\"type\":\"x\",\"id\":1,\"messages\":[{{\"id\":1,\"v\":{value}}}]}}");
        let mut frames = Vec::new();
        let ours = read_telegram_json(
            ChunkedReader::new(doc.as_bytes().to_vec(), 7),
            &default_limits(),
            &AtomicBool::new(false),
            |_chat| Ok(()),
            |frame| {
                frames.push(frame);
                Ok(ScanControl::Continue)
            },
        )
        .is_ok();
        let theirs = serde_json::from_slice::<serde_json::Value>(doc.as_bytes()).is_ok();
        assert!(
            ours && theirs,
            "valid value must be accepted by reader and serde_json: {value} (ours={ours}, serde={theirs})"
        );
        assert_eq!(frames.len(), 1, "one message expected for {value}");
        assert_eq!(
            &doc.as_bytes()[frames[0].byte_start as usize..frames[0].byte_end as usize],
            frames[0].raw_json.as_slice(),
            "raw span must equal the input slice for {value}"
        );
        if value.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
            assert!(
                frames[0]
                    .raw_json
                    .windows(value.len())
                    .any(|window| window == value.as_bytes()),
                "number spelling must survive verbatim for {value}"
            );
        }
    }
    for value in invalid {
        let doc = format!("{{\"type\":\"x\",\"id\":1,\"messages\":[{{\"id\":1,\"v\":{value}}}]}}");
        let ours = read_telegram_json(
            ChunkedReader::new(doc.as_bytes().to_vec(), 7),
            &default_limits(),
            &AtomicBool::new(false),
            |_chat| Ok(()),
            |_frame| Ok(ScanControl::Continue),
        )
        .is_ok();
        let theirs = serde_json::from_slice::<serde_json::Value>(doc.as_bytes()).is_ok();
        assert!(
            !ours && !theirs,
            "invalid value must be rejected by reader and serde_json: {value:?} (ours={ours}, serde={theirs})"
        );
    }
}
