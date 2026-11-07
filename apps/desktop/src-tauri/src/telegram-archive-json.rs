//! Bounded streaming reader for official Telegram Desktop JSON exports.
//!
//! Supports two shapes produced by the official exporter
//! (`Telegram/SourceFiles/export/output/export_output_json.cpp` in
//! telegramdesktop/tdesktop, verified at commit
//! fa588bb82443fb23d1b9f9fe9c6baa5bc9710f91):
//!
//! - single-peer export: root object `{ "name"?: string|null, "type": string|null,
//!   "id": number, "messages": [...] }`;
//! - account export: root object starting with `"about"`, optional known
//!   sections (`personal_information`, `profile_pictures`, `stories`,
//!   `profile_music`, `contacts`, `frequent_contacts`, `other_data`,
//!   `sessions`, `web_sessions`) and dialog sections `chats` / `left_chats`,
//!   each shaped as `{ "about": string, "list": [chat...] }`.
//!
//! Chat objects keep the verified writer ordering: optional `name`, then
//! `type`, then `id`, then `messages`. Any other ordering is rejected
//! explicitly instead of inventing chat identity. Message bodies are opaque:
//! the reader validates that each `messages` entry is a well-formed JSON
//! object and hands the exact raw bytes with absolute byte offsets to the
//! caller. Normalization is a caller responsibility.
//!
//! Memory stays bounded by one small read buffer, one bounded raw message
//! buffer, and one bounded chat header; whole-file reads and generic JSON
//! trees are never materialized.

use std::io::Read;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[path = "telegram-archive-json-tokens.rs"]
mod tokens;

#[cfg(test)]
#[path = "telegram-archive-json-tests.rs"]
mod telegram_archive_json_tests;

use tokens::{unescape_json_string, Scanner, MAX_ID_TOKEN_BYTES, MAX_KEY_BYTES};

/// Budgets for a single archive scan. All values must be positive.
#[derive(Debug)]
pub struct TelegramArchiveLimits {
    /// Total input bytes the scan may consume from the reader.
    pub max_input_bytes: u64,
    /// Maximum raw byte size of a single message object.
    pub max_message_bytes: usize,
    /// Maximum decoded byte size of a single chat header field (`name`).
    pub max_metadata_bytes: usize,
    /// Maximum messages scanned across the whole archive.
    pub max_messages: u64,
    /// Wall-clock deadline for the whole scan.
    pub max_duration: Duration,
}

/// Identity of a chat as declared by the export, before any messages.
#[derive(Clone, Debug)]
pub struct TelegramChatHeader {
    /// Verbatim JSON number token from the export (never fabricated).
    pub id: String,
    /// Decoded `name`, or `None` when the key is absent or JSON `null`.
    pub name: Option<String>,
    /// Decoded `type`; empty string when the export value is JSON `null`.
    pub kind: String,
}

/// One raw message with its exact byte span in the source stream.
#[derive(Debug)]
pub struct TelegramMessageFrame {
    /// Chat identity copied from the enclosing chat header.
    pub chat: TelegramChatHeader,
    /// Exact raw JSON bytes of the message object, unmodified.
    pub raw_json: Vec<u8>,
    /// Absolute offset of the opening `{` of the message object.
    pub byte_start: u64,
    /// Absolute offset one past the closing `}` of the message object.
    pub byte_end: u64,
}

/// Caller decision after each message.
#[derive(Debug)]
pub enum ScanControl {
    /// Keep scanning.
    Continue,
    /// Intentional pause: the scan stops without an error, `complete` is
    /// `false`, and the caller may resume from the reported byte offset.
    Stop,
}

/// Outcome of a finished or paused scan.
#[derive(Debug)]
pub struct TelegramScanSummary {
    /// Messages delivered to `on_message` during this scan.
    pub scanned_messages: u64,
    /// Absolute offset reached in the source stream.
    pub bytes_read: u64,
    /// `true` only when the export was consumed up to a clean end of input.
    /// `false` after an intentional `ScanControl::Stop` pause.
    pub complete: bool,
}

impl TelegramArchiveLimits {
    fn validate(&self) -> Result<(), String> {
        if self.max_input_bytes == 0
            || self.max_message_bytes == 0
            || self.max_metadata_bytes == 0
            || self.max_messages == 0
            || self.max_duration.is_zero()
        {
            return Err("Лимиты сканирования экспорта Telegram должны быть положительными.".into());
        }
        Ok(())
    }
}

