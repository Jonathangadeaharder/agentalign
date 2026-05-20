//! Canonical (identity) strategy — reads and writes the OpenCode canonical format directly.
//!
//! Format:
//! ```json
//! { "mcp": { "server_name": { "type": "local", "command": ["npx", "..."], ... } } }
//! ```

use agentalign_shared::error::Result;
use agentalign_shared::models::{CanonicalWorkspaceState, ClientCapabilities};
use agentalign_shared::traits::{ConfigurationAdapter, McpFormatStrategy};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::Path;

pub struct CanonicalStrategy;

impl ConfigurationAdapter for CanonicalStrategy {
    fn target_name(&self) -> &'static str {
        "canonical"
    }

    fn deserialize_to_canonical(&self, raw: &str, _base_path: &Path) -> Result<JsonValue> {
        let value: JsonValue = serde_json::from_str(raw)?;
        Ok(value)
    }

    fn serialize_from_canonical(&self, canonical: &JsonValue, _base_path: &Path) -> Result<String> {
        Ok(serde_json::to_string_pretty(canonical)?)
    }

    fn target_config_path(&self, base_path: &Path) -> std::path::PathBuf {
        base_path.join(".agents").join("mcp_config.json")
    }

    fn normalize_env(&self, env: &HashMap<String, String>) -> HashMap<String, String> {
        env.clone()
    }

    fn extract_unknowns(&self, _raw: &JsonValue) -> HashMap<String, JsonValue> {
        HashMap::new()
    }
}

impl McpFormatStrategy for CanonicalStrategy {
    fn validate(&self, _state: &CanonicalWorkspaceState) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> ClientCapabilities {
        ClientCapabilities {
            name: "canonical".into(),
            supports_stdio: true,
            supports_sse: true,
            supports_http: true,
            supports_env_section: true,
            placeholder_style: agentalign_shared::models::PlaceholderStyle::DollarBrace,
            max_id_length: None,
            forbidden_id_chars: vec![],
            requires_security_sandbox: false,
        }
    }
}
