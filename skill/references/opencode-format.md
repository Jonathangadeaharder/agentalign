# OpenCode MCP Config Format — Detailed Notes

## OpenCode Native Format (v1.15.12)

The Go source on GitHub (`internal/config/config.go`) is authoritative:

```go
type MCPType string

const (
    MCPStdio MCPType = "stdio"
    MCPSse   MCPType = "sse"
)

type MCPServer struct {
    Command string            `json:"command"`
    Env     []string          `json:"env"`
    Args    []string          `json:"args"`
    Type    MCPType           `json:"type"`
    URL     string            `json:"url"`
    Headers map[string]string `json:"headers"`
}
```

Default: if `type` is empty, it defaults to `MCPStdio` ("stdio").

### Correct Format

```json
{
  "mcp": {
    "local-server": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "pkg"],
      "env": ["SONARCLOUD_TOKEN=abc123", "ORG=myorg"],
      "enabled": true
    },
    "remote-server": {
      "type": "sse",
      "url": "https://api.example.com/mcp",
      "headers": { "Authorization": "Bearer ..." }
    }
  }
}
```

### Key Differences from Canonical

| Field | Canonical | OpenCode |
|-------|-----------|----------|
| `command` | `["npx", "-y", "pkg"]` (array) | `"npx"` (string) + `args: ["-y", "pkg"]` — DIFFERENT |
| `env` | `{"KEY": "VALUE"}` (object) | `["KEY=VALUE"]` (array of strings) — DIFFERENT |
| `type` | `"local"` / `"remote"` | `"stdio"` / `"sse"` — DIFFERENT |

**Three fields need conversion** (not just `env` as previously documented).

## Serialize Strategy (canonical → OpenCode)

1. `type`: map `"local"` → `"stdio"`, `"remote"` → `"sse"`, pass through others
2. `command` array: split → first element becomes `command` (string), rest become `args` (array)
3. `env` object: convert each key-value pair to `"KEY=VALUE"` string array
4. Preserve `url`, `headers`, `enabled` as-is
5. Preserve unknown fields from `extra`

## Deserialize Strategy (OpenCode → canonical)

1. `type`: map `"stdio"` → `"local"`, `"sse"` → `"remote"`, pass through others
2. `command`: handle both string+args and array formats
   - If string: merge `command` + `args` into single array
   - If array: copy directly
3. `env` array: parse `"KEY=VALUE"` strings → `{"KEY": "VALUE"}` object
   - If already an object: pass through
4. Copy `url`, `headers`, `enabled` as-is

## Previous Mistake: "Trust Runtime Not Go Source"

A previous session incorrectly concluded that OpenCode v1.15.12's Bun runtime rejects `type:"stdio"` and `command:"npx"` (string), only accepting `type:"local"` and `command` as array. This was WRONG.

The actual cause of the `ConfigInvalidError` was that the `agent` section (and other non-MCP sections) had been stripped from the config file by a watcher reverse-merge corruption cascade. The error "4 of 5 requests failed: config.providers, provider.list, app.agents, config.get" was about missing config sections, not about the `type` field.

**Lesson:** When OpenCode shows startup errors about `config.providers`, `app.agents`, etc., check for missing non-MCP sections in `opencode.json` — especially `agent`, `provider`, `model`. The MCP `type` field is unlikely to be the cause.

## MCP Startup Failure: -32000 Connection Closed

If an MCP server fails with `MCP error -32000: Connection closed`, the subprocess likely crashed immediately. Common causes:

1. **Missing env vars**: The server needs `env` but OpenCode isn't passing them. Verify the `env` field uses `["KEY=VALUE"]` format (not object).
2. **Wrong type**: Using `type:"local"` causes OpenCode to not match the `MCPStdio` case — the server subprocess never starts.
3. **Wrong command format**: Using `command` as array means the first element isn't extracted as the binary name — the subprocess can't be found.

Check logs at `~/.local/share/opencode/log/` for the actual error.

## Config File Location

OpenCode uses Viper with:
- `SetConfigName(".opencode")` — looks for `.opencode.json`
- `AddConfigPath("$HOME")`
- `AddConfigPath("$XDG_CONFIG_HOME/opencode")`
- `AddConfigPath("$HOME/.config/opencode")`

