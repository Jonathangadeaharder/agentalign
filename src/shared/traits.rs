use crate::shared::error::Result;
use crate::shared::models::CanonicalWorkspaceState;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::Path;

/// Configuration adapter: translates between canonical and agent-native formats.
/// All methods accept an explicit `base_path` to avoid global filesystem dependencies.
pub trait ConfigurationAdapter {
    fn target_name(&self) -> &'static str;
    fn deserialize_to_canonical(&self, raw: &str, base_path: &Path) -> Result<JsonValue>;
    fn serialize_from_canonical(&self, canonical: &JsonValue, base_path: &Path) -> Result<String>;
    fn target_config_path(&self, base_path: &Path) -> std::path::PathBuf;
    fn normalize_env(&self, env: &HashMap<String, String>) -> HashMap<String, String>;
    fn extract_unknowns(&self, raw: &JsonValue) -> HashMap<String, JsonValue>;
}

/// Strategy for multi-client MCP format translation.
pub trait McpFormatStrategy: ConfigurationAdapter {
    /// Validate that a canonical config is compatible with this agent.
    fn validate(&self, state: &CanonicalWorkspaceState) -> Result<()>;

    /// Get the client's transport capabilities.
    fn capabilities(&self) -> crate::shared::models::ClientCapabilities;

    /// Generate a stdio bridge command if native transport isn't supported.
    fn stdio_bridge_command(&self, _url: &str) -> Option<Vec<String>> {
        None
    }
}

/// Trim analyzer: evaluates resource usage and identifies candidates for pruning.
pub trait TrimAnalyzer {
    /// Analyze usage and return a report of unused/abandoned resources.
    fn analyze_usage(&self, agents_root: &Path) -> Result<Vec<crate::shared::models::UnusedReport>>;

    /// Name of this analyzer (e.g., "skills", "mcp", "processes").
    fn analyzer_name(&self) -> &'static str;
}
