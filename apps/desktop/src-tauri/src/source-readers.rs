#[path = "source-reader-github.rs"]
mod github;
#[path = "source-reader-instagram.rs"]
mod instagram;
#[path = "source-reader-telegram.rs"]
mod telegram;
#[path = "source-reader-vk.rs"]
mod vk;
use super::{
    grant::iso,
    model::{Checkpoint, Page, RawItem, SourceGrantRequest},
};
use crate::connections::{Connections, Provider};
use serde_json::Value;
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
        Provider::Telegram => telegram::telegram(hub, request, account, checkpoint, limit),
        Provider::Github => github::github(hub, request, account, checkpoint, limit),
        Provider::Vk => vk::vk(hub, request, account, checkpoint, limit),
        Provider::Instagram => instagram::instagram(hub, request, account, checkpoint, limit),
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
            budget: super::super::model::SourceBudget {
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
