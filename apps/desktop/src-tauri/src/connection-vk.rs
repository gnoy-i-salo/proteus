use super::{
    http::{identifier, required},
    model::{ConnectionConfig, ConnectionStatus, Provider, Token},
    oauth::{self, VkPending, VK_REDIRECT, VK_TOKEN},
    response::{reject_api_error, safe_label, seconds},
    session::Connections,
    storage::{self, save_secret},
};
use chrono::Utc;
use serde_json::Value;
impl Connections {
    pub(super) fn begin_vk(&self, config: ConnectionConfig, now: i64) -> Result<(), String> {
        let pending = VkPending::new(config.client_id.ok_or("Client ID не настроен.")?, now)?;
        save_secret(self.0.secrets.as_ref(), "vk-pending", &pending)?;
        if let Err(error) = oauth::open_browser(&pending.authorization_url()) {
            self.0.secrets.remove("vk-pending")?;
            return Err(error);
        }

        Ok(())
    }
    pub fn handle_callback(&self, raw: &str) -> Result<ConnectionStatus, String> {
        let callback = oauth::parse_callback(raw)?;
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния подключений.")?;
        let pending =
            oauth::claim_pending(self.0.secrets.as_ref(), &callback, Utc::now().timestamp())?;
        if callback.error.is_some() {
            return self.status(Provider::Vk);
        }
        let device_id = callback.device_id.ok_or("Отсутствует устройство входа.")?;
        let response = self.vk_post(
            VK_TOKEN,
            &[
                ("grant_type", "authorization_code".into()),
                ("client_id", pending.client_id.clone()),
                ("redirect_uri", VK_REDIRECT.into()),
                ("state", pending.state.clone()),
                ("code_verifier", pending.verifier),
                ("device_id", device_id.clone()),
            ],
            &[("code", callback.code.ok_or("Отсутствует код входа.")?)],
        )?;
        reject_api_error(&response)?;
        if response.get("state").and_then(Value::as_str) != Some(pending.state.as_str()) {
            return Err("VK вернул несовпадающее состояние входа.".into());
        }
        let access = required(&response, "access_token")?;
        let me = self.vk_post(
            "https://id.vk.ru/oauth2/user_info",
            &[("client_id", pending.client_id)],
            &[("access_token", access.clone())],
        )?;
        reject_api_error(&me)?;
        let user = me.get("user").ok_or("VK не подтвердил аккаунт.")?;
        let account_id = identifier(user, "user_id")?;
        let now = Utc::now().timestamp();
        let token = Token {
            access,
            refresh: response
                .get("refresh_token")
                .and_then(Value::as_str)
                .map(str::to_owned),
            device_id: Some(device_id),
            expires_at: Some(now + seconds(&response, "expires_in")?),
            refreshed_at: now,
            label: safe_label(user, "first_name", &account_id),
            account_id,
        };
        save_secret(self.0.secrets.as_ref(), "vk-token", &token)?;
        self.status(Provider::Vk)
    }
    pub(super) fn vk_post(
        &self,
        endpoint: &str,
        query: &[(&str, String)],
        form: &[(&str, String)],
    ) -> Result<Value, String> {
        let mut url = url::Url::parse(endpoint).map_err(|_| "Неверный адрес VK.")?;
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(key, value)| (*key, value)));
        self.0.http.post(url.as_str(), form)
    }
    pub(super) fn refresh_vk(
        &self,
        config: ConnectionConfig,
        token: &Token,
    ) -> Result<Value, String> {
        let state = storage::random_id(32)?;
        let response = self.vk_post(
            VK_TOKEN,
            &[
                (
                    "client_id",
                    config.client_id.ok_or("Client ID не настроен.")?,
                ),
                ("grant_type", "refresh_token".into()),
                (
                    "device_id",
                    token.device_id.clone().ok_or("Сессия VK повреждена.")?,
                ),
                ("redirect_uri", VK_REDIRECT.into()),
                ("state", state.clone()),
            ],
            &[(
                "refresh_token",
                token
                    .refresh
                    .clone()
                    .ok_or("Сессия истекла. Подключите VK заново.")?,
            )],
        )?;
        if response.get("error").is_none()
            && response.get("state").and_then(Value::as_str) != Some(&state)
        {
            return Err("Не удалось проверить обновление сессии VK.".into());
        }
        Ok(response)
    }
}
