use serde_json::Value;
pub(super) fn safe_label(value: &Value, key: &str, fallback: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| s.len() <= 128 && !s.chars().any(char::is_control))
        .unwrap_or(fallback)
        .into()
}
pub(super) fn seconds(value: &Value, key: &str) -> Result<i64, String> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .filter(|n| (1..=31536000).contains(n))
        .ok_or_else(|| "Сервис вернул неверный срок действия.".into())
}
pub(crate) fn reject_api_error(value: &Value) -> Result<(), String> {
    if value.get("error").is_some() {
        Err("Сервис отклонил запрос. Проверьте права приложения и повторите подключение.".into())
    } else {
        Ok(())
    }
}
