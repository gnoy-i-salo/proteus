use super::{array, date, Checkpoint, Connections, Page, RawItem, SourceGrantRequest};
use serde_json::{json, Value};
pub(super) fn telegram(
    hub: &Connections,
    request: &SourceGrantRequest,
    account: &str,
    checkpoint: &mut Checkpoint,
    limit: usize,
) -> Result<Page, String> {
    read_page(request, checkpoint, limit, |value| {
        hub.telegram_request_for(account, value)
    })
}
fn read_page(
    request: &SourceGrantRequest,
    checkpoint: &mut Checkpoint,
    limit: usize,
    mut call: impl FnMut(Value) -> Result<Value, String>,
) -> Result<Page, String> {
    let chat: i64 = request
        .resource_id
        .parse()
        .map_err(|_| "Неверный ID чата Telegram.")?;
    const MAX_ID: i64 = 9_007_199_254_740_991;
    if chat == 0 || !(-MAX_ID..=MAX_ID).contains(&chat) || !(1..=100).contains(&limit) {
        return Err("Неверный чат или размер порции Telegram.".into());
    }
    let from = if checkpoint.cursor.is_empty() {
        0
    } else {
        checkpoint
            .cursor
            .parse::<i64>()
            .map_err(|_| "Повреждена позиция истории Telegram.")?
    };
    if !(0..=MAX_ID).contains(&from) || checkpoint.offset > 3 {
        return Err("Повреждена позиция истории Telegram.".into());
    }
    let requested_limit = (limit + usize::from(from != 0)).min(100);
    // TDLib can return a short cached page while preloading older messages. Empty
    // ends the walk; repeated anchor-only pages end with explicit partial coverage.
    let value = call(
        json!({"@type":"getChatHistory","chat_id":chat,"from_message_id":from,"offset":0,"limit":requested_limit,"only_local":false}),
    )?;
    let messages = array(&value, "messages")?;
    if messages.len() > requested_limit {
        return Err("Telegram превысил запрошенный размер порции.".into());
    }
    let mut page = Page::default();
    let mut oldest = from;
    let mut previous = i64::MAX;
    for message in messages {
        let id = message
            .get("id")
            .and_then(Value::as_i64)
            .ok_or("Telegram вернул неверный ID сообщения.")?;
        if !(1..=MAX_ID).contains(&id)
            || id >= previous
            || message.get("chat_id").and_then(Value::as_i64) != Some(chat)
        {
            return Err(
                "Telegram вернул сообщения вне выбранного чата или порядка истории.".into(),
            );
        }
        previous = id;
        if (from != 0 && id >= from) || page.items.len() == limit {
            // An absent/deleted anchor may yield limit+1 older messages. Advance
            // only through delivered items; the extra message remains older than
            // our next anchor and is fetched again, never silently skipped.
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
        checkpoint.offset = (checkpoint.offset + 1).min(3);
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

#[cfg(test)]
#[path = "source-reader-telegram-tests.rs"]
mod tests;
