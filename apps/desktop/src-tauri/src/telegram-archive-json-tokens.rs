//! Byte-level streaming JSON primitives for the Telegram archive reader.
//!
//! `Scanner` reads through a small fixed buffer, tracks absolute byte offsets,
//! validates JSON structure and UTF-8 exactly, and can optionally capture every
//! consumed byte (used for one bounded raw message at a time). Nothing here
//! materializes a whole file or a generic JSON tree.

use super::TelegramArchiveLimits;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub(super) const READ_BUFFER_BYTES: usize = 8 * 1024;
const INTERRUPT_CHECK_INTERVAL: u64 = 4096;
pub(super) const MAX_NESTING_DEPTH: usize = 512;
pub(super) const MAX_KEY_BYTES: usize = 1024;
pub(super) const MAX_ID_TOKEN_BYTES: usize = 128;

const ERR_EOF: &str = "Неполный файл экспорта Telegram: неожиданный конец данных.";
const ERR_UTF8: &str = "Некорректный UTF-8 в строке экспорта Telegram.";
const ERR_ESCAPE: &str = "Некорректная escape-последовательность в строке экспорта Telegram.";
const ERR_SURROGATE: &str =
    "Некорректная последовательность суррогатов UTF-16 в экспорте Telegram.";
const ERR_CONTROL: &str = "Неэкранированный управляющий символ в строке экспорта Telegram.";
const ERR_MESSAGE_CAP: &str = "Превышен лимит размера сообщения экспорта Telegram.";

pub(super) struct Scanner<'a, R> {
    reader: R,
    buf: [u8; READ_BUFFER_BYTES],
    pos: usize,
    len: usize,
    /// Absolute offset of the byte at `buf[pos]`.
    consumed: u64,
    /// Absolute offset one past the last buffered byte.
    loaded: u64,
    max_input_bytes: u64,
    max_duration: Duration,
    started: Instant,
    cancelled: &'a AtomicBool,
    unchecked: u64,
}

impl<'a, R: Read> Scanner<'a, R> {
    pub fn new(reader: R, limits: &TelegramArchiveLimits, cancelled: &'a AtomicBool) -> Self {
        Self {
            reader,
            buf: [0; READ_BUFFER_BYTES],
            pos: 0,
            len: 0,
            consumed: 0,
            loaded: 0,
            max_input_bytes: limits.max_input_bytes,
            max_duration: limits.max_duration,
            started: Instant::now(),
            cancelled,
            unchecked: 0,
        }
    }

