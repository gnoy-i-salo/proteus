#[cfg(feature = "telegram")]
use super::telegram;
use super::{
    http::{identifier, Http, Network},
    model::{ConnectionConfig, ConnectionStatus, DevicePending, Provider, Token},
    oauth::VkPending,
    storage::{load_secret, OsSecrets, SecretStore},
};
use chrono::Utc;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
pub(super) struct Inner {
    pub(super) secrets: Arc<dyn SecretStore>,
    pub(super) http: Arc<dyn Http>,
    pub(super) gate: Mutex<()>,
    #[cfg(feature = "telegram")]
    pub(super) directory: PathBuf,
    #[cfg(feature = "telegram")]
    pub(super) telegram: Mutex<Option<telegram::Telegram>>,
}
#[derive(Clone)]
pub struct Connections(pub(super) Arc<Inner>);
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
    pub(super) fn config(&self, provider: Provider) -> Result<ConnectionConfig, String> {
        load_secret(
            self.0.secrets.as_ref(),
            &format!("{}-config", provider.key()),
        )?
        .ok_or("Подключение не настроено. Сначала сохраните параметры приложения.".into())
    }
    pub(super) fn status(&self, provider: Provider) -> Result<ConnectionStatus, String> {
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
    pub(super) fn disconnect(&self, provider: Provider) -> Result<ConnectionStatus, String> {
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
