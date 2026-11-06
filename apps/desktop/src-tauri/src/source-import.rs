use super::{iso, Provider, RawItem};
use serde_json::Value;
pub(crate) fn parse(
    provider: Provider,
    data: &str,
    extension: &str,
) -> Result<(Vec<RawItem>, Vec<String>), String> {
    if matches!(extension, "txt" | "md") {
        return Ok((
            vec![RawItem {
                id: "text".into(),
                text: data.into(),
                ..RawItem::default()
            }],
            vec!["Текстовый импорт не подтверждает автора, время и полноту оригинала.".into()],
        ));
    }
    if extension != "json" {
        return Err("Поддерживаются JSON, TXT и Markdown.".into());
    }
    let value: Value = serde_json::from_str(data).map_err(|_| "Некорректный JSON экспорта.")?;
    let mut items = Vec::new();
    let mut warnings = vec!["Импорт подтверждает только содержимое выбранного файла; удалённые сообщения и отсутствующие вложения неизвестны.".into()];
    match provider {
        Provider::Telegram => {
            if let Some(messages) = value.get("messages").and_then(Value::as_array) { telegram(messages, "chat", &mut items); }
            else if let Some(chats) = value.pointer("/chats/list").and_then(Value::as_array) {
                for (index, chat) in chats.iter().enumerate() { if let Some(messages) = chat.get("messages").and_then(Value::as_array) { telegram(messages, &format!("chat{index}"), &mut items); } }
            } else { return Err("Нужен JSON экспорта Telegram Desktop с messages или chats.list.".into()); }
        }
        Provider::Instagram => {
            let messages = value.get("messages").and_then(Value::as_array).filter(|_| value.get("participants").is_some()).ok_or("Нужен JSON выбранной переписки Instagram. Для иных материалов используйте текстовый импорт.")?;
            for (index, message) in messages.iter().enumerate() {
                let author = message.get("sender_name").and_then(Value::as_str).map(str::to_owned);
                items.push(RawItem { id: format!("message{index}"), author: author.clone(), author_label: author, text: message.get("content").and_then(Value::as_str).unwrap_or("").into(), created: message.get("timestamp_ms").and_then(Value::as_i64).and_then(|ms| iso(ms / 1000)), kind: "message".into(), mode: "dialogue".into(), ..RawItem::default() });
            }
            warnings.push("В экспорте Instagram автор указан отображаемым именем, а не подтверждённым ID; вложения не распознаны.".into());
        }
        _ => return Err("Для этого сервиса выберите TXT/Markdown с разрешённым материалом; этот формат JSON не поддерживается.".into()),
    }
    Ok((items, warnings))
}
fn telegram(messages: &[Value], chat: &str, items: &mut Vec<RawItem>) {
    for (index, message) in messages.iter().enumerate() {
        if message.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let text = match message.get("text") {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Array(parts)) => parts
                .iter()
                .filter_map(|part| {
                    part.as_str()
                        .or_else(|| part.get("text").and_then(Value::as_str))
                })
                .collect(),
            _ => String::new(),
        };
        let id = message
            .get("id")
            .and_then(Value::as_i64)
            .map(|id| id.to_string())
            .unwrap_or_else(|| index.to_string());
        items.push(RawItem {
            id: format!("{chat}:{id}"),
            author: message
                .get("from_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            author_label: message
                .get("from")
                .and_then(Value::as_str)
                .map(str::to_owned),
            text,
            created: message
                .get("date_unixtime")
                .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
                .and_then(iso),
            kind: "message".into(),
            mode: "dialogue".into(),
            reply_to: message
                .get("reply_to_message_id")
                .and_then(Value::as_i64)
                .map(|id| format!("{chat}:{id}")),
            forwarded: message.get("forwarded_from").is_some(),
            ..RawItem::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn telegram_export_preserves_authorship_rich_text_and_forward_marker() {
        let data = r#"{"messages":[{"id":1,"type":"message","from":"Example","from_id":"user1","date":"2024-01-01T12:30:00","date_unixtime":"1704112200","text":["  Привет\n",{"type":"bold","text":"МИР"}],"forwarded_from":"Other","reply_to_message_id":5}]}"#;
        let (items, _) = parse(Provider::Telegram, data, "json").unwrap();
        assert_eq!(items[0].text, "  Привет\nМИР");
        assert_eq!(items[0].author.as_deref(), Some("user1"));
        assert!(items[0].forwarded);
        assert_eq!(items[0].reply_to.as_deref(), Some("chat:5"));
    }
    #[test]
    fn instagram_export_keeps_context_metadata_without_inventing_attachment_text() {
        let data = r#"{"participants":[{"name":"Example"}],"messages":[{"sender_name":"Example","timestamp_ms":1704112200000,"content":"hey\n:)"},{"sender_name":"Other","timestamp_ms":1704112201000,"photos":[{"uri":"media/a.jpg"}]}]}"#;
        let (items, warnings) = parse(Provider::Instagram, data, "json").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "hey\n:)");
        assert!(items[1].text.is_empty());
        assert!(!warnings.is_empty());
    }
    #[test]
    fn plain_text_stays_exact_and_unknown_json_is_rejected() {
        let text = "  line\r\n\tline  ";
        assert_eq!(
            parse(Provider::Instagram, text, "txt").unwrap().0[0].text,
            text
        );
        assert!(parse(
            Provider::Instagram,
            r#"{"token":"not-a-conversation"}"#,
            "json"
        )
        .is_err());
    }
}
