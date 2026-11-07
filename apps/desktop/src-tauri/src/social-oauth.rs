use super::storage::random_id;
use super::storage::{load_secret, SecretStore};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use url::Url;

pub(crate) const VK_REDIRECT: &str = "https://gnoy-i-salo.github.io/proteus/auth/";
pub(crate) const VK_TOKEN: &str = "https://id.vk.ru/oauth2/auth";
pub(crate) const GH_TOKEN: &str = "https://github.com/login/oauth/access_token";
pub(crate) const INSTAGRAM_EXPORT: &str =
    "https://accountscenter.instagram.com/info_and_permissions/dyi/";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct VkPending {
    pub state: String,
    pub verifier: String,
    pub client_id: String,
    pub expires_at: i64,
}
impl VkPending {
    pub fn new(client_id: String, now: i64) -> Result<Self, String> {
        Ok(Self {
            state: random_id(32)?,
            verifier: random_id(32)?,
            client_id,
            expires_at: now + 600,
        })
    }
    pub fn authorization_url(&self) -> Url {
        let mut url = Url::parse("https://id.vk.ru/authorize").expect("constant VK URL");
        url.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("client_id", &self.client_id),
            ("redirect_uri", VK_REDIRECT),
            ("state", &self.state),
            ("code_challenge_method", "s256"),
            (
                "code_challenge",
                &URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.as_bytes())),
            ),
        ]);
        url
    }
}
pub(crate) struct VkCallback {
    pub state: String,
    pub code: Option<String>,
    pub device_id: Option<String>,
    pub error: Option<String>,
}
pub(crate) fn parse_callback(raw: &str) -> Result<VkCallback, String> {
    let invalid = || "Ссылка возврата недействительна.".to_string();
    if raw.len() > 16384 || raw.chars().any(char::is_control) {
        return Err(invalid());
    }
    let url = Url::parse(raw).map_err(|_| invalid())?;
    if url.scheme() != "proteus"
        || url.host_str() != Some("oauth")
        || url.path() != "/vk"
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(invalid());
    }
    let query = url.query().ok_or_else(invalid)?;
    for (index, byte) in query.bytes().enumerate() {
        if byte == b'%'
            && !query
                .as_bytes()
                .get(index + 1..index + 3)
                .is_some_and(|bytes| bytes.iter().all(u8::is_ascii_hexdigit))
        {
            return Err(invalid());
        }
    }
    let decoded = percent_encoding::percent_decode_str(query)
        .decode_utf8()
        .map_err(|_| invalid())?;
    if decoded.chars().any(|c| c.is_control() || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}')) { return Err(invalid()); }
    let mut values = HashMap::new();
    for (key, value) in url.query_pairs() {
        let limit = match key.as_ref() {
            "v" => 1,
            "code" => 4096,
            "device_id" | "state" => 512,
            "error" => 32,
            _ => return Err(invalid()),
        };
        if value.is_empty()
            || value.len() > limit
            || value.chars().any(char::is_whitespace)
            || values
                .insert(key.into_owned(), value.into_owned())
                .is_some()
        {
            return Err(invalid());
        }
    }
    if values.get("v").map(String::as_str) != Some("1") {
        return Err(invalid());
    }
    let state = values.remove("state").ok_or_else(invalid)?;
    let code = values.remove("code");
    let device_id = values.remove("device_id");
    let error = values.remove("error");
    if let Some(error) = &error {
        if code.is_some()
            || device_id.is_some()
            || !matches!(error.as_str(), "access_denied" | "provider_error")
        {
            return Err(invalid());
        }
    } else if code.is_none() || device_id.is_none() {
        return Err(invalid());
    }
    Ok(VkCallback {
        state,
        code,
        device_id,
        error,
    })
}
pub(crate) fn match_pending(
    pending: &VkPending,
    callback: &VkCallback,
    now: i64,
) -> Result<(), String> {
    if now >= pending.expires_at
        || pending.expires_at - now > 600
        || pending.state != callback.state
    {
        return Err("Возврат не соответствует ожидаемому входу или срок входа истёк. Начните подключение заново.".into());
    }
    Ok(())
}
// Caller holds the connection mutex across this claim, making read/check/delete atomic.
pub(crate) fn claim_pending(
    store: &dyn SecretStore,
    callback: &VkCallback,
    now: i64,
) -> Result<VkPending, String> {
    let pending: VkPending = load_secret(store, "vk-pending")?
        .ok_or("Вход не ожидается. Начните подключение в PROTEUS.")?;
    match_pending(&pending, callback, now)?;
    store.remove("vk-pending")?;
    Ok(pending)
}

