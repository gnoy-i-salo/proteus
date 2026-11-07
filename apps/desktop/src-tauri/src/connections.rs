#[path = "social-http.rs"]
pub(crate) mod http;
#[path = "social-oauth.rs"]
pub(crate) mod oauth;
#[path = "social-storage.rs"]
pub(crate) mod storage;
#[cfg(feature = "telegram")]
#[path = "telegram.rs"]
mod telegram;

use chrono::Utc;
use http::{identifier, required, Http, Network};
use oauth::{VkPending, GH_TOKEN, VK_REDIRECT, VK_TOKEN};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use storage::{load_secret, save_secret, OsSecrets, SecretStore};

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
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ConnectionConfig {
    pub provider: Option<Provider>,
    pub client_id: Option<String>,
    pub api_id: Option<i32>,
    pub api_hash: Option<String>,
    pub token: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
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
#[serde(rename_all = "kebab-case")]
pub(crate) struct Token {
    pub access: String,
    refresh: Option<String>,
    device_id: Option<String>,
    expires_at: Option<i64>,
    refreshed_at: i64,
    pub account_id: String,
    label: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct DevicePending {
    client_id: String,
    device_code: String,
    user_code: String,
    expires_at: i64,
    next_poll: i64,
    interval: i64,
}

struct Inner {
    secrets: Arc<dyn SecretStore>,
    http: Arc<dyn Http>,
    gate: Mutex<()>,
    #[cfg(feature = "telegram")]
    directory: PathBuf,
    #[cfg(feature = "telegram")]
    telegram: Mutex<Option<telegram::Telegram>>,
}
#[derive(Clone)]
pub struct Connections(Arc<Inner>);
impl Connections {
    pub fn new(directory: PathBuf) -> Result<Self, String> {
        #[cfg(not(feature = "telegram"))]
        let _ = directory;
        Ok(Self(Arc::new(Inner {
            secrets: Arc::new(OsSecrets),
            http: Arc::new(Network::new()?),
            gate: Mutex::new(()),
            #[cfg(feature = "telegram")]
            directory,
            #[cfg(feature = "telegram")]
            telegram: Mutex::new(None),
        })))
    }
    pub(crate) fn secrets(&self) -> Arc<dyn SecretStore> {
        self.0.secrets.clone()
    }
    pub(crate) fn http(&self) -> &dyn Http {
        self.0.http.as_ref()
    }
    #[cfg(test)]
    pub(crate) fn test(secrets: Arc<dyn SecretStore>, http: Arc<dyn Http>) -> Self {
        Self(Arc::new(Inner {
            secrets,
            http,
            gate: Mutex::new(()),
            #[cfg(feature = "telegram")]
            directory: std::env::temp_dir(),
            #[cfg(feature = "telegram")]
            telegram: Mutex::new(None),
        }))
    }
    fn config(&self, provider: Provider) -> Result<ConnectionConfig, String> {
        load_secret(
            self.0.secrets.as_ref(),
            &format!("{}-config", provider.key()),
        )?
        .ok_or("Подключение не настроено. Сначала сохраните параметры приложения.".into())
    }
    fn status(&self, provider: Provider) -> Result<ConnectionStatus, String> {
        let config: Option<ConnectionConfig> = load_secret(
            self.0.secrets.as_ref(),
            &format!("{}-config", provider.key()),
        )?;
        let mut status = ConnectionStatus {
            provider,
            status: if config.is_some() {
                "disconnected"
            } else {
                "unconfigured"
            }
            .into(),
            label: None,
            message: None,
            client_id: config.as_ref().and_then(|c| c.client_id.clone()),
            api_id: config.as_ref().and_then(|c| c.api_id),
            user_code: None,
            poll_after_ms: None,
            qr_code: None,
            capabilities: vec!["archive-import".into()],
        };
        if provider == Provider::Telegram {
            #[cfg(feature = "telegram")]
            if let Some(client) = self
                .0
                .telegram
                .lock()
                .map_err(|_| "Ошибка состояния Telegram.")?
                .as_ref()
            {
                let (state, qr_code) = client.view()?;
                status.status = state;
                status.qr_code = qr_code;
                if status.status == "connected" {
                    let me = client.me()?;
                    status.label = Some(telegram::label(&me));
                    status.capabilities.push("messages".into());
                } else {
                    status.poll_after_ms = Some(2000);
                }
            }
            #[cfg(not(feature = "telegram"))]
            {
                status.status = "unavailable".into();
                status.message =
                    Some("Эта сборка не включает TDLib. Доступен импорт экспорта Telegram.".into());
            }
            return Ok(status);
        }
        if let Some(token) = load_secret::<Token>(
            self.0.secrets.as_ref(),
            &format!("{}-token", provider.key()),
        )? {
            status.status = if token
                .expires_at
                .is_some_and(|end| end <= Utc::now().timestamp())
                && token.refresh.is_none()
            {
                "disconnected"
            } else {
                "connected"
            }
            .into();
            status.label = Some(token.label);
            status.capabilities.extend(
                match provider {
                    Provider::Github => vec!["profile", "repositories"],
                    Provider::Vk | Provider::Instagram => vec!["profile"],
                    Provider::Telegram => vec![],
                }
                .into_iter()
                .map(str::to_owned),
            );
            if matches!(provider, Provider::Vk | Provider::Instagram) {
                status.message = Some(
                    "Доступ к записям и комментариям проверяется при чтении выбранного источника."
                        .into(),
                );
            }
        }
        if provider == Provider::Github {
            if let Some(pending) =
                load_secret::<DevicePending>(self.0.secrets.as_ref(), "github-pending")?
            {
                if pending.expires_at > Utc::now().timestamp() {
                    status.status = "pending".into();
                    status.user_code = Some(pending.user_code);
                    status.poll_after_ms = Some(pending.interval as u64 * 1000);
                }
            }
        } else if provider == Provider::Vk
            && load_secret::<VkPending>(self.0.secrets.as_ref(), "vk-pending")?
                .is_some_and(|p| p.expires_at > Utc::now().timestamp())
        {
            status.status = "pending".into();
            status.poll_after_ms = Some(2000);
        }
        Ok(status)
    }
    fn configure(&self, mut request: ConnectionConfig) -> Result<ConnectionStatus, String> {
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
            Provider::Instagram => {
                let access = request
                    .token
                    .take()
                    .filter(|token| {
                        !token.is_empty()
                            && token.len() <= 4096
                            && !token.chars().any(char::is_whitespace)
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
            }
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
    fn begin(
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
            Provider::Vk => {
                let pending =
                    VkPending::new(config.client_id.ok_or("Client ID не настроен.")?, now)?;
                save_secret(self.0.secrets.as_ref(), "vk-pending", &pending)?;
                if let Err(error) = oauth::open_browser(&pending.authorization_url()) {
                    self.0.secrets.remove("vk-pending")?;
                    return Err(error);
                }
            }
            Provider::Github => {
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
            }
            Provider::Telegram => {
                #[cfg(feature = "telegram")]
                {
                    let mut slot = self
                        .0
                        .telegram
                        .lock()
                        .map_err(|_| "Ошибка состояния Telegram.")?;
                    if slot.as_ref().is_none_or(|client| {
                        client.status().is_ok_and(|state| state == "disconnected")
                    }) {
                        slot.take();
                        *slot = Some(self.start_telegram(&config)?);
                    }
                    let method = telegram_method.unwrap_or_default();
                    if slot
                        .as_ref()
                        .ok_or("Не удалось начать Telegram.")?
                        .prepare_login(method)?
                    {
                        // prepare_login returns restart only after authorizationStateClosed.
                        // On timeout/error the existing slot remains owned and is not reopened.
                        slot.take();
                        *slot = Some(self.start_telegram(&config)?);
                        if slot
                            .as_ref()
                            .ok_or("Не удалось начать Telegram.")?
                            .prepare_login(method)?
                        {
                            return Err(
                                "Telegram не подготовил новый вход. Повторите позже.".into()
                            );
                        }
                    }
                }
                #[cfg(not(feature = "telegram"))]
                return Err(
                    "Эта сборка не включает TDLib. Доступен импорт экспорта Telegram.".into(),
                );
            }
            Provider::Instagram => {
                return Err("Вставьте токен своего профессионального Instagram-аккаунта или импортируйте экспорт.".into());
            }
        }
        self.status(provider)
    }
    #[cfg(feature = "telegram")]
    fn start_telegram(&self, config: &ConnectionConfig) -> Result<telegram::Telegram, String> {
        let key = match self.0.secrets.get("telegram-database-key")? {
            Some(key) => key,
            None => {
                let key = storage::random_id(32)?;
                self.0.secrets.set("telegram-database-key", &key)?;
                key
            }
        };
        telegram::Telegram::start(
            config.api_id.ok_or("API ID не настроен.")?,
            config.api_hash.clone().ok_or("API hash не настроен.")?,
            &self.0.directory.join("telegram"),
            &key,
        )
    }
    fn open_page(&self, page: ConnectionPage) -> Result<(), String> {
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
    fn advance(
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
    fn poll_github(&self) -> Result<(), String> {
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
                Provider::Github => self.0.http.post(
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
                )?,
                Provider::Vk => {
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
                    response
                }
                Provider::Instagram => self.0.http.get(
                    "https://graph.instagram.com/refresh_access_token",
                    &[
                        ("grant_type", "ig_refresh_token".into()),
                        ("access_token", token.access.clone()),
                    ],
                    None,
                )?,
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
    fn reject_refresh_error(&self, provider: Provider, response: &Value) -> Result<(), String> {
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
    pub(crate) fn account_id(&self, provider: Provider) -> Result<String, String> {
        if provider == Provider::Telegram {
            return identifier(
                &self.telegram_request(serde_json::json!({"@type":"getMe"}))?,
                "id",
            );
        }
        Ok(self.token(provider)?.account_id)
    }
    pub(crate) fn token_for(
        &self,
        provider: Provider,
        expected_account: &str,
    ) -> Result<Token, String> {
        let token = self.token(provider)?;
        if token.account_id != expected_account {
            return Err("Подключённый аккаунт изменился. Выберите источник заново.".into());
        }
        Ok(token)
    }
    pub(crate) fn telegram_request_for(
        &self,
        expected_account: &str,
        request: Value,
    ) -> Result<Value, String> {
        #[cfg(feature = "telegram")]
        {
            let slot = self
                .0
                .telegram
                .lock()
                .map_err(|_| "Ошибка состояния Telegram.")?;
            let client = slot.as_ref().ok_or("Сначала подключите Telegram.")?;
            if identifier(&client.me()?, "id")? != expected_account {
                return Err("Подключённый аккаунт изменился. Выберите источник заново.".into());
            }
            client.request(request)
        }
        #[cfg(not(feature = "telegram"))]
        {
            let _ = (expected_account, request);
            Err("Эта сборка не включает TDLib.".into())
        }
    }
    // Parameter placement follows VKCOM/vkid-web-sdk auth.ts. No raw HTTP errors/logs
    // escape this boundary: the PKCE verifier exists in the native request URL only.
    fn vk_post(
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
    pub(crate) fn telegram_request(&self, request: Value) -> Result<Value, String> {
        #[cfg(feature = "telegram")]
        {
            self.0
                .telegram
                .lock()
                .map_err(|_| "Ошибка состояния Telegram.")?
                .as_ref()
                .ok_or("Сначала подключите Telegram.")?
                .request(request)
        }
        #[cfg(not(feature = "telegram"))]
        {
            let _ = request;
            Err("Эта сборка не включает TDLib. Импортируйте экспорт Telegram.".into())
        }
    }
    fn disconnect(&self, provider: Provider) -> Result<ConnectionStatus, String> {
        let _guard = self
            .0
            .gate
            .lock()
            .map_err(|_| "Ошибка состояния подключений.")?;
        #[cfg(feature = "telegram")]
        if provider == Provider::Telegram {
            let mut slot = self
                .0
                .telegram
                .lock()
                .map_err(|_| "Ошибка состояния Telegram.")?;
            if let Some(client) = slot.as_ref() {
                client.logout()?;
            }
            slot.take();
        }
        self.0
            .secrets
            .remove(&format!("{}-token", provider.key()))?;
        self.0
            .secrets
            .remove(&format!("{}-pending", provider.key()))?;
        self.status(provider)
    }
    pub fn shutdown(&self) {
        #[cfg(feature = "telegram")]
        if let Ok(mut slot) = self.0.telegram.lock() {
            if let Some(client) = slot.take() {
                client.close();
            }
        }
    }
}
fn safe_label(value: &Value, key: &str, fallback: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| s.len() <= 128 && !s.chars().any(char::is_control))
        .unwrap_or(fallback)
        .into()
}
fn seconds(value: &Value, key: &str) -> Result<i64, String> {
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

#[tauri::command]
pub async fn configure_connection(
    request: ConnectionConfig,
    connections: tauri::State<'_, Connections>,
) -> Result<ConnectionStatus, String> {
    let hub = connections.inner().clone();
    tauri::async_runtime::spawn_blocking(move || hub.configure(request))
        .await
        .map_err(|_| "Не удалось сохранить подключение.".to_string())?
}
#[tauri::command]
pub async fn connection_statuses(
    connections: tauri::State<'_, Connections>,
) -> Result<Vec<ConnectionStatus>, String> {
    let hub = connections.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        [
            Provider::Telegram,
            Provider::Vk,
            Provider::Github,
            Provider::Instagram,
        ]
        .into_iter()
        .map(|p| hub.status(p))
        .collect()
    })
    .await
    .map_err(|_| "Не удалось прочитать подключения.".to_string())?
}
#[tauri::command]
pub async fn begin_connection(
    provider: Provider,
    telegram_method: Option<TelegramMethod>,
    connections: tauri::State<'_, Connections>,
) -> Result<ConnectionStatus, String> {
    let hub = connections.inner().clone();
    tauri::async_runtime::spawn_blocking(move || hub.begin(provider, telegram_method))
        .await
        .map_err(|_| "Не удалось начать подключение.".to_string())?
}
#[tauri::command]
pub async fn open_connection_page(
    page: ConnectionPage,
    connections: tauri::State<'_, Connections>,
) -> Result<(), String> {
    let hub = connections.inner().clone();
    tauri::async_runtime::spawn_blocking(move || hub.open_page(page))
        .await
        .map_err(|_| "Не удалось открыть страницу сервиса.".to_string())?
}
#[tauri::command]
pub async fn advance_connection(
    provider: Provider,
    input: Option<String>,
    connections: tauri::State<'_, Connections>,
) -> Result<ConnectionStatus, String> {
    let hub = connections.inner().clone();
    tauri::async_runtime::spawn_blocking(move || hub.advance(provider, input))
        .await
        .map_err(|_| "Не удалось продолжить подключение.".to_string())?
}
#[tauri::command]
pub async fn disconnect_connection(
    provider: Provider,
    connections: tauri::State<'_, Connections>,
) -> Result<ConnectionStatus, String> {
    let hub = connections.inner().clone();
    tauri::async_runtime::spawn_blocking(move || hub.disconnect(provider))
        .await
        .map_err(|_| "Не удалось отключить аккаунт.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    struct NoNetwork;
    impl Http for NoNetwork {
        fn get(&self, _: &str, _: &[(&str, String)], _: Option<&str>) -> Result<Value, String> {
            panic!("unexpected network")
        }
        fn post(&self, _: &str, _: &[(&str, String)]) -> Result<Value, String> {
            panic!("unexpected network")
        }
    }
    #[test]
    fn source_token_is_bound_to_the_granted_account_even_after_reconnect() {
        let secrets = Arc::new(storage::MemorySecrets::default());
        save_secret(
            secrets.as_ref(),
            "github-token",
            &Token {
                access: "test-token".into(),
                refresh: None,
                device_id: None,
                expires_at: None,
                refreshed_at: 0,
                account_id: "new-account".into(),
                label: "test".into(),
            },
        )
        .unwrap();
        let hub = Connections::test(secrets, Arc::new(NoNetwork));
        assert!(hub.token_for(Provider::Github, "old-account").is_err());
        assert_eq!(
            hub.token_for(Provider::Github, "new-account")
                .unwrap()
                .account_id,
            "new-account"
        );
    }
    struct RefreshResponse(Result<Value, String>);
    impl Http for RefreshResponse {
        fn get(&self, _: &str, _: &[(&str, String)], _: Option<&str>) -> Result<Value, String> {
            self.0.clone()
        }
        fn post(&self, _: &str, _: &[(&str, String)]) -> Result<Value, String> {
            self.0.clone()
        }
    }
    fn expired_connection(provider: Provider, response: Result<Value, String>) -> Connections {
        let secrets = Arc::new(storage::MemorySecrets::default());
        save_secret(
            secrets.as_ref(),
            &format!("{}-config", provider.key()),
            &ConnectionConfig {
                provider: Some(provider),
                client_id: Some("123".into()),
                ..ConnectionConfig::default()
            },
        )
        .unwrap();
        save_secret(
            secrets.as_ref(),
            &format!("{}-token", provider.key()),
            &Token {
                access: "expired-access".into(),
                refresh: Some("expired-refresh".into()),
                device_id: Some("device".into()),
                expires_at: Some(1),
                refreshed_at: 0,
                account_id: "42".into(),
                label: "fixture".into(),
            },
        )
        .unwrap();
        Connections::test(secrets, Arc::new(RefreshResponse(response)))
    }
    #[test]
    fn terminal_refresh_errors_remove_only_invalid_credentials_and_require_reconnection() {
        for (provider, error) in [
            (Provider::Github, "bad_refresh_token"),
            (Provider::Github, "invalid_grant"),
            (Provider::Vk, "invalid_grant"),
        ] {
            let hub = expired_connection(
                provider,
                Ok(
                    serde_json::json!({"error":error,"error_description":"do not display provider text"}),
                ),
            );
            assert!(hub.token(provider).is_err());
            assert!(hub
                .secrets()
                .get(&format!("{}-token", provider.key()))
                .unwrap()
                .is_none());
            assert_eq!(hub.status(provider).unwrap().status, "disconnected");
            assert!(hub.config(provider).is_ok());
        }
    }
    #[test]
    fn transient_refresh_failures_preserve_credentials_for_retry() {
        for response in [
            Err("network timeout".into()),
            Ok(serde_json::json!({"error":"temporarily_unavailable"})),
            Ok(serde_json::json!({"error":"server_error"})),
            Ok(serde_json::json!({"error":"invalid_client"})),
        ] {
            let hub = expired_connection(Provider::Github, response);
            assert!(hub.token(Provider::Github).is_err());
            assert!(hub.secrets().get("github-token").unwrap().is_some());
        }
    }
}
