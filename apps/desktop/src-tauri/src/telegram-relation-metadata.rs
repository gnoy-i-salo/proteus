//! External Telegram export fields retain their exact spelling. Unknown content
//! is retained by acquisition but cannot establish reference eligibility here.
use super::super::telegram_document::TelegramDocument;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
pub(in crate::ingestion) struct Metadata {
    pub peer: Option<String>,
    pub quotes: BTreeSet<String>,
    pub complete: bool,
}
#[derive(Deserialize)]
struct Raw {
    reply_to_peer_id: Option<String>,
    text_entities: Option<Vec<Entity>>,
    text: Option<Text>,
    #[serde(flatten)]
    other: BTreeMap<String, Value>,
}
#[derive(Deserialize)]
struct Entity {
    #[serde(rename = "type")]
    kind: String,
    text: String,
    #[serde(flatten)]
    other: BTreeMap<String, Value>,
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
    Entity(Entity),
}
pub(in crate::ingestion) fn parse(document: &TelegramDocument) -> Result<Metadata, String> {
    let raw: Raw = serde_json::from_str(&document.raw_json)
        .map_err(|_| "Неоднозначные связи в исходном сообщении Telegram.")?;
    let mut metadata = Metadata {
        peer: raw.reply_to_peer_id,
        quotes: BTreeSet::new(),
        complete: document.kind == "message" && !document.is_forwarded,
    };
    for key in raw.other.keys() {
        if !matches!(
            key.as_str(),
            "id" | "type"
                | "from"
                | "from_id"
                | "date"
                | "date_unixtime"
                | "edited"
                | "edited_unixtime"
                | "text"
                | "reply_to_message_id"
                | "actor"
                | "actor_id"
                | "author"
                | "via_bot"
        ) {
            metadata.complete = false;
        }
    }
    if metadata.peer.is_some() && document.reply_to_original_id.is_none() {
        metadata.complete = false;
    }
    if let Some(entities) = &raw.text_entities {
        let mut text = String::new();
        for entity in entities {
            text.push_str(&entity.text);
            inspect(entity, &mut metadata);
        }
        if text != document.text {
            metadata.complete = false;
        }
    }
    // Parse rich entities directly: going through Value would collapse
    // duplicate type keys and could turn an ambiguous blockquote into plain text.
    let mut text = String::new();
    match &raw.text {
        Some(Text::Plain(value)) => text.push_str(value),
        Some(Text::Rich(parts)) => {
            for part in parts {
                match part {
                    TextPart::Plain(value) => text.push_str(value),
                    TextPart::Entity(entity) => {
                        text.push_str(&entity.text);
                        inspect(entity, &mut metadata);
                    }
                }
            }
        }
        None => {}
    }
    if text != document.text {
        metadata.complete = false;
    }
    Ok(metadata)
}
fn inspect(entity: &Entity, metadata: &mut Metadata) {
    if !matches!(
        entity.kind.as_str(),
        "plain"
            | "mention"
            | "hashtag"
            | "bot_command"
            | "link"
            | "email"
            | "bold"
            | "italic"
            | "code"
            | "pre"
            | "text_link"
            | "mention_name"
            | "phone"
            | "cashtag"
            | "underline"
            | "strikethrough"
            | "blockquote"
            | "bank_card"
            | "ton_address"
            | "spoiler"
            | "custom_emoji"
    ) || entity.other.keys().any(|key| {
        !matches!(
            key.as_str(),
            "href" | "language" | "user_id" | "document_id" | "collapsed"
        )
    }) {
        metadata.complete = false;
    }
    if entity.kind == "blockquote" {
        if entity.text.is_empty() {
            metadata.complete = false;
        } else {
            metadata.quotes.insert(entity.text.clone());
        }
    }
}
