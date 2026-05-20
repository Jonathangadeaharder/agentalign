//! Factory for creating MCP format strategies by agent type.

use agentalign_shared::traits::McpFormatStrategy;

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
}

impl AgentType {
    /// Return all agent types.
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
            _ => None,
        }
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
        assert_eq!(strategies.len(), 8);
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
    }
}
