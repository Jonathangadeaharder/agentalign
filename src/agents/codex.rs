use crate::agents::canonical::ParsedAgentFile;
use crate::agents::SubagentStrategy;
use crate::mcp::factory::AgentType;
use serde_yaml::Mapping;
use serde_yaml::Value as YamlValue;
use std::path::Path;

/// Codex agent strategy.
///
/// Codex discovers agents via `openai.yaml` files inside skill directories:
///   `~/.codex/skills/<skill-name>/agents/openai.yaml`
///
/// The `openai.yaml` provides UI metadata (display_name, short_description,
/// default_prompt). The agent's system prompt comes from SKILL.md in the
/// skill directory — which agentalign already syncs via skill symlinks.
///
/// Since our canonical agents are also available as skills (symlinked into
/// ~/.codex/skills/), we place the `openai.yaml` next to each skill's SKILL.md
/// so Codex can discover them.
pub struct CodexAgentStrategy;

impl SubagentStrategy for CodexAgentStrategy {
    fn agent_type(&self) -> AgentType {
        AgentType::Codex
    }

    /// Returns the parent skills directory — each agent gets its own
    /// `<skills_dir>/<agent_name>/agents/` subdirectory.
    fn agents_dir(&self, home: &Path) -> std::path::PathBuf {
        home.join(".codex").join("skills")
    }

    /// For Codex, the agent lives at `<skills_dir>/<agent_name>/agents/openai.yaml`.
    /// We override the per-agent target path via `agent_target_path`.
    fn format_agent(&self, agent: &ParsedAgentFile) -> anyhow::Result<String> {
        let mut interface = Mapping::new();

        interface.insert(
            YamlValue::String("interface".into()),
            {
                let mut fields = Mapping::new();
                fields.insert(
                    YamlValue::String("display_name".into()),
                    YamlValue::String(agent.name.clone()),
                );
                fields.insert(
                    YamlValue::String("short_description".into()),
                    YamlValue::String(agent.frontmatter.description.clone()),
                );
                // default_prompt: first line of body, truncated to 80 chars
                let prompt_preview = agent
                    .body
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(80)
                    .collect::<String>();
                fields.insert(
                    YamlValue::String("default_prompt".into()),
                    YamlValue::String(prompt_preview),
                );
                YamlValue::Mapping(fields)
            },
        );

        let yaml_str = serde_yaml::to_string(&interface)?;
        Ok(yaml_str)
    }

    /// Codex agents are per-skill: `<skills_dir>/<name>/agents/openai.yaml`.
    fn agent_target_path(&self, agents_dir: &Path, agent_name: &str) -> std::path::PathBuf {
        agents_dir
            .join(agent_name)
            .join("agents")
            .join("openai.yaml")
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
            body: "You are a vision agent. You analyze images.".to_string(),
        }
    }

    #[test]
    fn test_codex_format() {
        let agent = make_agent();
        let strategy = CodexAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(output.contains("interface:"));
        assert!(output.contains("display_name: vision"));
        assert!(output.contains("short_description: Vision agent"));
        assert!(output.contains("default_prompt:"));
        assert!(!output.contains("mode:"));
        assert!(!output.contains("permission:"));
    }

    #[test]
    fn test_codex_agents_dir() {
        let strategy = CodexAgentStrategy;
        let home = Path::new("/home/user");
        assert_eq!(
            strategy.agents_dir(home),
            Path::new("/home/user/.codex/skills")
        );
    }

    #[test]
    fn test_codex_agent_target_path() {
        let strategy = CodexAgentStrategy;
        let skills_dir = Path::new("/home/user/.codex/skills");
        assert_eq!(
            strategy.agent_target_path(skills_dir, "vision"),
            Path::new("/home/user/.codex/skills/vision/agents/openai.yaml")
        );
    }

    #[test]
    fn test_codex_agent_type() {
        let strategy = CodexAgentStrategy;
        assert_eq!(strategy.agent_type(), AgentType::Codex);
    }
}
