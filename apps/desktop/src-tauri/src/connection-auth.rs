use super::http::required;
use super::{
    model::{ConnectionConfig, ConnectionPage, ConnectionStatus, Provider, TelegramMethod, Token},
    oauth,
    response::{reject_api_error, seconds},
    session::Connections,
    storage::{load_secret, save_secret},
};
use chrono::Utc;
use serde_json::Value;
impl Connections {
    pub(super) fn configure(
        &self,
        mut request: ConnectionConfig,
    ) -> Result<ConnectionStatus, String> {
        let provider = request.provider.ok_or("Выберите сервис.")?;
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния подключений.")?;
        match provider {
            Provider::Github | Provider::Vk => {
                let client = request
                    .client_id
                    .as_ref()
                    .ok_or("Укажите Client ID приложения.")?;
                if client.is_empty()
                    || client.len() > 128
                    || !client
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
                    || (provider == Provider::Vk && !client.bytes().all(|c| c.is_ascii_digit()))
                {
                    return Err("Некорректный Client ID приложения.".into());
                }
                request.api_id = None;
                request.api_hash = None;
                request.token = None;
            }
            Provider::Telegram => {
                if !request.api_id.is_some_and(|id| id > 0) {
                    return Err("Укажите положительный Telegram API ID.".into());
                }
                if request.api_hash.as_ref().is_none_or(|hash| {
                    hash.len() != 32 || !hash.bytes().all(|c| c.is_ascii_hexdigit())
                }) {
                    return Err(
                        "Telegram API hash должен содержать 32 шестнадцатеричных символа.".into(),
                    );
                }
                request.client_id = None;
                request.token = None;
                #[cfg(feature = "telegram")]
                if let Some(client) = self
                    .0
                    .telegram
                    .lock()
                    .map_err(|_| "Ошибка состояния Telegram.")?
                    .take()
                {
                    client.close();
                }
            }
            Provider::Instagram => self.configure_instagram(&mut request)?,
        }
        if provider != Provider::Instagram {
            self.0
                .secrets
                .remove(&format!("{}-token", provider.key()))?;
            self.0
                .secrets
                .remove(&format!("{}-pending", provider.key()))?;
        }
        save_secret(
            self.0.secrets.as_ref(),
            &format!("{}-config", provider.key()),
            &request,
        )?;
        self.status(provider)
    }
    pub(super) fn begin(
        &self,
        provider: Provider,
        telegram_method: Option<TelegramMethod>,
    ) -> Result<ConnectionStatus, String> {
        if provider != Provider::Telegram && telegram_method.is_some() {
            return Err("Метод входа доступен только для Telegram.".into());
        }
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния подключений.")?;
        let config = self.config(provider)?;
        let now = Utc::now().timestamp();
        match provider {
            Provider::Vk => self.begin_vk(config, now)?,
            Provider::Github => self.begin_github(config, now)?,
            Provider::Telegram => self.begin_telegram(&config, telegram_method)?,
            Provider::Instagram => {
                return Err("Вставьте токен своего профессионального Instagram-аккаунта или импортируйте экспорт.".into());
            }
        }
        self.status(provider)
    }
    pub(super) fn open_page(&self, page: ConnectionPage) -> Result<(), String> {
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния подключений.")?;
        let url = match page {
            ConnectionPage::VkAuth => oauth::reopen_vk_url(
                self.0.secrets.as_ref(),
                &self
                    .config(Provider::Vk)?
                    .client_id
                    .ok_or("Client ID не настроен.")?,
                Utc::now().timestamp(),
            )?,
            ConnectionPage::InstagramExport => {
                url::Url::parse(oauth::INSTAGRAM_EXPORT).expect("fixed Instagram export URL")
            }
        };
        oauth::open_browser(&url)
    }
    pub(super) fn advance(
        &self,
        provider: Provider,
        input: Option<String>,
    ) -> Result<ConnectionStatus, String> {
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния подключений.")?;
        match provider {
            Provider::Github => self.poll_github()?,
            Provider::Telegram => {
                #[cfg(feature = "telegram")]
                {
                    if let Some(input) = input {
                        self.0
                            .telegram
                            .lock()
                            .map_err(|_| "Ошибка состояния Telegram.")?
                            .as_ref()
                            .ok_or("Начните вход в Telegram.")?
                            .submit(&input)?;
                    }
                }
                #[cfg(not(feature = "telegram"))]
                {
                    let _ = input;
                    return Err("TDLib не включена в сборку.".into());
                }
            }
            _ => {}
        }
        self.status(provider)
    }
    pub(crate) fn token(&self, provider: Provider) -> Result<Token, String> {
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния подключений.")?;
        let mut token: Token = load_secret(
            self.0.secrets.as_ref(),
            &format!("{}-token", provider.key()),
        )?
        .ok_or("Сначала подключите аккаунт этого сервиса.")?;
        let now = Utc::now().timestamp();
        let needs_refresh = token.expires_at.is_some_and(|end| end <= now + 60)
            || (provider == Provider::Instagram && now - token.refreshed_at >= 86400);
        if needs_refresh {
            let config = self.config(provider)?;
            let response = match provider {
                Provider::Github => self.refresh_github(config, &token)?,
                Provider::Vk => self.refresh_vk(config, &token)?,
                Provider::Instagram => self.refresh_instagram(&token)?,
                Provider::Telegram => return Err("Для Telegram используется сессия TDLib.".into()),
            };
            self.reject_refresh_error(provider, &response)?;
            token.access = required(&response, "access_token")?;
            if let Some(refresh) = response.get("refresh_token").and_then(Value::as_str) {
                token.refresh = Some(refresh.into());
            }
            token.expires_at = Some(now + seconds(&response, "expires_in")?);
            token.refreshed_at = now;
            save_secret(
                self.0.secrets.as_ref(),
                &format!("{}-token", provider.key()),
                &token,
            )?;
        }
        Ok(token)
    }
    pub(super) fn reject_refresh_error(
        &self,
        provider: Provider,
        response: &Value,
    ) -> Result<(), String> {
        // These protocol codes confirm an invalid/expired/revoked refresh grant.
        // Transport failures, rate limits and unknown errors must preserve retry state.
        if matches!(
            response.get("error").and_then(Value::as_str),
            Some("invalid_grant" | "bad_refresh_token")
        ) {
            self.0
                .secrets
                .remove(&format!("{}-token", provider.key()))?;
            return Err("Сессия истекла или отозвана. Подключите аккаунт заново.".into());
        }
        reject_api_error(response)
    }
}
