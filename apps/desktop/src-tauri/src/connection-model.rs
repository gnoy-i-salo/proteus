use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    Telegram,
    Vk,
    Github,
    Instagram,
}
#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TelegramMethod {
    #[default]
    Qr,
    Phone,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionPage {
    VkAuth,
    InstagramExport,
}
impl Provider {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Vk => "vk",
            Self::Github => "github",
            Self::Instagram => "instagram",
        }
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectionConfig {
    pub provider: Option<Provider>,
    pub client_id: Option<String>,
    pub api_id: Option<i32>,
    pub api_hash: Option<String>,
    pub token: Option<String>,
}

#[cfg_attr(test, derive(Deserialize))]
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub provider: Provider,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poll_after_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qr_code: Option<String>,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Token {
    pub access: String,
    pub(super) refresh: Option<String>,
    pub(super) device_id: Option<String>,
    pub(super) expires_at: Option<i64>,
    pub(super) refreshed_at: i64,
    pub account_id: String,
    pub(super) label: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DevicePending {
    pub(super) client_id: String,
    pub(super) device_code: String,
    pub(super) user_code: String,
    pub(super) expires_at: i64,
    pub(super) next_poll: i64,
    pub(super) interval: i64,
}
