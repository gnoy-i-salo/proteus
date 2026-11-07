use super::{
    http::{identifier, required},
    model::{ConnectionConfig, DevicePending, Token},
    oauth::{self, GH_TOKEN},
    response::{reject_api_error, safe_label, seconds},
    session::Connections,
    storage::{load_secret, save_secret},
};
use chrono::Utc;
use serde_json::Value;
impl Connections {
    pub(super) fn begin_github(&self, config: ConnectionConfig, now: i64) -> Result<(), String> {
        let client_id = config.client_id.ok_or("Client ID не настроен.")?;
        let response = self.0.http.post(
            "https://github.com/login/device/code",
            &[
                ("client_id", client_id.clone()),
                ("scope", "offline_access".into()),
            ],
        )?;
        reject_api_error(&response)?;
        let pending = DevicePending {
            client_id,
            device_code: required(&response, "device_code")?,
            user_code: required(&response, "user_code")?,
            expires_at: now + seconds(&response, "expires_in")?,
            next_poll: now + seconds(&response, "interval")?,
            interval: seconds(&response, "interval")?,
        };
        save_secret(self.0.secrets.as_ref(), "github-pending", &pending)?;
        if let Err(error) = oauth::open_browser(
            &url::Url::parse("https://github.com/login/device").expect("fixed GitHub URL"),
        ) {
            self.0.secrets.remove("github-pending")?;
            return Err(error);
        }

        Ok(())
    }
    pub(super) fn poll_github(&self) -> Result<(), String> {
        let Some(mut pending) =
            load_secret::<DevicePending>(self.0.secrets.as_ref(), "github-pending")?
        else {
            return Ok(());
        };
        let now = Utc::now().timestamp();
        if now >= pending.expires_at {
            self.0.secrets.remove("github-pending")?;
            return Err("Код GitHub истёк. Начните подключение заново.".into());
        }
        if now < pending.next_poll {
            return Ok(());
        }
        pending.next_poll = now + pending.interval;
        save_secret(self.0.secrets.as_ref(), "github-pending", &pending)?;
        let response = self.0.http.post(
            GH_TOKEN,
            &[
                ("client_id", pending.client_id.clone()),
                ("device_code", pending.device_code.clone()),
                (
                    "grant_type",
                    "urn:ietf:params:oauth:grant-type:device_code".into(),
                ),
            ],
        )?;
        match response.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => return Ok(()),
            Some("slow_down") => {
                pending.interval = (pending.interval + 5).min(60);
                pending.next_poll = now + pending.interval;
                save_secret(self.0.secrets.as_ref(), "github-pending", &pending)?;
                return Ok(());
            }
            Some(_) => {
                self.0.secrets.remove("github-pending")?;
                return Err(
                    "GitHub не завершил вход. Подтвердите доступ или начните подключение заново."
                        .into(),
                );
            }
            None => {}
        }
        self.0.secrets.remove("github-pending")?;
        let access = required(&response, "access_token")?;
        let me = self
            .0
            .http
            .get("https://api.github.com/user", &[], Some(&access))?;
        let account_id = identifier(&me, "id")?;
        let token = Token {
            access,
            refresh: response
                .get("refresh_token")
                .and_then(Value::as_str)
                .map(str::to_owned),
            device_id: None,
            expires_at: response
                .get("expires_in")
                .and_then(Value::as_i64)
                .filter(|seconds| *seconds > 0)
                .map(|seconds| now + seconds.min(31536000)),
            refreshed_at: now,
            label: safe_label(&me, "login", &account_id),
            account_id,
        };
        save_secret(self.0.secrets.as_ref(), "github-token", &token)
    }
    pub(super) fn refresh_github(
        &self,
        config: ConnectionConfig,
        token: &Token,
    ) -> Result<Value, String> {
        self.0.http.post(
            GH_TOKEN,
            &[
                (
                    "client_id",
                    config.client_id.ok_or("Client ID не настроен.")?,
                ),
                ("grant_type", "refresh_token".into()),
                (
                    "refresh_token",
                    token
                        .refresh
                        .clone()
                        .ok_or("Сессия истекла. Подключите GitHub заново.")?,
                ),
            ],
        )
    }
}
