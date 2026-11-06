//! Official TDLib JSON C ABI. A single ordered receiver owns all returned pointers.
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
        let state = self.state.lock().map_err(|_| "Ошибка состояния TDLib.")?;
        Ok(match state.get("@type").and_then(Value::as_str) {
            Some("authorizationStateReady") => "connected",
            Some("authorizationStateWaitPhoneNumber") => "phone-required",
            Some("authorizationStateWaitCode") => "code-required",
            Some("authorizationStateWaitPassword") => "password-required",
            Some("authorizationStateWaitEmailAddress") => "email-required",
            Some("authorizationStateWaitEmailCode") => "email-code-required",
            Some("authorizationStateWaitRegistration") => "unavailable",
            Some("authorizationStateClosed") => "disconnected",
            _ => "pending",
        }
        .into())
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
        self.request(json!({"@type":"logOut"})).map(|_| ())
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
