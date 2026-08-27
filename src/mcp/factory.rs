//! Factory for creating MCP format strategies by agent type.
//!
//! `AgentRegistry` is the single source of truth for which agents are actively
//! synced. All CLI commands and the watch daemon derive their agent list from
//! `AgentRegistry::synced_agents()` instead of hardcoding.

use crate::shared::traits::McpFormatStrategy;
use std::path::PathBuf;

/// Supported agent types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentType {
    Claude,
    Cursor,
    VSCode,
    Copilot,
    Windsurf,
    Zed,
    Gemini,
    Codex,
    OpenCode,
    Antigravity,
    ZCode,
    Grok,
    Qwen,
}

impl AgentType {
    /// Return all agent types (including inactive/future ones).
    pub fn all() -> Vec<AgentType> {
        vec![
            AgentType::Claude,
            AgentType::Cursor,
            AgentType::VSCode,
            AgentType::Copilot,
            AgentType::Windsurf,
            AgentType::Zed,
            AgentType::Gemini,
            AgentType::Codex,
            AgentType::OpenCode,
            AgentType::Antigravity,
            AgentType::ZCode,
            AgentType::Grok,
            AgentType::Qwen,
        ]
    }

    /// Human-readable name.
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentType::Claude => "claude",
            AgentType::Cursor => "cursor",
            AgentType::VSCode => "vscode",
            AgentType::Copilot => "copilot",
            AgentType::Windsurf => "windsurf",
            AgentType::Zed => "zed",
            AgentType::Gemini => "gemini",
            AgentType::Codex => "codex",
            AgentType::OpenCode => "opencode",
            AgentType::Antigravity => "antigravity",
            AgentType::ZCode => "zcode",
            AgentType::Grok => "grok",
            AgentType::Qwen => "qwen",
        }
    }

    /// Parse from string name (case-insensitive).
    pub fn from_name(name: &str) -> Option<AgentType> {
        match name.to_lowercase().as_str() {
            "claude" => Some(AgentType::Claude),
            "cursor" => Some(AgentType::Cursor),
            "vscode" | "vs-code" | "visual studio code" => Some(AgentType::VSCode),
            "copilot" => Some(AgentType::Copilot),
            "windsurf" => Some(AgentType::Windsurf),
            "zed" => Some(AgentType::Zed),
            "gemini" => Some(AgentType::Gemini),
            "codex" => Some(AgentType::Codex),
            "opencode" => Some(AgentType::OpenCode),
            "antigravity" => Some(AgentType::Antigravity),
            "zcode" | "z-code" => Some(AgentType::ZCode),
            "grok" => Some(AgentType::Grok),
            "qwen" | "qwen-code" => Some(AgentType::Qwen),
            _ => None,
        }
    }
}

/// Descriptor for a single agent's paths and configuration.
#[derive(Debug, Clone)]
pub struct AgentDescriptor {
    /// Agent type enum variant.
    pub agent_type: AgentType,
    /// Human-readable label for display (e.g., "Claude", "OpenCode").
    pub label: &'static str,
    /// Path to the agent's MCP config file (relative to home).
    pub config_path: PathBuf,
    /// Path to the agent's instruction file (relative to home), if applicable.
    pub instruction_path: Option<PathBuf>,
    /// Path to the agent's skills directory (relative to home), if applicable.
    pub skills_dir: Option<PathBuf>,
}

/// Central registry of all agent descriptors.
///
/// Single source of truth for agent paths. Every CLI command and the watch
/// daemon must derive their agent list from here.
pub struct AgentRegistry;

impl AgentRegistry {
    /// Build descriptors for all actively-synced agents.
    ///
    /// These are the agents that `sync`, `add`, `remove`, and `watch` operate on.
    pub fn synced_agents(home: &std::path::Path) -> Vec<AgentDescriptor> {
        vec![
            AgentDescriptor {
                agent_type: AgentType::Claude,
                label: "Claude",
                config_path: home.join(".claude.json"),
                instruction_path: Some(home.join(".claude").join("CLAUDE.md")),
                skills_dir: Some(home.join(".claude").join("skills")),
            },
            AgentDescriptor {
                agent_type: AgentType::Cursor,
                label: "Cursor",
                config_path: home.join(".cursor").join("mcp.json"),
                instruction_path: None,
                skills_dir: Some(home.join(".cursor").join("skills")),
            },
            AgentDescriptor {
                agent_type: AgentType::Gemini,
                label: "Gemini",
                config_path: home.join(".gemini").join("settings.json"),
                instruction_path: Some(home.join(".gemini").join("GEMINI.md")),
                skills_dir: Some(home.join(".gemini").join("config").join("skills")),
            },
            AgentDescriptor {
                agent_type: AgentType::OpenCode,
                label: "OpenCode",
                config_path: home.join(".config").join("opencode").join("opencode.json"),
                instruction_path: Some(home.join(".config").join("opencode").join("AGENTS.md")),
                skills_dir: None,
            },
            AgentDescriptor {
                agent_type: AgentType::Codex,
                label: "Codex",
                config_path: home.join(".codex").join("config.toml"),
                instruction_path: Some(home.join(".codex").join("AGENTS.md")),
                skills_dir: Some(home.join(".codex").join("skills")),
            },
            AgentDescriptor {
                agent_type: AgentType::Antigravity,
                label: "Antigravity",
                config_path: home.join(".gemini").join("antigravity").join("mcp_config.json"),
                instruction_path: None,
                skills_dir: None,
            },
            AgentDescriptor {
                agent_type: AgentType::ZCode,
                label: "ZCode",
                config_path: home.join(".zcode").join("cli").join("config.json"),
                instruction_path: Some(home.join(".zcode").join("AGENTS.md")),
                skills_dir: None, // ZCode reads ~/.agents/skills natively
            },
            AgentDescriptor {
                agent_type: AgentType::Grok,
                label: "Grok",
                config_path: home.join(".grok").join("config.toml"),
                instruction_path: Some(home.join(".grok").join("AGENTS.md")),
                skills_dir: Some(home.join(".grok").join("skills")),
            },
            AgentDescriptor {
                agent_type: AgentType::Qwen,
                label: "Qwen",
                config_path: home.join(".qwen").join("settings.json"),
                instruction_path: Some(home.join(".qwen").join("QWEN.md")),
                skills_dir: Some(home.join(".qwen").join("skills")),
            },
        ]
    }

