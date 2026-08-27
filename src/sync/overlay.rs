//! Non-destructive write preparation for agent config targets.
//!
//! Several targets are shared settings files (`~/.claude.json`,
//! `~/.gemini/settings.json`, `~/.qwen/settings.json`, VS Code's `mcp.json`), so a
//! whole-file replace would discard unrelated user settings. TOML targets merge
//! in-place inside their own strategy and pass through untouched.

use std::path::Path;

/// Overlay a serialized config onto the existing file, keeping every top-level
/// key the strategy did not emit. TOML targets are returned unchanged.
pub fn overlay_onto_existing(output: &str, target_path: &Path) -> String {
    if target_path.extension().and_then(|e| e.to_str()) == Some("toml") {
        return output.to_string();
    }
    if !target_path.exists() {
        return output.to_string();
    }

    let emitted = match serde_json::from_str::<serde_json::Value>(output) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => return output.to_string(),
    };

    let mut merged = match std::fs::read_to_string(target_path)
        .map(|raw| serde_json::from_str::<serde_json::Value>(&raw))
    {
        Ok(Ok(serde_json::Value::Object(map))) => map,
        _ => return output.to_string(),
    };

    for (key, value) in emitted {
        merged.insert(key, value);
    }

    serde_json::to_string_pretty(&serde_json::Value::Object(merged))
        .unwrap_or_else(|_| output.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unrelated_top_level_keys_survive_an_overlay() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("settings.json");
        std::fs::write(&target, r#"{"theme":"dark","mcpServers":{"old":{}}}"#).expect("write");

        let merged = overlay_onto_existing(r#"{"mcpServers":{}}"#, &target);

        let parsed: serde_json::Value = serde_json::from_str(&merged).expect("parse");
        assert_eq!(parsed["theme"], "dark");
        assert!(parsed["mcpServers"].as_object().expect("servers").is_empty());
    }

    #[test]
    fn test_toml_target_passes_through_unchanged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("config.toml");
        std::fs::write(&target, "model = \"gpt\"\n").expect("write");

        let output = "[mcp_servers]\n";

        assert_eq!(overlay_onto_existing(output, &target), output);
    }
}
