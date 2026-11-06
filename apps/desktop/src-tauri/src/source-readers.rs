use super::{iso, Checkpoint, Page, RawItem, SourceGrantRequest};
use crate::connections::{http::identifier, reject_api_error, Connections, Provider};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) fn next_page(
    hub: &Connections,
    request: &SourceGrantRequest,
    account: &str,
    checkpoint: &mut Checkpoint,
    limit: usize,
    cancelled: &AtomicBool,
    deadline: i64,
) -> Result<Page, String> {
    check(cancelled, deadline)?;
    match request.provider {
        Provider::Telegram => telegram(hub, request, account, checkpoint, limit),
        Provider::Github => github(hub, request, account, checkpoint, limit),
        Provider::Vk => vk(hub, request, account, checkpoint, limit),
        Provider::Instagram => instagram(hub, request, account, checkpoint, limit),
    }
}
fn check(cancelled: &AtomicBool, deadline: i64) -> Result<(), String> {
    if cancelled.load(Ordering::SeqCst) {
        return Err("Сбор отменён.".into());
    }
    if chrono::Utc::now().timestamp_millis() >= deadline {
        return Err("source-budget-exceeded".into());
    }
    Ok(())
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| "Сервис вернул неподдерживаемую структуру данных.".into())
}
fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or("").into()
}
fn optional(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}
fn date(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_i64).and_then(iso)
}
fn safe_web_url(value: &Value, key: &str, hosts: &[&str]) -> Option<String> {
    let raw = value.get(key)?.as_str()?;
    let url = url::Url::parse(raw).ok()?;
    (url.scheme() == "https"
        && hosts.contains(&url.host_str()?)
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none())
    .then(|| raw.into())
}
fn telegram(
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
fn github(
    hub: &Connections,
    request: &SourceGrantRequest,
    account: &str,
    checkpoint: &mut Checkpoint,
    limit: usize,
) -> Result<Page, String> {
    let token = hub.token_for(Provider::Github, account)?;
    let base = format!("https://api.github.com/repos/{}", request.resource_id);
    if checkpoint.phase.is_empty() {
        checkpoint.phase = "issues".into();
    }
    if matches!(
        checkpoint.phase.as_str(),
        "issues" | "issue-comments" | "review-comments"
    ) {
        let route = match checkpoint.phase.as_str() {
            "issues" => "issues",
            "issue-comments" => "issues/comments",
            _ => "pulls/comments",
        };
        if checkpoint.queue.is_empty() {
            let value = hub.http().get(
                &format!("{base}/{route}"),
                &[
                    ("state", "all".into()),
                    ("sort", "created".into()),
                    ("direction", "asc".into()),
                    ("per_page", "20".into()),
                    ("page", (checkpoint.offset + 1).to_string()),
                ],
                Some(&token.access),
            )?;
            let values = value
                .as_array()
                .ok_or("GitHub вернул неподдерживаемую страницу.")?;
            if values.len() > 20 {
                return Err("GitHub превысил размер запрошенной страницы.".into());
            }
            checkpoint.queue = values.clone();
            checkpoint.offset += 1;
            checkpoint.cursor = if values.len() < 20 { "last" } else { "" }.into();
        }
        let values: Vec<_> = checkpoint
            .queue
            .drain(..limit.min(checkpoint.queue.len()))
            .collect();
        let mut page = Page::default();
        for value in &values {
            let id = identifier(value, "id")?;
            let number = value.get("number").and_then(Value::as_u64).or_else(|| {
                value
                    .get("issue_url")
                    .or_else(|| value.get("pull_request_url"))
                    .and_then(Value::as_str)
                    .and_then(|url| url.rsplit('/').next()?.parse().ok())
            });
            let thread = number.map(|number| format!("issue:{number}"));
            page.items.push(RawItem {
                id: format!("{}:{id}", checkpoint.phase),
                author: value
                    .pointer("/user/login")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                author_label: value
                    .pointer("/user/login")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                text: text(value, "body"),
                kind: if checkpoint.phase == "issues" {
                    "post"
                } else {
                    "comment"
                }
                .into(),
                mode: "comment".into(),
                url: safe_web_url(value, "html_url", &["github.com"]),
                created: optional(value, "created_at"),
                reply_to: value
                    .get("in_reply_to_id")
                    .and_then(Value::as_u64)
                    .map(|id| format!("review-comments:{id}")),
                thread,
                ..RawItem::default()
            });
        }
        if checkpoint.queue.is_empty() && checkpoint.cursor == "last" {
            checkpoint.phase = match checkpoint.phase.as_str() {
                "issues" => "issue-comments",
                "issue-comments" => "review-comments",
                _ => "tree",
            }
            .into();
            checkpoint.offset = 0;
            checkpoint.cursor.clear();
        }
        return Ok(page);
    }
    if checkpoint.phase == "tree" {
        let value = hub.http().get(
            &format!("{base}/git/trees/HEAD"),
            &[("recursive", "1".into())],
            Some(&token.access),
        )?;
        let mut page = Page::default();
        for file in array(&value, "tree")? {
            if file.get("type").and_then(Value::as_str) != Some("blob") {
                continue;
            }
            let path = text(file, "path");
            let extension = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            if file
                .get("size")
                .and_then(Value::as_u64)
                .is_some_and(|size| size <= 1024 * 1024)
                && [
                    "md", "txt", "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "c", "h",
                    "cpp", "css", "scss", "html", "json", "yaml", "yml", "toml", "sh", "sql", "rb",
                    "swift", "kt",
                ]
                .contains(&extension.as_str())
            {
                checkpoint
                    .queue
                    .push(json!({"sha":identifier(file,"sha")?,"path":path}));
            } else {
                page.not_analyzed += 1;
            }
        }
        if value.get("truncated").and_then(Value::as_bool) == Some(true) {
            page.truncated = true;
            page.unavailable.push(
                "GitHub сократил дерево репозитория; не все файлы доступны в этой выборке.".into(),
            );
        }
        page.unavailable.push("История коммитов, Discussions, бинарные файлы и внешние submodules не включены; авторство текущего кода не приписывается владельцу репозитория.".into());
        checkpoint.phase = "blobs".into();
        page.done = checkpoint.queue.is_empty();
        return Ok(page);
    }
    if checkpoint.phase == "blobs" {
        let Some(file) = checkpoint.queue.get(checkpoint.index).cloned() else {
            return Ok(Page {
                done: true,
                ..Page::default()
            });
        };
        let sha = identifier(&file, "sha")?;
        if !sha.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("GitHub вернул неверный hash файла.".into());
        }
        let value = hub
            .http()
            .get(&format!("{base}/git/blobs/{sha}"), &[], Some(&token.access))?;
        if value.get("encoding").and_then(Value::as_str) != Some("base64") {
            return Err("GitHub вернул неизвестную кодировку файла.".into());
        }
        let data = STANDARD
            .decode(
                text(&value, "content")
                    .split_whitespace()
                    .collect::<String>(),
            )
            .map_err(|_| "GitHub вернул повреждённый файл.")?;
        let mut page = Page::default();
        checkpoint.index += 1;
        page.done = checkpoint.index == checkpoint.queue.len();
        if let Ok(body) = String::from_utf8(data) {
            if !body.contains('\0') {
                let path = text(&file, "path");
                page.items.push(RawItem {
                    id: format!("file:{path}"),
                    text: body,
                    kind: "code".into(),
                    mode: "formal".into(),
                    thread: Some(format!("file:{path}")),
                    ..RawItem::default()
                });
            } else {
                page.not_analyzed += 1;
            }
        } else {
            page.not_analyzed += 1;
        }
        return Ok(page);
    }
    Err("Неизвестный checkpoint GitHub.".into())
}
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
fn vk(
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
fn instagram(
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::connections::{
        http::Http,
        storage::{MemorySecrets, SecretStore},
        Provider,
    };
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};
    struct FakeHttp(Mutex<Vec<(String, Value)>>);
    impl Http for FakeHttp {
        fn get(
            &self,
            url: &str,
            _: &[(&str, String)],
            token: Option<&str>,
        ) -> Result<Value, String> {
            assert_eq!(token, Some("fixture-token"));
            let (expected, response) = self.0.lock().unwrap().remove(0);
            assert_eq!(url, expected);
            Ok(response)
        }
        fn post(&self, _: &str, _: &[(&str, String)]) -> Result<Value, String> {
            panic!("unexpected write")
        }
    }
    fn hub(responses: Vec<(String, Value)>) -> Connections {
        let store = Arc::new(MemorySecrets::default());
        store.set("github-token", &json!({"access":"fixture-token","refresh":null,"device-id":null,"expires-at":null,"refreshed-at":0,"account-id":"42","label":"fixture"}).to_string()).unwrap();
        Connections::test(store, Arc::new(FakeHttp(Mutex::new(responses))))
    }
    fn request() -> SourceGrantRequest {
        SourceGrantRequest {
            provider: Provider::Github,
            resource_id: "owner/repo".into(),
            author_id: None,
            from: None,
            to: None,
            allow_model_processing: true,
            operations: vec!["collect".into()],
            budget: super::super::SourceBudget {
                max_steps: 40,
                max_documents: 1000,
                max_bytes: 1048576,
                max_duration_ms: 300000,
            },
        }
    }
    #[test]
    fn github_reader_paginates_with_fixed_origin_and_never_follows_response_urls() {
        let hub = hub(vec![(
            "https://api.github.com/repos/owner/repo/issues".into(),
            json!([{"id":1,"number":7,"body":"line\nline", "user":{"login":"person"},"created_at":"2024-01-01T00:00:00Z","url":"https://evil.invalid","comments_url":"https://evil.invalid"}]),
        )]);
        let mut checkpoint = Checkpoint {
            phase: "issues".into(),
            ..Checkpoint::default()
        };
        let page = next_page(
            &hub,
            &request(),
            "42",
            &mut checkpoint,
            20,
            &AtomicBool::new(false),
            i64::MAX,
        )
        .unwrap();
        assert_eq!(page.items[0].text, "line\nline");
        assert_eq!(page.items[0].thread.as_deref(), Some("issue:7"));
        assert_eq!(checkpoint.phase, "issue-comments");
        assert!(!page.done);
    }
    #[test]
    fn cancellation_is_checked_before_any_provider_access() {
        assert!(next_page(
            &hub(vec![]),
            &request(),
            "42",
            &mut Checkpoint::default(),
            20,
            &AtomicBool::new(true),
            i64::MAX
        )
        .is_err());
    }
    #[test]
    fn github_page_size_changes_do_not_skip_or_repeat_documents() {
        let hub = hub(vec![(
            "https://api.github.com/repos/owner/repo/issues".into(),
            json!([{"id":1,"number":1,"body":"one"},{"id":2,"number":2,"body":"two"},{"id":3,"number":3,"body":"three"}]),
        )]);
        let mut checkpoint = Checkpoint::default();
        let first = next_page(
            &hub,
            &request(),
            "42",
            &mut checkpoint,
            1,
            &AtomicBool::new(false),
            i64::MAX,
        )
        .unwrap();
        assert_eq!(first.items.len(), 1);
        let second = next_page(
            &hub,
            &request(),
            "42",
            &mut checkpoint,
            20,
            &AtomicBool::new(false),
            i64::MAX,
        )
        .unwrap();
        assert_eq!(
            second
                .items
                .iter()
                .map(|v| v.text.as_str())
                .collect::<Vec<_>>(),
            vec!["two", "three"]
        );
    }
    #[test]
    fn instagram_posts_and_comments_use_the_same_confirmed_author_id() {
        let secrets = Arc::new(MemorySecrets::default());
        secrets.set("instagram-token", &json!({"access":"fixture-token","refresh":null,"device-id":null,"expires-at":null,"refreshed-at":chrono::Utc::now().timestamp(),"account-id":"42","label":"fixture"}).to_string()).unwrap();
        let hub = Connections::test(
            secrets,
            Arc::new(FakeHttp(Mutex::new(vec![
                (
                    "https://graph.instagram.com/42/media".into(),
                    json!({"data":[{"id":"100","caption":"caption"}]}),
                ),
                (
                    "https://graph.instagram.com/100/comments".into(),
                    json!({"data":[{"id":"101","text":"comment","username":"same-user","from":{"id":"42","username":"same-user"}}]}),
                ),
            ]))),
        );
        let mut selected = request();
        selected.provider = Provider::Instagram;
        selected.resource_id = "42".into();
        let mut checkpoint = Checkpoint::default();
        let post = next_page(
            &hub,
            &selected,
            "42",
            &mut checkpoint,
            20,
            &AtomicBool::new(false),
            i64::MAX,
        )
        .unwrap();
        let comment = next_page(
            &hub,
            &selected,
            "42",
            &mut checkpoint,
            20,
            &AtomicBool::new(false),
            i64::MAX,
        )
        .unwrap();
        assert_eq!(post.items[0].author, comment.items[0].author);
        assert_eq!(comment.items[0].author.as_deref(), Some("42"));
    }
}
