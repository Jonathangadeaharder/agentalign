use crate::agents::canonical::ParsedAgentFile;
use crate::agents::SubagentStrategy;
use crate::mcp::factory::AgentType;
use serde_yaml::Mapping;
use serde_yaml::Value as YamlValue;
use std::path::Path;

pub struct CursorAgentStrategy;

impl SubagentStrategy for CursorAgentStrategy {
    fn agent_type(&self) -> AgentType {
        AgentType::Cursor
    }

    fn agents_dir(&self, home: &Path) -> std::path::PathBuf {
        home.join(".cursor").join("agents")
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

        // Cursor's agent frontmatter schema doesn't support `color`
        // (name, description, model, tools only) — omit it, unlike Claude.

        // Map canonical permission {edit, bash} → disallowedTools.
        // Cursor (like Claude Code) denies tools by name in `disallowedTools`.
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
    fn test_cursor_format() {
        let agent = make_agent();
        let strategy = CursorAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(output.starts_with("---"));
        assert!(output.contains("name: vision"));
        assert!(output.contains("description: Vision agent"));
        assert!(output.contains("model: kimi-k2.6"));
        assert!(output.contains("tools: read_file, grep_search"));
        assert!(output.contains("disallowedTools: Edit, Write, Bash"));
        assert!(output.contains("You are a vision agent."));
    }

    #[test]
    fn test_cursor_no_disallowed_when_all_allowed() {
        let mut agent = make_agent();
        agent.frontmatter.permission.edit = "allow".to_string();
        agent.frontmatter.permission.bash = "allow".to_string();
        agent.frontmatter.tools = vec![];
        let strategy = CursorAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(
            !output.contains("disallowedTools"),
            "no disallowedTools when all permissions are allow"
        );
    }

    #[test]
    fn test_cursor_format_omits_unsupported_color_field() {
        let agent = make_agent();
        let strategy = CursorAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(
            !output.contains("color"),
            "Cursor's agent schema doesn't support `color` — it must not be emitted"
        );
    }

    #[test]
    fn test_cursor_agents_dir() {
        let strategy = CursorAgentStrategy;
        let home = Path::new("/home/user");
        assert_eq!(
            strategy.agents_dir(home),
            Path::new("/home/user/.cursor/agents")
        );
    }
}
