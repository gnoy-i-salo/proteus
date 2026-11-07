//! Complete, fingerprinted Telegram preview. Bodies live for one bounded callback;
//! a malformed tail or budget failure never produces a usable partial preview.
use super::fingerprint::{FingerprintReader, InputFingerprint};
use super::preview::{PreviewCollector, PreviewResource};
use super::telegram_archive_json::{read_telegram_json, ScanControl, TelegramArchiveLimits};
use serde::Deserialize;
use std::{cell::RefCell, io::Read, sync::atomic::AtomicBool};
pub struct ScannedPreview {
    pub fingerprint: InputFingerprint,
    pub resources: Vec<PreviewResource>,
}
pub fn scan_telegram_preview<R: Read>(
    reader: R,
    limits: &TelegramArchiveLimits,
    metadata_budget: usize,
    cancelled: &AtomicBool,
) -> Result<ScannedPreview, String> {
    if limits.max_message_bytes > 4 * 1024 * 1024 || limits.max_metadata_bytes > 65536 {
        return Err("Слишком большой бюджет одного сообщения или заголовка.".into());
    }
    let mut reader = FingerprintReader::new(
        reader,
        limits.max_input_bytes,
        limits.max_duration,
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    let collector = RefCell::new(PreviewCollector::new(limits.max_messages, metadata_budget)?);
    let summary = read_telegram_json(
        &mut reader,
        limits,
        cancelled,
        |chat| {
            collector
                .borrow_mut()
                .resource(&resource_id(&chat.id), chat.name.as_deref().unwrap_or(""))
        },
        |frame| {
            let value: MessageAuthor = serde_json::from_slice(&frame.raw_json)
                .map_err(|_| "Неподдерживаемое сообщение в архиве Telegram.")?;
            let author = value.author_id()?;
            collector
                .borrow_mut()
                .document(&resource_id(&frame.chat.id), author)?;
            Ok(ScanControl::Continue)
        },
    )?;
    if !summary.complete {
        return Err("Предпросмотр источника не завершён.".into());
    }
    let fingerprint = reader.finish().map_err(|error| error.to_string())?;
    if fingerprint.byte_length != summary.bytes_read {
        return Err("Не весь исходный файл вошёл в предпросмотр.".into());
    }
    Ok(ScannedPreview {
        fingerprint,
        resources: collector.into_inner().finish()?,
    })
}
fn resource_id(chat_id: &str) -> String {
    format!("telegram:chat:{chat_id}")
}
// These two names belong to Telegram's external export schema. Unknown fields
// are skipped by serde instead of materializing an entire message tree.
#[derive(Deserialize)]
struct MessageAuthor {
    from_id: Option<String>,
    actor_id: Option<String>,
}
impl MessageAuthor {
    fn author_id(&self) -> Result<Option<&str>, String> {
        if self.from_id.is_some() && self.actor_id.is_some() && self.from_id != self.actor_id {
            return Err("Неоднозначный автор сообщения Telegram.".into());
        }
        Ok(self.from_id.as_deref().or(self.actor_id.as_deref()))
    }
}
#[cfg(test)]
#[path = "ingestion-preview-scan-tests.rs"]
mod tests;
