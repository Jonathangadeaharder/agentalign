# Codex & Agy Agent Format Research (2026-06-12)

## Codex Agent Discovery

Codex CLI v0.139.0 discovers agents at:
```
agent_yaml_path = skill_root / "agents" / "openai.yaml"
```

Each skill directory under `~/.codex/skills/` can contain an `agents/` subdirectory
with an `openai.yaml` file. This was confirmed via binary string extraction from
the Codex binary.

### Verified examples from bundled skills

1. `~/.codex/skills/.system/skill-creator/agents/openai.yaml`
2. `~/.codex/skills/.system/playwright/agents/openai.yaml` (model: o3, tools: [shell, text_editor])
3. `~/.codex/skills/.system/openai-docs/agents/openai.yaml`
4. `~/.codex/skills/.system/imagegen/agents/openai.yaml`
5. `~/.codex/skills/.system/plugin-creator/agents/openai.yaml`
6. `~/.codex/skills/.system/skill-installer/agents/openai.yaml`

### openai.yaml format

```yaml
interface:
  display_name: Agent Name
  short_description: One-line description
  default_prompt: |
    Full system prompt text here...
```

### Feature flags

- `multi_agent` — stable, agents work
- `child_agents_md` — under dev, enabled but non-functional
- `multi_agent_v2` — under dev

### What does NOT work

- `~/.codex/agents/*.md` — NOT scanned by Codex. Initial implementation wrote here; incorrect.
- `child_agents_md` feature flag — exists but doesn't enable agent discovery

## Agy (Gemini CLI) Agent Discovery

Agy v1.0.7 (Go binary) discovers agents at:
- **Workspace-level**: `{workspace}/.agents/agents/{agent_name}/agent.json`
- **Plugin-level**: Inside plugin directories (agents/ subdir per plugin)
- **Runtime (conversation-level)**: `~/.gemini/antigravity-cli/brain/<conv-id>/.agents/agents/<name>/agent.json`

### Unverified paths

- `~/.gemini/agents/<name>/agent.json` — format is correct, but no confirmed filesystem scanner
  for this path in the binary. Agy may only discover agents from workspace `.agents/` dirs.

### Key binary strings found

- `{workspace}/.agents/agents/{agent_name}/agent.json` — workspace discovery pattern
- `custom_agent_config_absolute_uri` — Agy references agents via absolute URI
- `CustomAgentSpec cannot be provided if static config is already specified by the agent script`
- `CustomAgentSpec` — protobuf definition for agent configs
- `internal.CloudCode/ListAgents` — RPC for listing agents
- `GetSubagents` / `HasSubagents` — protobuf fields
- `learning/gemini/agents/skills` — internal Google3 path (not relevant for local installs)
- `configs/users/<username>/_agents` — Citc client path (internal, not relevant)
- `Failed to get default GeminiDir: %v, using hardcoded .gemini` — GeminiDir resolution

### agent.json format (verified against runtime files)

```json
{
  "name": "agent-name",
  "description": "Description text",
  "hidden": true,
  "config": {
    "customAgent": {
      "systemPromptSections": [
        {
          "title": "Agent System Instructions",
          "content": "System prompt body..."
        }
      ],
      "toolNames": ["view_file", "replace_file_content", "run_command"],
      "systemPromptConfig": {
        "includeSections": [
          "user_information", "mcp_servers", "skills",
          "subagent_reminder", "messaging", "artifacts", "user_rules"
        ]
      }
    }
  }
}
```

Verified against: `~/.gemini/antigravity-cli/brain/d4661b70-505f-4b09-b872-31f4b4700ed8/.agents/agents/python-uv/agent.json`

### Tool name mapping (canonical → Agy)

| Canonical tool | Agy tool name |
|---------------|--------------|
| Read | view_file |
| Edit | replace_file_content |
| Write | write_to_file |
| Grep | grep_search |
| Glob | find_by_name |
| Bash | run_command |
| WebFetch | read_url_content |
| send_message | send_message |
| multi_replace_file_content | multi_replace_file_content |
| list_dir | list_dir |
| search_web | search_web |
| schedule | schedule |
| manage_task | manage_task |
| define_subagent | define_subagent |
| invoke_subagent | invoke_subagent |
| manage_subagents | manage_subagents |
| call_mcp_tool | call_mcp_tool |

### /agents command

Agy has a `/agents` slash command for listing/invoking agents in interactive mode.
