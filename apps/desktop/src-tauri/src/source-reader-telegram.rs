use super::{array, date, Checkpoint, Connections, Page, RawItem, SourceGrantRequest};
use serde_json::{json, Value};
pub(super) fn telegram(
    hub: &Connections,
    request: &SourceGrantRequest,
    account: &str,
    checkpoint: &mut Checkpoint,
    limit: usize,
) -> Result<Page, String> {
    let chat: i64 = request
        .resource_id
        .parse()
        .map_err(|_| "Неверный ID чата Telegram.")?;
    let from = checkpoint.cursor.parse::<i64>().unwrap_or(0);
    // TDLib can return a short cached page while preloading older messages. Empty
    // ends the walk; repeated anchor-only pages end with explicit partial coverage.
    let value = hub.telegram_request_for(account, json!({"@type":"getChatHistory","chat_id":chat,"from_message_id":from,"offset":0,"limit":limit + usize::from(from != 0),"only_local":false}))?;
    let messages = array(&value, "messages")?;
    let mut page = Page::default();
    let mut oldest = from;
    for message in messages {
        let id = message
            .get("id")
            .and_then(Value::as_i64)
            .ok_or("Telegram вернул неверный ID сообщения.")?;
        if from != 0 && id >= from {
            continue;
        }
        oldest = if oldest == 0 { id } else { oldest.min(id) };
        let content = &message["content"];
        let body = content
            .pointer("/text/text")
            .or_else(|| content.pointer("/caption/text"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let sender = &message["sender_id"];
        let author = sender
            .get("user_id")
            .and_then(Value::as_i64)
            .map(|id| id.to_string())
            .or_else(|| {
                sender
                    .get("chat_id")
                    .and_then(Value::as_i64)
                    .map(|id| id.to_string())
            });
        page.items.push(RawItem {
            id: id.to_string(),
            author,
            text: body,
            created: date(message, "date"),
            kind: "message".into(),
            mode: "dialogue".into(),
            reply_to: message
                .pointer("/reply_to/message_id")
                .and_then(Value::as_i64)
                .filter(|_| {
                    message
                        .pointer("/reply_to/chat_id")
                        .and_then(Value::as_i64)
                        .is_none_or(|parent_chat| parent_chat == 0 || parent_chat == chat)
                })
                .map(|id| id.to_string()),
            forwarded: message.get("forward_info").is_some_and(|v| !v.is_null()),
            thread: Some(request.resource_id.clone()),
            ..RawItem::default()
        });
    }
    page.done = messages.is_empty();
    if oldest == from && !messages.is_empty() {
        checkpoint.offset += 1;
        if checkpoint.offset >= 3 {
            page.done = true;
            page.truncated = true;
            page.unavailable.push("Telegram не подтвердил окончание истории после повторных запросов; сохранён частичный корпус.".into());
        }
    } else {
        checkpoint.offset = 0;
    }
    checkpoint.cursor = oldest.to_string();
    Ok(page)
}
