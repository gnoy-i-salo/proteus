//! Host-only simulation publication request. Model actions cannot write history.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppendSimulationRequest {
    pub schema_version: u8,
    pub experiment_id: String,
    pub trial_id: String,
    pub context_lease_id: String,
    pub invocation_id: String,
    pub answer: String,
}
fn json_space(c: char) -> bool {
    c == '\u{feff}' || (c.is_whitespace() && c != '\u{85}')
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reply {
    action: String,
    messages: Vec<String>,
    summary: String,
    style_conflicts: Vec<String>,
}
impl AppendSimulationRequest {
    pub fn validate(&self) -> Result<(), String> {
        for id in [&self.experiment_id, &self.trial_id, &self.context_lease_id] {
            super::types::validate_host_id(id, "область симуляции")?;
        }
        if self.schema_version != 1
            || self.context_lease_id.len() > 128
            || !crate::cli::model::valid_id(&self.invocation_id)
            || self.answer.len() > 4 * 1024 * 1024
            || self.answer.trim_matches(json_space).is_empty()
        {
            return Err("Неверные параметры записи симуляции.".into());
        }
        Ok(())
    }
    pub(crate) fn reply_text(&self) -> Result<String, String> {
        self.validate()?;
        // Mirror structured-response container handling, never trim messages.
        let mut json = self.answer.trim_matches(json_space);
        if let Some(value) = json.strip_prefix("```") {
            json = value
                .strip_prefix("json")
                .unwrap_or(value)
                .trim_start_matches(json_space);
        }
        if let Some(value) = json.strip_suffix("```") {
            json = value.trim_end_matches(json_space);
        }
        let reply: Reply = serde_json::from_str(json)
            .map_err(|_| "Ответ не является финальной репликой Person.")?;
        let metadata = |text: &str| {
            !text.trim_matches(json_space).is_empty() && text.encode_utf16().count() <= 24000
        };
        if reply.action != "reply"
            || !(1..=30).contains(&reply.messages.len())
            || !metadata(&reply.summary)
            || reply.style_conflicts.len() > 30
            || reply.style_conflicts.iter().any(|text| !metadata(text))
            || reply
                .messages
                .iter()
                .any(|text| text.trim_matches(json_space).is_empty())
        {
            return Err("Ответ не соответствует формату реплики Person.".into());
        }
        let bytes = reply
            .messages
            .iter()
            .try_fold((reply.messages.len() - 1) * 2, |sum, message| {
                sum.checked_add(message.len())
            })
            .filter(|bytes| *bytes <= 65536)
            .ok_or("Текст симуляции превышает 64 КиБ.")?;
        let mut text = String::with_capacity(bytes);
        for (index, message) in reply.messages.iter().enumerate() {
            if index > 0 {
                text.push_str("\n\n");
            }
            text.push_str(message);
        }
        Ok(text)
    }
}
#[cfg(test)]
#[path = "memory-simulation-tests.rs"]
mod tests;
