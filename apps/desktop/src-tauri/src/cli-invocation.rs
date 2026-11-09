use super::{
    command::configure_kimi,
    executable::executable,
    model::{response, validate, Request, Response},
};
use crate::cli_policy::{codex_args, codex_capabilities};
use crate::cli_protocol::{answer_event, parse_event, KimiOutput, Reasoning, ReasoningBlock};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

pub(super) fn stop(child: &mut Child) {
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
const CLI_OUTPUT_LIMIT: u64 = 4 * 1024 * 1024;
pub(super) fn read_output(
    input: impl Read,
    sender: mpsc::SyncSender<std::io::Result<String>>,
    limit: u64,
) {
    // Read one extra byte so a valid earlier candidate cannot hide truncated output.
    let mut reader = BufReader::new(input.take(limit + 1));
    let mut total = 0;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(size) => {
                total += size as u64;
                if total > limit {
                    let _ = sender.send(Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "CLI output exceeds the output limit",
                    )));
                    break;
                }
                if sender.send(Ok(line)).is_err() {
                    break;
                }
            }
            Err(error) => {
                let _ = sender.send(Err(error));
                break;
            }
        }
    }
}
fn probe(path: &Path, args: &[&str], cancelled: &AtomicBool) -> Result<String, String> {
    let mut command = Command::new(path);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Не удалось проверить возможности установленного Codex CLI.")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("CLI capability output unavailable")?;
    let (sender, receiver) = mpsc::sync_channel(8);
    let reader = std::thread::spawn(move || read_output(stdout, sender, 64 * 1024));
    let start = Instant::now();
    let mut output = String::new();
    let result = loop {
        if cancelled.load(Ordering::SeqCst) || start.elapsed() > Duration::from_secs(5) {
            stop(&mut child);
            break Err("Проверка возможностей CLI отменена или не завершилась за 5 секунд. Данные не переданы.".into());
        }
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(line)) => output.push_str(&line),
            Ok(Err(_)) => {
                stop(&mut child);
                break Err("CLI capability output exceeds its limit".into());
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => match child.try_wait() {
                Ok(Some(status)) if status.success() => break Ok(output),
                Ok(Some(_)) => break Err(
                    "Обновите Codex CLI: проверка необходимых настроек изоляции не поддерживается."
                        .into(),
                ),
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => {
                    stop(&mut child);
                    break Err("CLI capability check failed".into());
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    };
    drop(receiver);
    let _ = reader.join();
    result
}
pub(super) fn execute(
    request: Request,
    directory: &Path,
    cancelled: Arc<AtomicBool>,
    emit: impl Fn(&str, u64, Option<ReasoningBlock>),
    session: Option<&mut crate::memory::session::MemorySession>,
) -> Result<Response, String> {
    validate(&request)?;
    let path = executable(&request.provider).ok_or_else(|| {
        format!(
            "{} CLI не найден. Установите CLI и выполните вход в терминале.",
            request.provider
        )
    })?;
    execute_at(request, directory, cancelled, emit, &path, session, &|| {
        chrono::Utc::now().timestamp_millis()
    })
}
pub(crate) fn execute_at(
    request: Request,
    directory: &Path,
    cancelled: Arc<AtomicBool>,
    emit: impl Fn(&str, u64, Option<ReasoningBlock>),
    path: &Path,
    session: Option<&mut crate::memory::session::MemorySession>,
    now: &dyn Fn() -> i64,
) -> Result<Response, String> {
    validate(&request)?;
    let mut context = super::memory_context::Context::new(&request, session, now, &cancelled)?;
    if request.provider == "codex" {
        // Probe without prompt data before creating files or starting model inference.
        let help = probe(path, &["exec", "--help"], &cancelled)?;
        let features = probe(path, &["features", "list"], &cancelled)?;
        codex_capabilities(&help, &features)?;
    }
    context.consume()?;
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    // Establish a private project root, preventing parent-directory discovery.
    std::fs::create_dir_all(directory.join(".git")).map_err(|e| e.to_string())?;
    let mut command = Command::new(path);
    command
        .current_dir(directory)
        .stdin(if request.provider == "codex" {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let prompt = format!("{}\n\nINPUT DATA:\n{}", request.system_prompt, request.task);
    if request.provider == "codex" {
        command.args(codex_args(
            &request.operation,
            &request.effort,
            &request.model,
        )?);
    } else {
        configure_kimi(&mut command, directory, &request)?;
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
    context.check()?;
    let mut child = command
        .spawn()
        .map_err(|e| format!("CLI startup failed: {e}"))?;
    if let Err(error) = context.check() {
        stop(&mut child);
        return Err(error);
    }
    if request.provider == "codex" {
        let mut stdin = child.stdin.take().ok_or("CLI stdin unavailable")?;
        if stdin
            .write_all(prompt.as_bytes())
            .and_then(|()| stdin.flush())
            .is_err()
        {
            stop(&mut child);
            return Err("CLI closed its input".into());
        }
    }
    let mut kimi = KimiOutput::default();
    emit("started", 0, None);
    let stdout = child.stdout.take().ok_or("CLI stdout unavailable")?;
    let (sender, receiver) = mpsc::sync_channel(64);
    let reader = std::thread::spawn(move || read_output(stdout, sender, CLI_OUTPUT_LIMIT));
    let mut answer = String::new();
    let mut reasoning = Reasoning::default();
    let mut failure = false;
    let result = loop {
        if let Err(error) = context.check() {
            stop(&mut child);
            break Err(error);
        }
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
                if let Err(error) = context.check() {
                    stop(&mut child);
                    break Err(error);
                }
                let parsed = match parse_event(&request.provider, &line) {
                    Ok(event) => event,
                    Err(error) => {
                        stop(&mut child);
                        break Err(error);
                    }
                };
                if let Some(event) = parsed {
                    if let Some(block) = reasoning.accept(&request.provider, &event) {
                        emit("reasoning", start.elapsed().as_millis() as u64, Some(block));
                    }
                    if request.provider == "kimi" {
                        if let Err(error) = kimi.accept(&event) {
                            stop(&mut child);
                            break Err(error);
                        }
                    }
                    if request.provider == "codex" {
                        if let Some(text) = answer_event(&request.provider, &event) {
                            answer = text;
                            emit("answer", start.elapsed().as_millis() as u64, None);
                        }
                    }
                    if event["type"] == "error" || event["type"] == "turn.failed" {
                        failure = true;
                    }
                    if event["item"]["type"] == "web_search" || event.get("tool_calls").is_some() {
                        emit("search", start.elapsed().as_millis() as u64, None);
                    }
                }
            }
            Ok(Err(error)) => {
                stop(&mut child);
                break Err(format!("CLI output could not be read: {error}"));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => match child.try_wait() {
                Ok(Some(status)) => {
                    if request.provider == "kimi" {
                        match kimi.finish(status.success()) {
                            Ok(text) => {
                                answer = text;
                                emit("answer", start.elapsed().as_millis() as u64, None);
                            }
                            Err(error) => break Err(error),
                        }
                    }
                    if !status.success() || failure || answer.trim().is_empty() {
                        break Err(format!("{} CLI не вернул ответ ({}). Проверьте вход, выбранную модель и лимиты в терминале.", request.provider, status));
                    }
                    if let Err(error) = context.complete(&answer) {
                        break Err(error);
                    }
                    let elapsed = start.elapsed().as_millis() as u64;
                    emit("finished", elapsed, None);
                    break Ok(response(&request, answer, elapsed, reasoning));
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
