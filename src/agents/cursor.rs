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

        if let Some(ref color) = agent.frontmatter.color {
            frontmatter.insert(
                YamlValue::String("color".into()),
                YamlValue::String(color.clone()),
            );
        }

        let yaml_str = serde_yaml::to_string(&frontmatter)?;
        let yaml_trimmed = yaml_str.trim_end_matches('\n');

        Ok(format!("---\n{}\n---\n\n{}", yaml_trimmed, agent.body))
    }
}