pub(crate) fn open_browser(url: &Url) -> Result<(), String> {
    if !allowed_browser_url(url) {
        return Err("Адрес авторизации не разрешён.".into());
    }
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut cmd = std::process::Command::new("rundll32.exe");
        cmd.arg("url.dll,FileProtocolHandler");
        cmd
    };
    #[cfg(target_os = "linux")]
    let mut command = std::process::Command::new("xdg-open");
    command
        .arg(url.as_str())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|_| "Не удалось открыть системный браузер.".into())
}
pub(crate) fn reopen_vk_url(
    store: &dyn SecretStore,
    client_id: &str,
    now: i64,
) -> Result<Url, String> {
    let pending: VkPending = load_secret(store, "vk-pending")?
        .ok_or("Вход VK не ожидается. Начните подключение заново.")?;
    if pending.client_id != client_id || pending.expires_at <= now || pending.expires_at - now > 600
    {
        return Err("Попытка входа VK истекла. Начните подключение заново.".into());
    }
    Ok(pending.authorization_url())
}
fn allowed_browser_url(url: &Url) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    match (url.host_str(), url.path()) {
        (Some("id.vk.ru"), "/authorize") => true,
        (Some("github.com"), "/login/device")
        | (Some("accountscenter.instagram.com"), "/info_and_permissions/dyi/") => {
            url.query().is_none()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_callback_matches_browser_v1_and_rejects_injection() {
        let valid = "proteus://oauth/vk?v=1&code=abc%2B%2F%3D&device_id=device&state=state";
        let parsed = parse_callback(valid).unwrap();
        assert_eq!(parsed.code.as_deref(), Some("abc+/="));
        for suffix in [
            "&state=other",
            "&access_token=secret",
            "&return_to=https://example.org",
            "&error=access_denied",
            "#fragment",
        ] {
            assert!(parse_callback(&format!("{valid}{suffix}")).is_err());
        }
        for value in [
            valid.replace("oauth/vk", "evil/vk"),
            valid.replace("v=1", "v=2"),
            valid.replace("abc%2B%2F%3D", "%GG"),
            valid.replace("state=state", "state=%00"),
        ] {
            assert!(parse_callback(&value).is_err());
        }
    }
    #[test]
    fn browser_reopen_preserves_live_vk_transaction_and_expired_attempt_requires_restart() {
        let store = super::super::storage::MemorySecrets::default();
        let pending = VkPending::new("123".into(), 100).unwrap();
        super::super::storage::save_secret(&store, "vk-pending", &pending).unwrap();
        let before = store.get("vk-pending").unwrap();
        assert_eq!(
            reopen_vk_url(&store, "123", 101).unwrap(),
            pending.authorization_url()
        );
        assert_eq!(before, store.get("vk-pending").unwrap());
        assert!(reopen_vk_url(&store, "123", 700).is_err());
        assert!(reopen_vk_url(&store, "other-client", 101).is_err());
        assert_eq!(before, store.get("vk-pending").unwrap());
    }
    #[test]
    fn browser_opener_admits_only_fixed_authentication_and_export_pages() {
        for url in [
            INSTAGRAM_EXPORT,
            "https://github.com/login/device",
            "https://id.vk.ru/authorize?client_id=123&state=state",
        ] {
            assert!(allowed_browser_url(&Url::parse(url).unwrap()));
        }
        for url in ["https://accountscenter.instagram.com.evil/info_and_permissions/dyi/","https://accountscenter.instagram.com/info_and_permissions/dyi/?next=https://evil.invalid","https://accountscenter.instagram.com/info_and_permissions/dyi/#fragment","http://accountscenter.instagram.com/info_and_permissions/dyi/","https://github.com/login/device?next=evil","https://user@id.vk.ru/authorize","https://id.vk.ru:8443/authorize","https://id.vk.ru/other"] {assert!(!allowed_browser_url(&Url::parse(url).unwrap()));}
    }
    #[test]
    fn pkce_is_random_and_only_expected_unexpired_state_completes() {
        let pending = VkPending::new("123".into(), 100).unwrap();
        assert_eq!(pending.verifier.len(), 43);
        assert_ne!(
            pending.state,
            VkPending::new("123".into(), 100).unwrap().state
        );
        let query: HashMap<_, _> = pending
            .authorization_url()
            .query_pairs()
            .into_owned()
            .collect();
        assert!(!query.contains_key("code_verifier"));
        assert_eq!(query["redirect_uri"], VK_REDIRECT);
        let callback = VkCallback {
            state: pending.state.clone(),
            code: Some("code".into()),
            device_id: Some("device".into()),
            error: None,
        };
        assert!(match_pending(&pending, &callback, 101).is_ok());
        assert!(match_pending(&pending, &callback, 700).is_err());
        assert!(match_pending(
            &pending,
            &VkCallback {
                state: "other".into(),
                ..callback
            },
            101
        )
        .is_err());
    }
    #[test]
    fn callback_claim_is_single_use_and_wrong_state_does_not_destroy_valid_pending() {
        use super::super::storage::{save_secret, MemorySecrets};
        let store = MemorySecrets::default();
        let pending = VkPending::new("123".into(), 100).unwrap();
        save_secret(&store, "vk-pending", &pending).unwrap();
        let mut callback = VkCallback {
            state: "wrong".into(),
            code: Some("code".into()),
            device_id: Some("device".into()),
            error: None,
        };
        assert!(claim_pending(&store, &callback, 101).is_err());
        callback.state = pending.state;
        assert!(claim_pending(&store, &callback, 101).is_ok());
        assert!(claim_pending(&store, &callback, 101).is_err());
        assert!(store.get("vk-pending").unwrap().is_none());
    }
}
