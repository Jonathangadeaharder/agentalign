//! VS Code MCP format strategy.
//!
//! Format (JSON):
//! ```json
//! {
//!   "servers": {
//!     "server_name": {
//!       "command": "npx",
//!       "args": ["@browsermcp/mcp"],
//!       "url": "https://..."
//!     }
//!   }
//! }
//! ```
//!
//! Root key is `"servers"` (not `"mcpServers"`).
//! Supports `${env:VAR}` and `${input:name}` placeholder syntax.

use agentalign_shared::error::{AdapterError, Result};
use agentalign_shared::models::{
    CanonicalWorkspaceState, ClientCapabilities, PlaceholderStyle,
};
use agentalign_shared::traits::{ConfigurationAdapter, McpFormatStrategy};
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::path::Path;

pub struct VSCodeStrategy;

impl ConfigurationAdapter for VSCodeStrategy {
    fn target_name(&self) -> &'static str {
        "vscode"
    }

    fn deserialize_to_canonical(&self, raw: &str, _base_path: &Path) -> Result<JsonValue> {
        let raw_val: JsonValue = serde_json::from_str(raw)?;

        let servers = raw_val
            .get("servers")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                AdapterError::Other("Missing 'servers' key in VS Code config".into())
            })?;

        let mut canonical_servers = serde_json::Map::new();

        for (name, entry) in servers {
            let entry_obj = entry.as_object().ok_or_else(|| {
                AdapterError::Other(format!("Server '{}' is not an object", name))
            })?;

            let mut server = serde_json::Map::new();

            if entry_obj.contains_key("url") {
                server.insert("type".into(), json!("remote"));
                if let Some(url) = entry_obj.get("url") {
                    server.insert("url".into(), url.clone());
                }
            } else {
                server.insert("type".into(), json!("local"));
            }

            if let Some(cmd) = entry_obj.get("command").and_then(|v| v.as_str()) {
                let mut cmd_vec = vec![cmd.to_string()];
                if let Some(args) = entry_obj.get("args").and_then(|v| v.as_array()) {
                    for arg in args {
                        cmd_vec.push(arg.as_str().unwrap_or("").to_string());
                    }
                }
                server.insert("command".into(), json!(cmd_vec));
            }

            // Flatten extras
            let known = ["command", "args", "url", "type", "env"];
            for (k, v) in entry_obj {
                if !known.contains(&k.as_str()) {
                    server.insert(k.clone(), v.clone());
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
            let transport = entry_obj
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("local");

            if transport == "remote" {
                if let Some(url) = entry_obj.get("url") {
                    agent_entry.insert("url".into(), url.clone());
                }
            } else if let Some(cmd_arr) = entry_obj.get("command").and_then(|v| v.as_array()) {
                if let Some(first) = cmd_arr.first().and_then(|v| v.as_str()) {
                    agent_entry.insert("command".into(), json!(first));
                    if cmd_arr.len() > 1 {
                        let args: Vec<JsonValue> = cmd_arr[1..].iter().map(|v| v.clone()).collect();
                        agent_entry.insert("args".into(), JsonValue::Array(args));
                    }
                }
            }

            // Merge extras
            let known = ["type", "command", "url", "headers", "env", "enabled"];
            for (k, v) in entry_obj {
                if !known.contains(&k.as_str()) {
                    agent_entry.insert(k.clone(), v.clone());
                }
            }

            servers.insert(name.clone(), JsonValue::Object(agent_entry));
        }

        let mut root = serde_json::Map::new();
        root.insert("servers".into(), JsonValue::Object(servers));
        Ok(serde_json::to_string_pretty(&JsonValue::Object(root))?)
    }

    fn target_config_path(&self, base_path: &Path) -> std::path::PathBuf {
        // VS Code stores MCP config in global storage
        base_path.join(".vscode")
            .join("globalStorage")
            .join(".mcp.json")
    }

    fn normalize_env(&self, env: &HashMap<String, String>) -> HashMap<String, String> {
        // Normalize `${VAR}` → `${env:VAR}` and `$VAR` → `${env:VAR}`
        use crate::mcp::interpolation;
        let vs_style = PlaceholderStyle::EnvDollarBrace;
        env.iter()
            .map(|(k, v)| (k.clone(), interpolation::normalize_value(v, &vs_style)))
            .collect()
    }

    fn extract_unknowns(&self, raw: &JsonValue) -> HashMap<String, JsonValue> {
        let mut result = HashMap::new();
        if let Some(servers) = raw.get("servers").and_then(|v| v.as_object()) {
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

impl McpFormatStrategy for VSCodeStrategy {
    fn validate(&self, _state: &CanonicalWorkspaceState) -> Result<()> {
        // VS Code has no special server ID restrictions
        Ok(())
    }

    fn capabilities(&self) -> ClientCapabilities {
        crate::mcp::capabilities::vscode_capabilities()
    }
}
