use super::{
    http::identifier,
    model::{ConnectionConfig, Token},
    response::{reject_api_error, safe_label},
    session::Connections,
    storage::save_secret,
};
use chrono::Utc;
use serde_json::Value;
impl Connections {
    pub(super) fn configure_instagram(&self, request: &mut ConnectionConfig) -> Result<(), String> {
        let access = request
            .token
            .take()
            .filter(|token| {
                !token.is_empty() && token.len() <= 4096 && !token.chars().any(char::is_whitespace)
            })
            .ok_or("Введите токен своего профессионального Instagram-аккаунта.")?;
        let identity = self.0.http.get(
            "https://graph.instagram.com/me",
            &[("fields", "user_id,username".into())],
            Some(&access),
        )?;
        reject_api_error(&identity)?;
        let account_id =
            identifier(&identity, "user_id").or_else(|_| identifier(&identity, "id"))?;
        let token = Token {
            access,
            refresh: None,
            device_id: None,
            expires_at: None,
            refreshed_at: Utc::now().timestamp(),
            label: safe_label(&identity, "username", &account_id),
            account_id,
        };
        save_secret(self.0.secrets.as_ref(), "instagram-token", &token)?;
        request.client_id = None;
        request.api_id = None;
        request.api_hash = None;

        Ok(())
    }
    pub(super) fn refresh_instagram(&self, token: &Token) -> Result<Value, String> {
        self.0.http.get(
            "https://graph.instagram.com/refresh_access_token",
            &[
                ("grant_type", "ig_refresh_token".into()),
                ("access_token", token.access.clone()),
            ],
            None,
        )
    }
}
