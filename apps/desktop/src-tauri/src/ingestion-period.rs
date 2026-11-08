//! Shared source period parsing. Legacy date-only values keep their UTC-midnight semantics.
pub fn date_ms(value: &str) -> Result<i64, String> {
    if let Ok(date) = chrono::DateTime::parse_from_rfc3339(value) {
        return Ok(date.timestamp_millis());
    }
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|date| date.and_utc().timestamp_millis())
        .ok_or_else(|| "Неверное время источника.".into())
}
