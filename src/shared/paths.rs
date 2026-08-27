//! Platform-specific config directories shared by the MCP, instruction and
//! rules channels. One owner per directory, so a path fix lands everywhere.

use std::path::{Path, PathBuf};

/// VS Code's user profile directory, parent of `mcp.json`, `prompts/` and `settings.json`.
pub fn vscode_user_dir(home: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    let base = home.join("AppData").join("Roaming").join("Code");
    #[cfg(target_os = "macos")]
    let base = home
        .join("Library")
        .join("Application Support")
        .join("Code");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = home.join(".config").join("Code");

    base.join("User")
}

/// VS Code's user prompts directory, where Copilot reads `*.instructions.md`.
pub fn vscode_prompts_dir(home: &Path) -> PathBuf {
    vscode_user_dir(home).join("prompts")
}

/// Copilot CLI's config directory, honouring the documented `COPILOT_HOME` override.
pub fn copilot_home(home: &Path) -> PathBuf {
    match std::env::var_os("COPILOT_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home.join(".copilot"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompts_dir_sits_under_user_dir() {
        let home = Path::new("/home/dummy");
        assert_eq!(vscode_prompts_dir(home), vscode_user_dir(home).join("prompts"));
    }
}