Agentalign writes to: `~/.config/opencode/opencode.json` (no dot prefix). Works because Viper reads JSON files from the configured path.

## Non-MCP Section Preservation

`serialize_from_canonical()` reads the existing file on disk and only replaces the `"mcp"` key. All other top-level keys (`agent`, `provider`, `model`, `tools`, `plugin`, etc.) are preserved. However, if the file was previously corrupted (e.g., by the watcher reverse-merge destroying the canonical, then sync writing a stripped version), the preserved non-MCP sections will also be stripped. **Always verify non-MCP sections after a sync that follows a watcher bug.**

## Watcher Reverse-Merge Bug (fixed 2026-05-30)

When sync writes OpenCode format to disk, the watcher's FSEvents callback fires:

1. Reads new OpenCode config from disk
2. Calls `deserialize_to_canonical()` → produces correct canonical JSON
3. Calls `delta_merger::compute_delta()` → detects format differences as "updates"
4. Calls `serde_json::from_value(v)` on each updated server → creates `McpServerDefinition`
5. Overwrites canonical with the new definition

Problem: step 4 can lose data if the round-trip through `McpServerDefinition` strips fields. Also, step 5 overwrites the canonical even when the "update" is just a format-conversion artifact (e.g., env object vs env array).

**Fix:** Guard in `entries_to_update` loop — skip update if canonical already has `command` or `url` data for that server key.

```rust
if canonical.mcp.contains_key(key) {
    let existing = &canonical.mcp[key];
    let has_data = existing.command.is_some() || existing.url.is_some();
    if has_data {
        eprintln!("  ~ {} (skipped — canonical already has command/url data)", key);
        continue;
    }
}
```

## Watcher Daemon Management (launchd)

The watcher runs as `com.agentalign.magic` via launchd. It auto-respawns on kill.

```bash
# Stop (actually stops — must remove from launchd)
launchctl stop com.agentalign.magic && launchctl remove com.agentalign.magic

# Restart
launchctl load ~/Library/LaunchAgents/com.agentalign.magic.plist

# Check status
launchctl list | grep agentalign
ps aux | grep "agentalign watch"
```

**Always stop the watcher before restoring a corrupted canonical.** The watcher detects file changes within ~500ms and will re-corrupt canonical if it sees the format difference between the restored canonical and the agent configs.

## local_entries.json

`~/.agents/local_entries.json` protects agent-specific servers from removal during sync. Format:

```json
["github-copilot", "github"]
```

Servers in this set are exempt from the delta merger's `entries_to_remove` path. Combined with `preserve_local_entries()` in `push_to_agents`, servers in this set that exist in the target file but not in canonical are re-merged after `serialize_from_canonical()` produces output, so they survive one-way pushes too.

## agent_skip.json

`~/.agents/agent_skip.json` prevents servers from being written to specific agents during sync. Format:

```json
{
  "Cursor": ["github-copilot", "github"],
  "OpenCode": ["github-copilot", "github"]
}
```

Skipped servers are filtered from the canonical JSON before `serialize_from_canonical()` runs. This prevents sync from overwriting agent-auto-created entries (like Cursor's `github-copilot`) with canonical data. The agent label must match the `descriptor.label` used in `push_to_agents()` (e.g., "Cursor", "OpenCode", "Claude", "Gemini", "Antigravity", "Codex").

## Why Both Are Needed (github-copilot Example)

Cursor auto-creates `github-copilot` on startup. Without both files:
- No `local_entries.json`: sync removes `github-copilot` from Cursor → Cursor re-adds it → watcher reverse-merges to canonical → propagates to all agents
- No `agent_skip.json`: sync writes canonical content (without `github-copilot`) to Cursor → overwrites Cursor's auto-created entry → next Cursor restart re-adds it → cycle repeats
- No `preserve_local_entries()`: one-way push replaces the MCP section entirely, losing local-entry servers that were in the existing file

With all three: `github-copilot` stays only in Cursor, never enters canonical, never propagates to other agents, and survives both watcher syncs and manual `agentalign sync` runs.
