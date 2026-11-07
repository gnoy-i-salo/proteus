use super::{
    array, optional, safe_web_url, text, Checkpoint, Connections, Page, Provider, RawItem,
    SourceGrantRequest,
};
use crate::connections::http::identifier;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
pub(super) fn github(
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
