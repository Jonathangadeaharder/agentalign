//! Zed MCP format strategy.
//!
//! Embedded under `"context_servers"` key inside the general Zed settings.json.
//!
//! Format (inside ~/.config/zed/settings.json):
//! ```json
//! {
//!   "context_servers": {
//!     "server_name": {
//!       "command": "npx",
//!       "args": ["..."],
//!       "env": { "KEY": "${env:VALUE}" }
//!     }
//!   }
//! }
//! ```
//!
//! Zed supports env maps and uses `${env:VAR}` placeholder syntax.

use agentalign_shared::error::{AdapterError, Result};
use agentalign_shared::models::{
    CanonicalWorkspaceState, ClientCapabilities, PlaceholderStyle,
};
use agentalign_shared::traits::{ConfigurationAdapter, McpFormatStrategy};
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::path::Path;

pub struct ZedStrategy;

impl ConfigurationAdapter for ZedStrategy {
    fn target_name(&self) -> &'static str {
        "zed"
    }

    fn deserialize_to_canonical(&self, raw: &str, _base_path: &Path) -> Result<JsonValue> {
        let raw_val: JsonValue = serde_json::from_str(raw)?;

        let servers = raw_val
            .get("context_servers")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                AdapterError::Other("Missing 'context_servers' key in Zed config".into())
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

            // Copy env section
            if let Some(env) = entry_obj.get("env") {
                server.insert("env".into(), env.clone());
            }

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

        // Preserve other Zed settings keys
        if let Some(raw_obj) = raw_val.as_object() {
            for (k, v) in raw_obj {
                if k != "context_servers" {
                    root.insert(format!("__zed_{}", k), v.clone());
                }
            }
        }

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

            // Copy env section
            if let Some(env) = entry_obj.get("env") {
                agent_entry.insert("env".into(), env.clone());
            }

            let known = ["type", "command", "url", "headers", "env", "enabled"];
            for (k, v) in entry_obj {
                if !known.contains(&k.as_str()) {
                    agent_entry.insert(k.clone(), v.clone());
                }
            }

            servers.insert(name.clone(), JsonValue::Object(agent_entry));
        }

        let mut root = serde_json::Map::new();
        root.insert("context_servers".into(), JsonValue::Object(servers));

        // Restore preserved Zed settings
        if let Some(canon_obj) = canonical.as_object() {
            for (k, v) in canon_obj {
                if k.starts_with("__zed_") {
                    let original_key = k.trim_start_matches("__zed_");
                    root.insert(original_key.to_string(), v.clone());
                }
            }
        }

        Ok(serde_json::to_string_pretty(&JsonValue::Object(root))?)
    }

    fn target_config_path(&self, base_path: &Path) -> std::path::PathBuf {
        base_path.join(".config").join("zed").join("settings.json")
    }

    fn normalize_env(&self, env: &HashMap<String, String>) -> HashMap<String, String> {
        // Zed uses ${env:VAR} syntax
        use crate::mcp::interpolation;
        let zed_style = PlaceholderStyle::EnvDollarBrace;
        env.iter()
            .map(|(k, v)| (k.clone(), interpolation::normalize_value(v, &zed_style)))
            .collect()
    }

    fn extract_unknowns(&self, raw: &JsonValue) -> HashMap<String, JsonValue> {
        let mut result = HashMap::new();
        if let Some(servers) = raw.get("context_servers").and_then(|v| v.as_object()) {
            let known = ["command", "args", "url", "env"];
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

impl McpFormatStrategy for ZedStrategy {
    fn validate(&self, _state: &CanonicalWorkspaceState) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> ClientCapabilities {
        crate::mcp::capabilities::zed_capabilities()
    }
}
