//! Canonical Telegram acquisition data. The exact raw object remains authoritative;
//! this projection never infers a target author, grant, holdout split or read time.
//! Archive byte offsets belong to the acquisition receipt, not document identity.
use super::telegram_identity::ArchiveVersion;
use crate::ingestion::telegram_archive_json::TelegramMessageFrame;
use serde::{Deserialize, Serialize};
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TelegramDocument {
    pub schema_version: u8,
    pub id: String,
    pub source_key: String,
    pub original_id: String,
    pub resource_id: String,
    pub resource_label: Option<String>,
    pub resource_kind: String,
    pub kind: String,
    pub author_id: Option<String>,
    pub author_label: Option<String>,
    pub text: String,
    pub created_at: Option<String>,
    pub edited_at: Option<String>,
    pub reply_to_original_id: Option<String>,
    pub is_forwarded: bool,
    pub raw_json: String,
}
const INVALID: &str = "Неподдерживаемое или повреждённое сообщение Telegram.";
pub fn normalize(origin: &str, frame: TelegramMessageFrame) -> Result<TelegramDocument, String> {
    normalize_with_version(ArchiveVersion::CURRENT, origin, frame)
}
pub fn normalize_with_version(
    version: ArchiveVersion,
    origin: &str,
    frame: TelegramMessageFrame,
) -> Result<TelegramDocument, String> {
    super::input::validate_origin(origin)?;
    if frame.raw_json.len() > 4 * 1024 * 1024
        || frame.byte_end.checked_sub(frame.byte_start) != Some(frame.raw_json.len() as u64)
    {
        return Err(INVALID.into());
    }
    let message: Message = serde_json::from_slice(&frame.raw_json).map_err(|_| INVALID)?;
    if message.id <= 0
        || !matches!(message.kind.as_str(), "message" | "service")
        || message.reply_to_message_id.is_some_and(|id| id <= 0)
        || message
            .from_id
            .as_ref()
            .zip(message.actor_id.as_ref())
            .is_some_and(|(a, b)| a != b)
        || message
            .from
            .as_ref()
            .zip(message.actor.as_ref())
            .is_some_and(|(a, b)| a != b)
    {
        return Err(INVALID.into());
    }
    let author_id = message.from_id.or(message.actor_id);
    if author_id.as_ref().is_some_and(|id| {
        id.len() > 128 || id.trim().is_empty() || id.chars().any(char::is_control)
    }) {
        return Err(INVALID.into());
    }
    let original_id = message.id.to_string();
    let resource_id = version.resource_id(&frame.chat);
    let (source_key, id) = version.identifiers(origin, &resource_id, &original_id)?;
    let text = match message.text {
        Some(Text::Plain(value)) => value,
        Some(Text::Rich(parts)) => parts
            .into_iter()
            .map(|part| match part {
                TextPart::Plain(value) => value,
                TextPart::Entity { text } => text,
            })
            .collect(),
        None => String::new(),
    };
    Ok(TelegramDocument {
        schema_version: version.document_version(),
        id,
        source_key,
        original_id,
        resource_id,
        resource_label: frame.chat.name,
        resource_kind: frame.chat.kind,
        kind: message.kind,
        author_id,
        author_label: message.from.or(message.actor),
        text,
        created_at: message.date_unixtime.map(timestamp).transpose()?,
        edited_at: message.edited_unixtime.map(timestamp).transpose()?,
        reply_to_original_id: message.reply_to_message_id.map(|id| id.to_string()),
        is_forwarded: message.forwarded_from.is_some() || message.forwarded_from_id.is_some(),
        raw_json: String::from_utf8(frame.raw_json).map_err(|_| INVALID)?,
    })
}
fn timestamp(value: UnixTime) -> Result<String, String> {
    let seconds = match value {
        UnixTime::Number(n) => n,
        UnixTime::Text(s) => s.parse().map_err(|_| INVALID)?,
    };
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .ok_or_else(|| INVALID.into())
}
// These names and text entities belong to the external Telegram export schema.
// Known duplicate fields are rejected by serde; all original fields, including
// unknown relations and attachment metadata, survive in raw_json unchanged.
#[derive(Deserialize)]
struct Message {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
    from_id: Option<String>,
    from: Option<String>,
    actor_id: Option<String>,
    actor: Option<String>,
    text: Option<Text>,
    date_unixtime: Option<UnixTime>,
    edited_unixtime: Option<UnixTime>,
    reply_to_message_id: Option<i64>,
    forwarded_from: Option<String>,
    forwarded_from_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Text {
    Plain(String),
    Rich(Vec<TextPart>),
}
#[derive(Deserialize)]
#[serde(untagged)]
enum TextPart {
    Plain(String),
    Entity { text: String },
}
#[derive(Deserialize)]
#[serde(untagged)]
enum UnixTime {
    Text(String),
    Number(i64),
}
#[cfg(test)]
#[path = "telegram-document-tests.rs"]
mod tests;
