# Agent Sync Architecture

## Source: `src/agents/mod.rs`

### Flow

1. `canonical::load_all_agents(home)` reads `~/.agents/agents/*.md`
2. For each `SubagentStrategy` in `SubagentRegistry::synced_strategies()`:
   - Get `agents_dir(home)` path
   - For each canonical agent: `format_agent(agent)` → write to `agents_dir/{name}.md`
   - Orphan cleanup: compare `.manifest.json` synced names vs current canonical names; remove stale files
3. Save updated `.manifest.json`

### SubagentStrategy trait

```rust
pub trait SubagentStrategy {
    fn agent_type(&self) -> AgentType;
    fn agents_dir(&self, home: &Path) -> PathBuf;
    fn format_agent(&self, agent: &ParsedAgentFile) -> anyhow::Result<String>;
}
```

### Registered strategies

| Strategy | Agent dir | Output format |
|----------|-----------|---------------|
| `ClaudeAgentStrategy` | `~/.claude/agents/` | `<name>.md` (pass-through) |
| `OpenCodeAgentStrategy` | `~/.config/opencode/agent/` | `<name>.md` (pass-through) |
| `GeminiAgentStrategy` | `~/.gemini/agents/` | `<name>/agent.json` (Agy customAgent JSON) |
| `CodexAgentStrategy` | `~/.codex/skills/` | `<name>/agents/openai.yaml` |

### SubagentStrategy trait

```rust
pub trait SubagentStrategy {
    fn agent_type(&self) -> AgentType;
    fn agents_dir(&self, home: &Path) -> PathBuf;
    fn format_agent(&self, agent: &ParsedAgentFile) -> anyhow::Result<String>;
    fn agent_target_path(&self, agents_dir: &Path, agent_name: &str) -> PathBuf {
        // Default: agents_dir/<name>.md
        agents_dir.join(format!("{}.md", agent_name))
    }
}
```

Claude and OpenCode use the default `agent_target_path`. Gemini overrides to return
`<agents_dir>/<name>/agent.json`. Codex overrides to return `<agents_dir>/<name>/agents/openai.yaml`.

### Codex agent format (openai.yaml)

Codex discovers agents at `skill_root / "agents" / "openai.yaml"` inside each skill directory.
The `~/.codex/agents/*.md` path is NOT consumed.

```yaml
interface:
  display_name: lean-math-expert
  short_description: "Short description here"
  default_prompt: |
    Full system prompt here...
```

Fields: `display_name`, `short_description`, `default_prompt` under `interface`.
Reference examples: `~/.codex/skills/.system/skill-creator/agents/openai.yaml`,
`~/.codex/skills/.system/playwright/agents/openai.yaml`.

### Agy (Gemini CLI) agent format (agent.json)

Agy uses `customAgent` config in JSON format. Tool names must be mapped:

| Canonical | Agy tool name |
|-----------|--------------|
| Read | view_file |
| Edit | replace_file_content |
| Write | write_to_file |
| Grep | grep_search |
| Glob | find_by_name |
| Bash | run_command |
| WebFetch | read_url_content |

```json
{
  "name": "agent-name",
  "description": "Description",
  "hidden": true,
  "config": {
    "customAgent": {
      "systemPromptSections": [
        { "title": "Agent System Instructions", "content": "..." }
      ],
      "toolNames": ["view_file", "run_command"],
      "systemPromptConfig": {
        "includeSections": ["user_information", "mcp_servers", "skills",
          "subagent_reminder", "messaging", "artifacts", "user_rules"]
      }
    }
  }
}
```

Agy discovers workspace-level agents at `<project_root>/.agents/agents/<name>/agent.json`.
Home-level discovery from `~/.gemini/agents/` is unverified.

### Frontmatter parsing

`ParsedAgentFile` is deserialized via `serde_yaml`. The `tools` field expects a YAML sequence:

```yaml
# WRONG — parse error, agent silently skipped
tools: Read, Edit, Write

# CORRECT
tools:
  - Read
  - Edit
  - Write
```

Other list fields (if any) follow the same rule.

### Manifest format

`~/.agents/agents/.manifest.json`:

```json
{
  "claude": ["vision", "python-uv", "lean-math-expert"],
  "opencode": ["vision", "python-uv", "lean-math-expert"],
  "gemini": ["vision", "python-uv", "lean-math-expert"]
}
```

Used for orphan detection: names in manifest but not in canonical = candidates for removal.

### Missing coverage

Antigravity has no `SubagentStrategy` implementation. Adding one requires:
1. New `src/agents/antigravity.rs`
2. Implement `SubagentStrategy` with correct `agents_dir` and `format_agent`
3. Add to `SubagentRegistry::synced_strategies()`
4. Add `AgentType` variant if not present

### Per-agent format differences

| Agent | Format | Notes |
|-------|--------|-------|
| Claude | `.md` with YAML frontmatter | Pass-through from canonical |
| OpenCode | `.md` with YAML frontmatter | Pass-through from canonical |
| Gemini/Agy | `agent.json` (customAgent JSON) | Tool name mapping required |
| Codex | `openai.yaml` | `interface.display_name/short_description/default_prompt` |
