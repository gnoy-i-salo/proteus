use std::path::PathBuf;

pub(super) fn executable(provider: &str) -> Option<PathBuf> {
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
