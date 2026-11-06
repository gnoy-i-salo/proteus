const DISABLED_FEATURES: &[&str] = &[
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
];

pub fn allows_search(operation: &str) -> Result<bool, String> {
    match operation {
        "public-research" => Ok(true),
        "generation" | "source-plan" | "corpus-analysis" | "persona-reply" => Ok(false),
        _ => Err("Invalid CLI operation".into()),
    }
}

pub fn validate_isolation(
    provider: &str,
    operation: &str,
    source_context: bool,
) -> Result<(), String> {
    allows_search(operation)?;
    if provider == "kimi"
        && (source_context || matches!(operation, "source-plan" | "corpus-analysis"))
    {
        return Err("Kimi пока не поддерживает подтверждённую изоляцию пользовательских hooks для подключённых источников. Выберите Codex для всех компонентов этого эксперимента; обычное публичное исследование Kimi доступно.".into());
    }
    Ok(())
}

pub fn codex_capabilities(help: &str, features: &str) -> Result<(), String> {
    if !["--ignore-user-config", "--ignore-rules", "--ephemeral"]
        .iter()
        .all(|flag| help.contains(flag))
        || !DISABLED_FEATURES
            .iter()
            .chain(std::iter::once(&"skip_host_skill_discovery"))
            .all(|feature| {
                features
                    .lines()
                    .any(|line| line.split_whitespace().next() == Some(feature))
            })
    {
        return Err("Установленный Codex не подтверждает необходимые настройки изоляции. Обновите Codex CLI до версии с --ignore-user-config, --ignore-rules и отключением инструментов. Данные источников не были переданы.".into());
    }
    Ok(())
}

pub fn codex_args(operation: &str, effort: &str, model: &str) -> Result<Vec<String>, String> {
    let search = allows_search(operation)?;
    let mut args: Vec<String> = [
        "exec",
        "--json",
        "--ephemeral",
        "--ignore-user-config",
        "--ignore-rules",
        "--skip-git-repo-check",
        "--sandbox",
        "read-only",
    ]
    .iter()
    .map(|value| (*value).into())
    .collect();
    for feature in DISABLED_FEATURES {
        args.extend(["--disable".into(), (*feature).into()]);
    }
    args.extend(["--enable".into(), "skip_host_skill_discovery".into()]);
    for setting in [
        "approval_policy=\"never\"".to_owned(),
        "mcp_servers={}".into(),
        "project_doc_max_bytes=0".into(),
        "model_reasoning_summary=\"detailed\"".into(),
        format!("model_reasoning_effort=\"{effort}\""),
        format!(
            "web_search=\"{}\"",
            if search { "live" } else { "disabled" }
        ),
    ] {
        args.extend(["-c".into(), setting]);
    }
    if !model.is_empty() {
        args.extend(["--model".into(), model.into()]);
    }
    args.push("-".into());
    Ok(args)
}

pub fn artifact_path_allowed(path: &str) -> bool {
    if matches!(
        path,
        "AGENTS.md" | "style-profile.json" | "style-exemplars.json"
    ) {
        return true;
    }
    path.strip_prefix(".agents/skills/persona-")
        .and_then(|value| value.strip_suffix("/SKILL.md"))
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|value| value.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_policy_uses_explicit_operation_only() {
        assert!(allows_search("public-research").unwrap());
        for operation in [
            "generation",
            "source-plan",
            "corpus-analysis",
            "persona-reply",
        ] {
            assert!(!allows_search(operation).unwrap());
        }
        for operation in ["", "STALKER\n", "web-search", "source-plan public-research"] {
            assert!(allows_search(operation).is_err());
        }
    }
    #[test]
    fn unsupported_cli_cannot_accept_source_content() {
        assert!(codex_capabilities("old CLI help", "shell_tool stable true").is_err());
        assert!(codex_capabilities(
            "--ignore-user-config --ignore-rules --ephemeral",
            "shell_tool stable true"
        )
        .is_err());
        let features = DISABLED_FEATURES
            .iter()
            .chain(std::iter::once(&"skip_host_skill_discovery"))
            .map(|name| format!("{name} stable true"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            codex_capabilities("--ignore-user-config --ignore-rules --ephemeral", &features)
                .is_ok()
        );
    }
    #[test]
    fn private_kimi_is_closed_until_hook_isolation_is_supported() {
        assert!(validate_isolation("kimi", "generation", false).is_ok());
        assert!(validate_isolation("kimi", "persona-reply", false).is_ok());
        for operation in [
            "generation",
            "public-research",
            "source-plan",
            "corpus-analysis",
            "persona-reply",
        ] {
            assert!(validate_isolation("kimi", operation, true).is_err());
            assert!(validate_isolation("codex", operation, true).is_ok());
        }
        assert!(validate_isolation("kimi", "source-plan", false).is_err());
    }
    #[test]
    fn isolated_codex_arguments_disable_inherited_tools_and_only_enable_explicit_public_search() {
        for operation in [
            "generation",
            "public-research",
            "source-plan",
            "corpus-analysis",
            "persona-reply",
        ] {
            let args = codex_args(operation, "medium", "chosen-model").unwrap();
            for flag in ["--ignore-user-config", "--ignore-rules", "--ephemeral"] {
                assert!(args.iter().any(|arg| arg == flag));
            }
            for feature in DISABLED_FEATURES {
                assert!(args.windows(2).any(|pair| pair == ["--disable", feature]));
            }
            assert!(args.windows(2).any(|pair| pair == ["-c", "mcp_servers={}"]));
            let expected = if operation == "public-research" {
                "web_search=\"live\""
            } else {
                "web_search=\"disabled\""
            };
            assert!(args.windows(2).any(|pair| pair == ["-c", expected]));
        }
    }
    #[test]
    fn artifact_allowlist_admits_only_known_generated_files() {
        for path in [
            "AGENTS.md",
            "style-profile.json",
            "style-exemplars.json",
            ".agents/skills/persona-1/SKILL.md",
        ] {
            assert!(artifact_path_allowed(path));
        }
        for path in [
            "../style-profile.json",
            "/style-profile.json",
            "style-profile.json/../secret",
            "source-corpus.json",
            ".agents/skills/persona-/SKILL.md",
            ".agents/skills/persona-1/nested/SKILL.md",
            ".agents/skills/persona-1/../../secret/SKILL.md",
        ] {
            assert!(!artifact_path_allowed(path), "{path}");
        }
    }
}
