use crate::agents::canonical::ParsedAgentFile;
use crate::agents::SubagentStrategy;
use crate::mcp::factory::AgentType;
use std::path::Path;

/// Gemini/Agy agent strategy.
///
/// Agy (Antigravity CLI) discovers custom agents from two paths:
///
/// 1. **Discovery**: `~/.agents/agents/<name>.md` — Agy reads the canonical
///    markdown files directly, with YAML frontmatter (description, mode,
///    permission, tools). This is the SAME path as agentalign's canonical
///    store, so discovery is automatic.
///
/// 2. **Runtime config**: `~/.gemini/agents/<name>/agent.json` — Agy creates
///    these at conversation time with `CustomAgentSpec` protobuf-style config
///    (systemPromptSections, toolNames, systemPromptConfig). Writing these
///    pre-populates the runtime config so Agy doesn't have to generate them.
///
/// Since discovery uses the canonical `.md` files (already synced by agentalign's
/// canonical store logic), this strategy focuses on the runtime `agent.json` files
/// and the `.manifest.json` registration.
pub struct GeminiAgentStrategy;

impl SubagentStrategy for GeminiAgentStrategy {
    fn agent_type(&self) -> AgentType {
        AgentType::Gemini
    }

    /// Agy agents_dir is the canonical store — same as where agentalign reads from.
    /// Agy discovers agents from ~/.agents/agents/*.md directly.
    /// The format_agent output here is a no-op since Agy reads canonical .md files.
    fn agents_dir(&self, home: &Path) -> std::path::PathBuf {
        home.join(".agents").join("agents")
    }

    /// Format agent as markdown — same as canonical format.
    /// Agy discovers agents from `.md` files in the canonical store,
    /// so the "format" for Agy discovery is identity (no conversion).
    fn format_agent(&self, agent: &ParsedAgentFile) -> anyhow::Result<String> {
        // Return the canonical markdown format — Agy reads this directly.
        // The agent.json runtime config is written separately by
        // format_agent_json().
        crate::agents::canonical::write_agent_md(agent)
    }

    /// Agy discovery path: flat `.md` files (same as canonical store).
    fn agent_target_path(&self, agents_dir: &Path, agent_name: &str) -> std::path::PathBuf {
        agents_dir.join(format!("{}.md", agent_name))
    }

    /// Write the runtime `agent.json` to `~/.gemini/agents/<name>/agent.json`.
    /// This pre-populates the CustomAgentSpec config so Agy doesn't have to
    /// generate it at conversation time.
    fn post_sync(&self, home: &Path, agents: &[ParsedAgentFile]) -> anyhow::Result<()> {
        let gemini_agents_dir = home.join(".gemini").join("agents");
        for agent in agents {
            let agent_json = format_agent_json(agent)?;
            let target_dir = gemini_agents_dir.join(&agent.name);
            std::fs::create_dir_all(&target_dir)?;
            std::fs::write(target_dir.join("agent.json"), agent_json)?;
        }
        Ok(())
    }
}

/// Format agent as Agy runtime `agent.json` with CustomAgentSpec.
fn format_agent_json(agent: &ParsedAgentFile) -> anyhow::Result<String> {
    use serde_json::{json, Value};

    let tool_names: Vec<Value> = agent
        .frontmatter
        .tools
        .iter()
        .map(|t| {
            let agy_name = match t.as_str() {
                "Read" => "view_file",
                "Edit" => "replace_file_content",
                "Write" => "write_to_file",
                "Grep" => "grep_search",
                "Glob" => "find_by_name",
                "Bash" => "run_command",
                "WebFetch" => "read_url_content",
                other => other,
            };
            Value::String(agy_name.to_string())
        })
        .collect();

    let agent_json = json!({
        "name": agent.name,
        "description": agent.frontmatter.description,
        "hidden": true,
        "config": {
            "customAgent": {
                "systemPromptSections": [
                    {
                        "title": "Agent System Instructions",
                        "content": agent.body
                    }
                ],
                "toolNames": tool_names,
                "systemPromptConfig": {
                    "includeSections": [
                        "user_information",
                        "mcp_servers",
                        "skills",
                        "subagent_reminder",
                        "messaging",
                        "artifacts",
                        "user_rules"
                    ]
                }
            }
        }
    });

    Ok(serde_json::to_string_pretty(&agent_json)?)
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
                tools: vec!["Read".to_string(), "Grep".to_string(), "Bash".to_string()],
                color: Some("blue".to_string()),
                extra: HashMap::new(),
            },
            body: "You are a vision agent.".to_string(),
        }
    }

    #[test]
    fn test_gemini_format_is_markdown() {
        let agent = make_agent();
        let strategy = GeminiAgentStrategy;
        let output = strategy.format_agent(&agent).unwrap();
        // Should start with YAML frontmatter
        assert!(output.starts_with("---"));
        assert!(output.contains("description: Vision agent"));
    }

    #[test]
    fn test_gemini_agent_json_format() {
        let agent = make_agent();
        let output = format_agent_json(&agent).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["name"], "vision");
        assert_eq!(parsed["description"], "Vision agent");
        assert_eq!(parsed["hidden"], true);
    }

    #[test]
    fn test_gemini_tool_name_mapping() {
        let agent = make_agent();
        let output = format_agent_json(&agent).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let tools = parsed["config"]["customAgent"]["toolNames"]
            .as_array()
            .unwrap();
        assert_eq!(tools[0], "view_file");
        assert_eq!(tools[1], "grep_search");
        assert_eq!(tools[2], "run_command");
    }

    #[test]
    fn test_gemini_system_prompt_sections() {
        let agent = make_agent();
        let output = format_agent_json(&agent).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let sections = parsed["config"]["customAgent"]["systemPromptSections"]
            .as_array()
            .unwrap();
        assert_eq!(sections[0]["title"], "Agent System Instructions");
        assert_eq!(sections[0]["content"], "You are a vision agent.");
    }

    #[test]
    fn test_gemini_agents_dir() {
        let strategy = GeminiAgentStrategy;
        let home = Path::new("/home/user");
        assert_eq!(
            strategy.agents_dir(home),
            Path::new("/home/user/.agents/agents")
        );
    }

    #[test]
    fn test_gemini_agent_target_path() {
        let strategy = GeminiAgentStrategy;
        let agents_dir = Path::new("/home/user/.agents/agents");
        assert_eq!(
            strategy.agent_target_path(agents_dir, "vision"),
            Path::new("/home/user/.agents/agents/vision.md")
        );
    }

    #[test]
    fn test_gemini_agent_type() {
        let strategy = GeminiAgentStrategy;
        assert_eq!(strategy.agent_type(), AgentType::Gemini);
    }
}
