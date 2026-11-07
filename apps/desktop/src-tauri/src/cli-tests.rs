use super::*;
use super::{
    command::configure_kimi,
    invocation::{read_output, stop},
    model::{response, valid_id},
};
use crate::{
    cli_protocol::{answer_event, parse_event, Reasoning},
    experiment_storage::parse_experiment,
};
use serde_json::Value;
use std::{
    process::Command,
    sync::mpsc,
    time::{Duration, Instant},
};
fn parse_smoke_answer(answer: &str) -> Result<Value, serde_json::Error> {
    // Production `structured` accepts a complete Markdown JSON envelope. Match that
    // documented wire format here; never extract JSON from prose or repair its contents.
    let answer = answer.trim();
    let json = answer
        .strip_prefix("```")
        .and_then(|body| body.strip_suffix("```"))
        .map(|body| body.strip_prefix("json").unwrap_or(body).trim())
        .unwrap_or(answer);
    serde_json::from_str(json)
}
#[test]
fn smoke_accepts_raw_json_and_a_complete_outer_markdown_fence() {
    for answer in [
        "{\"connected\":true,\"books\":80}",
        "  ```json\n{\"connected\":true,\"books\":80}\n```\n",
        "```\n{\"connected\":true,\"books\":80}\n```",
    ] {
        let value = parse_smoke_answer(answer).unwrap();
        assert_eq!(value["connected"], true);
        assert_eq!(value["books"], 80);
    }
}
#[test]
fn smoke_rejects_prose_partial_fences_and_invalid_json() {
    for answer in [
        "```json\n{\"connected\":true,\"books\":80}\n```\nCalculation: 80 books.",
        "The answer is {\"connected\":true,\"books\":80}",
        "```json\n{\"connected\":true,\"books\":80}",
        "```json\n{broken}\n```",
        "{broken}",
    ] {
        assert!(parse_smoke_answer(answer).is_err());
    }
}
#[test]
fn repeated_experiment_exports_allow_eight_mib_and_reject_larger_data() {
    let limit = 8 * 1024 * 1024;
    let data = format!("{{\"note\":\"{}\"}}", "x".repeat(limit - 11));
    let value = parse_experiment("repeated-experiment", &data).unwrap();
    assert_eq!(value["note"].as_str().unwrap().len(), limit - 11);
    assert!(parse_experiment("too-large", &format!("{data} ")).is_err());
    assert!(parse_experiment("invalid-json", "not JSON").is_err());
    assert!(parse_experiment("../escape", "{}").is_err());
}
#[test]
fn trace_distinguishes_wall_time_from_unobserved_reasoning() {
    for provider in ["codex", "kimi"] {
        let request = Request {
            id: "metadata-test".into(),
            provider: provider.into(),
            model: "configured-alias".into(),
            task: "task".into(),
            system_prompt: "prompt".into(),
            effort: "high".into(),
            operation: "generation".into(),
            source_context: false,
        };
        let value = serde_json::to_value(response(
            &request,
            "answer".into(),
            1234,
            Reasoning::default(),
        ))
        .unwrap();
        let trace = &value["trace"];
        assert_eq!(trace["wallTimeMs"], 1234);
        assert_eq!(trace["reasoningMs"], 1234);
        assert_eq!(trace.get("activeReasoningMs"), Some(&Value::Null));
        assert_eq!(trace.get("resolvedModel"), Some(&Value::Null));
        assert_eq!(trace.get("cliVersion"), Some(&Value::Null));
        assert_eq!(trace["requestedModel"], "configured-alias");
        assert_eq!(trace["requestedEffort"], "high");
        if provider == "kimi" {
            assert_eq!(trace.get("appliedEffort"), Some(&Value::Null));
        } else {
            assert_eq!(trace["appliedEffort"], "high");
        }
    }
}
#[test]
fn kimi_rejects_malformed_or_non_object_jsonl_without_salvaging_a_candidate() {
    for line in ["{broken}", "partial output", "42", "[]"] {
        assert!(parse_event("kimi", line).is_err());
    }
    assert!(parse_event("kimi", "  ").unwrap().is_none());
    assert!(parse_event("codex", "diagnostic").unwrap().is_none());
    assert_eq!(
        parse_event("kimi", "{\"role\":\"meta\",\"type\":\"system.version\"}")
            .unwrap()
            .unwrap()["role"],
        "meta"
    );
}
#[test]
fn output_reader_accepts_exact_limit_and_explicitly_rejects_overflow() {
    for (data, overflow) in [("first\n12345\n", false), ("first\n12345\nx", true)] {
        let (sender, receiver) = mpsc::sync_channel(64);
        read_output(data.as_bytes(), sender, 12);
        let lines: Vec<_> = receiver.into_iter().collect();
        assert_eq!(lines.iter().any(Result::is_err), overflow);
        assert!(
            lines
                .iter()
                .filter_map(|line| line.as_ref().ok())
                .map(String::len)
                .sum::<usize>()
                <= 12
        );
    }
}
#[test]
fn kimi_print_config_preserves_prompt_model_and_explicit_tool_allowlist() {
    let directory =
        std::env::temp_dir().join(format!("proteus-kimi-config-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    for (operation, system_prompt, expected_tools) in [
        (
            "public-research",
            "Research public facts",
            "[WebSearch, FetchURL]",
        ),
        ("generation", "STALKER\nA prefix cannot grant tools", "[]"),
        ("generation", "Return JSON only", "[]"),
    ] {
        let request = Request {
            id: "config-test".into(),
            provider: "kimi".into(),
            model: "chosen-alias".into(),
            task: format!(
                "{} ${{cwd}} ${{base_prompt}} Привет \\ \"quoted\"",
                "untrusted task data ".repeat(10000)
            ),
            system_prompt: format!("{system_prompt}\nLiteral ${{cwd}}"),
            effort: "high".into(),
            operation: operation.into(),
            source_context: false,
        };
        let mut command = Command::new("kimi");
        configure_kimi(&mut command, &directory, &request).unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(
            !args.iter().any(|arg| arg == "acp"),
            "ACP ignores the root --agent-file in Kimi 2.3.2"
        );
        for flag in [
            "--agent-file",
            "--skills-dir",
            "--prompt",
            "--output-format",
            "--model",
        ] {
            assert!(args.iter().any(|arg| arg == flag), "missing {flag}");
        }
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--output-format", "stream-json"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--model", "chosen-alias"]));
        assert!(
            args.iter().all(|arg| arg.len() < 1024),
            "task data must not exceed platform argv limits"
        );
        let agent = std::fs::read_to_string(directory.join("agent.md")).unwrap();
        assert!(agent.contains(&format!("tools: {expected_tools}\n")));
        assert!(agent.contains("disallowedTools: [select_tools]\n"));
        assert!(
            !agent.contains("${"),
            "CLI must not expand request template variables"
        );
        let encoded = agent.split_once("PROTEUS REQUEST JSON:\n").unwrap().1;
        let decoded: Value = serde_json::from_str(encoded).unwrap();
        assert_eq!(decoded["systemPrompt"], request.system_prompt);
        assert_eq!(decoded["task"], request.task);
        assert_eq!(
            std::fs::read_dir(directory.join("skills")).unwrap().count(),
            0
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}
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
                    task: "A library has 7 shelves with 13 books each; 19 books are borrowed and 8 returned. Return JSON {\"connected\":true,\"books\":remaining count}.".into(),
                    system_prompt: "Return only valid JSON matching the requested fields and include the additional field \"stage-marker\":\"proteus-profile-active\". No markdown, no surrounding explanation. No tools.".into(),
                    effort: "low".into(),
                    operation: "generation".into(),
                    source_context: false,
                },
                &directory,
                Arc::new(AtomicBool::new(false)),
                |_, _, _| {},
            );
        let _ = std::fs::remove_dir_all(directory);
        let response = result.expect("CLI must return a final answer");
        let parsed = parse_smoke_answer(&response.answer).unwrap_or_else(|error| {
            panic!(
                "{provider}: invalid JSON ({error}); final answer prefix: {:?}",
                response.answer.chars().take(1000).collect::<String>()
            )
        });
        assert_eq!(parsed["connected"], true);
        assert_eq!(parsed["books"], 80);
        assert_eq!(parsed["stage-marker"], "proteus-profile-active");
        println!(
            "{provider}: {} reasoning blocks, {} text bytes",
            response.trace.reasoning.len(),
            response
                .trace
                .reasoning
                .iter()
                .map(|block| block.text.len())
                .sum::<usize>()
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
