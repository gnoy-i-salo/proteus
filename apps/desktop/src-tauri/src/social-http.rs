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
    let status = response.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            401 => "Авторизация истекла или отозвана. Подключите аккаунт заново.",
            403 => {
                "Сервис не разрешил чтение этого ресурса. Проверьте права приложения и аккаунта."
            }
            404 => "Ресурс не найден или недоступен этому аккаунту.",
            429 => "Лимит запросов сервиса исчерпан. Повторите позже.",
            _ => "Сервис вернул ошибку. Повторите позже.",
        }
        .into());
    }
    let mut body = Vec::new();
    response
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut body)
        .map_err(|_| "Ответ сервиса не получен полностью.".to_string())?;
    if body.len() > 8 * 1024 * 1024 {
        return Err("Ответ сервиса превышает допустимый размер.".into());
    }
    serde_json::from_slice(&body)
        .map_err(|_| "Сервис вернул неподдерживаемый формат ответа.".into())
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
