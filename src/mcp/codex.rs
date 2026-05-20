//! Codex CLI MCP format strategy (TOML).
//!
//! Uses `toml` for comment-preserving TOML parsing.
//!
//! Format:
//! ```toml
//! [mcp_servers.server_name]
//! command = "npx"
//! args = ["package"]
//!
//! [mcp_servers.server_name.env]
//! KEY = "value"
//! ```

use agentalign_shared::error::{AdapterError, Result};
use agentalign_shared::models::{
    CanonicalWorkspaceState, ClientCapabilities,
};
use agentalign_shared::traits::{ConfigurationAdapter, McpFormatStrategy};
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::path::Path;

pub struct CodexStrategy;

impl ConfigurationAdapter for CodexStrategy {
    fn target_name(&self) -> &'static str {
        "codex"
    }

    fn deserialize_to_canonical(&self, raw: &str, _base_path: &Path) -> Result<JsonValue> {
        // Parse TOML using tomli for reading
        let parsed: toml_edit::DocumentMut = raw
            .parse()
            .map_err(|e: toml_edit::TomlError| AdapterError::TomlParse(e.to_string()))?;

        let mut canonical_servers = serde_json::Map::new();

        // Walk all tables matching mcp_servers.<name>
        for (key, table) in parsed.iter() {
            if key != "mcp_servers" {
                continue;
            }
            let tbl = table
                .as_table()
                .ok_or_else(|| AdapterError::Other("mcp_servers is not a table".into()))?;

            for (name, server_tbl) in tbl.iter() {
                let server = server_tbl
                    .as_table()
                    .ok_or_else(|| {
                        AdapterError::Other(format!(
                            "Server '{}' is not a table",
                            name
                        ))
                    })?;

                let mut entry = serde_json::Map::new();
                entry.insert("type".into(), json!("local"));

                let mut cmd_vec: Vec<String> = Vec::new();
                if let Some(cmd) = server.get("command").and_then(|v| v.as_str()) {
                    cmd_vec.push(cmd.to_string());
                }
                if let Some(args) = server.get("args").and_then(|v| v.as_array()) {
                    for arg in args {
                        if let Some(s) = arg.as_str() {
                            cmd_vec.push(s.to_string());
                        }
                    }
                }
                if !cmd_vec.is_empty() {
                    entry.insert("command".into(), json!(cmd_vec));
                }

                // Extract env sub-table
                if let Some(env_tbl) = server.get("env").and_then(|v| v.as_table()) {
                    let mut env_map = serde_json::Map::new();
                    for (env_k, env_v) in env_tbl.iter() {
                        if let Some(s) = env_v.as_str() {
                            env_map.insert(env_k.to_string(), json!(s));
                        }
                    }
                    if !env_map.is_empty() {
                        entry.insert("env".into(), JsonValue::Object(env_map));
                    }
                }

                // Collect extras
                let known = ["command", "args", "env", "type"];
                for (k, v) in server.iter() {
                    if !known.contains(&k) {
                        if let Some(s) = v.as_str() {
                            entry.insert(k.to_string(), json!(s));
                        } else if let Some(i) = v.as_integer() {
                            entry.insert(k.to_string(), json!(i));
                        } else if let Some(b) = v.as_bool() {
                            entry.insert(k.to_string(), json!(b));
                        } else if let Some(arr) = v.as_array() {
                            let jarr: Vec<JsonValue> = arr
                                .iter()
                                .map(|x| {
                                    if let Some(s) = x.as_str() {
                                        json!(s)
                                    } else if let Some(i) = x.as_integer() {
                                        json!(i)
                                    } else {
                                        json!(null)
                                    }
                                })
                                .collect();
                            entry.insert(k.to_string(), JsonValue::Array(jarr));
                        }
                    }
                }

                canonical_servers.insert(name.to_string(), JsonValue::Object(entry));
            }
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

        // Parse existing file to preserve comments, non-MCP tables, and formatting.
        // This is the critical format-preservation step — without it, every sync
        // would destroy user-authored comments and third-party settings.
        let mut doc: toml_edit::DocumentMut = if target_path.exists() {
            let existing = std::fs::read_to_string(&target_path)
                .map_err(AdapterError::Io)?;
            existing.parse::<toml_edit::DocumentMut>()
                .map_err(|e: toml_edit::TomlError| AdapterError::TomlParse(e.to_string()))?
        } else {
            toml_edit::DocumentMut::new()
        };

        // Remove stale mcp_servers entries before re-populating
        if doc.contains_key("mcp_servers") {
            doc.remove("mcp_servers");
        }

        // Build mcp_servers table from canonical entries using index syntax.
        // toml_edit's IndexMut auto-creates intermediate tables.
        for (name, entry) in mcp {
            let entry_obj = entry.as_object().ok_or_else(|| {
                AdapterError::Other(format!("Server '{}' is not an object", name))
            })?;

            let transport = entry_obj
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("local");

            if transport == "remote" {
                if let Some(url) = entry_obj.get("url").and_then(|v| v.as_str()) {
                    doc["mcp_servers"][name.as_str()]["url"] = toml_edit::value(url);
                }
            } else if let Some(cmd_arr) = entry_obj.get("command").and_then(|v| v.as_array()) {
                if let Some(first) = cmd_arr.first().and_then(|v| v.as_str()) {
                    doc["mcp_servers"][name.as_str()]["command"] = toml_edit::value(first);
                    if cmd_arr.len() > 1 {
                        let rest: Vec<&str> = cmd_arr[1..].iter().filter_map(|v| v.as_str()).collect();
                        let mut arr = toml_edit::Array::new();
                        for arg in rest {
                            arr.push(arg);
                        }
                        doc["mcp_servers"][name.as_str()]["args"] = toml_edit::value(arr);
                    }
                }
            }

            // Add env sub-table
            if let Some(env) = entry_obj.get("env").and_then(|v| v.as_object()) {
                for (ek, ev) in env {
                    if let Some(s) = ev.as_str() {
                        doc["mcp_servers"][name.as_str()]["env"][ek.as_str()] = toml_edit::value(s);
                    }
                }
            }
        }

        Ok(doc.to_string())
    }

    fn target_config_path(&self, base_path: &Path) -> std::path::PathBuf {
        base_path.join(".codex").join("config.toml")
    }

    fn normalize_env(&self, env: &HashMap<String, String>) -> HashMap<String, String> {
        // Codex uses ${VAR} syntax (same as canonical)
        env.clone()
    }

    fn extract_unknowns(&self, raw: &JsonValue) -> HashMap<String, JsonValue> {
        // TOML extras are handled differently — during parse we
        // already capture them in the extra fields per server.
        HashMap::new()
    }
}

impl McpFormatStrategy for CodexStrategy {
    fn validate(&self, state: &CanonicalWorkspaceState) -> Result<()> {
        // Codex server IDs must be valid TOML table keys
        for name in state.mcp.keys() {
            if name.is_empty() {
                return Err(AdapterError::Validation(
                    "Server ID cannot be empty".into(),
                ));
            }
            // TOML keys cannot contain brackets or periods for table names
            if name.contains('[') || name.contains(']') || name.contains('.') {
                return Err(AdapterError::Validation(format!(
                    "Server ID '{}' is not a valid TOML table key",
                    name
                )));
            }
        }
        Ok(())
    }

    fn capabilities(&self) -> ClientCapabilities {
        crate::mcp::capabilities::codex_capabilities()
    }
}
