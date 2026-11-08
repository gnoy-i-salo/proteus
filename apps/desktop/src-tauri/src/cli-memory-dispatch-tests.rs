use super::*;
use crate::memory::{session::InvocationBinding, types::ModelCallBudget};
use serde_json::json;
use std::{
    path::Path,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
fn fixture(dir: &Path) -> std::path::PathBuf {
    let source = dir.join("fixture.rs");
    std::fs::write(&source, include_str!("fixtures/cli-memory-process.rs")).unwrap();
    let binary = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    let output = Command::new("rustc")
        .args(["--edition=2021"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}
fn dispatch_case(mode: &str) {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let receipt = bound.recall_recorded("контекст", None, || 70003).unwrap();
    bound
        .configure_research_model(
            InvocationBinding {
                provider: "codex".into(),
                model: "fixture-model".into(),
                effort: "medium".into(),
                operation: "corpus-analysis".into(),
            },
            ModelCallBudget {
                max_calls: 2,
                max_input_bytes: 100000,
                max_duration_ms: 1000,
            },
            || 70003,
        )
        .unwrap();
    let reserved = bound
        .reserve_research_context(
            std::slice::from_ref(&receipt.receipt.id),
            "  prompt\r\n",
            "точный текст 😀\t",
            1,
            || 70003,
        )
        .unwrap();
    let request = serde_json::from_value(json!({
        "id":"memory-call", "provider":"codex", "model":"fixture-model", "effort":"medium",
        "operation":"corpus-analysis", "sourceContext":true,
        "systemPrompt":reserved.input.system_prompt, "task":reserved.input.task,
        "memoryContext":{"sessionId":"native-session", "leaseId":reserved.id}
    }))
    .unwrap();
    let binary = fixture(&dir);
    let run = dir.join("run");
    std::fs::create_dir(&run).unwrap();
    if mode == "failed" {
        std::fs::write(run.join("fail-exit"), "yes").unwrap();
    }
    if mode == "revoked" {
        std::fs::write(run.join("wait"), "yes").unwrap();
    }
    if mode == "probe-cancelled" {
        std::fs::write(dir.join("wait-probe"), "yes").unwrap();
    }
    let events = Mutex::new(Vec::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let (done, finished) = std::sync::mpsc::channel();
    let revoke = ["revoked", "probe-cancelled"].contains(&mode).then(|| {
        let source = sources.clone();
        let revoke_grant = mode == "revoked";
        let received = if revoke_grant {
            run.join("received.txt")
        } else {
            dir.join("probed")
        };
        let cancel = cancelled.clone();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            while !received.exists() {
                if started.elapsed() > std::time::Duration::from_secs(10) {
                    cancel.store(true, Ordering::SeqCst);
                    return false;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            if revoke_grant {
                if source
                    .with_ingestion_job_state(|db| db.remove(&grant))
                    .is_err()
                    || finished
                        .recv_timeout(std::time::Duration::from_secs(10))
                        .is_err()
                {
                    cancel.store(true, Ordering::SeqCst);
                    return false;
                }
            } else {
                cancel.store(true, Ordering::SeqCst);
            }
            true
        })
    });
    let now = || {
        if mode == "expired" && dir.join("probed").exists() {
            return 80000;
        }
        if mode == "cancelled" && run.join("answer-written").exists() {
            sources.cancel_ingestion_operation("memory").unwrap();
        }
        if mode == "cli-cancelled" && run.join("answer-written").exists() {
            cancelled.store(true, Ordering::SeqCst);
        }
        70003
    };
    let result = crate::cli::execute_fixture(
        request,
        &run,
        cancelled.clone(),
        |kind, _, _| {
            events.lock().unwrap().push(kind.to_owned());
            if mode == "stop-started" && kind == "started" {
                cancelled.store(true, Ordering::SeqCst);
            }
        },
        &binary,
        if mode == "missing" {
            None
        } else {
            Some(&mut bound)
        },
        &now,
    );
    let _ = done.send(());
    if let Some(revoke) = revoke {
        assert!(
            revoke.join().unwrap(),
            "fixture did not reach the checkpoint or dispatch failed to stop"
        );
    }
    let proof = bound.completed_context_receipts(std::slice::from_ref(&reserved.id), || 70003);
    let retained = bound.authorized_receipts(std::slice::from_ref(&receipt.receipt.id), || 70003);
    let received = std::fs::read_to_string(run.join("received.txt")).ok();
    let expected = format!(
        "{}\n\nINPUT DATA:\n{}",
        reserved.input.system_prompt, reserved.input.task
    );
    let events = events.into_inner().unwrap();
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
    if mode == "success" {
        assert!(result.is_ok());
        assert_eq!(received.as_deref(), Some(expected.as_str()));
        assert!(events.iter().any(|event| event == "finished"));
        let proof = proof.expect("successful native dispatch must complete its context");
        assert_eq!(proof[0].receipt.id, receipt.receipt.id);
    } else {
        assert!(result.is_err(), "{mode} dispatch succeeded");
        assert!(proof.is_err());
        if ["stop-started", "probe-cancelled"].contains(&mode) {
            assert!(
                retained.is_err(),
                "CLI cancellation must close its source session"
            );
        }
        assert!(!events.iter().any(|event| event == "finished"));
        if ["expired", "missing", "probe-cancelled"].contains(&mode) {
            assert!(received.is_none());
        }
        if ["cancelled", "cli-cancelled"].contains(&mode) {
            assert!(!events.iter().any(|event| event == "answer"));
        }
    }
}
#[test]
fn native_dispatch_completes_only_the_exact_bound_input() {
    dispatch_case("success");
}
#[test]
fn native_dispatch_does_not_complete_failed_processes() {
    dispatch_case("failed");
}
#[test]
fn native_dispatch_rechecks_expiry_after_capability_probes() {
    dispatch_case("expired");
}
#[test]
fn native_dispatch_rechecks_cancellation_before_output() {
    dispatch_case("cancelled");
}
#[test]
fn native_dispatch_rejects_memory_reference_without_native_session() {
    dispatch_case("missing");
}

#[test]
fn native_dispatch_observes_cli_cancellation_before_output() {
    dispatch_case("cli-cancelled");
}

#[test]
fn native_dispatch_stops_a_running_process_when_source_grant_is_revoked() {
    dispatch_case("revoked");
}

#[test]
fn native_cli_cancellation_closes_the_bound_source_session() {
    dispatch_case("stop-started");
}

#[test]
fn native_cli_probe_cancellation_closes_session_without_sending_sources() {
    dispatch_case("probe-cancelled");
}
