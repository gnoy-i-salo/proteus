#[cfg(feature = "telegram")]
use super::{http::identifier, storage, telegram};
use super::{
    model::{ConnectionConfig, TelegramMethod},
    session::Connections,
};
use serde_json::Value;
impl Connections {
    pub(super) fn begin_telegram(
        &self,
        config: &ConnectionConfig,
        telegram_method: Option<TelegramMethod>,
    ) -> Result<(), String> {
        #[cfg(feature = "telegram")]
        {
            let mut slot = self
                .0
                .telegram
                .lock()
                .map_err(|_| "Ошибка состояния Telegram.")?;
            if slot
                .as_ref()
                .is_none_or(|client| client.status().is_ok_and(|state| state == "disconnected"))
            {
                slot.take();
                *slot = Some(self.start_telegram(config)?);
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
                *slot = Some(self.start_telegram(config)?);
                if slot
                    .as_ref()
                    .ok_or("Не удалось начать Telegram.")?
                    .prepare_login(method)?
                {
                    return Err("Telegram не подготовил новый вход. Повторите позже.".into());
                }
            }
        }
        #[cfg(not(feature = "telegram"))]
        {
            let _ = (config, telegram_method);
            Err("Эта сборка не включает TDLib. Доступен импорт экспорта Telegram.".into())
        }

        #[cfg(feature = "telegram")]
        Ok(())
    }
    #[cfg(feature = "telegram")]
    pub(super) fn start_telegram(
        &self,
        config: &ConnectionConfig,
    ) -> Result<telegram::Telegram, String> {
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
}
