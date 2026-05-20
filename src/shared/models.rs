use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Canonical MCP configuration format (OpenCode-derived).
/// Serves as the universal intermediate representation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalWorkspaceState {
    pub mcp: HashMap<String, McpServerDefinition>,
}

/// A single MCP server definition in canonical form.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpServerDefinition {
    #[serde(rename = "type")]
    pub transport: TransportType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// Pass-through guardian: preserves agent-specific fields
    /// that the canonical model doesn't explicitly define.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransportType {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "remote")]
    Remote,
}

/// Agent layout mapping — describes how each agent's config
/// relates to the canonical source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLayout {
    pub version: u32,
    pub canonical_mcp: String,
    pub canonical_skills: String,
    pub agents: HashMap<String, AgentEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEntry {
    pub skill_dir: Option<String>,
    pub mcp_file: String,
    pub mcp_format: String,
    pub extra_dirs: Vec<String>,
}

/// Client transport capabilities matrix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    pub name: String,
    pub supports_stdio: bool,
    pub supports_sse: bool,
    pub supports_http: bool,
    pub supports_env_section: bool,
    pub placeholder_style: PlaceholderStyle,
    pub max_id_length: Option<usize>,
    pub forbidden_id_chars: Vec<char>,
    pub requires_security_sandbox: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlaceholderStyle {
    #[serde(rename = "dollar_brace")]
    DollarBrace,        // ${VAR}
    #[serde(rename = "dollar")]
    Dollar,             // $VAR
    #[serde(rename = "env_dollar_brace")]
    EnvDollarBrace,     // ${env:VAR}
    #[serde(rename = "input_dollar_brace")]
    InputDollarBrace,   // ${input:name}
}

/// Sync transaction record for rollback support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncTransaction {
    pub id: String,
    pub timestamp: i64,
    pub agent: String,
    pub target_path: String,
    pub backup_path: String,
    pub checksum_before: String,
    pub checksum_after: String,
    pub status: TransactionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransactionStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "committed")]
    Committed,
    #[serde(rename = "rolled_back")]
    RolledBack,
}

/// Usage ledger entry for telemetry-driven pruning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEntry {
    pub server_id: String,
    pub last_used_timestamp: i64,
    pub total_call_count: u64,
    pub context_window_byte_cost: Option<u64>,
}

/// Report from agenttrim analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnusedReport {
    pub key_identifier: String,
    pub kind: ReportKind,
    pub path_context: Option<String>,
    pub last_active_timestamp: Option<i64>,
    pub safe_to_purge: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReportKind {
    Skill,
    McpServer,
    OrphanedProcess,
    StaleBackup,
}

/// Placeholder mapping for secret-splitting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretMapping {
    pub placeholder: String,
    pub keychain_label: String,
    pub target_agent: String,
    pub original_path: Vec<String>,  // JSON path to the secret in canonical config
}

/// Environment mapping for interpolation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentMapping {
    pub key: String,
    pub value: String,
    pub scope: EnvScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EnvScope {
    Global,
    PerAgent(String),
}