/// Streams an official Telegram Desktop JSON export.
///
/// `on_chat` is invoked once per chat before its messages (including chats
/// with empty `messages`); `on_message` runs synchronously for each message
/// and provides backpressure through `ScanControl::Stop`. Cancellation and
/// the deadline are checked between reads and inside long values. Format,
/// limit, I/O, cancellation and deadline failures are returned as explicit
/// errors; only `ScanControl::Stop` pauses with `Ok(complete = false)`.
pub fn read_telegram_json<R: Read>(
    reader: R,
    limits: &TelegramArchiveLimits,
    cancelled: &AtomicBool,
    on_chat: impl FnMut(&TelegramChatHeader) -> Result<(), String>,
    on_message: impl FnMut(TelegramMessageFrame) -> Result<ScanControl, String>,
) -> Result<TelegramScanSummary, String> {
    limits.validate()?;
    let mut scanner = Scanner::new(reader, limits, cancelled);
    scanner.skip_bom()?;
    scanner.skip_ws()?;
    scanner.expect(b'{', "корне экспорта")?;
    let mut driver = Driver {
        limits,
        scanned: 0,
        on_chat,
        on_message,
    };
    let flow = driver.scan_root(&mut scanner)?;
    if flow == Flow::Paused {
        return Ok(TelegramScanSummary {
            scanned_messages: driver.scanned,
            bytes_read: scanner.next_offset(),
            complete: false,
        });
    }
    scanner.skip_ws()?;
    if scanner.peek()?.is_some() {
        return Err("После корня экспорта Telegram остались лишние данные.".into());
    }
    Ok(TelegramScanSummary {
        scanned_messages: driver.scanned,
        bytes_read: scanner.next_offset(),
        complete: true,
    })
}

#[derive(PartialEq)]
enum Flow {
    Complete,
    Paused,
}

struct Driver<'a, OnChat, OnMessage> {
    limits: &'a TelegramArchiveLimits,
    scanned: u64,
    on_chat: OnChat,
    on_message: OnMessage,
}

