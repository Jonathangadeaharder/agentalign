//! Grok CLI MCP format strategy (TOML).
//!
//! Uses `toml_edit` for comment-preserving TOML parsing.
//!
//! Format (same key structure as Codex, with `enabled` and `headers` sub-table):
//! ```toml
//! [mcp_servers.server_name]
//! command = "/path/to/command"
//! args = ["-y", "package"]
//! enabled = true
//!
//! [mcp_servers.remote_name]
//! url = "https://..."
//! enabled = true
//!
//! [mcp_servers.remote_name.headers]
//! Authorization = "Bearer ..."
//! ```

use crate::shared::error::{AdapterError, Result};
use crate::shared::models::{CanonicalWorkspaceState, ClientCapabilities};
use crate::shared::traits::{ConfigurationAdapter, McpFormatStrategy};
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::path::Path;

pub struct GrokStrategy;

impl ConfigurationAdapter for GrokStrategy {
    fn target_name(&self) -> &'static str {
        "grok"
    }

    fn deserialize_to_canonical(&self, raw: &str, _base_path: &Path) -> Result<JsonValue> {
        let parsed: toml_edit::DocumentMut = raw
            .parse()
            .map_err(|e: toml_edit::TomlError| AdapterError::TomlParse(e.to_string()))?;

        let mut canonical_servers = serde_json::Map::new();

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
                        AdapterError::Other(format!("Server '{}' is not a table", name))
                    })?;

                let mut entry = serde_json::Map::new();

                // Determine transport type
                if server.contains_key("url") {
                    entry.insert("type".into(), json!("remote"));
                } else {
                    entry.insert("type".into(), json!("local"));
                }

                // Collect command + args into command array
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

                // Extract url
                if let Some(url) = server.get("url").and_then(|v| v.as_str()) {
                    entry.insert("url".into(), json!(url));
                }

                // Extract headers sub-table
                if let Some(headers_tbl) = server.get("headers").and_then(|v| v.as_table()) {
                    let mut headers_map = serde_json::Map::new();
                    for (hk, hv) in headers_tbl.iter() {
                        if let Some(s) = hv.as_str() {
                            headers_map.insert(hk.to_string(), json!(s));
                        }
                    }
                    if !headers_map.is_empty() {
                        entry.insert("headers".into(), JsonValue::Object(headers_map));
                    }
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

                // Collect extras (enabled, etc.)
                let known = ["command", "args", "env", "headers", "url", "type", "enabled"];
                for (k, v) in server.iter() {
                    if !known.contains(&k) {
                        if let Some(s) = v.as_str() {
                            entry.insert(k.to_string(), json!(s));
                        } else if let Some(i) = v.as_integer() {
                            entry.insert(k.to_string(), json!(i));
                        } else if let Some(b) = v.as_bool() {
                            entry.insert(k.to_string(), json!(b));
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
        let mut doc: toml_edit::DocumentMut = if target_path.exists() {
            let existing = std::fs::read_to_string(&target_path).map_err(AdapterError::Io)?;
            existing
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e: toml_edit::TomlError| AdapterError::TomlParse(e.to_string()))?
        } else {
            toml_edit::DocumentMut::new()
        };

        // Remove stale mcp_servers entries before re-populating
        if doc.contains_key("mcp_servers") {
            doc.remove("mcp_servers");
        }

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
                // Headers sub-table
                if let Some(headers) = entry_obj.get("headers").and_then(|v| v.as_object()) {
                    for (hk, hv) in headers {
                        if let Some(s) = hv.as_str() {
                            doc["mcp_servers"][name.as_str()]["headers"][hk.as_str()] =
                                toml_edit::value(s);
                        }
                    }
                }
            } else if let Some(cmd_arr) = entry_obj.get("command").and_then(|v| v.as_array()) {
                if let Some(first) = cmd_arr.first().and_then(|v| v.as_str()) {
                    doc["mcp_servers"][name.as_str()]["command"] = toml_edit::value(first);
                    if cmd_arr.len() > 1 {
                        let rest: Vec<&str> =
                            cmd_arr[1..].iter().filter_map(|v| v.as_str()).collect();
                        let mut arr = toml_edit::Array::new();
                        for arg in rest {
                            arr.push(arg);
                        }
                        doc["mcp_servers"][name.as_str()]["args"] = toml_edit::value(arr);
                    } else {
                        // Grok expects args even if empty
                        doc["mcp_servers"][name.as_str()]["args"] =
                            toml_edit::value(toml_edit::Array::new());
                    }
                }
            }

            // Grok always sets enabled = true
            doc["mcp_servers"][name.as_str()]["enabled"] = toml_edit::value(true);

            // Env sub-table
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
        base_path.join(".grok").join("config.toml")
    }

    fn normalize_env(&self, env: &HashMap<String, String>) -> HashMap<String, String> {
        env.clone()
    }

    fn extract_unknowns(&self, _raw: &JsonValue) -> HashMap<String, JsonValue> {
        HashMap::new()
    }
}

impl McpFormatStrategy for GrokStrategy {
    fn validate(&self, state: &CanonicalWorkspaceState) -> Result<()> {
        for name in state.mcp.keys() {
            if name.is_empty() {
                return Err(AdapterError::Validation("Server ID cannot be empty".into()));
            }
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
        ClientCapabilities {
            name: "grok".to_string(),
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
