---
name: agentalign
description: "Synchronize MCP tool configs, skills, and rules across coding agents (OpenCode, Claude, Cursor, Gemini, Codex) via ~/.agents/ canonical store."
version: 1.0.0
author: Hermes Agent
license: MIT
metadata:
  hermes:
    tags: [MCP, Config-Sync, Multi-Agent, DevTooling]
    related_skills: [opencode, claude-code, gemini-cli, codex, native-mcp]
---

# agentalign — Agent Configuration Unification Engine

CLI tool (Rust, `~/projects/agentalign`) that keeps MCP server configs synchronized across all coding agents on a machine. One source of truth in `~/.agents/`, bidirectional sync via file watcher or manual CLI.

## When to Use

- Adding/removing/toggling an MCP server and wanting it propagated to all agents
- Diagnosing why an MCP server appears in one agent but not another
- Understanding the sync pipeline between agent configs
- Debugging zombie/orphan entries (server removed but tool rules remain)

## Architecture

```
~/.agents/mcp_config.json          ← canonical MCP config (source of truth, OpenCode-format)
~/.agents/.sync_state.json         ← SHA-256 hashes per agent file (loop prevention)
~/.agents/cache.toml               ← transaction log with backup refs (for rollback)
~/.agents/backups/                 ← pre-sync file backups
~/.agents/local_entries.json       ← User-preserved keys exempt from removal (JSON array of server names)
~/.agents/agent_skip.json           ← Per-agent skip list: servers NOT pushed to specific agents during sync (JSON object: {"AgentName": ["server1"]})
~/.agents/agents/                  ← canonical agent definitions (source of truth for agent sync)
~/.agents/agents/.manifest.json    ← tracks which agent names have been synced per agent type
~/.agents/AGENTS.md                ← canonical instruction file (symlinked to per-agent CLAUDE.md etc.)
~/.agents/skills/                  ← canonical skill directories (symlinked into per-agent skills/ dirs)

Agent configs (watched):
  ~/.claude/.mcp.json              ← ClaudeStrategy: mcpServers, command+args split
  ~/.cursor/mcp.json               ← ClaudeStrategy { is_cursor: true }
  ~/.gemini/config/mcp_config.json ← GeminiStrategy: mcp + transport field
  ~/.config/opencode/opencode.json ← OpenCodeStrategy: mcp + command array + tools map
  ~/.codex/config.toml             ← CodexStrategy: TOML format
  ~/.gemini/antigravity/mcp_config.json ← AntigravityStrategy: mcpServers, command+args, env support

Agent definitions (synced from ~/.agents/agents/ canonical):
  ~/.claude/agents/<name>.md           ← ClaudeAgentStrategy: markdown with YAML frontmatter
  ~/.config/opencode/agent/<name>.md   ← OpenCodeAgentStrategy: same format as Claude
  ~/.gemini/agents/<name>/agent.json   ← GeminiAgentStrategy: Agy customAgent JSON format
  ~/.codex/skills/<name>/agents/openai.yaml ← CodexAgentStrategy: Codex openai.yaml inside skill dirs
```

## CLI Commands

```bash
agentalign migrate [--dry-run]    # Scan existing agent configs → build ~/.agents/mcp_config.json
agentalign sync [--dry-run]       # Push canonical → all agent configs (skips unchanged files)
agentalign add <NAME> [--type local|remote] [--command "..."] [--url "..."] [--enabled] [--no-sync] [--dry-run]
                                   # Add MCP server to canonical + propagate
agentalign remove <NAME> [--no-sync] [--dry-run]
                                   # Remove MCP server from canonical + propagate + cleanup orphans
agentalign restore [--agent X] [--id UUID] [--list]  # Rollback transactions
agentalign magic on|off|status    # Toggle LaunchAgent file watcher (auto bidirectional sync)
agentalign watch                  # Run file watcher daemon directly
```

## Sync Domains

agentalign syncs THREE independent domains, each with its own canonical store and strategy:

