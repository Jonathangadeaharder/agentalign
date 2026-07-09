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
/// Unlike Claude Code (which accepts comma-separated strings), Grok requires
/// `tools` and `disallowedTools` as YAML arrays and uses its own built-in
/// tool names (e.g. `search_replace` not `Edit`, `run_terminal_command` not
/// `Bash`). String values are silently ignored, so permissions would be lost
/// without the translation below.
///
/// Agent files live at `~/.grok/agents/*.md`.
pub struct GrokAgentStrategy;

/// Translate a canonical (Claude Code-style) tool name to its Grok equivalent.
/// MCP tool names (e.g. `svelte_list-sections`) pass through unchanged.
fn translate_tool(name: &str) -> &str {
    match name {
        "Read" | "read" => "read_file",
        "Edit" | "edit" | "Write" | "write" => "search_replace",
        "Grep" | "grep" => "grep",
        "Glob" | "glob" => "list_dir",
        "Bash" | "bash" => "run_terminal_command",
        "WebFetch" | "webfetch" | "web_fetch" => "web_fetch",
        other => other,
    }
}

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
            // Translate canonical names and dedup in order: multiple canonical
            // names (Edit + Write) collapse to one Grok name (search_replace).
            let mut seen: Vec<&str> = Vec::new();
            for t in &agent.frontmatter.tools {
                let g = translate_tool(t.as_str());
                if !seen.contains(&g) {
                    seen.push(g);
                }
            }
            frontmatter.insert(
                YamlValue::String("tools".into()),
                YamlValue::Sequence(
                    seen.iter()
                        .map(|s| YamlValue::String((*s).into()))
                        .collect(),
                ),
            );
        }

        if let Some(ref color) = agent.frontmatter.color {
            frontmatter.insert(
                YamlValue::String("color".into()),
                YamlValue::String(color.clone()),
            );
        }

        // Map canonical permission {edit, bash} → disallowedTools as a YAML
        // array of Grok-native tool names. String format is silently ignored.
        let mut disallowed: Vec<&str> = Vec::new();
        if agent
            .frontmatter
            .permission
            .edit
            .eq_ignore_ascii_case("deny")
        {
            disallowed.push("search_replace");
        }
        if agent
            .frontmatter
            .permission
            .bash
            .eq_ignore_ascii_case("deny")
        {
            disallowed.push("run_terminal_command");
        }
        if !disallowed.is_empty() {
            frontmatter.insert(
                YamlValue::String("disallowedTools".into()),
                YamlValue::Sequence(
                    disallowed
                        .iter()
                        .map(|s| YamlValue::String((*s).into()))
                        .collect(),
                ),
            );
        }

        // Grok-specific: load AGENTS.md context for this subagent.
        frontmatter.insert(YamlValue::String("agents_md".into()), YamlValue::Bool(true));

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
                tools: vec!["Read".to_string(), "Grep".to_string()],
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
        // tools must be a YAML array, not a comma string.
        assert!(output.contains("tools:\n- read_file"));
        assert!(output.contains("- grep"));
        assert!(!output.contains("tools: read_file"));
        assert!(output.contains("color: blue"));
        // disallowedTools must be a YAML array with grok-native names.
        assert!(output.contains("disallowedTools:\n- search_replace"));
        assert!(output.contains("- run_terminal_command"));
        assert!(!output.contains("disallowedTools: Edit"));
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
    fn test_grok_translates_claude_tool_names() {
        let mut agent = make_agent();
        agent.frontmatter.permission.edit = "allow".to_string();
        agent.frontmatter.permission.bash = "allow".to_string();
        agent.frontmatter.tools = vec![
            "Read".to_string(),
            "Edit".to_string(),
            "Write".to_string(),
            "Grep".to_string(),
            "Glob".to_string(),
            "Bash".to_string(),
            "WebFetch".to_string(),
        ];
        let strategy = GrokAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(output.contains("- read_file"));
        assert!(output.contains("- search_replace"));
        assert!(output.contains("- grep"));
        assert!(output.contains("- list_dir"));
        assert!(output.contains("- run_terminal_command"));
        assert!(output.contains("- web_fetch"));
        // Claude Code names must not leak through.
        assert!(!output.contains("- Read"));
        assert!(!output.contains("- Edit"));
        assert!(!output.contains("- Bash"));
        assert!(!output.contains("- WebFetch"));
    }

    #[test]
    fn test_grok_dedups_collapsed_tool_names() {
        // Edit + Write both map to search_replace — emit only one.
        let mut agent = make_agent();
        agent.frontmatter.permission.edit = "allow".to_string();
        agent.frontmatter.permission.bash = "allow".to_string();
        agent.frontmatter.tools = vec!["Edit".to_string(), "Write".to_string()];
        let strategy = GrokAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        let count = output.matches("- search_replace").count();
        assert_eq!(count, 1, "duplicate tool names must be deduped: {}", output);
    }

    #[test]
    fn test_grok_passes_through_mcp_tool_names() {
        let mut agent = make_agent();
        agent.frontmatter.permission.edit = "allow".to_string();
        agent.frontmatter.permission.bash = "allow".to_string();
        agent.frontmatter.tools = vec!["svelte_list-sections".to_string()];
        let strategy = GrokAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(output.contains("- svelte_list-sections"));
    }

    #[test]
    fn test_grok_translates_lowercase_tool_names() {
        let mut agent = make_agent();
        agent.frontmatter.permission.edit = "allow".to_string();
        agent.frontmatter.permission.bash = "allow".to_string();
        agent.frontmatter.tools = vec!["read".to_string(), "edit".to_string()];
        let strategy = GrokAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(output.contains("- read_file"));
        assert!(output.contains("- search_replace"));
    }

    #[test]
    fn test_grok_case_insensitive_permission_deny() {
        let mut agent = make_agent();
        agent.frontmatter.permission.edit = "Deny".to_string();
        agent.frontmatter.permission.bash = "DENY".to_string();
        agent.frontmatter.tools = vec![];
        let strategy = GrokAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();

        assert!(output.contains("- search_replace"), "Deny should deny edit");
        assert!(
            output.contains("- run_terminal_command"),
            "DENY should deny bash"
        );
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
