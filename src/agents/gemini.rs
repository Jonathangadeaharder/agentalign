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

    /// Gemini agents_dir is a Gemini-specific copy at ~/.gemini/agents.
    /// Agy discovers agents from ~/.agents/agents/*.md directly (the canonical
    /// store), so the .md files written here are a reference copy — NOT the
    /// canonical source. This avoids overwriting ~/.agents/agents/ on sync.
    /// The runtime agent.json files are written by post_sync to
    /// ~/.gemini/agents/<name>/agent.json.
    fn agents_dir(&self, home: &Path) -> std::path::PathBuf {
        home.join(".gemini").join("agents")
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

    let tool_names: Vec<Value> = if !agent.frontmatter.tools.is_empty() {
        // Explicit tools list — map canonical names to Agy names
        agent
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
            .collect()
    } else {
        // No explicit tools — derive from full Agy tool set, excluding denied
        // tools based on canonical permission {edit, bash}.
        let all_tools = vec![
            "view_file",
            "replace_file_content",
            "write_to_file",
            "grep_search",
            "find_by_name",
            "run_command",
            "read_url_content",
        ];
        all_tools
            .iter()
            .filter(|t| match **t {
                "replace_file_content" | "write_to_file" => {
                    agent.frontmatter.permission.edit != "deny"
                }
                "run_command" => agent.frontmatter.permission.bash != "deny",
                _ => true,
            })
            .map(|t| Value::String(t.to_string()))
            .collect()
    };

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
    fn test_gemini_tool_names_from_permission() {
        // When tools is empty and permission denies edit+bash, toolNames
        // should exclude replace_file_content, write_to_file, run_command
        // but include view_file, grep_search, find_by_name, read_url_content.
        let mut agent = make_agent();
        agent.frontmatter.tools = vec![]; // no explicit tools
        // permission is already {edit: deny, bash: deny} from make_agent()
        let output = format_agent_json(&agent).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let tools: Vec<String> = parsed["config"]["customAgent"]["toolNames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();

        // Should have read-only tools only
        assert!(tools.contains(&"view_file".to_string()));
        assert!(tools.contains(&"grep_search".to_string()));
        assert!(tools.contains(&"find_by_name".to_string()));
        assert!(tools.contains(&"read_url_content".to_string()));
        // Should NOT have edit/bash tools
        assert!(!tools.contains(&"replace_file_content".to_string()));
        assert!(!tools.contains(&"write_to_file".to_string()));
        assert!(!tools.contains(&"run_command".to_string()));
        // Should not be empty
        assert!(!tools.is_empty());
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
            Path::new("/home/user/.gemini/agents")
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
