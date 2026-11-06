use reqwest::blocking::Client;
use serde_json::Value;
use std::{io::Read, time::Duration};

pub(crate) trait Http: Send + Sync {
    fn get(
        &self,
        url: &str,
        query: &[(&str, String)],
        token: Option<&str>,
    ) -> Result<Value, String>;
    fn post(&self, url: &str, form: &[(&str, String)]) -> Result<Value, String>;
}
pub(crate) struct Network(Client);
impl Network {
    pub fn new() -> Result<Self, String> {
        Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent("PROTEUS-desktop/0.3")
            .build()
            .map(Self)
            .map_err(|_| "Не удалось подготовить защищённое подключение.".into())
    }
}
fn decode(response: reqwest::blocking::Response) -> Result<Value, String> {
    let endpoint = response.url().clone();
    let status = response.status().as_u16();
    let successful = (200..300).contains(&status);
    if !successful && !(matches!(status, 400 | 401) && token_endpoint(&endpoint)) {
        return Err(status_error(status));
    }
    let limit = if successful {
        8 * 1024 * 1024
    } else {
        64 * 1024
    };
    let mut body = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut body)
        .map_err(|_| "Ответ сервиса не получен полностью.".to_string())?;
    decode_body(&endpoint, status, &body)
}
fn status_error(status: u16) -> String {
    match status {
        401 => "Авторизация истекла или отозвана. Подключите аккаунт заново.",
        403 => "Сервис не разрешил чтение этого ресурса. Проверьте права приложения и аккаунта.",
        404 => "Ресурс не найден или недоступен этому аккаунту.",
        429 => "Лимит запросов сервиса исчерпан. Повторите позже.",
        _ => "Сервис вернул ошибку. Повторите позже.",
    }
    .into()
}
fn token_endpoint(endpoint: &url::Url) -> bool {
    endpoint.scheme() == "https"
        && endpoint.username().is_empty()
        && endpoint.password().is_none()
        && endpoint.port().is_none()
        && endpoint.fragment().is_none()
        && matches!(
            (endpoint.host_str(), endpoint.path()),
            (Some("github.com"), "/login/oauth/access_token")
                | (Some("id.vk.ru"), "/oauth2/auth")
                | (Some("graph.instagram.com"), "/refresh_access_token")
        )
}
fn decode_body(endpoint: &url::Url, status: u16, body: &[u8]) -> Result<Value, String> {
    let successful = (200..300).contains(&status);
    if body.len()
        > if successful {
            8 * 1024 * 1024
        } else {
            64 * 1024
        }
    {
        return Err("Ответ сервиса превышает допустимый размер.".into());
    }
    if !successful && !(matches!(status, 400 | 401) && token_endpoint(endpoint)) {
        return Err(status_error(status));
    }
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| "Сервис вернул неподдерживаемый формат ответа.".to_string())?;
    if token_endpoint(endpoint) {
        let error = value.get("error");
        let code = match error.and_then(Value::as_str) {
            Some("invalid_grant") => Some("invalid_grant"),
            Some("bad_refresh_token") => Some("bad_refresh_token"),
            _ if endpoint.host_str() == Some("graph.instagram.com")
                && error.is_some_and(|error| {
                    error.get("code").and_then(Value::as_u64) == Some(190)
                        && error.get("type").and_then(Value::as_str) == Some("OAuthException")
                        && error.get("is_transient").and_then(Value::as_bool) != Some(true)
                }) =>
            {
                Some("invalid_grant")
            }
            _ => None,
        };
        if let Some(code) = code {
            // Native callers receive only a fixed protocol code, never provider text,
            // echoed tokens or account details from a failed authentication response.
            return Ok(serde_json::json!({"error":code}));
        }
    }
    if successful {
        Ok(value)
    } else {
        Err(status_error(status))
    }
}
impl Http for Network {
    fn get(
        &self,
        url: &str,
        query: &[(&str, String)],
        token: Option<&str>,
    ) -> Result<Value, String> {
        let request = self
            .0
            .get(url)
            .header("Accept", "application/json")
            .query(query);
        let request = if let Some(token) = token {
            request.bearer_auth(token)
        } else {
            request
        };
        decode(
            request.send().map_err(|_| {
                "Не удалось связаться с сервисом. Проверьте подключение.".to_string()
            })?,
        )
    }
    fn post(&self, url: &str, form: &[(&str, String)]) -> Result<Value, String> {
        decode(
            self.0
                .post(url)
                .header("Accept", "application/json")
                .form(form)
                .send()
                .map_err(|_| {
                    "Не удалось связаться с сервисом. Проверьте подключение.".to_string()
                })?,
        )
    }
}
pub(crate) fn required(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| {
            !text.is_empty() && text.len() <= 8192 && !text.chars().any(char::is_control)
        })
        .map(str::to_owned)
        .ok_or_else(|| "В ответе сервиса отсутствуют необходимые данные.".into())
}
pub(crate) fn identifier(value: &Value, key: &str) -> Result<String, String> {
    let id = value
        .get(key)
        .and_then(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .or_else(|| value.as_i64().map(|n| n.to_string()))
        })
        .ok_or("Сервис не подтвердил аккаунт.")?;
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
    {
        return Err("Сервис вернул некорректный идентификатор.".into());
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn body(endpoint: &str, status: u16, value: Value) -> Result<Value, String> {
        decode_body(
            &url::Url::parse(endpoint).unwrap(),
            status,
            &serde_json::to_vec(&value).unwrap(),
        )
    }
    #[test]
    fn terminal_http_oauth_errors_return_only_allowlisted_code_at_exact_token_endpoints() {
        for (endpoint, status, code) in [
            (
                "https://github.com/login/oauth/access_token",
                400,
                "bad_refresh_token",
            ),
            (
                "https://id.vk.ru/oauth2/auth?client_id=123",
                401,
                "invalid_grant",
            ),
        ] {
            let decoded=body(endpoint,status,json!({"error":code,"error_description":"private provider detail","access_token":"must not escape","refresh_token":"must not escape"})).unwrap();
            assert_eq!(decoded, json!({"error":code}));
        }
    }
    #[test]
    fn graph_invalid_token_is_normalized_but_explicit_transient_is_retryable() {
        let endpoint =
            "https://graph.instagram.com/refresh_access_token?grant_type=ig_refresh_token";
        assert_eq!(body(endpoint,400,json!({"error":{"code":190,"type":"OAuthException","message":"private account detail"}})).unwrap(),json!({"error":"invalid_grant"}));
        assert!(body(
            endpoint,
            400,
            json!({"error":{"code":190,"type":"OAuthException","is_transient":true}})
        )
        .is_err());
        assert!(body(
            endpoint,
            400,
            json!({"error":{"code":190,"type":"OtherError"}})
        )
        .is_err());
    }
    #[test]
    fn transient_statuses_untrusted_endpoints_and_unknown_errors_do_not_invalidate_tokens() {
        for endpoint in [
            "https://github.com.evil/login/oauth/access_token",
            "http://github.com/login/oauth/access_token",
            "https://github.com:8443/login/oauth/access_token",
            "https://github.com/user",
            "https://api.github.com/user",
            "https://id.vk.ru/oauth2/user_info",
            "https://graph.instagram.com/me",
        ] {
            assert!(body(endpoint, 400, json!({"error":"invalid_grant"})).is_err());
        }
        for status in [403, 429, 500, 503] {
            assert!(body(
                "https://github.com/login/oauth/access_token",
                status,
                json!({"error":"invalid_grant"})
            )
            .is_err());
        }
        for code in [
            "invalid_client",
            "temporarily_unavailable",
            "server_error",
            "unknown",
        ] {
            assert!(body(
                "https://github.com/login/oauth/access_token",
                400,
                json!({"error":code})
            )
            .is_err());
        }
        let endpoint = url::Url::parse("https://github.com/login/oauth/access_token").unwrap();
        assert!(decode_body(&endpoint, 400, b"not JSON").is_err());
        let huge = json!({"error":"invalid_grant","error_description":"x".repeat(65536)});
        assert!(decode_body(&endpoint, 400, &serde_json::to_vec(&huge).unwrap()).is_err());
    }
}
