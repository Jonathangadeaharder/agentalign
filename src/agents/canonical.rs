use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentPermission {
    #[serde(default = "default_allow")]
    pub edit: String,
    #[serde(default = "default_allow")]
    pub bash: String,
}

fn default_allow() -> String {
    "allow".to_string()
}

impl Default for AgentPermission {
    fn default() -> Self {
        Self {
            edit: "allow".to_string(),
            bash: "allow".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalAgentDefinition {
    pub description: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub permission: AgentPermission,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_yaml::Value>,
}

fn default_mode() -> String {
    "subagent".to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedAgentFile {
    pub name: String,
    pub frontmatter: CanonicalAgentDefinition,
    pub body: String,
}

pub fn parse_agent_md(content: &str, filename: &str) -> anyhow::Result<ParsedAgentFile> {
    let name = filename
        .strip_suffix(".md")
        .unwrap_or(filename)
        .to_string();

    if !content.starts_with("---") {
        anyhow::bail!("Agent file must start with YAML frontmatter (---)");
    }

    let rest = &content[3..];
    let end = rest
        .find("\n---")
        .ok_or_else(|| anyhow::anyhow!("Missing closing --- in frontmatter"))?;
    let yaml_str = &rest[..end];
    let body = rest[end + 4..].trim_start().to_string();

    let frontmatter: CanonicalAgentDefinition = serde_yaml::from_str(yaml_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse frontmatter for '{}': {}", name, e))?;

    Ok(ParsedAgentFile {
        name,
        frontmatter,
        body,
    })
}

pub fn write_agent_md(parsed: &ParsedAgentFile) -> anyhow::Result<String> {
    let yaml_str = serde_yaml::to_string(&parsed.frontmatter)
        .map_err(|e| anyhow::anyhow!("Failed to serialize frontmatter: {}", e))?;
    let yaml_trimmed = yaml_str.trim_end_matches('\n');

    Ok(format!("---\n{}\n---\n\n{}", yaml_trimmed, parsed.body))
}

pub fn canonical_agents_dir(home: &Path) -> std::path::PathBuf {
    home.join(".agents").join("agents")
}

pub fn load_all_agents(home: &Path) -> anyhow::Result<Vec<ParsedAgentFile>> {
    let dir = canonical_agents_dir(home);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut agents = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        let content = std::fs::read_to_string(&path)?;
        match parse_agent_md(&content, filename) {
            Ok(agent) => agents.push(agent),
            Err(e) => eprintln!("  warning: skipping agent '{}': {}", filename, e),
        }
    }

    Ok(agents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_agent_md_basic() {
        let content = "---\ndescription: Test agent\nmode: subagent\nmodel: test-model\npermission:\n  edit: allow\n  bash: deny\n---\n\nRules body here.";
        let parsed = parse_agent_md(content, "test-agent.md").unwrap();
        assert_eq!(parsed.name, "test-agent");
        assert_eq!(parsed.frontmatter.description, "Test agent");
        assert_eq!(parsed.frontmatter.mode, "subagent");
        assert_eq!(parsed.frontmatter.model, Some("test-model".to_string()));
        assert_eq!(parsed.frontmatter.permission.edit, "allow");
        assert_eq!(parsed.frontmatter.permission.bash, "deny");
        assert_eq!(parsed.body, "Rules body here.");
    }

    #[test]
    fn test_parse_agent_md_with_tools() {
        let content = "---\ndescription: Vision agent\nmode: subagent\nmodel: kimi-k2.6\npermission:\n  edit: deny\n  bash: deny\ntools:\n  - read_file\n  - grep_search\n---\n\nBody text.";
        let parsed = parse_agent_md(content, "vision.md").unwrap();
        assert_eq!(parsed.name, "vision");
        assert_eq!(parsed.frontmatter.tools, vec!["read_file", "grep_search"]);
    }

    #[test]
    fn test_parse_agent_md_defaults() {
        let content = "---\ndescription: Minimal agent\n---\n\nBody.";
        let parsed = parse_agent_md(content, "minimal.md").unwrap();
        assert_eq!(parsed.frontmatter.mode, "subagent");
        assert_eq!(parsed.frontmatter.permission.edit, "allow");
        assert_eq!(parsed.frontmatter.permission.bash, "allow");
        assert!(parsed.frontmatter.model.is_none());
        assert!(parsed.frontmatter.tools.is_empty());
    }

    #[test]
    fn test_parse_agent_md_no_frontmatter() {
        let content = "Just some text without frontmatter.";
        let result = parse_agent_md(content, "bad.md");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_agent_md_unclosed_frontmatter() {
        let content = "---\ndescription: Test\n\nNo closing delimiter.";
        let result = parse_agent_md(content, "bad.md");
        assert!(result.is_err());
    }

    #[test]
    fn test_write_agent_md_roundtrip() {
        let content = "---\ndescription: Test agent\nmode: subagent\nmodel: test-model\npermission:\n  edit: allow\n  bash: deny\n---\n\nRules body here.";
        let parsed = parse_agent_md(content, "test.md").unwrap();
        let output = write_agent_md(&parsed).unwrap();
        let reparsed = parse_agent_md(&output, "test.md").unwrap();
        assert_eq!(parsed, reparsed);
    }
}
