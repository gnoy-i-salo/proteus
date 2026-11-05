use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

#[derive(Default, Clone)]
pub struct Runs(pub Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>);
#[derive(Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Request {
    id: String,
    provider: String,
    model: String,
    task: String,
    system_prompt: String,
    effort: String,
}
#[derive(Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct Event {
    id: String,
    kind: String,
    elapsed_ms: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Trace {
    provider: String,
    reasoning_ms: u64,
    terminal_ms: u64,
    summary: String,
}
#[derive(Serialize)]
pub struct Response {
    answer: String,
    trace: Trace,
}
#[derive(Serialize)]
pub struct Availability {
    provider: String,
    available: bool,
    path: Option<String>,
}

fn executable(provider: &str) -> Option<PathBuf> {
    if !["codex", "kimi"].contains(&provider) {
        return None;
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    let name = if cfg!(windows) {
        format!("{provider}.exe")
    } else {
        provider.to_owned()
    };
    let mut paths: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join(&name)).collect())
        .unwrap_or_default();
    for directory in [
        home.join(".local/bin"),
        home.join(".cargo/bin"),
        home.join(".kimi-code/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ] {
        paths.push(directory.join(&name));
    }
    if provider == "codex" {
        paths.push(PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex"));
    }
    paths.into_iter().find(|p| p.is_file())
}
#[tauri::command]
pub fn cli_status() -> Vec<Availability> {
    ["codex", "kimi"]
        .into_iter()
        .map(|provider| {
            let path = executable(provider);
            Availability {
                provider: provider.into(),
                available: path.is_some(),
                path: path.map(|p| p.to_string_lossy().into_owned()),
            }
        })
        .collect()
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
fn validate(request: &Request) -> Result<(), String> {
    if !valid_id(&request.id)
        || !["codex", "kimi"].contains(&request.provider.as_str())
        || !["low", "medium", "high"].contains(&request.effort.as_str())
        || request.task.len() > 200_000
        || request.system_prompt.len() > 40_000
        || request.model.len() > 120
        || request.model.contains(['\n', '\r', '\0'])
    {
        return Err("Invalid CLI request".into());
    }
    Ok(())
}
// Only final assistant content is consumed. Reasoning and tool payloads never enter the UI.
fn answer_event(provider: &str, event: &Value) -> Option<String> {
    if provider == "codex"
        && event["type"] == "item.completed"
        && event["item"]["type"] == "agent_message"
    {
        return event["item"]["text"].as_str().map(str::to_owned);
    }
    if provider == "kimi"
        && event["role"] == "assistant"
        && event
            .get("tool_calls")
            .is_none_or(|v| v.as_array().is_some_and(Vec::is_empty))
    {
        return event["content"].as_str().map(str::to_owned);
    }
    None
}
fn stop(child: &mut Child) {
    #[cfg(unix)]
    {
        // The child is its own process group; terminate only this invocation and its descendants.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}
fn execute(
    request: Request,
    directory: &Path,
    cancelled: Arc<AtomicBool>,
    emit: impl Fn(&str, u64),
) -> Result<Response, String> {
    validate(&request)?;
    let path = executable(&request.provider).ok_or_else(|| {
        format!(
            "{} CLI не найден. Установите CLI и выполните вход в терминале.",
            request.provider
        )
    })?;
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    // Establish a private project root, preventing parent-directory discovery.
    std::fs::create_dir_all(directory.join(".git")).map_err(|e| e.to_string())?;
    let search = request.system_prompt.starts_with("STALKER\n");
    let mut command = Command::new(path);
    command
        .current_dir(directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let prompt = format!("{}\n\nINPUT DATA:\n{}", request.system_prompt, request.task);
    if request.provider == "codex" {
        command.args([
            "exec",
            "--json",
            "--ephemeral",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "--disable",
            "shell_tool",
            "--disable",
            "multi_agent",
            "-c",
            "approval_policy=\"never\"",
            "-c",
            &format!("model_reasoning_effort=\"{}\"", request.effort),
            "-c",
            if search {
                "web_search=\"live\""
            } else {
                "web_search=\"disabled\""
            },
        ]);
        if !request.model.is_empty() {
            command.args(["--model", &request.model]);
        }
        command.arg("-");
    } else {
        let agent = directory.join("agent.md");
        let tools = if search {
            "[WebSearch, FetchURL]"
        } else {
            "[]"
        };
        std::fs::write(&agent, format!("---\nname: proteus\ndescription: Bounded persona experiment\ntools: {tools}\n---\n{}\n", request.system_prompt)).map_err(|e| e.to_string())?;
        command.arg("--agent-file").arg(agent).args([
            "--output-format",
            "stream-json",
            "-p",
            &request.task,
        ]);
        if !request.model.is_empty() {
            command.args(["--model", &request.model]);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let start = Instant::now();
    if cancelled.load(Ordering::SeqCst) {
        return Err("Отменено".into());
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("CLI startup failed: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        if request.provider == "codex" && stdin.write_all(prompt.as_bytes()).is_err() {
            stop(&mut child);
            return Err("CLI closed its input".into());
        }
    }
    emit("started", 0);
    let stdout = child.stdout.take().ok_or("CLI stdout unavailable")?;
    let (sender, receiver) = mpsc::sync_channel(64);
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout.take(4 * 1024 * 1024)).lines() {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let mut answer = String::new();
    let mut failure = false;
    let result = loop {
        if cancelled.load(Ordering::SeqCst) || start.elapsed() > Duration::from_secs(600) {
            stop(&mut child);
            break Err(if cancelled.load(Ordering::SeqCst) {
                "Отменено"
            } else {
                "CLI не завершился за 10 минут. Повторите с другой моделью."
            }
            .into());
        }
        match receiver.recv_timeout(Duration::from_millis(80)) {
            Ok(Ok(line)) => {
                if let Ok(event) = serde_json::from_str::<Value>(&line) {
                    if let Some(text) = answer_event(&request.provider, &event) {
                        answer = text;
                        emit("answer", start.elapsed().as_millis() as u64);
                    }
                    if event["type"] == "error" || event["type"] == "turn.failed" {
                        failure = true;
                    }
                    if event["item"]["type"] == "web_search" || event.get("tool_calls").is_some() {
                        emit("search", start.elapsed().as_millis() as u64);
                    }
                }
            }
            Ok(Err(_)) => {
                stop(&mut child);
                break Err("CLI output could not be read".into());
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() || failure || answer.trim().is_empty() {
                        break Err(format!("{} CLI не вернул ответ ({}). Проверьте вход, выбранную модель и лимиты в терминале.", request.provider, status));
                    }
                    let elapsed = start.elapsed().as_millis() as u64;
                    emit("finished", elapsed);
                    break Ok(Response {
                        answer,
                        trace: Trace {
                            provider: request.provider.clone(),
                            reasoning_ms: elapsed,
                            terminal_ms: 0,
                            summary: "CLI returned an answer".into(),
                        },
                    });
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(80)),
                Err(error) => {
                    stop(&mut child);
                    break Err(error.to_string());
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    };
    drop(receiver);
    let _ = reader.join();
    result
}
#[tauri::command]
pub async fn run_cli(
    app: tauri::AppHandle,
    state: tauri::State<'_, Runs>,
    request: Request,
) -> Result<Response, String> {
    validate(&request)?;
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("runs")
        .join(&request.id);
    let runs = state.0.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut active = runs.lock().map_err(|e| e.to_string())?;
        if !active.is_empty() {
            return Err("Another CLI stage is already running".into());
        }
        active.insert(request.id.clone(), cancelled.clone());
    }
    let id = request.id.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        let event_id = request.id.clone();
        let result = execute(request, &directory, cancelled, |kind, elapsed_ms| {
            let _ = app.emit(
                "cli-progress",
                Event {
                    id: event_id.clone(),
                    kind: kind.into(),
                    elapsed_ms,
                },
            );
        });
        // Transient prompts are not retained; completed experiment artifacts are saved separately.
        let _ = std::fs::remove_dir_all(directory);
        result
    })
    .await
    .map_err(|e| e.to_string());
    runs.lock().map_err(|e| e.to_string())?.remove(&id);
    result?
}
#[tauri::command]
pub fn cancel_cli(state: tauri::State<'_, Runs>, id: String) -> Result<(), String> {
    if let Some(flag) = state.0.lock().map_err(|e| e.to_string())?.get(&id) {
        flag.store(true, Ordering::SeqCst);
    }
    Ok(())
}
#[tauri::command]
pub fn save_experiment(app: tauri::AppHandle, id: String, data: String) -> Result<String, String> {
    if !valid_id(&id) || data.len() > 2_000_000 {
        return Err("Invalid experiment".into());
    }
    let value: Value = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("experiments")
        .join(id);
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let temp = directory.join("result.tmp");
    std::fs::write(&temp, data).map_err(|e| e.to_string())?;
    std::fs::rename(temp, directory.join("result.json")).map_err(|e| e.to_string())?;
    if let Some(people) = value["people"].as_array() {
        for (index, person) in people.iter().take(2).enumerate() {
            let folder = directory.join(if index == 0 { "person-a" } else { "person-b" });
            if let Some(files) = person["artifacts"].as_object() {
                for (path, content) in files {
                    let valid = path == "AGENTS.md"
                        || (path.starts_with(".agents/skills/persona-")
                            && path.ends_with("/SKILL.md")
                            && !path.contains(".."));
                    if valid {
                        if let Some(content) = content.as_str() {
                            let target = folder.join(path);
                            std::fs::create_dir_all(
                                target.parent().ok_or("Invalid artifact path")?,
                            )
                            .map_err(|e| e.to_string())?;
                            std::fs::write(target, content).map_err(|e| e.to_string())?;
                        }
                    }
                }
            }
        }
    }
    Ok(directory.to_string_lossy().into_owned())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn final_answers_only() {
        assert_eq!(
            answer_event(
                "codex",
                &serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":"answer"}})
            ),
            Some("answer".into())
        );
        assert_eq!(
            answer_event(
                "codex",
                &serde_json::json!({"type":"item.completed","item":{"type":"reasoning","text":"private"}})
            ),
            None
        );
        assert_eq!(
            answer_event(
                "kimi",
                &serde_json::json!({"role":"assistant","content":"answer"})
            ),
            Some("answer".into())
        );
        assert_eq!(
            answer_event(
                "kimi",
                &serde_json::json!({"role":"assistant","content":"tool","tool_calls":[{}]})
            ),
            None
        );
        assert_eq!(
            answer_event(
                "kimi",
                &serde_json::json!({"role":"tool","content":"private"})
            ),
            None
        );
    }
    #[test]
    #[ignore = "Uses the user's authenticated CLI accounts; run explicitly"]
    fn live_cli_connections() {
        for provider in ["codex", "kimi"] {
            let directory = std::env::temp_dir().join(format!(
                "proteus-broker-test-{}-{provider}",
                std::process::id()
            ));
            let result = execute(
                Request {
                    id: "connection-test".into(),
                    provider: provider.into(),
                    model: String::new(),
                    task: "Return only {\"connected\":true}".into(),
                    system_prompt: "Reply with the requested JSON. No tools.".into(),
                    effort: "low".into(),
                },
                &directory,
                Arc::new(AtomicBool::new(false)),
                |_, _| {},
            );
            let _ = std::fs::remove_dir_all(directory);
            let response = result.expect("CLI must return a final answer");
            assert_eq!(
                serde_json::from_str::<Value>(&response.answer).unwrap()["connected"],
                true
            );
            assert_eq!(response.trace.terminal_ms, 0);
            assert!(response.trace.reasoning_ms > 0);
        }
    }
    #[test]
    #[cfg(unix)]
    fn cancellation_terminates_the_owned_process_group() {
        use std::os::unix::process::CommandExt;
        let mut child = Command::new("/bin/sh")
            .args(["-c", "sleep 30 & wait"])
            .process_group(0)
            .spawn()
            .unwrap();
        let start = Instant::now();
        stop(&mut child);
        assert!(child.try_wait().unwrap().is_some());
        assert!(start.elapsed() < Duration::from_secs(3));
    }
    #[test]
    fn ids_cannot_escape_storage() {
        assert!(valid_id("abc-123"));
        for id in ["", "../secret", "/tmp/other", "a/b", "a\\b"] {
            assert!(!valid_id(id));
        }
    }
}
