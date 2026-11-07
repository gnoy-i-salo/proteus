use super::{
    array, date, text, Checkpoint, Connections, Page, Provider, RawItem, SourceGrantRequest,
};
use crate::connections::{http::identifier, reject_api_error};
use serde_json::{json, Value};
fn vk_call(
    hub: &Connections,
    account: &str,
    method: &str,
    mut fields: Vec<(&str, String)>,
) -> Result<Value, String> {
    if !["wall.get", "wall.getById", "wall.getComments"].contains(&method) {
        return Err("Метод VK не разрешён.".into());
    }
    fields.push(("access_token", hub.token_for(Provider::Vk, account)?.access));
    fields.push(("v", "5.199".into()));
    let value = hub
        .http()
        .post(&format!("https://api.vk.com/method/{method}"), &fields)?;
    reject_api_error(&value)?;
    value
        .get("response")
        .cloned()
        .ok_or("VK не вернул результат чтения.".into())
}
pub(super) fn vk(
    hub: &Connections,
    request: &SourceGrantRequest,
    account: &str,
    checkpoint: &mut Checkpoint,
    limit: usize,
) -> Result<Page, String> {
    let selected_post = request
        .resource_id
        .strip_prefix("wall")
        .and_then(|v| v.split_once('_'));
    let owner = selected_post
        .map(|(owner, _)| owner)
        .or_else(|| request.resource_id.strip_prefix("id"))
        .ok_or("Неверный ресурс VK.")?;
    if checkpoint.phase.is_empty() {
        checkpoint.phase = "wall".into();
    }
    if checkpoint.phase == "wall" {
        let value = if selected_post.is_some() {
            vk_call(
                hub,
                account,
                "wall.getById",
                vec![
                    (
                        "posts",
                        request.resource_id.trim_start_matches("wall").into(),
                    ),
                    ("extended", "1".into()),
                ],
            )?
        } else {
            vk_call(
                hub,
                account,
                "wall.get",
                vec![
                    ("owner_id", owner.into()),
                    ("offset", checkpoint.offset.to_string()),
                    ("count", limit.to_string()),
                    ("extended", "1".into()),
                ],
            )?
        };
        let values = if let Some(items) = value.as_array() {
            items
        } else {
            array(&value, "items")?
        };
        let mut page = Page::default();
        for value in values {
            let id = identifier(value, "id")?;
            checkpoint.queue.push(json!({"post":id}));
            page.items.push(vk_item(value, "post", owner, &id));
        }
        checkpoint.offset += values.len() as u64;
        if selected_post.is_some()
            || values.len() < limit
            || value
                .get("count")
                .and_then(Value::as_u64)
                .is_some_and(|count| checkpoint.offset >= count)
        {
            checkpoint.phase = "comments".into();
            checkpoint.offset = 0;
        }
        page.done = checkpoint.phase == "comments" && checkpoint.queue.is_empty();
        return Ok(page);
    }
    let Some(post) = checkpoint.queue.get(checkpoint.index).cloned() else {
        return Ok(Page {
            done: true,
            ..Page::default()
        });
    };
    let post = identifier(&post, "post")?;
    let value = vk_call(
        hub,
        account,
        "wall.getComments",
        vec![
            ("owner_id", owner.into()),
            ("post_id", post.clone()),
            ("offset", checkpoint.offset.to_string()),
            ("count", limit.to_string()),
            ("sort", "asc".into()),
            ("thread_items_count", "0".into()),
            ("preview_length", "0".into()),
        ],
    )?;
    let values = array(&value, "items")?;
    let mut page = Page::default();
    for value in values {
        let replies = value
            .pointer("/thread/count")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if replies > 0 {
            page.not_analyzed += replies;
            page.truncated = true;
            page.unavailable.push("Вложенные ветки комментариев VK не включены; доступна основная ветка выбранной записи.".into());
        }
        page.items.push(vk_item(value, "comment", owner, &post));
    }
    checkpoint.offset += values.len() as u64;
    if values.len() < limit
        || value
            .get("count")
            .and_then(Value::as_u64)
            .is_some_and(|count| checkpoint.offset >= count)
    {
        checkpoint.index += 1;
        checkpoint.offset = 0;
    }
    page.done = checkpoint.index >= checkpoint.queue.len();
    Ok(page)
}
fn vk_item(value: &Value, kind: &str, owner: &str, post: &str) -> RawItem {
    let id = value
        .get("id")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .to_string();
    RawItem {
        id: format!("{kind}:{owner}:{post}:{id}"),
        author: value
            .get("from_id")
            .and_then(Value::as_i64)
            .map(|id| id.to_string()),
        text: text(value, "text"),
        kind: kind.into(),
        mode: "comment".into(),
        url: Some(format!("https://vk.com/wall{owner}_{post}")),
        created: date(value, "date"),
        reply_to: value
            .get("reply_to_comment")
            .and_then(Value::as_i64)
            .filter(|id| *id != 0)
            .map(|id| format!("comment:{owner}:{post}:{id}")),
        forwarded: value.get("copy_history").is_some(),
        thread: Some(format!("{owner}_{post}")),
        ..RawItem::default()
    }
}
