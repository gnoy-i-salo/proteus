use super::{
    array, optional, safe_web_url, text, Checkpoint, Connections, Page, Provider, RawItem,
    SourceGrantRequest,
};
use crate::connections::{http::identifier, reject_api_error};
use serde_json::{json, Value};
pub(super) fn instagram(
    hub: &Connections,
    request: &SourceGrantRequest,
    account: &str,
    checkpoint: &mut Checkpoint,
    limit: usize,
) -> Result<Page, String> {
    let token = hub.token_for(Provider::Instagram, account)?;
    if token.account_id != request.resource_id {
        return Err("Разрешён только подключённый Instagram-аккаунт.".into());
    }
    if checkpoint.phase.is_empty() {
        checkpoint.phase = "media".into();
    }
    let media = checkpoint.phase == "media";
    let resource = if media {
        request.resource_id.clone()
    } else {
        match checkpoint.queue.get(checkpoint.index) {
            Some(value) => identifier(value, "id")?,
            None => {
                return Ok(Page {
                    done: true,
                    ..Page::default()
                })
            }
        }
    };
    let route = if media { "media" } else { "comments" };
    let fields = if media {
        "id,caption,timestamp,permalink"
    } else {
        "id,text,timestamp,username,from{id,username}"
    };
    let mut query = vec![("fields", fields.into()), ("limit", limit.to_string())];
    if !checkpoint.cursor.is_empty() {
        query.push(("after", checkpoint.cursor.clone()));
    }
    let value = hub.http().get(
        &format!("https://graph.instagram.com/{resource}/{route}"),
        &query,
        Some(&token.access),
    )?;
    reject_api_error(&value)?;
    let mut page = Page::default();
    for value in array(&value, "data")? {
        let id = identifier(value, "id")?;
        if media {
            checkpoint.queue.push(json!({"id":id}));
        }
        page.items.push(RawItem {
            id: format!("{route}:{id}"),
            author: if media {
                Some(token.account_id.clone())
            } else {
                value
                    .get("from")
                    .and_then(|from| identifier(from, "id").ok())
            },
            author_label: if media {
                None
            } else {
                value
                    .get("from")
                    .and_then(|from| optional(from, "username"))
                    .or_else(|| optional(value, "username"))
            },
            text: text(value, if media { "caption" } else { "text" }),
            created: optional(value, "timestamp"),
            kind: if media { "post" } else { "comment" }.into(),
            mode: "comment".into(),
            url: safe_web_url(value, "permalink", &["www.instagram.com", "instagram.com"]),
            thread: Some(resource.clone()),
            ..RawItem::default()
        });
    }
    // Provider next URLs are untrusted data. Only copy the bounded opaque cursor into a fixed endpoint.
    let after = value
        .pointer("/paging/cursors/after")
        .and_then(Value::as_str)
        .filter(|s| s.len() <= 4096 && !s.chars().any(char::is_control));
    if value.pointer("/paging/next").is_some() {
        let after = after.ok_or("Instagram вернул некорректный курсор.")?;
        if after == checkpoint.cursor {
            return Err("Instagram повторил курсор страницы.".into());
        }
        checkpoint.cursor = after.into();
    } else {
        checkpoint.cursor.clear();
        if media {
            checkpoint.phase = "comments".into();
        } else {
            checkpoint.index += 1;
        }
    }
    page.done = checkpoint.phase == "comments" && checkpoint.index >= checkpoint.queue.len();
    if !media {
        page.truncated = true;
        page.unavailable.push("Ответы на комментарии Instagram, сообщения и содержимое изображений не включены; API ограничен подключённым профессиональным аккаунтом.".into());
    }
    Ok(page)
}
