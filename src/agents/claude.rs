use crate::agents::canonical::ParsedAgentFile;
use crate::agents::SubagentStrategy;
use crate::mcp::factory::AgentType;
use serde_yaml::Mapping;
use serde_yaml::Value as YamlValue;
use std::path::Path;

pub struct ClaudeAgentStrategy;

impl SubagentStrategy for ClaudeAgentStrategy {
    fn agent_type(&self) -> AgentType {
        AgentType::Claude
    }

    fn agents_dir(&self, home: &Path) -> std::path::PathBuf {
        home.join(".claude").join("agents")
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
        // Claude Code denies tools by name in `disallowedTools`.
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
    fn test_claude_format() {
        let agent = make_agent();
        let strategy = ClaudeAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(output.starts_with("---"));
        assert!(output.contains("name: vision"));
        assert!(output.contains("description: Vision agent"));
        assert!(output.contains("model: kimi-k2.6"));
        assert!(output.contains("tools: read_file, grep_search"));
        assert!(output.contains("color: blue"));
        assert!(output.contains("disallowedTools: Edit, Write, Bash"));
        assert!(!output.contains("mode:"));
        assert!(!output.contains("permission:"));
        assert!(output.contains("You are a vision agent."));
    }

    #[test]
    fn test_claude_no_disallowed_when_all_allowed() {
        let mut agent = make_agent();
        agent.frontmatter.permission.edit = "allow".to_string();
        agent.frontmatter.permission.bash = "allow".to_string();
        agent.frontmatter.tools = vec![];
        let strategy = ClaudeAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(
            !output.contains("disallowedTools"),
            "no disallowedTools when all permissions are allow"
        );
    }
}