| Domain | Canonical | Strategies | Mechanism |
|--------|-----------|------------|-----------|
| MCP servers | `~/.agents/mcp_config.json` | Claude, Cursor, Gemini, OpenCode, Codex, Antigravity | Format conversion (see Format Differences) |
| Agent definitions | `~/.agents/agents/*.md` | Claude, OpenCode, Gemini, Codex | Format conversion per strategy; orphan cleanup via `.manifest.json` |
| Instruction files | `~/.agents/AGENTS.md` | Claude (CLAUDE.md), OpenCode (AGENTS.md), Gemini (GEMINI.md), Codex (CODEX.md) | Symlink |
| Skills | `~/.agents/skills/*/` | Claude, Gemini, Codex | Symlink |

### Agent Sync (`src/agents/`)

`sync_agents()` reads all `.md` files from `~/.agents/agents/`, parses frontmatter, and writes
each to the three registered agent dirs via `SubagentStrategy::format_agent()`. Orphans (agents
that were previously synced but removed from canonical) are cleaned up. A `.manifest.json` tracks
which names were synced per agent type.

**SubagentRegistry covers Claude, OpenCode, Gemini/Agy, and Codex.** Each strategy uses the correct proprietary format for its target system.

**Per-system agent formats:**

| System | Discovery format | Discovery path | Runtime config |
|--------|-----------------|----------------|----------------|
| Claude | `.md` + YAML frontmatter | `~/.claude/agents/<name>.md` | — |
| OpenCode | `.md` + YAML frontmatter | `~/.config/opencode/agent/<name>.md` | — |
| Agy | `.md` + YAML frontmatter | `~/.agents/agents/<name>.md` (canonical store) | `~/.gemini/agents/<name>/agent.json` (CustomAgentSpec) |
| Codex | `openai.yaml` | `~/.codex/skills/<name>/agents/openai.yaml` | — |

**Agy details:** Agy discovers agents from the canonical `~/.agents/agents/*.md` directly — no separate copy needed. The `GeminiAgentStrategy.agents_dir` returns the canonical path so `sync_agents` is idempotent for discovery. The `post_sync` hook writes `agent.json` runtime configs to `~/.gemini/agents/<name>/agent.json` with `CustomAgentSpec` protobuf-style config (systemPromptSections, toolNames, systemPromptConfig). Tool name mapping: Read→view_file, Edit→replace_file_content, Write→write_to_file, Grep→grep_search, Glob→find_by_name, Bash→run_command, WebFetch→read_url_content.

**Codex details:** Codex scans `agents/openai.yaml` inside each skill directory under `~/.codex/skills/`. The `CodexAgentStrategy.agent_target_path` returns the nested path `<skills_dir>/<name>/agents/openai.yaml`. The `child_agents_md` feature flag exists but is non-functional. The `multi_agent` feature is stable.

**Trait:** `SubagentStrategy` has `post_sync()` hook (default no-op) called after all agents are synced for a strategy — used for additional side-effects like writing runtime configs.
| OpenCode | `.md` with YAML frontmatter | `~/.config/opencode/agent/<name>.md` | Pass-through from canonical |
| Gemini/Agy | `agent.json` (customAgent JSON) | `~/.gemini/agents/<name>/agent.json` | Format conversion; tool name mapping (Read→view_file, Edit→replace_file_content, Write→write_to_file, Grep→grep_search, Glob→find_by_name, Bash→run_command, WebFetch→read_url_content) |
| Codex | `openai.yaml` | `~/.codex/skills/<name>/agents/openai.yaml` | Format conversion; `interface.display_name`, `interface.short_description`, `interface.default_prompt`; Codex discovers agents at `skill_root / "agents" / "openai.yaml"` |

**Agy agent discovery caveat:** Agy discovers workspace-level agents at `<project_root>/.agents/agents/<name>/agent.json`. Home-level discovery from `~/.gemini/agents/` is unverified — the format is correct but the path may not be scanned. If Agy agents don't appear, place `.agents/agents/` symlinks in project roots instead.



## Sync Flow

### Manual (one-shot)
1. User edits `~/.agents/mcp_config.json` (or runs `agentalign add/remove`)
2. `agentalign sync` reads canonical, converts to each agent's native format via strategy, writes
3. Non-destructive: skips files whose content is already identical to the serialized output
4. Transaction recorded in `cache.toml` with pre-sync backup

