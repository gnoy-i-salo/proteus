use super::model::SourceGrantRequest;
use crate::connections::Provider;
use chrono::{DateTime, NaiveDate, Utc};
pub(crate) fn timestamp(text: &str) -> Result<i64, String> {
    if let Ok(value) = DateTime::parse_from_rfc3339(text) {
        return Ok(value.timestamp());
    }
    if let Ok(value) = DateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%z") {
        return Ok(value.timestamp());
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|date| date.and_utc().timestamp())
        .ok_or_else(|| "Неверный формат даты.".into())
}
pub(crate) fn iso(seconds: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(seconds, 0).map(|value| value.to_rfc3339())
}
pub(super) fn validate_request(request: &mut SourceGrantRequest) -> Result<(), String> {
    if !request.allow_model_processing {
        return Err(
            "Подтвердите передачу выбранного материала настроенному модельному провайдеру.".into(),
        );
    }
    let budget = &request.budget;
    if !(1..=256).contains(&budget.max_steps)
        || !(1..=10000).contains(&budget.max_documents)
        || !(1024..=16 * 1024 * 1024).contains(&budget.max_bytes)
        || !(1000..=1800000).contains(&budget.max_duration_ms)
    {
        return Err("Бюджет источника выходит за допустимые пределы.".into());
    }
    if request.operations.is_empty()
        || request.operations.len() > 5
        || request
            .operations
            .iter()
            .any(|op| !["describe", "coverage", "search", "collect", "read"].contains(&op.as_str()))
        || request
            .operations
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != request.operations.len()
    {
        return Err("Недопустимые операции источника.".into());
    }
    if request
        .author_id
        .as_ref()
        .is_some_and(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
    {
        return Err("Неверный идентификатор автора.".into());
    }
    let from = request.from.as_deref().map(timestamp).transpose()?;
    let to = request
        .to
        .as_deref()
        .map(|text| {
            timestamp(text).map(|seconds| seconds + if text.len() == 10 { 86399 } else { 0 })
        })
        .transpose()?;
    if from.zip(to).is_some_and(|(from, to)| from > to) {
        return Err("Начало периода позже окончания.".into());
    }
    request.from = from.and_then(iso);
    request.to = to.and_then(iso);
    request.resource_id = canonical_resource(request.provider, &request.resource_id)?;
    Ok(())
}
pub(super) fn canonical_resource(provider: Provider, raw: &str) -> Result<String, String> {
    let invalid = || "Нужна ссылка или идентификатор одного поддерживаемого ресурса.".to_string();
    if raw.len() > 512
        || raw.contains('%')
        || raw.chars().any(char::is_whitespace)
        || raw.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Err(invalid());
    }
    if let Some(id) = raw.strip_prefix("import:") {
        return if valid_id(id) {
            Ok(raw.into())
        } else {
            Err(invalid())
        };
    }
    let resource = if raw.starts_with("https://") {
        let url = url::Url::parse(raw).map_err(|_| invalid())?;
        let hosts: &[&str] = match provider {
            Provider::Github => &["github.com", "www.github.com"],
            Provider::Vk => &["vk.com", "www.vk.com", "vk.ru", "www.vk.ru"],
            Provider::Telegram => &["t.me"],
            Provider::Instagram => &[],
        };
        if !hosts.contains(&url.host_str().unwrap_or(""))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        url.path().trim_matches('/').to_string()
    } else {
        raw.to_string()
    };
    let valid = match provider {
        Provider::Github => {
            let parts: Vec<_> = resource.split('/').collect();
            parts.len() == 2
                && parts.iter().all(|part| {
                    !part.is_empty()
                        && part.len() <= 100
                        && part
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
                })
        }
        Provider::Vk => {
            resource.strip_prefix("id").is_some_and(positive_number)
                || resource
                    .strip_prefix("wall")
                    .and_then(|v| v.split_once('_'))
                    .is_some_and(|(owner, post)| {
                        owner.parse::<i64>().is_ok_and(|id| id != 0) && positive_number(post)
                    })
        }
        Provider::Telegram => {
            resource
                .parse::<i64>()
                .is_ok_and(|id| id != 0 && id.unsigned_abs() < (1_u64 << 53))
                || resource.strip_prefix('@').is_some_and(valid_username)
        }
        Provider::Instagram => positive_number(&resource),
    };
    if valid {
        Ok(resource)
    } else if provider == Provider::Telegram
        && raw.starts_with("https://t.me/")
        && valid_username(&resource)
    {
        Ok(format!("@{resource}"))
    } else {
        Err(invalid())
    }
}
fn positive_number(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|c| c.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|id| id > 0)
}
fn valid_username(value: &str) -> bool {
    (5..=64).contains(&value.len()) && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
pub(super) fn valid_id(value: &str) -> bool {
    (16..=80).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
}