impl<'a, OnChat, OnMessage> Driver<'a, OnChat, OnMessage>
where
    OnChat: FnMut(&TelegramChatHeader) -> Result<(), String>,
    OnMessage: FnMut(TelegramMessageFrame) -> Result<ScanControl, String>,
{
    fn scan_root<R: Read>(&mut self, scanner: &mut Scanner<'_, R>) -> Result<Flow, String> {
        let mut scratch: Vec<u8> = Vec::new();
        scanner.skip_ws()?;
        let first_key = self.read_key(scanner, &mut scratch)?;
        if first_key.as_slice() == b"about" {
            self.scan_account_root(scanner, &mut scratch)
        } else {
            self.scan_chat_object(scanner, &mut scratch, Some(first_key), "корне экспорта")
        }
    }

    fn read_key<R: Read>(
        &mut self,
        scanner: &mut Scanner<'_, R>,
        scratch: &mut Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        scratch.clear();
        scanner.scan_string_raw(Some(scratch), MAX_KEY_BYTES)?;
        unescape_json_string(scratch, MAX_KEY_BYTES).map(String::into_bytes)
    }

    /// Parses one chat object (single-peer export root or `list` entry).
    /// `first_key` is used for the single-peer root, where the caller already
    /// consumed the root `{` and read the first key; `list` entries instead
    /// consume their `{` here and read keys from the stream.
    fn scan_chat_object<R: Read>(
        &mut self,
        scanner: &mut Scanner<'_, R>,
        scratch: &mut Vec<u8>,
        first_key: Option<Vec<u8>>,
        context: &str,
    ) -> Result<Flow, String> {
        let mut need_comma = first_key.is_some();
        if first_key.is_none() {
            scanner.expect(b'{', context)?;
        }
        let mut builder = ChatBuilder::default();
        if let Some(key) = first_key {
            let flow = self.apply_chat_key(scanner, scratch, &key, context, &mut builder)?;
            if flow == Flow::Paused {
                return Ok(Flow::Paused);
            }
        }
        loop {
            scanner.skip_ws()?;
            match scanner.peek()? {
                Some(b'}') => {
                    scanner.next()?;
                    break;
                }
                Some(b',') if need_comma => {
                    scanner.next()?;
                    scanner.skip_ws()?;
                }
                Some(b'"') if !need_comma => {}
                _ => {
                    return Err(format!(
                        "Не удалось распознать экспорт Telegram Desktop: в {context} ожидалась запятая или конец объекта."
                    ));
                }
            }
            need_comma = true;
            let key = self.read_key(scanner, scratch)?;
            let flow = self.apply_chat_key(scanner, scratch, &key, context, &mut builder)?;
            if flow == Flow::Paused {
                return Ok(Flow::Paused);
            }
        }
        match builder.stage {
            4 => Ok(Flow::Complete),
            0 | 1 => Err(format!(
                "Не удалось распознать экспорт Telegram Desktop: в {context} отсутствует ключ \"type\"."
            )),
            2 => Err(format!(
                "Не удалось распознать экспорт Telegram Desktop: в {context} отсутствует ключ \"id\"."
            )),
            _ => Err(format!(
                "Не удалось распознать экспорт Telegram Desktop: в {context} отсутствует ключ \"messages\"."
            )),
        }
    }

    fn apply_chat_key<R: Read>(
        &mut self,
        scanner: &mut Scanner<'_, R>,
        scratch: &mut Vec<u8>,
        key: &[u8],
        context: &str,
        builder: &mut ChatBuilder,
    ) -> Result<Flow, String> {
        match key {
            b"name" if builder.stage == 0 => {
                builder.name = self.read_name(scanner, scratch)?;
                builder.stage = 1;
                Ok(Flow::Complete)
            }
            b"type" if builder.stage <= 1 => {
                builder.kind = self.read_kind(scanner, scratch)?;
                builder.stage = 2;
                Ok(Flow::Complete)
            }
            b"id" if builder.stage == 2 => {
                builder.id = self.read_id(scanner, scratch)?;
                builder.stage = 3;
                Ok(Flow::Complete)
            }
            b"messages" if builder.stage == 3 => {
                let header = builder.header();
                (self.on_chat)(&header)?;
                let flow = self.scan_messages(scanner, &header)?;
                builder.stage = 4;
                Ok(flow)
            }
            b"name" | b"type" | b"id" | b"messages" => Err(format!(
                "Не удалось распознать экспорт Telegram Desktop: ключ \"{}\" повторяется или нарушает порядок name/type/id/messages в {context}.",
                String::from_utf8_lossy(key)
            )),
            _ => Err(format!(
                "Не удалось распознать экспорт Telegram Desktop: неожиданный ключ \"{}\" в {context}.",
                String::from_utf8_lossy(key)
            )),
        }
    }

    fn scan_messages<R: Read>(
        &mut self,
        scanner: &mut Scanner<'_, R>,
        header: &TelegramChatHeader,
    ) -> Result<Flow, String> {
        colon(scanner, "messages")?;
        scanner.expect(b'[', "messages")?;
        let mut raw: Vec<u8> = Vec::new();
        scanner.skip_ws()?;
        if scanner.peek()? == Some(b']') {
            scanner.next()?;
            return Ok(Flow::Complete);
        }
        loop {
            if self.scanned == self.limits.max_messages {
                return Err("Превышен лимит сообщений экспорта Telegram.".into());
            }
            scanner.check_interrupt()?;
            scanner.skip_ws()?;
            if scanner.peek()? != Some(b'{') {
                return Err(
                    "Не удалось распознать экспорт Telegram Desktop: сообщение должно быть объектом."
                        .into(),
                );
            }
            let byte_start = scanner.next_offset();
            scanner.scan_value(Some(&mut raw), self.limits.max_message_bytes)?;
            let byte_end = scanner.next_offset();
            self.scanned = self
                .scanned
                .checked_add(1)
                .ok_or("Счётчик сообщений экспорта Telegram переполнен.")?;
            let frame = TelegramMessageFrame {
                chat: header.clone(),
                raw_json: std::mem::take(&mut raw),
                byte_start,
                byte_end,
            };
            match (self.on_message)(frame)? {
                ScanControl::Continue => {}
                ScanControl::Stop => return Ok(Flow::Paused),
            }
            scanner.skip_ws()?;
            match scanner.next()? {
                Some(b',') => {}
                Some(b']') => return Ok(Flow::Complete),
                _ => {
                    return Err(
                        "Не удалось распознать экспорт Telegram Desktop: в messages ожидалась запятая или конец массива."
                            .into(),
                    );
                }
            }
        }
    }

    fn scan_account_root<R: Read>(
        &mut self,
        scanner: &mut Scanner<'_, R>,
        scratch: &mut Vec<u8>,
    ) -> Result<Flow, String> {
        // The "about" key itself was consumed by the caller; its value must
        // be the about string the official writer always emits first.
        colon(scanner, "корне экспорта")?;
        scanner.scan_string_raw(None, 0)?;
        let mut seen = 1_u32 << ABOUT_BIT;
        let mut sections = 0_u32;
        loop {
            scanner.skip_ws()?;
            match scanner.peek()? {
                Some(b'}') => {
                    scanner.next()?;
                    break;
                }
                Some(b',') => {
                    scanner.next()?;
                    scanner.skip_ws()?;
                }
                _ => {
                    return Err(
                        "Не удалось распознать экспорт Telegram Desktop: в корне экспорта ожидалась запятая или конец объекта."
                            .into(),
                    );
                }
            }
            let key = self.read_key(scanner, scratch)?;
            let bit = match key.as_slice() {
                b"about" => ABOUT_BIT,
                b"personal_information" => PERSONAL_BIT,
                b"profile_pictures" => PROFILE_PICTURES_BIT,
                b"stories" => STORIES_BIT,
                b"profile_music" => PROFILE_MUSIC_BIT,
                b"contacts" => CONTACTS_BIT,
                b"frequent_contacts" => FREQUENT_CONTACTS_BIT,
                b"other_data" => OTHER_DATA_BIT,
                b"sessions" => SESSIONS_BIT,
                b"web_sessions" => WEB_SESSIONS_BIT,
                b"chats" => CHATS_BIT,
                b"left_chats" => LEFT_CHATS_BIT,
                _ => {
                    return Err(format!(
                        "Не удалось распознать экспорт Telegram Desktop: неожиданный ключ \"{}\" в корне экспорта.",
                        String::from_utf8_lossy(&key)
                    ));
                }
            };
            let mask = 1_u32 << bit;
            if seen & mask != 0 {
                return Err(format!(
                    "Не удалось распознать экспорт Telegram Desktop: ключ \"{}\" повторяется в корне экспорта.",
                    String::from_utf8_lossy(&key)
                ));
            }
            seen |= mask;
            colon(scanner, "корне экспорта")?;
            if key.as_slice() == b"chats" || key.as_slice() == b"left_chats" {
                let flow = self.scan_chats_section(scanner, scratch)?;
                if flow == Flow::Paused {
                    return Ok(Flow::Paused);
                }
            } else {
                scanner.scan_value(None, 0)?;
            }
            sections += 1;
        }
        if sections == 0 {
            return Err(
                "Не удалось распознать экспорт Telegram Desktop: разделы экспорта не найдены."
                    .into(),
            );
        }
        Ok(Flow::Complete)
    }

    fn scan_chats_section<R: Read>(
        &mut self,
        scanner: &mut Scanner<'_, R>,
        scratch: &mut Vec<u8>,
    ) -> Result<Flow, String> {
        scanner.expect(b'{', "разделе chats")?;
        scanner.skip_ws()?;
        let key = self.read_key(scanner, scratch)?;
        if key.as_slice() != b"about" {
            return Err(
                "Не удалось распознать экспорт Telegram Desktop: в разделе chats ожидался ключ \"about\"."
                    .into(),
            );
        }
        colon(scanner, "разделе chats")?;
        scanner.skip_ws()?;
        scanner.scan_string_raw(None, 0)?;
        scanner.skip_ws()?;
        scanner.expect(b',', "разделе chats")?;
        scanner.skip_ws()?;
        let key = self.read_key(scanner, scratch)?;
        if key.as_slice() != b"list" {
            return Err(
                "Не удалось распознать экспорт Telegram Desktop: в разделе chats ожидался ключ \"list\"."
                    .into(),
            );
        }
        colon(scanner, "списке chats")?;
        scanner.expect(b'[', "списке chats")?;
        scanner.skip_ws()?;
        if scanner.peek()? == Some(b']') {
            scanner.next()?;
        } else {
            loop {
                scanner.skip_ws()?;
                if scanner.peek()? != Some(b'{') {
                    return Err(
                        "Не удалось распознать экспорт Telegram Desktop: элемент списка chats должен быть объектом чата."
                            .into(),
                    );
                }
                let flow = self.scan_chat_object(scanner, scratch, None, "объекте чата")?;
                if flow == Flow::Paused {
                    return Ok(Flow::Paused);
                }
                scanner.skip_ws()?;
                match scanner.next()? {
                    Some(b',') => {}
                    Some(b']') => break,
                    _ => {
                        return Err(
                            "Не удалось распознать экспорт Telegram Desktop: в списке chats ожидалась запятая или конец массива."
                                .into(),
                        );
                    }
                }
            }
        }
        scanner.skip_ws()?;
        scanner.expect(b'}', "разделе chats")?;
        Ok(Flow::Complete)
    }

    fn read_name<R: Read>(
        &mut self,
        scanner: &mut Scanner<'_, R>,
        scratch: &mut Vec<u8>,
    ) -> Result<Option<String>, String> {
        colon(scanner, "поле name")?;
        match scanner.peek()? {
            Some(b'"') => {
                scratch.clear();
                scanner
                    .scan_string_raw(Some(scratch), self.limits.max_metadata_bytes)
                    .map_err(remap_metadata_cap)?;
                Ok(Some(unescape_json_string(
                    scratch,
                    self.limits.max_metadata_bytes,
                )?))
            }
            Some(b'n') => {
                scanner.scan_value(None, 0)?;
                Ok(None)
            }
            _ => Err(
                "Не удалось распознать экспорт Telegram Desktop: поле name чата должно быть строкой или null."
                    .into(),
            ),
        }
    }

    fn read_kind<R: Read>(
        &mut self,
        scanner: &mut Scanner<'_, R>,
        scratch: &mut Vec<u8>,
    ) -> Result<String, String> {
        colon(scanner, "поле type")?;
        match scanner.peek()? {
            Some(b'"') => {
                scratch.clear();
                scanner
                    .scan_string_raw(Some(scratch), self.limits.max_metadata_bytes)
                    .map_err(remap_metadata_cap)?;
                unescape_json_string(scratch, self.limits.max_metadata_bytes)
            }
            Some(b'n') => {
                scanner.scan_value(None, 0)?;
                Ok(String::new())
            }
            _ => Err(
                "Не удалось распознать экспорт Telegram Desktop: поле type чата должно быть строкой или null."
                    .into(),
            ),
        }
    }

    fn read_id<R: Read>(
        &mut self,
        scanner: &mut Scanner<'_, R>,
        scratch: &mut Vec<u8>,
    ) -> Result<String, String> {
        colon(scanner, "поле id")?;
        match scanner.peek()? {
            Some(b'-') | Some(b'0'..=b'9') => {
                scratch.clear();
                scanner.scan_integer_raw(Some(scratch), MAX_ID_TOKEN_BYTES)?;
                // Chat identity is the exact integer token emitted by the
                // exporter; fractions and exponents are rejected instead of
                // being coerced into a fabricated id.
                if matches!(scanner.peek()?, Some(b'.') | Some(b'e') | Some(b'E')) {
                    return Err(
                        "Не удалось распознать экспорт Telegram Desktop: идентификатор чата должен быть целым числом."
                            .into(),
                    );
                }
                String::from_utf8(scratch.clone())
                    .map_err(|_| "Некорректный идентификатор чата в экспорте Telegram.".to_string())
            }
            _ => Err(
                "Не удалось распознать экспорт Telegram Desktop: идентификатор чата должен быть числом."
                    .into(),
            ),
        }
    }
}

