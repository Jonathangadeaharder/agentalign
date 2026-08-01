//! Qwen Code MCP format strategy.
//!
//! Qwen Code is a fork of the Gemini CLI and uses the same gemini-family JSON
//! format (top-level `mcpServers`). Unlike Gemini it stores MCP servers inside
//! the shared `~/.qwen/settings.json` together with non-MCP sections (`model`,
//! `permissions`, `ui`, ...). Format conversion therefore delegates to
//! `GeminiStrategy`, while serialization read-modify-writes the settings file
//! so non-MCP sections survive sync.

use crate::mcp::gemini::GeminiStrategy;
use crate::shared::error::{AdapterError, Result};
use crate::shared::models::{CanonicalWorkspaceState, ClientCapabilities};
use crate::shared::traits::{ConfigurationAdapter, McpFormatStrategy};
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::path::Path;

#[derive(Default)]
pub struct QwenStrategy {
    inner: GeminiStrategy,
}

impl ConfigurationAdapter for QwenStrategy {
    fn target_name(&self) -> &'static str {
        "qwen"
    }

    fn deserialize_to_canonical(&self, raw: &str, base_path: &Path) -> Result<JsonValue> {
        self.inner.deserialize_to_canonical(raw, base_path)
    }

    fn serialize_from_canonical(&self, canonical: &JsonValue, base_path: &Path) -> Result<String> {
        // Gemini format conversion yields a bare {"mcpServers": ...} document.
        let mcp_doc: JsonValue =
            serde_json::from_str(&self.inner.serialize_from_canonical(canonical, base_path)?)?;
        let mcp_servers = mcp_doc.get("mcpServers").cloned().unwrap_or(json!({}));

        // Parse existing file to preserve non-MCP sections (model, ui, etc.)
        let target_path = self.target_config_path(base_path);
        let mut doc: JsonValue = if target_path.exists() {
            let existing = std::fs::read_to_string(&target_path).map_err(AdapterError::Io)?;
            serde_json::from_str(&existing).map_err(AdapterError::Serialization)?
        } else {
            json!({})
        };

        if let Some(obj) = doc.as_object_mut() {
            obj.insert("mcpServers".into(), mcp_servers);
        }

        Ok(serde_json::to_string_pretty(&doc)?)
    }

    fn target_config_path(&self, base_path: &Path) -> std::path::PathBuf {
        base_path.join(".qwen").join("settings.json")
    }

    fn normalize_env(&self, env: &HashMap<String, String>) -> HashMap<String, String> {
        self.inner.normalize_env(env)
    }

    fn extract_unknowns(&self, raw: &JsonValue) -> HashMap<String, JsonValue> {
        self.inner.extract_unknowns(raw)
    }
}

impl McpFormatStrategy for QwenStrategy {
    fn validate(&self, state: &CanonicalWorkspaceState) -> Result<()> {
        self.inner.validate(state)
    }

    fn capabilities(&self) -> ClientCapabilities {
        crate::mcp::capabilities::gemini_capabilities()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::traits::ConfigurationAdapter;

    fn canonical_doc() -> JsonValue {
        json!({
            "mcp": {
                "srv": {
                    "type": "local",
                    "command": ["npx", "-y", "some-server"]
                }
            }
        })
    }

    #[test]
    fn test_qwen_target_name() {
        assert_eq!(QwenStrategy::default().target_name(), "qwen");
    }

    #[test]
    fn test_qwen_target_config_path() {
        let base = Path::new("/home/user");
        assert_eq!(
            QwenStrategy::default().target_config_path(base),
            base.join(".qwen").join("settings.json")
        );
    }

    #[test]
    fn test_serialize_produces_mcp_servers_without_existing_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let out = QwenStrategy::default()
            .serialize_from_canonical(&canonical_doc(), tmp.path())
            .unwrap();
        let doc: JsonValue = serde_json::from_str(&out).unwrap();
        let srv = &doc["mcpServers"]["srv"];
        assert_eq!(srv["command"], "npx");
        assert_eq!(srv["args"], json!(["-y", "some-server"]));
    }

    #[test]
    fn test_serialize_preserves_non_mcp_sections() {
        let tmp = tempfile::TempDir::new().unwrap();
        let settings = tmp.path().join(".qwen").join("settings.json");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(
            &settings,
            r#"{"model": "qwen3-coder", "mcpServers": {"old": {"command": "x"}}}"#,
        )
        .unwrap();

        let out = QwenStrategy::default()
            .serialize_from_canonical(&canonical_doc(), tmp.path())
            .unwrap();
        let doc: JsonValue = serde_json::from_str(&out).unwrap();
        assert_eq!(doc["model"], "qwen3-coder");
        assert!(doc["mcpServers"].get("old").is_none());
        assert!(doc["mcpServers"].get("srv").is_some());
    }

    #[test]
    fn test_deserialize_round_trip() {
        let raw = r#"{"mcpServers": {"srv": {"command": "npx", "args": ["-y", "pkg"]}}}"#;
        let canonical = QwenStrategy::default()
            .deserialize_to_canonical(raw, Path::new("/home/user"))
            .unwrap();
        assert_eq!(
            canonical["mcp"]["srv"]["command"],
            json!(["npx", "-y", "pkg"])
        );
    }
}
