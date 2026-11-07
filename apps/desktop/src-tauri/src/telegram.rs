//! Official TDLib JSON C ABI. A single ordered receiver owns all returned pointers.
use super::TelegramMethod;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    os::raw::{c_char, c_double, c_int},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    time::Duration,
};

unsafe extern "C" {
    fn td_create_client_id() -> c_int;
    fn td_send(client_id: c_int, request: *const c_char);
    fn td_receive(timeout: c_double) -> *const c_char;
    fn td_execute(request: *const c_char) -> *const c_char;
}
type Waiting = HashMap<(i32, u64), mpsc::Sender<Value>>;
trait AuthTransport {
    fn authorization_state(&self) -> Result<Value, String>;
    fn call(&self, request: Value) -> Result<Value, String>;
    fn cancel_pending(&self) -> Result<bool, String>;
    fn wait_closed(&self) -> Result<(), String>;
}
fn prepare_login(client: &impl AuthTransport, method: TelegramMethod) -> Result<bool, String> {
    let mut state = client.authorization_state()?;
    for _ in 0..200 {
        if !matches!(
            state.get("@type").and_then(Value::as_str),
            None | Some(
                "authorizationStateWaitTdlibParameters"
                    | "authorizationStateLoggingOut"
                    | "authorizationStateClosing"
            )
        ) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
        state = client.authorization_state()?;
    }
    let name = state.get("@type").and_then(Value::as_str).unwrap_or("");
    if name == "authorizationStateReady" {
        return Ok(false);
    }
    if name == "authorizationStateClosed" {
        return Ok(true);
    }
    if method == TelegramMethod::Qr && name == "authorizationStateWaitOtherDeviceConfirmation" {
        return Ok(false);
    }
    if method == TelegramMethod::Phone && name == "authorizationStateWaitPhoneNumber" {
        return Ok(false);
    }
    if !matches!(
        name,
        "authorizationStateWaitPhoneNumber"
            | "authorizationStateWaitOtherDeviceConfirmation"
            | "authorizationStateWaitCode"
            | "authorizationStateWaitPassword"
            | "authorizationStateWaitEmailAddress"
            | "authorizationStateWaitEmailCode"
            | "authorizationStateWaitRegistration"
            | "authorizationStateWaitPremiumPurchase"
    ) {
        return Err("Telegram ещё завершает предыдущую операцию. Повторите вход позже.".into());
    }
    if method == TelegramMethod::Qr {
        client.call(json!({"@type":"requestQrCodeAuthentication","other_user_ids":[]}))?;
        Ok(false)
    } else {
        // Explicit cancellation of this pending attempt; a just-confirmed QR may
        // race cancellation inside TDLib. An already observed Ready is preserved.
        if !client.cancel_pending()? {
            return Ok(client.authorization_state()?["@type"] == "authorizationStateClosed");
        }
        client.wait_closed()?;
        Ok(true)
    }
}
fn state_name(state: &Value) -> &'static str {
    match state.get("@type").and_then(Value::as_str) {
        Some("authorizationStateReady") => "connected",
        Some("authorizationStateWaitPhoneNumber") => "phone-required",
        Some("authorizationStateWaitOtherDeviceConfirmation") => "qr-required",
        Some("authorizationStateWaitCode") => "code-required",
        Some("authorizationStateWaitPassword") => "password-required",
        Some("authorizationStateWaitEmailAddress") => "email-required",
        Some("authorizationStateWaitEmailCode") => "email-code-required",
        Some("authorizationStateWaitRegistration" | "authorizationStateWaitPremiumPurchase") => {
            "unavailable"
        }
        Some("authorizationStateClosed") => "disconnected",
        _ => "pending",
    }
}
fn auth_view(state: &Value) -> Result<(String, Option<String>), String> {
    let status = state_name(state);
    if status != "qr-required" {
        return Ok((status.into(), None));
    }
    let invalid = || "Telegram вернул неподдерживаемый QR-код. Начните вход заново.".to_string();
    let link = state
        .get("link")
        .and_then(Value::as_str)
        .filter(|link| link.len() <= 2048 && !link.chars().any(char::is_control))
        .ok_or_else(invalid)?;
    let url = url::Url::parse(link).map_err(|_| invalid())?;
    let fields: Vec<_> = url.query_pairs().collect();
    if url.scheme() != "tg"
        || url.host_str() != Some("login")
        || !url.path().is_empty()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
        || fields.len() != 1
        || fields[0].0 != "token"
        || !(16..=1024).contains(&fields[0].1.len())
        || !fields[0]
            .1
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
    {
        return Err(invalid());
    }
    use base64::{engine::general_purpose::STANDARD, Engine};
    let code = qrcode::QrCode::new(link.as_bytes()).map_err(|_| invalid())?;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(256, 256)
        .quiet_zone(true)
        .build();
    let image = format!("data:image/svg+xml;base64,{}", STANDARD.encode(svg));
    if image.len() > 128 * 1024 {
        return Err(invalid());
    }
    Ok((status.into(), Some(image)))
}
struct Runtime {
    waiting: Mutex<Waiting>,
    states: Mutex<HashMap<i32, Arc<Mutex<Value>>>>,
    next: AtomicU64,
}
static RUNTIME: OnceLock<Arc<Runtime>> = OnceLock::new();
fn runtime() -> Arc<Runtime> {
    RUNTIME
        .get_or_init(|| {
            let runtime = Arc::new(Runtime {
                waiting: Mutex::new(HashMap::new()),
                states: Mutex::new(HashMap::new()),
                next: AtomicU64::new(1),
            });
            let receiver = runtime.clone();
            std::thread::spawn(move || {
                // No TDLib log files or credential-bearing diagnostic text.
                for command in [
                    json!({"@type":"setLogStream","log_stream":{"@type":"logStreamEmpty"}}),
                    json!({"@type":"setLogVerbosityLevel","new_verbosity_level":0}),
                ] {
                    if let Ok(command) = CString::new(command.to_string()) {
                        unsafe {
                            td_execute(command.as_ptr());
                        }
                    }
                }
                loop {
                    let pointer = unsafe { td_receive(0.2) };
                    if pointer.is_null() {
                        continue;
                    }
                    // Copy before the next receive/execute invalidates TDLib's buffer.
                    let bytes = unsafe { CStr::from_ptr(pointer).to_bytes().to_vec() };
                    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                        continue;
                    };
                    let Some(client) = value
                        .get("@client_id")
                        .and_then(Value::as_i64)
                        .map(|v| v as i32)
                    else {
                        continue;
                    };
                    if value.get("@type").and_then(Value::as_str)
                        == Some("updateAuthorizationState")
                    {
                        if let Ok(states) = receiver.states.lock() {
                            if let Some(state) = states.get(&client) {
                                if let Ok(mut state) = state.lock() {
                                    *state = value["authorization_state"].clone();
                                }
                            }
                        }
                    }
                    if let Some(extra) = value.get("@extra").and_then(Value::as_u64) {
                        if let Ok(mut waiting) = receiver.waiting.lock() {
                            if let Some(sender) = waiting.remove(&(client, extra)) {
                                let _ = sender.send(value);
                            }
                        }
                    }
                }
            });
            runtime
        })
        .clone()
}
pub(crate) struct Telegram {
    id: i32,
    runtime: Arc<Runtime>,
    state: Arc<Mutex<Value>>,
    closing: AtomicBool,
}
impl Telegram {
    pub fn start(
        api_id: i32,
        api_hash: String,
        directory: &Path,
        key: &str,
    ) -> Result<Self, String> {
        std::fs::create_dir_all(directory)
            .map_err(|_| "Не удалось создать базу Telegram.".to_string())?;
        let runtime = runtime();
        let id = unsafe { td_create_client_id() };
        let state = Arc::new(Mutex::new(Value::Null));
        runtime
            .states
            .lock()
            .map_err(|_| "Ошибка состояния TDLib.")?
            .insert(id, state.clone());
        let client = Self {
            id,
            runtime,
            state,
            closing: AtomicBool::new(false),
        };
        let auth = client.request(json!({"@type":"getAuthorizationState"}))?;
        *client.state.lock().map_err(|_| "Ошибка состояния TDLib.")? = auth.clone();
        if auth.get("@type").and_then(Value::as_str)
            == Some("authorizationStateWaitTdlibParameters")
        {
            use base64::{
                engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
                Engine,
            };
            let encryption_key = STANDARD.encode(
                URL_SAFE_NO_PAD
                    .decode(key)
                    .map_err(|_| "Ключ базы Telegram повреждён.".to_string())?,
            );
            client.request(json!({"@type":"setTdlibParameters","use_test_dc":false,"database_directory":directory,"files_directory":directory.join("files"),"database_encryption_key":encryption_key,"use_file_database":true,"use_chat_info_database":true,"use_message_database":true,"use_secret_chats":false,"api_id":api_id,"api_hash":api_hash,"system_language_code":"ru","device_model":"PROTEUS desktop","system_version":std::env::consts::OS,"application_version":env!("CARGO_PKG_VERSION")}))?;
        }
        Ok(client)
    }
    pub fn request(&self, mut request: Value) -> Result<Value, String> {
        let (extra, response) = self.send_request(&mut request)?;
        self.receive_response(extra, response)
    }
    fn send_request(&self, request: &mut Value) -> Result<(u64, mpsc::Receiver<Value>), String> {
        let extra = self.runtime.next.fetch_add(1, Ordering::Relaxed);
        request["@extra"] = json!(extra);
        let request = CString::new(request.to_string())
            .map_err(|_| "Некорректный запрос TDLib.".to_string())?;
        let (sender, response) = mpsc::channel();
        self.runtime
            .waiting
            .lock()
            .map_err(|_| "Ошибка очереди TDLib.")?
            .insert((self.id, extra), sender);
        unsafe {
            td_send(self.id, request.as_ptr());
        }
        Ok((extra, response))
    }
    fn receive_response(
        &self,
        extra: u64,
        response: mpsc::Receiver<Value>,
    ) -> Result<Value, String> {
        let result = response.recv_timeout(Duration::from_secs(30));
        if let Ok(mut waiting) = self.runtime.waiting.lock() {
            waiting.remove(&(self.id, extra));
        }
        let result =
            result.map_err(|_| "Telegram не ответил вовремя. Повторите запрос.".to_string())?;
        if result.get("@type").and_then(Value::as_str) == Some("error") {
            return Err(match result.get("code").and_then(Value::as_i64) { Some(429) => "Telegram ограничил частоту запросов. Повторите позже.", Some(401) => "Сессия Telegram истекла. Подключите аккаунт заново.", _ => "Telegram отклонил запрос. Проверьте введённые данные или доступ к выбранному чату." }.into());
        }
        Ok(result)
    }
    pub fn status(&self) -> Result<String, String> {
        Ok(state_name(&self.authorization_state()?).into())
    }
    pub fn view(&self) -> Result<(String, Option<String>), String> {
        auth_view(&self.authorization_state()?)
    }
    pub fn prepare_login(&self, method: TelegramMethod) -> Result<bool, String> {
        prepare_login(self, method)
    }
    pub fn submit(&self, input: &str) -> Result<(), String> {
        if input.is_empty() || input.len() > 1024 || input.chars().any(char::is_control) {
            return Err("Проверьте введённые данные Telegram.".into());
        }
        let request = match self.status()?.as_str() {
            "phone-required" => {
                json!({"@type":"setAuthenticationPhoneNumber","phone_number":input,"settings":null})
            }
            "code-required" => json!({"@type":"checkAuthenticationCode","code":input}),
            "password-required" => json!({"@type":"checkAuthenticationPassword","password":input}),
            "email-required" => {
                json!({"@type":"setAuthenticationEmailAddress","email_address":input})
            }
            "email-code-required" => {
                json!({"@type":"checkAuthenticationEmailCode","code":{"@type":"emailAddressAuthenticationCode","code":input}})
            }
            _ => {
                return Err(
                    "Telegram сейчас не ожидает ввода. Дождитесь обновления состояния.".into(),
                )
            }
        };
        self.request(request).map(|_| ())
    }
    pub fn me(&self) -> Result<Value, String> {
        self.request(json!({"@type":"getMe"}))
    }
    pub fn logout(&self) -> Result<(), String> {
        let state = self.authorization_state()?;
        if state["@type"] == "authorizationStateClosed" {
            return Ok(());
        }
        if !matches!(
            state.get("@type").and_then(Value::as_str),
            Some("authorizationStateLoggingOut" | "authorizationStateClosing")
        ) {
            self.request(json!({"@type":"logOut"}))?;
        }
        self.wait_closed()
    }
    pub fn close(&self) {
        if self.closing.swap(true, Ordering::SeqCst)
            || self.status().is_ok_and(|status| status == "disconnected")
        {
            return;
        }
        if let Ok(request) = CString::new(json!({"@type":"close"}).to_string()) {
            unsafe {
                td_send(self.id, request.as_ptr());
            }
        }
        for _ in 0..100 {
            if self.status().is_ok_and(|status| status == "disconnected") {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
impl AuthTransport for Telegram {
    fn authorization_state(&self) -> Result<Value, String> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| "Ошибка состояния TDLib.".into())
    }
    fn call(&self, request: Value) -> Result<Value, String> {
        self.request(request)
    }
    fn cancel_pending(&self) -> Result<bool, String> {
        let state = self.state.lock().map_err(|_| "Ошибка состояния TDLib.")?;
        if matches!(
            state.get("@type").and_then(Value::as_str),
            Some("authorizationStateReady" | "authorizationStateClosed")
        ) {
            return Ok(false);
        }
        if matches!(
            state.get("@type").and_then(Value::as_str),
            Some("authorizationStateLoggingOut" | "authorizationStateClosing")
        ) {
            return Ok(true);
        }
        let (extra, response) = self.send_request(&mut json!({"@type":"logOut"}))?;
        drop(state);
        self.receive_response(extra, response)?;
        Ok(true)
    }
    fn wait_closed(&self) -> Result<(), String> {
        for _ in 0..200 {
            if self.authorization_state()?["@type"] == "authorizationStateClosed" {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err("Telegram ещё завершает отмену входа. Дождитесь окончания и повторите.".into())
    }
}
impl Drop for Telegram {
    fn drop(&mut self) {
        self.close();
        if let Ok(mut states) = self.runtime.states.lock() {
            states.remove(&self.id);
        }
    }
}
pub(crate) fn label(me: &Value) -> String {
    me.get("usernames")
        .and_then(|v| v.get("active_usernames"))
        .and_then(|v| v.get(0))
        .and_then(Value::as_str)
        .or_else(|| me.get("first_name").and_then(Value::as_str))
        .unwrap_or("Telegram")
        .chars()
        .filter(|c| !c.is_control())
        .take(100)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    struct FakeAuth {
        state: Mutex<Value>,
        requests: Mutex<Vec<Value>>,
        close_succeeds: bool,
        ready_before_cancel: bool,
    }
    impl FakeAuth {
        fn new(state: &str) -> Self {
            Self {
                state: Mutex::new(json!({"@type":state})),
                requests: Mutex::new(vec![]),
                close_succeeds: true,
                ready_before_cancel: false,
            }
        }
    }
    impl AuthTransport for FakeAuth {
        fn authorization_state(&self) -> Result<Value, String> {
            Ok(self.state.lock().unwrap().clone())
        }
        fn call(&self, request: Value) -> Result<Value, String> {
            if request["@type"] == "requestQrCodeAuthentication" {
                *self.state.lock().unwrap() = json!({"@type":"authorizationStateWaitOtherDeviceConfirmation","link":"tg://login?token=AAAA"});
            }
            self.requests.lock().unwrap().push(request);
            Ok(json!({"@type":"ok"}))
        }
        fn cancel_pending(&self) -> Result<bool, String> {
            if self.ready_before_cancel {
                *self.state.lock().unwrap() = json!({"@type":"authorizationStateReady"});
                return Ok(false);
            }
            self.call(json!({"@type":"logOut"}))?;
            Ok(true)
        }
        fn wait_closed(&self) -> Result<(), String> {
            if self.close_succeeds {
                *self.state.lock().unwrap() = json!({"@type":"authorizationStateClosed"});
                Ok(())
            } else {
                Err("close timeout".into())
            }
        }
    }
    #[test]
    fn qr_login_requests_once_and_preserves_ready_sessions() {
        let client = FakeAuth::new("authorizationStateWaitPhoneNumber");
        assert!(!prepare_login(&client, TelegramMethod::Qr).unwrap());
        assert_eq!(
            client.requests.lock().unwrap()[0],
            json!({"@type":"requestQrCodeAuthentication","other_user_ids":[]})
        );
        assert!(!prepare_login(&client, TelegramMethod::Qr).unwrap());
        assert_eq!(client.requests.lock().unwrap().len(), 1);
        let ready = FakeAuth::new("authorizationStateReady");
        assert!(!prepare_login(&ready, TelegramMethod::Phone).unwrap());
        assert!(!prepare_login(&ready, TelegramMethod::Qr).unwrap());
        assert!(ready.requests.lock().unwrap().is_empty());
    }
    #[test]
    fn phone_fallback_restarts_only_after_closed_and_preserves_observed_ready_session() {
        let qr = FakeAuth::new("authorizationStateWaitOtherDeviceConfirmation");
        assert!(prepare_login(&qr, TelegramMethod::Phone).unwrap());
        assert_eq!(
            qr.requests.lock().unwrap().as_slice(),
            &[json!({"@type":"logOut"})]
        );
        assert_eq!(
            qr.authorization_state().unwrap()["@type"],
            "authorizationStateClosed"
        );
        let fresh = FakeAuth::new("authorizationStateWaitPhoneNumber");
        assert!(!prepare_login(&fresh, TelegramMethod::Phone).unwrap());
        let mut slow = FakeAuth::new("authorizationStateWaitOtherDeviceConfirmation");
        slow.close_succeeds = false;
        assert!(prepare_login(&slow, TelegramMethod::Phone).is_err());
        let mut racing = FakeAuth::new("authorizationStateWaitOtherDeviceConfirmation");
        racing.ready_before_cancel = true;
        assert!(!prepare_login(&racing, TelegramMethod::Phone).unwrap());
        assert!(racing.requests.lock().unwrap().is_empty());
    }
    fn decode_svg(data_url: &str) -> Vec<u8> {
        let svg = String::from_utf8(
            STANDARD
                .decode(data_url.strip_prefix("data:image/svg+xml;base64,").unwrap())
                .unwrap(),
        )
        .unwrap();
        let width: usize = svg
            .split("width=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let mut pixels = vec![255u8; width * width];
        let path = svg
            .split(" d=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        for rect in path.split('M').filter(|part| !part.is_empty()) {
            let values: Vec<usize> = rect
                .split([' ', 'h', 'v', 'H', 'V'])
                .map(|v| v.parse().unwrap())
                .collect();
            for y in values[1]..values[1] + values[3] {
                for x in values[0]..values[0] + values[2] {
                    pixels[y * width + x] = 0;
                }
            }
        }
        quircs::Quirc::default()
            .identify(width, width, &pixels)
            .next()
            .unwrap()
            .unwrap()
            .decode()
            .unwrap()
            .payload
    }
    #[test]
    fn qr_image_decodes_independently_rotates_and_disappears_after_confirmation() {
        let link = "tg://login?token=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let (state, image) = auth_view(
            &json!({"@type":"authorizationStateWaitOtherDeviceConfirmation","link":link}),
        )
        .unwrap();
        assert_eq!(state, "qr-required");
        let image = image.unwrap();
        assert!(image.len() <= 128 * 1024);
        assert!(!image.contains("tg://"));
        assert_eq!(decode_svg(&image), link.as_bytes());
        let rotated=auth_view(&json!({"@type":"authorizationStateWaitOtherDeviceConfirmation","link":link.replace('A',"B")})).unwrap().1.unwrap();
        assert_ne!(image, rotated);
        for (native, expected) in [
            ("authorizationStateWaitPassword", "password-required"),
            ("authorizationStateReady", "connected"),
            ("authorizationStateClosed", "disconnected"),
        ] {
            assert_eq!(
                auth_view(&json!({"@type":native,"link":link})).unwrap(),
                (expected.into(), None)
            );
        }
        assert!(auth_view(&json!({"@type":"authorizationStateWaitOtherDeviceConfirmation","link":"https://evil.invalid"})).is_err());
        for invalid in [
            "tg://other?token=AAAAAAAAAAAAAAAAAAAA",
            "tg://login?token=AAAAAAAAAAAAAAAAAAAA&token=BBBBBBBBBBBBBBBBBBBB",
            "tg://login?token=AAAAAAAAAAAAAAAAAAAA#fragment",
            "tg://login/path?token=AAAAAAAAAAAAAAAAAAAA",
            "tg://login?token=AAAAAAAAAAAA%00AAAAAA",
            "tg://user@login?token=AAAAAAAAAAAAAAAAAAAA",
        ] {
            assert!(auth_view(
                &json!({"@type":"authorizationStateWaitOtherDeviceConfirmation","link":invalid})
            )
            .is_err());
        }
    }
}