const ABOUT_BIT: u32 = 0;
const PERSONAL_BIT: u32 = 1;
const PROFILE_PICTURES_BIT: u32 = 2;
const STORIES_BIT: u32 = 3;
const PROFILE_MUSIC_BIT: u32 = 4;
const CONTACTS_BIT: u32 = 5;
const FREQUENT_CONTACTS_BIT: u32 = 6;
const OTHER_DATA_BIT: u32 = 7;
const SESSIONS_BIT: u32 = 8;
const WEB_SESSIONS_BIT: u32 = 9;
const CHATS_BIT: u32 = 10;
const LEFT_CHATS_BIT: u32 = 11;

fn colon<R: Read>(scanner: &mut Scanner<'_, R>, context: &str) -> Result<(), String> {
    scanner.skip_ws()?;
    scanner.expect(b':', context)?;
    scanner.skip_ws()
}

fn remap_metadata_cap(error: String) -> String {
    if error.contains("размера сообщения") {
        return "Превышен лимит метаданных экспорта Telegram.".into();
    }
    error
}

#[derive(Default)]
struct ChatBuilder {
    /// 0 = start, 1 = `name` seen, 2 = `type` seen, 3 = `id` seen,
    /// 4 = `messages` seen (terminal).
    stage: u8,
    name: Option<String>,
    kind: String,
    id: String,
}

impl ChatBuilder {
    fn header(&self) -> TelegramChatHeader {
        TelegramChatHeader {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind.clone(),
        }
    }
}