### Magic mode (bidirectional via LaunchAgent)
1. `notify` crate watches all agent config paths + canonical (FSEvents on macOS)
2. 500ms debounce on file change events
3. Change detection: compare SHA-256 hash against `.sync_state.json`
4. If canonical changed → regenerate all agent configs
5. If agent config changed → compute delta via `delta_merger::compute_delta(canonical, agent, local_entries)` → apply add/update/remove to canonical → propagate to other agents
6. If agent config deleted → recreate from canonical
7. OpenCode: `post_sync_cleanup()` removes orphan `tools.*` entries referencing removed MCP servers

### Strategy Pattern
Each agent has a `McpFormatStrategy` implementing `ConfigurationAdapter`:
- `deserialize_to_canonical(raw, base_path)` — parse native format → canonical JSON
- `serialize_from_canonical(canonical, base_path)` — canonical JSON → native format
- `target_config_path(base_path)` — where the agent's config lives
- `validate(state)` — check canonical compatibility
- `capabilities()` — transport capabilities (stdio, SSE, HTTP)
- `post_sync_cleanup(&mut doc, mcp_server_names)` — remove orphan entries after mcp section written (default no-op; OpenCode cleans `tools.*` orphans)

## Resolved Bugs (fixed 2026-05-21)

All four original bugs have been fixed:

- **P1 FIXED**: Watch daemon now uses `delta_merger::compute_delta()` instead of additive-only insert. Deletions in agent configs propagate to canonical and then to other agents. Argument order: `compute_delta(&canonical_servers, &agent_servers, &local_entries)` so `entries_to_remove` = canonical keys absent from agent.
- **P2 FIXED**: `post_sync_cleanup()` added to `ConfigurationAdapter` trait (default no-op). OpenCode override scans `tools` map, removes entries whose server-name prefix (`name*` or `name_tool`) isn't in current `mcp` keys.
- **P3 FIXED**: `agentalign add` and `agentalign remove` subcommands added. Both modify canonical then optionally sync (`--no-sync` to skip, `--dry-run` to preview).
- **P4 FIXED**: Deleted agent configs are now detected and recreated from canonical instead of silently skipped.

## Remaining Issues
- **Backup filename collision.** Backups use `{Agent}_{timestamp}.bak`. Two syncs in the same second overwrite the same backup.

## References

- `references/deletion-sync-plan.md` — Plan for P1 deletion-sync fix
- `references/antigravity-mcp-integration.md` — Antigravity strategy implementation details, config discovery path, SonarCloud Free plan MCP restriction
- `references/opencode-format.md` — OpenCode MCP config format details (Go struct, serialize/deserialize strategy, Viper config merging, watcher reverse-merge bug fix)
- `references/sonarcloud-community-mcp.md` — SonarCloud community MCP server setup for Free plan (two-gateway problem, token generation, agent-specific config formats)
- `references/agent-sync.md` — Agent definition sync architecture (SubagentStrategy trait, frontmatter parsing, manifest format, per-system format differences)
- `references/codex-agi-agent-formats.md` — Codex openai.yaml and Agy agent.json format research (discovery paths, field schemas, tool name mapping, feature flags)

## Canonical Format (OpenCode-derived)

```json
{
  "mcp": {
    "server-name": {
      "type": "local",
      "command": ["npx", "@pkg/mcp"],
      "enabled": true
    },
    "remote-server": {
      "type": "remote",
      "url": "https://api.example.com/mcp",
      "headers": { "Authorization": "Bearer ..." }
    }
  }
}
```

