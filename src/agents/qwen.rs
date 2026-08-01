use crate::agents::canonical::ParsedAgentFile;
use crate::agents::SubagentStrategy;
use crate::mcp::factory::AgentType;
use std::path::Path;

/// Qwen Code subagent strategy.
///
/// Qwen Code is a Gemini CLI fork and discovers user-level subagents from
/// `~/.qwen/agents/<name>.md`: Markdown with YAML frontmatter, the body is the
/// system prompt. This is the gemini-family agent format, so the strategy
/// writes the canonical markdown unchanged (identity, same as Gemini).
pub struct QwenAgentStrategy;

impl SubagentStrategy for QwenAgentStrategy {
    fn agent_type(&self) -> AgentType {
        AgentType::Qwen
    }

    fn agents_dir(&self, home: &Path) -> std::path::PathBuf {
        home.join(".qwen").join("agents")
    }

    fn format_agent(&self, agent: &ParsedAgentFile) -> anyhow::Result<String> {
        crate::agents::canonical::write_agent_md(agent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::canonical::{AgentPermission, CanonicalAgentDefinition};
    use std::collections::HashMap;

    fn make_agent() -> ParsedAgentFile {
        ParsedAgentFile {
            name: "vision".to_string(),
            frontmatter: CanonicalAgentDefinition {
                description: "Vision agent".to_string(),
                mode: "subagent".to_string(),
                model: Some("qwen3-coder".to_string()),
                permission: AgentPermission::default(),
                tools: vec![],
                color: None,
                extra: HashMap::new(),
            },
            body: "You are a vision agent.".to_string(),
        }
    }

    #[test]
    fn test_qwen_format_is_canonical_markdown() {
        let agent = make_agent();
        let strategy = QwenAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();
        assert!(output.starts_with("---"));
        assert!(output.contains("description: Vision agent"));
        assert!(output.contains("You are a vision agent."));
    }

    #[test]
    fn test_qwen_agents_dir() {
        let strategy = QwenAgentStrategy;
        let home = Path::new("/home/user");
        assert_eq!(
            strategy.agents_dir(home),
            Path::new("/home/user/.qwen/agents")
        );
    }

    #[test]
    fn test_qwen_agent_target_path() {
        let strategy = QwenAgentStrategy;
        let agents_dir = Path::new("/home/user/.qwen/agents");
        assert_eq!(
            strategy.agent_target_path(agents_dir, "vision"),
            Path::new("/home/user/.qwen/agents/vision.md")
        );
    }

    #[test]
    fn test_qwen_agent_type() {
        let strategy = QwenAgentStrategy;
        assert_eq!(strategy.agent_type(), AgentType::Qwen);
    }
}