    /// Build descriptors for agents whose config files exist on disk.
    pub fn discovered_agents(home: &std::path::Path) -> Vec<AgentDescriptor> {
        Self::synced_agents(home)
            .into_iter()
            .filter(|d| d.config_path.exists())
            .collect()
    }
}

/// Factory for constructing MCP format strategy instances.
pub struct McpFormatFactory;

impl McpFormatFactory {
    /// Create a strategy for the given agent type.
    pub fn from_agent(agent: AgentType) -> Box<dyn McpFormatStrategy> {
        match agent {
            AgentType::Claude => Box::new(super::claude::ClaudeStrategy::default()),
            AgentType::Cursor => Box::new(super::claude::ClaudeStrategy { is_cursor: true }),
            AgentType::VSCode => Box::new(super::vscode::VSCodeStrategy),
            AgentType::Copilot => Box::new(super::copilot::CopilotStrategy),
            AgentType::Windsurf => Box::new(super::windsurf::WindsurfStrategy),
            AgentType::Zed => Box::new(super::zed::ZedStrategy),
            AgentType::Gemini => Box::new(super::gemini::GeminiStrategy::default()),
            AgentType::Codex => Box::new(super::codex::CodexStrategy),
            AgentType::OpenCode => Box::new(super::opencode::OpenCodeStrategy),
            AgentType::Antigravity => Box::new(super::antigravity::AntigravityStrategy),
            AgentType::ZCode => Box::new(super::zcode::ZCodeStrategy),
            AgentType::Grok => Box::new(super::grok::GrokStrategy),
            AgentType::Qwen => Box::new(super::qwen::QwenStrategy::default()),
        }
    }

    /// Create strategies for all agent types.
    pub fn all_agents() -> Vec<Box<dyn McpFormatStrategy>> {
        AgentType::all().into_iter().map(Self::from_agent).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_agents_non_empty() {
        let strategies = McpFormatFactory::all_agents();
        assert_eq!(strategies.len(), 13);
    }

    #[test]
    fn test_agent_names() {
        assert_eq!(McpFormatFactory::from_agent(AgentType::Claude).target_name(), "claude");
        assert_eq!(McpFormatFactory::from_agent(AgentType::Cursor).target_name(), "cursor");
        assert_eq!(McpFormatFactory::from_agent(AgentType::VSCode).target_name(), "vscode");
        assert_eq!(McpFormatFactory::from_agent(AgentType::Copilot).target_name(), "copilot");
        assert_eq!(McpFormatFactory::from_agent(AgentType::Windsurf).target_name(), "windsurf");
        assert_eq!(McpFormatFactory::from_agent(AgentType::Zed).target_name(), "zed");
        assert_eq!(McpFormatFactory::from_agent(AgentType::Gemini).target_name(), "gemini");
        assert_eq!(McpFormatFactory::from_agent(AgentType::Codex).target_name(), "codex");
        assert_eq!(McpFormatFactory::from_agent(AgentType::OpenCode).target_name(), "opencode");
        assert_eq!(McpFormatFactory::from_agent(AgentType::Antigravity).target_name(), "antigravity");
        assert_eq!(McpFormatFactory::from_agent(AgentType::ZCode).target_name(), "zcode");
        assert_eq!(McpFormatFactory::from_agent(AgentType::Grok).target_name(), "grok");
        assert_eq!(McpFormatFactory::from_agent(AgentType::Qwen).target_name(), "qwen");
    }

    #[test]
    fn test_qwen_from_name() {
        assert_eq!(AgentType::from_name("qwen"), Some(AgentType::Qwen));
        assert_eq!(AgentType::from_name("QWEN"), Some(AgentType::Qwen));
        assert_eq!(AgentType::from_name("qwen-code"), Some(AgentType::Qwen));
    }

    #[test]
    fn test_qwen_registry_paths() {
        let home = std::path::Path::new("/home/dummy");
        let qwen = AgentRegistry::synced_agents(home)
            .into_iter()
            .find(|d| matches!(d.agent_type, AgentType::Qwen))
            .expect("qwen descriptor");
        assert_eq!(qwen.config_path, home.join(".qwen").join("settings.json"));
        assert_eq!(
            qwen.instruction_path.expect("qwen instruction_path"),
            home.join(".qwen").join("QWEN.md")
        );
        assert_eq!(
            qwen.skills_dir.expect("qwen skills_dir"),
            home.join(".qwen").join("skills")
        );
    }

    #[test]
    fn test_gemini_skills_dir_matches_skills_sync_path() {
        // skills/mod.rs syncs to ~/.gemini/config/skills (agy reads
        // ~/.gemini/config/); the registry entry, consumed by the watch
        // daemon, must point at the same directory.
        let home = std::path::Path::new("/home/dummy");
        let gemini = AgentRegistry::synced_agents(home)
            .into_iter()
            .find(|d| matches!(d.agent_type, AgentType::Gemini))
            .expect("gemini descriptor");
        assert_eq!(
            gemini.skills_dir.expect("gemini skills_dir"),
            home.join(".gemini").join("config").join("skills")
        );
    }
}