Key: `command` is always an array (unlike Claude's `command`+`args` split). `type` is always explicit (`"local"` or `"remote"`). Extra fields preserved via `serde(flatten)`.

## Format Differences Across Agents

| Agent | Key | Command Format | Type Value | Remote | Extras |
|-------|-----|---------------|-----------|--------|--------|
| OpenCode | `mcp` | `command:"npx", args:["-y","pkg"]` (string+array) | `"stdio"` | `url` | `env:["KEY=VAL"]`, `tools` map, `headers`, `enabled` |
| Claude | `mcpServers` | `command:"npx", args:["pkg"]` | `"stdio"` | `url` only | `env` not supported |
| Cursor | `mcpServers` | same as Claude | `"stdio"` | same | no `/` or `\` in IDs |
| Gemini | `mcp` | `command:["npx","pkg"]` (array) | `"local"` | `transport:"sse"` | different transport field |
| Codex | TOML | `command=["npx","pkg"]` | `"local"` | `url`, `transport` | TOML tables |
| Antigravity | `mcpServers` | same as Claude | `"stdio"` | same | `env:{}` object supported, no ID restrictions |

**OpenCode needs three conversions from canonical.** (1) `type`: canonical `"local"` → `"stdio"`, canonical `"remote"` → `"sse"`. (2) `command`: canonical array `["npx","-y","pkg"]` → OpenCode string `"npx"` + `args:["-y","pkg"]`. (3) `env`: canonical `{"KEY":"VAL"}` → OpenCode `["KEY=VAL"]`. The Go source (`internal/config/config.go`) defines `MCPServer{Command string, Args []string, Env []string, Type MCPType}` where `MCPStdio="stdio"`, `MCPSse="sse"`. **Trust the Go source — the Bun runtime uses the same Go config struct via CGo.**

## Debugging Checklist

1. Check canonical MCP: `cat ~/.agents/mcp_config.json | python3 -m json.tool`
2. Check sync state: `cat ~/.agents/.sync_state.json`
3. Check transaction log: `cat ~/.agents/cache.toml`
4. Find a server across all configs: `grep -r "server-name" ~/.claude/ ~/.cursor/ ~/.gemini/ ~/.config/opencode/`
5. Check magic mode: `agentalign magic status` / `launchctl list com.agentalign.magic`
6. Watch daemon logs: `tail -f /tmp/agentalign.log /tmp/agentalign.err`
7. **Check agent sync:** `cat ~/.agents/agents/.manifest.json` — see which agent names synced where
8. **Verify agent definitions per-system:** `ls ~/.claude/agents/ ~/.config/opencode/agent/ ~/.gemini/agents/`
9. **Check instruction symlinks:** `ls -la ~/.claude/CLAUDE.md ~/.gemini/GEMINI.md ~/.codex/CODEX.md ~/.config/opencode/AGENTS.md`

## Pitfalls

- **Antigravity uses `{env:VAR}` in mcp-remote `--header` args.** This syntax is resolved by `mcp-remote` from the subprocess environment. macOS GUI apps (Antigravity, Cursor) don't inherit shell env vars from `.zshrc`. Fix: add the `"env"` field to MCP server entries so the VS Code fork passes the env var to the `npx` subprocess. Example: `"env": {"SONAR_TOKEN": "actual_value"}`.
- **SonarCloud: `api.sonarcloud.io` vs `sonarcloud.io/api` — different gateways.** The domain `api.sonarcloud.io` is a separate AWS API Gateway that returns 403 for ALL requests (even unauthenticated, even with valid PATs) on Free-tier orgs. The domain `sonarcloud.io/api` is the working REST API gateway. MCP configs pointing to `api.sonarcloud.io/mcp` will always 403 on Free plan. **Workaround:** Use `community-sonarcloud-mcp-server` (npm) which calls `sonarcloud.io/api` via stdio — no `mcp-remote` needed, works on Free plan. Config: `{"command":"npx","args":["-y","community-sonarcloud-mcp-server"],"env":{"SONARCLOUD_TOKEN":"...","SONARCLOUD_ORGANIZATION":"..."}}`.
- **OpenCode MCP format: trust Go source, NOT older Bun errors.** OpenCode's Go source (`internal/config/config.go`) is authoritative: `type:"stdio"` (not `"local"`), `command:"npx"` (string, not array), `args:[]string`, `env:[]string("KEY=VALUE")`. The v1.15.12 runtime accepts these. A previous session incorrectly concluded the Bun runtime rejects `"stdio"` — that was caused by a different bug (missing `agent` section in config, not the `type` value). If you see `ConfigInvalidError`, check for missing non-MCP sections (`agent`, `provider`, etc.) before blaming the `type` field.
- **Watcher reverse-merge can corrupt canonical.** When `agentalign sync` writes agent-specific format (e.g., OpenCode `command:"npx"` instead of canonical `command:["npx","-y","pkg"]`), the watcher detects the file change, deserializes back to canonical, and overwrites the canonical with the lossy version — losing `args`, flattening `env`, etc. **Guard (implemented):** the watcher's `entries_to_update` loop skips canonical entries that already have `command` or `url` data, preventing format-conversion round-trips from stripping fields. If the guard is missing or bypassed, canonical gets reduced to `{type: "local"}` shells.
- **Adding a new agent to agentalign requires updating 6+ locations.** The agent list is hardcoded in: `factory.rs` (enum + `from_agent` + `all` + `from_name`), `capabilities.rs` (new fn), `watch.rs` (3 agent path lists), `main.rs` (discover + sync + migrate + list = 4 lists). Missing any one causes silent exclusion from that code path.
- **Agent definitions must use YAML-array frontmatter for list fields.** `tools: Read, Edit, Write` (comma-string) causes serde parse error → agent silently skipped during sync. Correct: `tools:\n  - Read\n  - Edit\n  - Write`. The canonical format (`~/.agents/agents/*.md`) uses the same frontmatter schema as Claude agent files; agentalign parses via `serde_yaml` which requires explicit YAML sequences, not comma-separated strings.
- **Agent sync covers 4 of 6 agents.** `SubagentRegistry::synced_strategies()` returns [OpenCode, Claude, Gemini, Codex]. Antigravity has no `SubagentStrategy` impl — agents added to canonical will NOT propagate to it.
- **Codex agents require `openai.yaml` inside skill dirs, NOT standalone `.md` files.** The correct path is `~/.codex/skills/<agent_name>/agents/openai.yaml`. The `~/.codex/agents/*.md` path is NOT consumed by Codex — it was a wrong initial implementation. The Codex binary contains `agent_yaml_path = skill_root / "agents" / "openai.yaml"` as the discovery logic.
- **Agy (Gemini CLI) agents use `agent.json` with `customAgent` config, NOT markdown.** The format is `{name, description, hidden, config: {customAgent: {systemPromptSections, toolNames, systemPromptConfig}}}`. Tool names must be mapped to Agy names (Read→view_file, Edit→replace_file_content, etc.). The format was verified against runtime `agent.json` files created by Agy in `~/.gemini/antigravity-cli/brain/<id>/.agents/agents/`.
- **Agy home-level agent discovery is unverified.** Agy discovers workspace-level agents at `<project_root>/.agents/agents/<name>/agent.json` and via plugins. The `~/.gemini/agents/` path is written by agentalign but may not be scanned by Agy at startup. The Agy binary contains `CustomAgentSpec`, `custom_agent_config_absolute_uri`, and `ListAgents` RPC but no confirmed home-level filesystem scanner.
- **SubagentStrategy trait now includes `agent_target_path`.** Strategies that output to nested directory structures (Codex: `<skill_dir>/agents/openai.yaml`, Agy: `<agents_dir>/<name>/agent.json`) override the default `agent_target_path(agents_dir, name)` method. The default implementation returns `agents_dir.join(format!("{}.md", name))` for Claude/OpenCode.
- **`local_entries.json` protects agent-local servers from removal.** `~/.agents/local_entries.json` is a JSON array of server names (e.g., `["github-copilot", "github"]`) that are exempt from removal during delta merge. Servers in this set won't be deleted even if absent from canonical.
- **`agent_skip.json` prevents servers from being pushed to specific agents.** `~/.agents/agent_skip.json` is a JSON object mapping agent labels to arrays of server names to skip during sync (e.g., `{"Cursor": ["github-copilot", "github"], "OpenCode": ["github-copilot", "github"]}`). Skipped servers are filtered from the canonical JSON before `serialize_from_canonical()` runs, so they're never written to that agent's config.
- **`local_entries.json` + `agent_skip.json` work together for agent-local auto-created servers.** Example: Cursor auto-creates `github-copilot` MCP on startup. To keep it only in Cursor: (1) add `["github-copilot", "github"]` to `local_entries.json` so sync doesn't remove it, (2) add `{"Cursor": ["github-copilot", "github"]}` to `agent_skip.json` so sync doesn't overwrite Cursor's auto-created entry, (3) `preserve_local_entries()` in `push_to_agents` re-merges local-entry servers from the existing file after serialize, ensuring they survive even one-way pushes. Without both mechanisms, the server either gets removed (no local_entries) or overwritten then removed (no agent_skip + preserve_local_entries).
- **Cursor auto-creates `github-copilot` MCP server.** When GitHub Copilot is enabled in Cursor, it automatically adds `{"github-copilot": {"type": "remote", "url": "https://api.githubcopilot.com/mcp/"}}` (and sometimes a duplicate `"github"` entry) to `~/.cursor/mcp.json`. If this server is in canonical, sync propagates it to all agents. If it's not in canonical, sync removes it, then Cursor re-adds it, then the watcher reverse-merges it back to canonical → sync loop. The correct fix: keep it out of canonical, protect with local_entries + agent_skip.
- **Watcher reverse-merge can also corrupt non-MCP sections of agent configs.** When the watcher corrupts canonical (losing `command`/`env` data), the next `agentalign sync` reads the stripped canonical and writes it to agent configs. Since `serialize_from_canonical()` preserves existing non-MCP sections from the agent config file, this is usually safe. But if the agent config was overwritten by the watcher's own `sync_selected_agents()` call (which fires after reverse-merge), the non-MCP sections may already be lost. **Recovery:** restore from `.bak` files or re-add missing sections manually. Always verify after a watcher bug incident.
- **Watcher daemon is managed by launchd (`com.agentalign.magic`).** It respawns automatically after kill. To actually stop it: `launchctl stop com.agentalign.magic && launchctl remove com.agentalign.magic`. To restart: `launchctl load ~/Library/LaunchAgents/com.agentalign.magic.plist`. Always stop the watcher before manually restoring canonical — otherwise it will re-corrupt it within seconds.
- **Canonical restoration pattern after watcher corruption.** (1) Stop watcher via launchctl. (2) Restore `~/.agents/mcp_config.json` with full data (command arrays, env objects). (3) Run `agentalign sync`. (4) Verify each agent config has correct non-MCP sections (check `.bak` files for reference). (5) Restart watcher. (6) Wait 3s, verify canonical still intact.
- **`delta_merger::compute_delta` argument order matters.** For agent→canonical propagation: call `compute_delta(&canonical_servers, &agent_servers, &local_entries)`. This makes `entries_to_remove` = keys in canonical but absent from agent (agent deleted them). Swapping args inverts the meaning.
- **OpenCode `tools` map orphans are auto-cleaned** by `post_sync_cleanup`. If you see stale `tools.*` entries, the watch daemon may not have triggered — run `agentalign sync` manually.
- **Sync is full-replace on `mcp` section, but `preserve_local_entries()` rescues local servers.** `agentalign sync` replaces the entire `mcp`/`mcpServers` section. Agent-specific keys not in canonical are lost unless: (1) they're in `local_entries.json` AND (2) `preserve_local_entries()` re-merges them from the existing file after `serialize_from_canonical()` produces output. The function tries both `"mcpServers"` and `"mcp"` keys to handle all agent formats.
- **Sync preserves non-MCP sections but cannot recover lost data.** `serialize_from_canonical()` reads the existing agent config and only replaces the `mcp`/`mcpServers` key — other sections (`agent`, `provider`, `model`, etc.) are preserved. But if the canonical was previously corrupted (e.g., by watcher reverse-merge) and sync already wrote a stripped version, the non-MCP sections in the agent config are already gone. **Always verify agent configs after a sync that follows a watcher bug.** Keep backups (`.bak` files) for recovery.
- **Transaction `checksum_after` is empty.** In `cache.toml`, many transactions have `checksum_after = ""` and `status = "pending"` — `finalize_transaction` isn't always called. Rollback may fail if the backup checksum doesn't match.
- **Backup filename collision.** Backups use `{Agent}_{timestamp}.bak`. Two syncs in the same second overwrite the same backup.
- **Rust duplicate impl blocks.** When adding a trait method to an existing `impl Trait for Type`, merge into the existing block. Separate `impl` blocks for the same trait+type cause `E0119: conflicting implementations`.
- **macOS codesign after binary replacement.** Replacing `~/.local/bin/agentalign` with `cp target/release/agentalign ~/.local/bin/` triggers macOS `com.apple.provenance` xattr → SIGKILL on launch. Fix: `codesign -s - --force --deep ~/.local/bin/agentalign` after every `cp`. This is required for ANY Rust binary installed to a user-local bin dir on macOS.
