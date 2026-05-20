//! Claude Desktop (and Cursor) MCP format strategy.
//!
//! Format (JSON):
//! ```json
//! {
//!   "mcpServers": {
//!     "server_name": {
//!       "command": "npx",
//!       "args": ["@browsermcp/mcp"],
//!       "url": "https://..."
//!     }
//!   }
//! }
//! ```
//!
//! Remote servers use a bare `url` field (no explicit transport type).
//! `env` section is **not** supported by Claude Desktop (returns empty).

use agentalign_shared::error::{AdapterError, Result};
use agentalign_shared::models::{CanonicalWorkspaceState, ClientCapabilities};
use agentalign_shared::traits::{ConfigurationAdapter, McpFormatStrategy};
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::path::Path;

pub struct ClaudeStrategy {
    /// Whether this is Cursor (which has `/` and `\` restrictions).
    pub is_cursor: bool,
}

impl Default for ClaudeStrategy {
    fn default() -> Self {
        Self { is_cursor: false }
    }
}

impl ConfigurationAdapter for ClaudeStrategy {
    fn target_name(&self) -> &'static str {
        if self.is_cursor {
            "cursor"
        } else {
            "claude"
        }
    }

    fn deserialize_to_canonical(&self, raw: &str, _base_path: &Path) -> Result<JsonValue> {
        let raw_val: JsonValue = serde_json::from_str(raw)?;

        let servers = raw_val
            .get("mcpServers")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                AdapterError::Other("Missing 'mcpServers' key in config".into())
            })?;

        let mut canonical_servers = serde_json::Map::new();

        for (name, entry) in servers {
            let entry_obj = entry.as_object().ok_or_else(|| {
                AdapterError::Other(format!("Server '{}' is not an object", name))
            })?;

            let mut server = serde_json::Map::new();

            // Determine transport type
            if entry_obj.contains_key("url") {
                server.insert(
                    "type".into(),
                    json!("remote"),
                );
                if let Some(url) = entry_obj.get("url") {
                    server.insert("url".into(), url.clone());
                }
            } else {
                server.insert(
                    "type".into(),
                    json!("local"),
                );
            }

            // Split command/args back into command array
            if let Some(cmd) = entry_obj.get("command") {
                let mut cmd_vec = vec![cmd.as_str().unwrap_or("").to_string()];
                if let Some(args) = entry_obj.get("args").and_then(|v| v.as_array()) {
                    for arg in args {
                        cmd_vec.push(arg.as_str().unwrap_or("").to_string());
                    }
                }
                server.insert("command".into(), json!(cmd_vec));
            }

            // Preserve unknown fields via flatten
            let known_keys = ["command", "args", "url", "type"];
            let mut extra = serde_json::Map::new();
            for (k, v) in entry_obj {
                if !known_keys.contains(&k.as_str()) {
                    extra.insert(k.clone(), v.clone());
                }
            }
            if !extra.is_empty() {
                for (k, v) in extra {
                    server.insert(k, v);
                }
            }

            canonical_servers.insert(name.clone(), JsonValue::Object(server));
        }

        let mut root = serde_json::Map::new();
        root.insert("mcp".into(), JsonValue::Object(canonical_servers));
        Ok(JsonValue::Object(root))
    }

    fn serialize_from_canonical(&self, canonical: &JsonValue, _base_path: &Path) -> Result<String> {
        let mcp = canonical
            .get("mcp")
            .and_then(|v| v.as_object())
            .ok_or_else(|| AdapterError::Other("Missing 'mcp' key in canonical".into()))?;

        let mut servers = serde_json::Map::new();

        for (name, entry) in mcp {
            let entry_obj = entry.as_object().ok_or_else(|| {
                AdapterError::Other(format!("Server '{}' is not an object", name))
            })?;

            let mut agent_entry = serde_json::Map::new();

            // Determine transport
            let transport = entry_obj
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("local");

            if transport == "remote" {
                if let Some(url) = entry_obj.get("url") {
                    agent_entry.insert("url".into(), url.clone());
                }
            } else {
                // local: split command[0] → command, rest → args
                if let Some(cmd_arr) = entry_obj.get("command").and_then(|v| v.as_array()) {
                    if let Some(first) = cmd_arr.first().and_then(|v| v.as_str()) {
                        agent_entry.insert("command".into(), json!(first));
                        if cmd_arr.len() > 1 {
                            let args: Vec<JsonValue> = cmd_arr[1..]
                                .iter()
                                .map(|v| v.clone())
                                .collect();
                            agent_entry.insert("args".into(), JsonValue::Array(args));
                        }
                    }
                }
            }

            // Merge extras (flattened)
            for (k, v) in entry_obj {
                let known = ["type", "command", "url", "headers", "env", "enabled"];
                if !known.contains(&k.as_str()) {
                    agent_entry.insert(k.clone(), v.clone());
                }
            }

            servers.insert(name.clone(), JsonValue::Object(agent_entry));
        }

        let mut root = serde_json::Map::new();
        root.insert(
            "mcpServers".into(),
            JsonValue::Object(servers),
        );
        Ok(serde_json::to_string_pretty(&JsonValue::Object(root))?)
    }

    fn target_config_path(&self, base_path: &Path) -> std::path::PathBuf {
        if self.is_cursor {
            base_path.join(".cursor").join("mcp.json")
        } else {
            base_path.join(".claude").join(".mcp.json")
        }
    }

    fn normalize_env(&self, env: &HashMap<String, String>) -> HashMap<String, String> {
        // Claude Desktop does not support env section — return empty
        // Callers should warn/prevent sync of env vars to Claude.
        HashMap::new()
    }

    fn extract_unknowns(&self, raw: &JsonValue) -> HashMap<String, JsonValue> {
        let mut result = HashMap::new();
        if let Some(servers) = raw.get("mcpServers").and_then(|v| v.as_object()) {
            let known = ["command", "args", "url"];
            for (name, entry) in servers {
                if let Some(obj) = entry.as_object() {
                    let mut extras = serde_json::Map::new();
                    for (k, v) in obj {
                        if !known.contains(&k.as_str()) {
                            extras.insert(k.clone(), v.clone());
                        }
                    }
                    if !extras.is_empty() {
                        result.insert(name.clone(), JsonValue::Object(extras));
                    }
                }
            }
        }
        result
    }
}

impl McpFormatStrategy for ClaudeStrategy {
    fn validate(&self, state: &CanonicalWorkspaceState) -> Result<()> {
        if self.is_cursor {
            let forbidden = ['/', '\\'];
            for name in state.mcp.keys() {
                if name.chars().any(|c| forbidden.contains(&c)) {
                    return Err(AdapterError::Validation(format!(
                        "Cursor server ID '{}' contains forbidden character",
                        name
                    )));
                }
            }
        }
        Ok(())
    }

    fn capabilities(&self) -> ClientCapabilities {
        if self.is_cursor {
            crate::mcp::capabilities::cursor_capabilities()
        } else {
            crate::mcp::capabilities::claude_capabilities()
        }
    }

    fn stdio_bridge_command(&self, _url: &str) -> Option<Vec<String>> {
        // Claude and Cursor support SSE natively, no bridge needed
        None
    }
}