    pub fn check_interrupt(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err("Сканирование экспорта отменено.".into());
        }
        if self.started.elapsed() > self.max_duration {
            return Err("Истекло время сканирования экспорта Telegram.".into());
        }
        Ok(())
    }

    fn fill(&mut self) -> Result<bool, String> {
        if self.pos < self.len {
            return Ok(true);
        }
        self.check_interrupt()?;
        // Never read past the input budget: ask for the remaining bytes plus
        // one probe byte (capped by the buffer) so an exact fit is
        // indistinguishable from one byte over budget without over-reading.
        // The probe byte is never delivered: if it loads, the budget check
        // below fails the scan before any consumer sees it.
        let remaining = self.max_input_bytes.saturating_sub(self.loaded);
        let want = remaining.saturating_add(1).min(READ_BUFFER_BYTES as u64) as usize;
        let read = self
            .reader
            .read(&mut self.buf[..want])
            .map_err(|error| format!("Ошибка чтения экспорта Telegram: {error}"))?;
        if read > want {
            return Err(
                "Ошибка чтения экспорта Telegram: некорректная реализация чтения вернула больше байт, чем запрошено."
                    .into(),
            );
        }
        // A slow or cancelled reader may fire the interrupt while blocking:
        // the result (data or EOF) must not surface before the check.
        self.check_interrupt()?;
        if read == 0 {
            return Ok(false);
        }
        self.pos = 0;
        self.len = read;
        self.loaded = self
            .loaded
            .checked_add(read as u64)
            .ok_or("Счётчик байт экспорта Telegram переполнен.")?;
        if self.loaded > self.max_input_bytes {
            return Err("Превышен лимит размера входного файла экспорта Telegram.".into());
        }
        Ok(true)
    }

    pub fn next(&mut self) -> Result<Option<u8>, String> {
        if self.pos == self.len && !self.fill()? {
            return Ok(None);
        }
        let byte = self.buf[self.pos];
        self.pos += 1;
        self.consumed = self
            .consumed
            .checked_add(1)
            .ok_or("Счётчик байт экспорта Telegram переполнен.")?;
        self.unchecked += 1;
        if self.unchecked >= INTERRUPT_CHECK_INTERVAL {
            self.unchecked = 0;
            self.check_interrupt()?;
        }
        Ok(Some(byte))
    }

    pub fn peek(&mut self) -> Result<Option<u8>, String> {
        if self.pos == self.len && !self.fill()? {
            return Ok(None);
        }
        Ok(Some(self.buf[self.pos]))
    }

    pub fn expect(&mut self, expected: u8, context: &str) -> Result<(), String> {
        match self.next()? {
            Some(byte) if byte == expected => Ok(()),
            Some(byte) => Err(format!(
                "Не удалось распознать экспорт Telegram Desktop: в {context} ожидался байт {:?}, найден {:?}.",
                expected as char, byte as char
            )),
            None => Err(ERR_EOF.into()),
        }
    }

    pub fn skip_ws(&mut self) -> Result<(), String> {
        while let Some(byte) = self.peek()? {
            if matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
                self.next()?;
            } else {
                break;
            }
        }
        Ok(())
    }

    /// Like `skip_ws`, but captures the skipped bytes when sinking.
    fn skip_ws_captured(
        &mut self,
        sink: &mut Option<&mut Vec<u8>>,
        cap: usize,
    ) -> Result<(), String> {
        while let Some(byte) = self.peek()? {
            if !matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
                break;
            }
            let byte = self.next()?.expect("peeked byte");
            sink_push(sink, byte, cap)?;
        }
        Ok(())
    }

    /// Absolute offset of the next byte that will be consumed.
    pub fn next_offset(&self) -> u64 {
        self.consumed
    }

    /// Consumes an optional UTF-8 BOM, keeping offsets absolute.
    pub fn skip_bom(&mut self) -> Result<(), String> {
        if self.peek()? != Some(0xEF) {
            return Ok(());
        }
        self.expect(0xEF, "начале файла")?;
        self.expect(0xBB, "начале файла")?;
        self.expect(0xBF, "начале файла")?;
        Ok(())
    }

    /// Scans one JSON string starting at its opening quote. Validates escapes,
    /// rejects raw control characters and validates UTF-8 exactly. When `sink`
    /// is `Some`, every consumed byte (quotes included) is pushed to it and
    /// `cap` is enforced during the scan.
    pub fn scan_string_raw(
        &mut self,
        mut sink: Option<&mut Vec<u8>>,
        cap: usize,
    ) -> Result<(), String> {
        match self.next()? {
            Some(b'"') => sink_push(&mut sink, b'"', cap)?,
            Some(other) => {
                return Err(format!(
                    "Не удалось распознать экспорт Telegram Desktop: ожидалась строка, найден байт {:?}.",
                    other as char
                ));
            }
            None => return Err(ERR_EOF.into()),
        }
        loop {
            let byte = self.next()?.ok_or_else(|| ERR_EOF.to_string())?;
            match byte {
                b'"' => {
                    sink_push(&mut sink, byte, cap)?;
                    return Ok(());
                }
                b'\\' => {
                    sink_push(&mut sink, byte, cap)?;
                    self.scan_escape(&mut sink, cap)?;
                }
                0x00..=0x1F => return Err(ERR_CONTROL.into()),
                0x20..=0x7F => sink_push(&mut sink, byte, cap)?,
                lead => {
                    sink_push(&mut sink, lead, cap)?;
                    self.scan_utf8_tail(lead, &mut sink, cap)?;
                }
            }
        }
    }

    fn scan_escape(&mut self, sink: &mut Option<&mut Vec<u8>>, cap: usize) -> Result<(), String> {
        let escaped = self.next()?.ok_or_else(|| ERR_EOF.to_string())?;
        match escaped {
            b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => sink_push(sink, escaped, cap),
            b'u' => {
                sink_push(sink, b'u', cap)?;
                let first = self.scan_hex4(sink, cap)?;
                if (0xD800..=0xDBFF).contains(&first) {
                    // High surrogate: a valid pair must follow and is kept as
                    // escaped bytes; anything else is rejected here, not only
                    // during header decoding.
                    match self.next()? {
                        Some(b'\\') => sink_push(sink, b'\\', cap)?,
                        _ => return Err(ERR_SURROGATE.into()),
                    }
                    match self.next()? {
                        Some(b'u') => sink_push(sink, b'u', cap)?,
                        _ => return Err(ERR_SURROGATE.into()),
                    }
                    let second = self.scan_hex4(sink, cap)?;
                    if !(0xDC00..=0xDFFF).contains(&second) {
                        return Err(ERR_SURROGATE.into());
                    }
                } else if (0xDC00..=0xDFFF).contains(&first) {
                    return Err(ERR_SURROGATE.into());
                }
                Ok(())
            }
            _ => Err(ERR_ESCAPE.into()),
        }
    }

    /// Reads exactly four hex digits (capturing them), validating escapes.
    fn scan_hex4(&mut self, sink: &mut Option<&mut Vec<u8>>, cap: usize) -> Result<u32, String> {
        let mut value = 0_u32;
        for _ in 0..4 {
            let digit = self.next()?.ok_or_else(|| ERR_EOF.to_string())?;
            let decoded = match digit {
                b'0'..=b'9' => u32::from(digit - b'0'),
                b'a'..=b'f' => u32::from(digit - b'a') + 10,
                b'A'..=b'F' => u32::from(digit - b'A') + 10,
                _ => return Err(ERR_ESCAPE.into()),
            };
            sink_push(sink, digit, cap)?;
            value = value * 16 + decoded;
        }
        Ok(value)
    }

    fn scan_utf8_tail(
        &mut self,
        lead: u8,
        sink: &mut Option<&mut Vec<u8>>,
        cap: usize,
    ) -> Result<(), String> {
        let (tail, second_min, second_max) = match lead {
            0xC2..=0xDF => (1, 0x80, 0xBF),
            0xE0 => (2, 0xA0, 0xBF),
            0xE1..=0xEC => (2, 0x80, 0xBF),
            0xED => (2, 0x80, 0x9F),
            0xEE..=0xEF => (2, 0x80, 0xBF),
            0xF0 => (3, 0x90, 0xBF),
            0xF1..=0xF3 => (3, 0x80, 0xBF),
            0xF4 => (3, 0x80, 0x8F),
            _ => return Err(ERR_UTF8.into()),
        };
        let second = self.next()?.ok_or_else(|| ERR_EOF.to_string())?;
        if !(second_min..=second_max).contains(&second) {
            return Err(ERR_UTF8.into());
        }
        sink_push(sink, second, cap)?;
        for _ in 1..tail {
            let byte = self.next()?.ok_or_else(|| ERR_EOF.to_string())?;
            if !(0x80..=0xBF).contains(&byte) {
                return Err(ERR_UTF8.into());
            }
            sink_push(sink, byte, cap)?;
        }
        Ok(())
    }

    /// Scans one JSON number starting at its first byte (`-` or digit) and
    /// validates the full grammar.
    pub fn scan_number_raw(
        &mut self,
        mut sink: Option<&mut Vec<u8>>,
        cap: usize,
    ) -> Result<(), String> {
        let first = self.next()?.ok_or_else(|| ERR_EOF.to_string())?;
        match first {
            b'-' => {
                sink_push(&mut sink, first, cap)?;
                match self.next()? {
                    Some(byte @ b'0'..=b'9') => {
                        sink_push(&mut sink, byte, cap)?;
                        if byte == b'0' {
                            self.reject_digit()?;
                        } else {
                            self.scan_digits(&mut sink, cap, false)?;
                        }
                    }
                    _ => {
                        return Err(
                            "Не удалось распознать экспорт Telegram Desktop: некорректное число."
                                .into(),
                        );
                    }
                }
            }
            byte @ b'0'..=b'9' => {
                sink_push(&mut sink, byte, cap)?;
                if byte == b'0' {
                    self.reject_digit()?;
                } else {
                    self.scan_digits(&mut sink, cap, false)?;
                }
            }
            other => {
                return Err(format!(
                    "Не удалось распознать экспорт Telegram Desktop: ожидалось число, найден байт {:?}.",
                    other as char
                ));
            }
        }
        if self.peek()? == Some(b'.') {
            self.next()?;
            sink_push(&mut sink, b'.', cap)?;
            self.scan_digits(&mut sink, cap, true)?;
        }
        if matches!(self.peek()?, Some(b'e') | Some(b'E')) {
            // Preserve the original exponent marker byte (`e` vs `E`) so the
            // captured spelling stays verbatim.
            let marker = self.next()?.expect("peeked exponent marker");
            sink_push(&mut sink, marker, cap)?;
            if matches!(self.peek()?, Some(b'+') | Some(b'-')) {
                let sign = self.next()?.expect("peeked sign");
                sink_push(&mut sink, sign, cap)?;
            }
            self.scan_digits(&mut sink, cap, true)?;
        }
        Ok(())
    }

    /// Scans one JSON integer token (`-?(0 | [1-9][0-9]*)`) starting at its
    /// first byte, for chat identity fields where the official exporter emits
    /// an exact integer. Fractions and exponents are rejected by the caller
    /// (`read_id`); digits are captured verbatim without numeric coercion.
    pub fn scan_integer_raw(
        &mut self,
        mut sink: Option<&mut Vec<u8>>,
        cap: usize,
    ) -> Result<(), String> {
        match self.next()? {
            Some(b'-') => {
                sink_push(&mut sink, b'-', cap)?;
                match self.next()? {
                    Some(byte @ b'0'..=b'9') => {
                        sink_push(&mut sink, byte, cap)?;
                        if byte == b'0' {
                            self.reject_digit()?;
                        } else {
                            self.scan_digits(&mut sink, cap, false)?;
                        }
                    }
                    _ => {
                        return Err(
                            "Не удалось распознать экспорт Telegram Desktop: некорректное число."
                                .into(),
                        );
                    }
                }
            }
            Some(byte @ b'0'..=b'9') => {
                sink_push(&mut sink, byte, cap)?;
                if byte == b'0' {
                    self.reject_digit()?;
                } else {
                    self.scan_digits(&mut sink, cap, false)?;
                }
            }
            Some(other) => {
                return Err(format!(
                    "Не удалось распознать экспорт Telegram Desktop: ожидалось число, найден байт {:?}.",
                    other as char
                ));
            }
            None => return Err(ERR_EOF.into()),
        }
        Ok(())
    }

    fn scan_digits(
        &mut self,
        sink: &mut Option<&mut Vec<u8>>,
        cap: usize,
        require: bool,
    ) -> Result<(), String> {
        let mut any = false;
        while let Some(byte @ b'0'..=b'9') = self.peek()? {
            self.next()?;
            sink_push(sink, byte, cap)?;
            any = true;
        }
        if require && !any {
            return Err(
                "Не удалось распознать экспорт Telegram Desktop: некорректное число.".into(),
            );
        }
        Ok(())
    }

    fn reject_digit(&mut self) -> Result<(), String> {
        if matches!(self.peek()?, Some(b'0'..=b'9')) {
            return Err(
                "Не удалось распознать экспорт Telegram Desktop: некорректное число.".into(),
            );
        }
        Ok(())
    }

    fn skip_literal(
        &mut self,
        literal: &str,
        mut sink: Option<&mut Vec<u8>>,
        cap: usize,
    ) -> Result<(), String> {
        for expected in literal.bytes() {
            let byte = self.next()?.ok_or_else(|| ERR_EOF.to_string())?;
            if byte != expected {
                return Err(
                    "Не удалось распознать экспорт Telegram Desktop: некорректное значение.".into(),
                );
            }
            sink_push(&mut sink, byte, cap)?;
        }
        Ok(())
    }

    /// Validates and skips (or captures) one arbitrary JSON value using an
    /// explicit heap stack, so deeply nested inputs cannot overflow the call
    /// stack. `cap` bounds the sink when capturing.
    pub fn scan_value(&mut self, mut sink: Option<&mut Vec<u8>>, cap: usize) -> Result<(), String> {
        enum Close {
            Object,
            Array,
        }
        let mut stack: Vec<Close> = Vec::new();
        'dispatch: loop {
            match self.peek()?.ok_or_else(|| ERR_EOF.to_string())? {
                b'{' => {
                    self.next()?;
                    sink_push(&mut sink, b'{', cap)?;
                    self.skip_ws_captured(&mut sink, cap)?;
                    if self.peek()? == Some(b'}') {
                        self.next()?;
                        sink_push(&mut sink, b'}', cap)?;
                    } else {
                        if stack.len() >= MAX_NESTING_DEPTH {
                            return Err("Слишком глубокая вложенность в экспорте Telegram.".into());
                        }
                        stack.push(Close::Object);
                        self.skip_ws_captured(&mut sink, cap)?;
                        self.scan_string_raw(reborrow(&mut sink), cap)?;
                        self.skip_ws_captured(&mut sink, cap)?;
                        self.punct(b':', &mut sink, cap)?;
                        self.skip_ws_captured(&mut sink, cap)?;
                        continue 'dispatch;
                    }
                }
                b'[' => {
                    self.next()?;
                    sink_push(&mut sink, b'[', cap)?;
                    self.skip_ws_captured(&mut sink, cap)?;
                    if self.peek()? == Some(b']') {
                        self.next()?;
                        sink_push(&mut sink, b']', cap)?;
                    } else {
                        if stack.len() >= MAX_NESTING_DEPTH {
                            return Err("Слишком глубокая вложенность в экспорте Telegram.".into());
                        }
                        stack.push(Close::Array);
                        continue 'dispatch;
                    }
                }
                b'"' => self.scan_string_raw(reborrow(&mut sink), cap)?,
                b't' => self.skip_literal("true", reborrow(&mut sink), cap)?,
                b'f' => self.skip_literal("false", reborrow(&mut sink), cap)?,
                b'n' => self.skip_literal("null", reborrow(&mut sink), cap)?,
                b'-' | b'0'..=b'9' => self.scan_number_raw(reborrow(&mut sink), cap)?,
                other => {
                    return Err(format!(
                        "Не удалось распознать экспорт Telegram Desktop: некорректный байт {:?}.",
                        other as char
                    ));
                }
            }
            loop {
                let closing_object = match stack.last() {
                    None => return Ok(()),
                    Some(Close::Array) => false,
                    Some(Close::Object) => true,
                };
                self.skip_ws_captured(&mut sink, cap)?;
                let byte = self.next()?.ok_or_else(|| ERR_EOF.to_string())?;
                match byte {
                    b',' => {
                        sink_push(&mut sink, b',', cap)?;
                        self.skip_ws_captured(&mut sink, cap)?;
                        if closing_object {
                            self.scan_string_raw(reborrow(&mut sink), cap)?;
                            self.skip_ws_captured(&mut sink, cap)?;
                            self.punct(b':', &mut sink, cap)?;
                            self.skip_ws_captured(&mut sink, cap)?;
                        }
                        continue 'dispatch;
                    }
                    b'}' if closing_object => {
                        sink_push(&mut sink, b'}', cap)?;
                        stack.pop();
                    }
                    b']' if !closing_object => {
                        sink_push(&mut sink, b']', cap)?;
                        stack.pop();
                    }
                    other if closing_object => {
                        return Err(format!(
                            "Не удалось распознать экспорт Telegram Desktop: в объекте ожидалась запятая или }}, найден {:?}.",
                            other as char
                        ));
                    }
                    other => {
                        return Err(format!(
                            "Не удалось распознать экспорт Telegram Desktop: в массиве ожидалась запятая или ], найден {:?}.",
                            other as char
                        ));
                    }
                }
            }
        }
    }

    fn punct(
        &mut self,
        expected: u8,
        sink: &mut Option<&mut Vec<u8>>,
        cap: usize,
    ) -> Result<(), String> {
        let byte = self.next()?.ok_or_else(|| ERR_EOF.to_string())?;
        if byte != expected {
            return Err(format!(
                "Не удалось распознать экспорт Telegram Desktop: ожидался {:?}, найден {:?}.",
                expected as char, byte as char
            ));
        }
        sink_push(sink, byte, cap)
    }
}

