use crate::agents::canonical::ParsedAgentFile;
use crate::agents::SubagentStrategy;
use crate::mcp::factory::AgentType;
use serde_yaml::Mapping;
use serde_yaml::Value as YamlValue;
use std::path::Path;

/// Grok CLI subagent strategy.
///
/// Grok is Claude Code-compatible: same frontmatter fields (name, description,
/// model, tools, disallowedTools, color) plus `agents_md: true` which tells
/// Grok to load AGENTS.md context for the subagent.
///
/// Agent files live at `~/.grok/agents/*.md`.
pub struct GrokAgentStrategy;

impl SubagentStrategy for GrokAgentStrategy {
    fn agent_type(&self) -> AgentType {
        AgentType::Grok
    }

    fn agents_dir(&self, home: &Path) -> std::path::PathBuf {
        home.join(".grok").join("agents")
    }

    fn format_agent(&self, agent: &ParsedAgentFile) -> anyhow::Result<String> {
        let mut frontmatter = Mapping::new();

        frontmatter.insert(
            YamlValue::String("name".into()),
            YamlValue::String(agent.name.clone()),
        );

        frontmatter.insert(
            YamlValue::String("description".into()),
            YamlValue::String(agent.frontmatter.description.clone()),
        );

        if let Some(ref model) = agent.frontmatter.model {
            frontmatter.insert(
                YamlValue::String("model".into()),
                YamlValue::String(model.clone()),
            );
        }

        if !agent.frontmatter.tools.is_empty() {
            let tools_str = agent.frontmatter.tools.join(", ");
            frontmatter.insert(
                YamlValue::String("tools".into()),
                YamlValue::String(tools_str),
            );
        }

        if let Some(ref color) = agent.frontmatter.color {
            frontmatter.insert(
                YamlValue::String("color".into()),
                YamlValue::String(color.clone()),
            );
        }

        // Map canonical permission {edit, bash} → disallowedTools.
        // Grok is Claude Code-compatible (--deny = --disallowedTools).
        let mut disallowed: Vec<&str> = Vec::new();
        if agent.frontmatter.permission.edit == "deny" {
            disallowed.push("Edit");
            disallowed.push("Write");
        }
        if agent.frontmatter.permission.bash == "deny" {
            disallowed.push("Bash");
        }
        if !disallowed.is_empty() {
            frontmatter.insert(
                YamlValue::String("disallowedTools".into()),
                YamlValue::String(disallowed.join(", ")),
            );
        }

        // Grok-specific: load AGENTS.md context for this subagent.
        frontmatter.insert(
            YamlValue::String("agents_md".into()),
            YamlValue::Bool(true),
        );

        let yaml_str = serde_yaml::to_string(&frontmatter)?;
        let yaml_trimmed = yaml_str.trim_end_matches('\n');

        Ok(format!("---\n{}\n---\n\n{}", yaml_trimmed, agent.body))
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
                model: Some("kimi-k2.6".to_string()),
                permission: AgentPermission {
                    edit: "deny".to_string(),
                    bash: "deny".to_string(),
                },
                tools: vec!["read_file".to_string(), "grep_search".to_string()],
                color: Some("blue".to_string()),
                extra: HashMap::new(),
            },
            body: "You are a vision agent.".to_string(),
        }
    }

    #[test]
    fn test_grok_format() {
        let agent = make_agent();
        let strategy = GrokAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(output.starts_with("---"));
        assert!(output.contains("name: vision"));
        assert!(output.contains("description: Vision agent"));
        assert!(output.contains("model: kimi-k2.6"));
        assert!(output.contains("tools: read_file, grep_search"));
        assert!(output.contains("color: blue"));
        assert!(output.contains("disallowedTools: Edit, Write, Bash"));
        assert!(output.contains("agents_md: true"));
        assert!(output.contains("You are a vision agent."));
    }

    #[test]
    fn test_grok_no_disallowed_when_all_allowed() {
        let mut agent = make_agent();
        agent.frontmatter.permission.edit = "allow".to_string();
        agent.frontmatter.permission.bash = "allow".to_string();
        agent.frontmatter.tools = vec![];
        let strategy = GrokAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(
            !output.contains("disallowedTools"),
            "no disallowedTools when all permissions are allow"
        );
        // agents_md should always be present
        assert!(output.contains("agents_md: true"));
    }

    #[test]
    fn test_grok_agents_dir() {
        let strategy = GrokAgentStrategy;
        let home = Path::new("/home/user");
        assert_eq!(
            strategy.agents_dir(home),
            Path::new("/home/user/.grok/agents")
        );
    }

    #[test]
    fn test_grok_agent_type() {
        let strategy = GrokAgentStrategy;
        assert_eq!(strategy.agent_type(), AgentType::Grok);
    }
}
