//! ZCode MCP format strategy.
//!
//! ZCode stores MCP servers in `~/.zcode/cli/config.json` under a nested
//! `mcp.servers` key:
//! ```json
//! {
//!   "model": { "provider": "...", "model": "..." },
//!   "providers": { ... },
//!   "mcp": {
//!     "servers": {
//!       "server_name": {
//!         "command": ["npx", "-y", "..."],
//!         "url": "https://...",
//!         "type": "local"
//!       }
//!     }
//!   }
//! }
//! ```
//!
//! The strategy read-modify-writes the config file, preserving all non-MCP
//! sections (`model`, `providers`, etc.) and replacing only `mcp.servers`.

use crate::shared::error::{AdapterError, Result};
use crate::shared::models::{CanonicalWorkspaceState, ClientCapabilities};
use crate::shared::traits::{ConfigurationAdapter, McpFormatStrategy};
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::path::Path;

pub struct ZCodeStrategy;

impl ConfigurationAdapter for ZCodeStrategy {
    fn target_name(&self) -> &'static str {
        "zcode"
    }

    fn deserialize_to_canonical(&self, raw: &str, _base_path: &Path) -> Result<JsonValue> {
        let raw_val: JsonValue = serde_json::from_str(raw)?;

        let servers = raw_val
            .get("mcp")
            .and_then(|v| v.get("servers"))
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                AdapterError::Other("Missing 'mcp.servers' key in config".into())
            })?;

        let mut canonical_servers = serde_json::Map::new();

        for (name, entry) in servers {
            let entry_obj = entry.as_object().ok_or_else(|| {
                AdapterError::Other(format!("Server '{}' is not an object", name))
            })?;

            let mut server = serde_json::Map::new();

            // Preserve type if present, else infer
            if let Some(t) = entry_obj.get("type").and_then(|v| v.as_str()) {
                server.insert("type".into(), json!(t));
            } else if entry_obj.contains_key("url") {
                server.insert("type".into(), json!("remote"));
            } else {
                server.insert("type".into(), json!("local"));
            }

            // Copy url, headers, enabled directly
            for key in ["url", "headers", "enabled"] {
                if let Some(v) = entry_obj.get(key) {
                    server.insert(key.into(), v.clone());
                }
            }

            // Copy command array directly
            if let Some(cmd) = entry_obj.get("command") {
                server.insert("command".into(), cmd.clone());
            }

            // Preserve unknown fields
            let known_keys = ["type", "command", "url", "headers", "enabled"];
            let mut extra = serde_json::Map::new();
            for (k, v) in entry_obj {
                if !known_keys.contains(&k.as_str()) {
                    extra.insert(k.clone(), v.clone());
                }
            }
            if !extra.is_empty() {
                server.insert("extra".into(), JsonValue::Object(extra));
            }

            canonical_servers.insert(name.clone(), JsonValue::Object(server));
        }

        let mut root = serde_json::Map::new();
        root.insert("mcp".into(), JsonValue::Object(canonical_servers));
        Ok(JsonValue::Object(root))
    }

    fn serialize_from_canonical(&self, canonical: &JsonValue, base_path: &Path) -> Result<String> {
        let mcp = canonical
            .get("mcp")
            .and_then(|v| v.as_object())
            .ok_or_else(|| AdapterError::Other("Missing 'mcp' key in canonical".into()))?;

        let target_path = self.target_config_path(base_path);

        // Parse existing file to preserve non-MCP sections (model, providers, etc.)
        let mut doc: JsonValue = if target_path.exists() {
            let existing = std::fs::read_to_string(&target_path)
                .map_err(AdapterError::Io)?;
            serde_json::from_str(&existing)
                .map_err(AdapterError::Serialization)?
        } else {
            json!({})
        };

        // Build new mcp.servers section
        let mut mcp_servers = serde_json::Map::new();
        for (name, entry) in mcp {
            let entry_obj = entry.as_object().ok_or_else(|| {
                AdapterError::Other(format!("Server '{}' is not an object", name))
            })?;

            let mut server = serde_json::Map::new();

            // Copy type
            if let Some(t) = entry_obj.get("type").and_then(|v| v.as_str()) {
                server.insert("type".into(), json!(t));
            }

            // Copy command array
            if let Some(cmd) = entry_obj.get("command") {
                server.insert("command".into(), cmd.clone());
            }

            // Copy url, headers, enabled
            for key in ["url", "headers", "enabled"] {
                if let Some(v) = entry_obj.get(key) {
                    server.insert(key.into(), v.clone());
                }
            }

            // Restore extras
            if let Some(extra) = entry_obj.get("extra").and_then(|v| v.as_object()) {
                for (k, v) in extra {
                    server.insert(k.clone(), v.clone());
                }
            }

            mcp_servers.insert(name.clone(), JsonValue::Object(server));
        }

        // Replace mcp.servers while preserving rest of file
        if let Some(obj) = doc.as_object_mut() {
            let mcp_section = obj
                .entry("mcp".to_string())
                .or_insert_with(|| json!({}));
            if let Some(mcp_obj) = mcp_section.as_object_mut() {
                mcp_obj.insert("servers".into(), JsonValue::Object(mcp_servers));
            }
        }

        Ok(serde_json::to_string_pretty(&doc)?)
    }

    fn target_config_path(&self, base_path: &Path) -> std::path::PathBuf {
        base_path.join(".zcode").join("cli").join("config.json")
    }

    fn normalize_env(&self, env: &HashMap<String, String>) -> HashMap<String, String> {
        env.clone()
    }

    fn extract_unknowns(&self, raw: &JsonValue) -> HashMap<String, JsonValue> {
        let mut unknowns = HashMap::new();
        if let Some(obj) = raw.as_object() {
            for (k, v) in obj {
                if k != "mcp" {
                    unknowns.insert(k.clone(), v.clone());
                }
            }
        }
        unknowns
    }
}

impl McpFormatStrategy for ZCodeStrategy {
    fn validate(&self, state: &CanonicalWorkspaceState) -> Result<()> {
        for name in state.mcp.keys() {
            if name.is_empty() {
                return Err(AdapterError::Validation(
                    "Server ID cannot be empty".into(),
                ));
            }
        }
        Ok(())
    }

    fn capabilities(&self) -> ClientCapabilities {
        // ZCode supports local (stdio) and remote (SSE) transports.
        ClientCapabilities {
            name: "zcode".to_string(),
            supports_stdio: true,
            supports_sse: true,
            supports_http: true,
            supports_env_section: true,
            placeholder_style: crate::shared::models::PlaceholderStyle::EnvDollarBrace,
            max_id_length: None,
            forbidden_id_chars: vec![],
            requires_security_sandbox: false,
        }
    }
}
