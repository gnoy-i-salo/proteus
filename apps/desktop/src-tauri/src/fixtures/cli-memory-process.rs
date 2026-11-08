//! Test-only provider process: no network, credentials or external tools.
use std::{
    fs,
    io::{self, Read},
    path::Path,
    time::Duration,
};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help") {
        println!("--ignore-user-config --ignore-rules --ephemeral");
        return;
    }
    if args.first().is_some_and(|arg| arg == "features") {
        for feature in [
            "shell_tool",
            "unified_exec",
            "multi_agent",
            "multi_agent_v2",
            "apps",
            "plugins",
            "remote_plugin",
            "hooks",
            "view_image",
            "computer_use",
            "browser_use",
            "browser_use_external",
            "in_app_browser",
            "image_generation",
            "artifact",
            "workspace_dependencies",
            "code_mode",
            "code_mode_host",
            "skill_search",
            "skill_mcp_dependency_install",
            "tool_suggest",
            "memories",
            "goals",
            "sleep_tool",
            "enable_mcp_apps",
            "skip_host_skill_discovery",
        ] {
            println!("{feature} stable false");
        }
        let binary = std::env::current_exe().unwrap();
        let directory = binary.parent().unwrap();
        fs::write(directory.join("probed"), "done").unwrap();
        if directory.join("wait-probe").exists() {
            loop {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        return;
    }
    let mut input = String::new();
    io::stdin()
        .take(1024 * 1024)
        .read_to_string(&mut input)
        .unwrap();
    fs::write("received.txt", input).unwrap();
    if Path::new("wait").exists() {
        loop {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    if Path::new("fail-exit").exists() {
        std::process::exit(1);
    }
    fs::write("answer-written", "done").unwrap();
    println!(
        "{}",
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"fixture answer"}}"#
    );
    println!("{}", r#"{"type":"turn.completed"}"#);
}
