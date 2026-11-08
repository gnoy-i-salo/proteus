//! Acquisition of one explicitly selected public page; no crawling or model calls.
#[path = "selected-page-http.rs"]
mod http;
#[path = "selected-page-policy.rs"]
mod policy;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    net::IpAddr,
    time::{Duration, Instant},
};
use url::Url;

pub struct Limits {
    pub max_bytes: usize,
    pub max_redirects: usize,
    pub max_duration: Duration,
}

pub struct PageSnapshot {
    pub selected_url: String,
    pub final_url: String,
    pub redirects: Vec<String>,
    pub content_type: String,
    pub original: String,
    pub sha256: String,
    pub retrieved_at: String,
}

pub(super) struct Response {
    pub status: u16,
    pub location: Option<String>,
    pub content_type: Option<String>,
    pub content_encoding: Option<String>,
    pub content_length: Option<u64>,
    pub body: Box<dyn Read>,
}

pub(super) trait Transport {
    fn resolve(&self, host: &str, timeout: Duration) -> Result<Vec<IpAddr>, String>;
    fn get(&self, url: &Url, addresses: &[IpAddr], timeout: Duration) -> Result<Response, String>;
}

/// The caller owns selection/grant validation. This transport never uses account
/// credentials and returns source bytes, not evidence of personal authorship.
pub fn fetch(
    selected_url: &str,
    limits: &Limits,
    check: &mut impl FnMut() -> Result<(), String>,
) -> Result<PageSnapshot, String> {
    acquire(&http::PublicHttp, selected_url, limits, check)
}

pub(super) fn acquire(
    transport: &impl Transport,
    selected_url: &str,
    limits: &Limits,
    check: &mut impl FnMut() -> Result<(), String>,
) -> Result<PageSnapshot, String> {
    if limits.max_bytes == 0
        || limits.max_bytes > 16 * 1024 * 1024
        || limits.max_redirects > 5
        || limits.max_duration.is_zero()
        || limits.max_duration > Duration::from_secs(60)
    {
        return Err("Неверные ограничения загрузки страницы.".into());
    }
    let started = Instant::now();
    let mut remaining = || {
        check()?;
        limits
            .max_duration
            .checked_sub(started.elapsed())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| "Время загрузки страницы истекло.".to_string())
    };
    let mut url = policy::parse(selected_url)?;
    let mut redirects = Vec::new();
    loop {
        remaining()?;
        let addresses = match url.host().ok_or("Нет имени сервера.")? {
            url::Host::Ipv4(ip) => vec![ip.into()],
            url::Host::Ipv6(ip) => vec![ip.into()],
            url::Host::Domain(host) => transport.resolve(host, remaining()?)?,
        };
        policy::validate_addresses(&addresses)?;
        let mut response = transport.get(&url, &addresses, remaining()?)?;
        remaining()?;
        if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            if redirects.len() >= limits.max_redirects {
                return Err("Превышено число перенаправлений страницы.".into());
            }
            let location = response
                .location
                .as_deref()
                .ok_or("Перенаправление не содержит адреса.")?;
            if location.len() > 8192
                || location.trim() != location
                || location.chars().any(char::is_control)
            {
                return Err("Неверный адрес перенаправления.".into());
            }
            let next = policy::parse(
                url.join(location)
                    .map_err(|_| "Неверное перенаправление.")?
                    .as_str(),
            )?;
            if url.scheme() == "https" && next.scheme() != "https" {
                return Err("Страница перенаправляет на незащищённое соединение.".into());
            }
            redirects.push(next.to_string());
            url = next;
            continue;
        }
        if response.status != 200 {
            return Err("Страница недоступна или вернула неполный ответ.".into());
        }
        let content_type = media_type(response.content_type.as_deref())?;
        if response
            .content_encoding
            .as_deref()
            .is_some_and(|v| !v.eq_ignore_ascii_case("identity"))
        {
            return Err("Сжатые страницы не поддерживаются этим загрузчиком.".into());
        }
        if response
            .content_length
            .is_some_and(|len| len > limits.max_bytes as u64)
        {
            return Err("Страница превышает допустимый размер.".into());
        }
        let mut bytes = Vec::new();
        let mut buffer = [0; 8192];
        loop {
            remaining()?;
            let count = response
                .body
                .read(&mut buffer)
                .map_err(|_| "Не удалось прочитать страницу полностью.")?;
            remaining()?;
            if count == 0 {
                break;
            }
            if count > limits.max_bytes - bytes.len() {
                return Err("Страница превышает допустимый размер.".into());
            }
            bytes.extend_from_slice(&buffer[..count]);
        }
        if response
            .content_length
            .is_some_and(|len| len != bytes.len() as u64)
        {
            return Err("Размер полученной страницы не совпадает с заявленным.".into());
        }
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let original =
            String::from_utf8(bytes).map_err(|_| "Страница должна использовать UTF-8.")?;
        remaining()?;
        return Ok(PageSnapshot {
            selected_url: selected_url.into(),
            final_url: url.into(),
            redirects,
            content_type,
            original,
            sha256,
            retrieved_at: chrono::Utc::now().to_rfc3339(),
        });
    }
}

fn media_type(value: Option<&str>) -> Result<String, String> {
    let value = value
        .filter(|v| v.len() <= 512)
        .ok_or("Не указан поддерживаемый тип страницы.")?;
    let mut parts = value.split(';');
    let kind = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    if !matches!(kind.as_str(), "text/html" | "text/plain" | "text/markdown") {
        return Err("Поддерживаются только текстовые и HTML-страницы.".into());
    }
    for parameter in parts {
        let (name, value) = parameter
            .trim()
            .split_once('=')
            .ok_or("Неверный тип страницы.")?;
        if !name.trim().eq_ignore_ascii_case("charset")
            || !value.trim().trim_matches('"').eq_ignore_ascii_case("utf-8")
        {
            return Err("Поддерживаются страницы с кодировкой UTF-8.".into());
        }
    }
    Ok(kind)
}

#[cfg(test)]
#[path = "selected-page-tests.rs"]
mod tests;