fn reborrow<'a, 'b: 'a>(sink: &'a mut Option<&'b mut Vec<u8>>) -> Option<&'a mut Vec<u8>> {
    sink.as_mut().map(|inner| &mut **inner)
}

fn sink_push(sink: &mut Option<&mut Vec<u8>>, byte: u8, cap: usize) -> Result<(), String> {
    if let Some(out) = sink.as_mut() {
        out.push(byte);
        if out.len() > cap {
            return Err(ERR_MESSAGE_CAP.into());
        }
    }
    Ok(())
}

/// Decodes a raw string scanned by `scan_string_raw` (quotes included).
/// Validates escape semantics including surrogate pairs; `max_out` bounds the
/// decoded length.
pub(super) fn unescape_json_string(raw: &[u8], max_out: usize) -> Result<String, String> {
    if raw.len() < 2 {
        return Err(ERR_ESCAPE.into());
    }
    let inner = &raw[1..raw.len() - 1];
    if !inner.contains(&b'\\') {
        if inner.len() > max_out {
            return Err("Превышен лимит метаданных экспорта Telegram.".into());
        }
        return std::str::from_utf8(inner)
            .map(str::to_owned)
            .map_err(|_| ERR_UTF8.to_string());
    }
    let mut out = String::new();
    let mut index = 0;
    while index < inner.len() {
        let byte = inner[index];
        if byte != b'\\' {
            let width = utf8_width(byte)?;
            let end = index
                .checked_add(width)
                .filter(|end| *end <= inner.len())
                .ok_or_else(|| ERR_UTF8.to_string())?;
            out.push_str(
                std::str::from_utf8(&inner[index..end]).map_err(|_| ERR_UTF8.to_string())?,
            );
            index = end;
            continue;
        }
        let escaped = *inner.get(index + 1).ok_or_else(|| ERR_ESCAPE.to_string())?;
        match escaped {
            b'"' => {
                out.push('"');
                index += 2;
            }
            b'\\' => {
                out.push('\\');
                index += 2;
            }
            b'/' => {
                out.push('/');
                index += 2;
            }
            b'b' => {
                out.push('\u{0008}');
                index += 2;
            }
            b'f' => {
                out.push('\u{000C}');
                index += 2;
            }
            b'n' => {
                out.push('\n');
                index += 2;
            }
            b'r' => {
                out.push('\r');
                index += 2;
            }
            b't' => {
                out.push('\t');
                index += 2;
            }
            b'u' => {
                let high = read_hex4(inner, index + 2)?;
                let (ch, width) = if (0xD800..=0xDBFF).contains(&high) {
                    if inner.get(index + 6) != Some(&b'\\') || inner.get(index + 7) != Some(&b'u') {
                        return Err(ERR_SURROGATE.into());
                    }
                    let low = read_hex4(inner, index + 8)?;
                    if !(0xDC00..=0xDFFF).contains(&low) {
                        return Err(ERR_SURROGATE.into());
                    }
                    let code = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                    (
                        char::from_u32(code).ok_or_else(|| ERR_SURROGATE.to_string())?,
                        12,
                    )
                } else if (0xDC00..=0xDFFF).contains(&high) {
                    return Err(ERR_SURROGATE.into());
                } else {
                    (
                        char::from_u32(high).ok_or_else(|| ERR_SURROGATE.to_string())?,
                        6,
                    )
                };
                out.push(ch);
                index += width;
            }
            _ => return Err(ERR_ESCAPE.into()),
        }
        if out.len() > max_out {
            return Err("Превышен лимит метаданных экспорта Telegram.".into());
        }
    }
    Ok(out)
}

fn read_hex4(text: &[u8], start: usize) -> Result<u32, String> {
    let mut value = 0_u32;
    for offset in 0..4 {
        let digit = *text
            .get(start + offset)
            .ok_or_else(|| ERR_ESCAPE.to_string())?;
        let decoded = match digit {
            b'0'..=b'9' => u32::from(digit - b'0'),
            b'a'..=b'f' => u32::from(digit - b'a') + 10,
            b'A'..=b'F' => u32::from(digit - b'A') + 10,
            _ => return Err(ERR_ESCAPE.into()),
        };
        value = value * 16 + decoded;
    }
    Ok(value)
}

fn utf8_width(lead: u8) -> Result<usize, String> {
    match lead {
        0x00..=0x7F => Ok(1),
        0xC2..=0xDF => Ok(2),
        0xE0..=0xEF => Ok(3),
        0xF0..=0xF4 => Ok(4),
        _ => Err(ERR_UTF8.into()),
    }
}
