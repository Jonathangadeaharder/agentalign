use crate::agents::canonical::ParsedAgentFile;
use crate::agents::SubagentStrategy;
use crate::mcp::factory::AgentType;
use serde_yaml::Mapping;
use serde_yaml::Value as YamlValue;
use std::path::Path;

/// ZCode subagent strategy.
///
/// ZCode discovers user-level subagents from `~/.zcode/agents/<name>.md`. The
/// file is Markdown with flat YAML frontmatter; the body becomes the system
/// prompt. The schema (read by ZCode's `parseSubagentMarkdown`) accepts:
/// `name`, `description`, `model`, `color`, `permissionMode`, `tools`,
/// `disallowedTools`, `skills`, `maxTurns`, `background`, `mcpServers`.
///
/// Canonical `permission: { edit, bash }` maps to zcode's `disallowedTools`:
/// `edit: deny` excludes `Edit, Write`; `bash: deny` excludes `Bash`. This is
/// the closest zcode analog to opencode/claude's permission model.
pub struct ZCodeAgentStrategy;

impl SubagentStrategy for ZCodeAgentStrategy {
    fn agent_type(&self) -> AgentType {
        AgentType::ZCode
    }

    fn agents_dir(&self, home: &Path) -> std::path::PathBuf {
        home.join(".zcode").join("agents")
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

        if let Some(ref color) = agent.frontmatter.color {
            frontmatter.insert(
                YamlValue::String("color".into()),
                YamlValue::String(color.clone()),
            );
        }

        // Translate canonical permission {edit, bash} into zcode disallowedTools.
        // edit: deny  -> Edit, Write
        // bash: deny   -> Bash
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

    fn make_agent(permission: AgentPermission) -> ParsedAgentFile {
        ParsedAgentFile {
            name: "vision".to_string(),
            frontmatter: CanonicalAgentDefinition {
                description: "Vision agent".to_string(),
                mode: "subagent".to_string(),
                model: Some("meta-router/meta-vision".to_string()),
                permission,
                tools: vec![],
                color: Some("blue".to_string()),
                extra: HashMap::new(),
            },
            body: "You are a vision agent.".to_string(),
        }
    }

    #[test]
    fn test_zcode_format_read_only() {
        let agent = make_agent(AgentPermission {
            edit: "deny".to_string(),
            bash: "deny".to_string(),
        });
        let strategy = ZCodeAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(output.starts_with("---"));
        assert!(output.contains("name: vision"));
        assert!(output.contains("description: Vision agent"));
        assert!(output.contains("model: meta-router/meta-vision"));
        assert!(output.contains("color: blue"));
        // edit: deny -> Edit, Write (pushed first); bash: deny -> Bash (pushed after)
        assert!(output.contains("disallowedTools: Edit, Write, Bash"));
        // zcode has no `mode` or `permission` field
        assert!(!output.contains("mode:"));
        assert!(!output.contains("permission:"));
        assert!(output.contains("You are a vision agent."));
    }

    #[test]
    fn test_zcode_format_all_allowed() {
        let agent = make_agent(AgentPermission {
            edit: "allow".to_string(),
            bash: "allow".to_string(),
        });
        let strategy = ZCodeAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(output.contains("name: vision"));
        assert!(output.contains("model: meta-router/meta-vision"));
        assert!(
            !output.contains("disallowedTools"),
            "no disallowedTools when all allowed"
        );
    }

    #[test]
    fn test_zcode_format_partial_deny() {
        let agent = make_agent(AgentPermission {
            edit: "allow".to_string(),
            bash: "deny".to_string(),
        });
        let strategy = ZCodeAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(output.contains("disallowedTools: Bash"));
        assert!(
            !output.contains("Edit"),
            "Edit should not appear when edit is allowed"
        );
    }

    #[test]
    fn test_zcode_format_no_model() {
        let mut agent = make_agent(AgentPermission::default());
        agent.frontmatter.model = None;
        agent.frontmatter.color = None;
        let strategy = ZCodeAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(!output.contains("model:"));
        assert!(!output.contains("color:"));
    }

    #[test]
    fn test_zcode_agents_dir() {
        let strategy = ZCodeAgentStrategy;
        let home = Path::new("/home/user");
        assert_eq!(
            strategy.agents_dir(home),
            Path::new("/home/user/.zcode/agents")
        );
    }

    #[test]
    fn test_zcode_agent_type() {
        let strategy = ZCodeAgentStrategy;
        assert_eq!(strategy.agent_type(), AgentType::ZCode);
    }
}
