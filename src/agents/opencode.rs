use crate::agents::canonical::ParsedAgentFile;
use crate::agents::SubagentStrategy;
use crate::mcp::factory::AgentType;
use serde_yaml::Mapping;
use serde_yaml::Value as YamlValue;
use std::path::Path;

pub struct OpenCodeAgentStrategy;

impl SubagentStrategy for OpenCodeAgentStrategy {
    fn agent_type(&self) -> AgentType {
        AgentType::OpenCode
    }

    fn agents_dir(&self, home: &Path) -> std::path::PathBuf {
        home.join(".config").join("opencode").join("agent")
    }

    fn format_agent(&self, agent: &ParsedAgentFile) -> anyhow::Result<String> {
        let mut frontmatter = Mapping::new();

        frontmatter.insert(
            YamlValue::String("description".into()),
            YamlValue::String(agent.frontmatter.description.clone()),
        );

        frontmatter.insert(
            YamlValue::String("mode".into()),
            YamlValue::String(agent.frontmatter.mode.clone()),
        );

        if let Some(ref model) = agent.frontmatter.model {
            frontmatter.insert(
                YamlValue::String("model".into()),
                YamlValue::String(model.clone()),
            );
        }

        let mut perm = Mapping::new();
        perm.insert(
            YamlValue::String("edit".into()),
            YamlValue::String(agent.frontmatter.permission.edit.clone()),
        );
        perm.insert(
            YamlValue::String("bash".into()),
            YamlValue::String(agent.frontmatter.permission.bash.clone()),
        );
        frontmatter.insert(YamlValue::String("permission".into()), YamlValue::Mapping(perm));

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
                tools: vec!["read_file".to_string()],
                color: None,
                extra: HashMap::new(),
            },
            body: "You are a vision agent.".to_string(),
        }
    }

    #[test]
    fn test_opencode_format() {
        let agent = make_agent();
        let strategy = OpenCodeAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(output.starts_with("---"));
        assert!(output.contains("mode: subagent"));
        assert!(output.contains("model: kimi-k2.6"));
        assert!(output.contains("permission:"));
        assert!(output.contains("edit: deny"));
        assert!(output.contains("bash: deny"));
        assert!(!output.contains("tools:"));
        assert!(output.contains("You are a vision agent."));
    }
}
