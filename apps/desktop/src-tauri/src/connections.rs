#[path = "connection-auth.rs"]
mod auth;
#[path = "connection-github.rs"]
mod github;
#[path = "social-http.rs"]
pub(crate) mod http;
#[path = "connection-instagram.rs"]
mod instagram;
#[path = "connection-model.rs"]
mod model;
#[path = "social-oauth.rs"]
pub(crate) mod oauth;
#[path = "connection-response.rs"]
mod response;
#[path = "connection-session.rs"]
mod session;
#[path = "social-storage.rs"]
pub(crate) mod storage;
#[path = "connection-storage-codec.rs"]
mod storage_codec;
#[cfg(feature = "telegram")]
#[path = "telegram.rs"]
mod telegram;
#[path = "connection-telegram.rs"]
mod telegram_session;
#[path = "connection-vk.rs"]
mod vk;
pub use model::{ConnectionConfig, ConnectionPage, ConnectionStatus, Provider, TelegramMethod};
pub(crate) use response::reject_api_error;
pub use session::Connections;
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
#[path = "connection-tests.rs"]
mod tests;
#[cfg(test)]
#[path = "connection-wire-tests.rs"]
mod wire_tests;
