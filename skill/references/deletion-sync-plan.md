# agentalign Deletion Sync — Implementation Record

Completed: 2026-05-21

## Problem

User removed browsermcp from Antigravity config, but it persisted in OpenCode, Claude, Cursor, and canonical `~/.agents/mcp_config.json`. Three interlocking bugs (all now fixed).

## Fixes Applied

### Fix 1: Wire delta_merger into watch daemon
File: `watch.rs:process_changes`

Replaced naive `canonical.mcp.insert(k, def)` loop with `delta_merger::compute_delta()`.

**Critical: argument order** — `compute_delta(&canonical_servers, &agent_servers, &local_entries)`:
- `entries_to_remove` = canonical keys NOT in agent (agent deleted them) → remove from canonical
- `entries_to_add` = agent keys NOT in canonical → add to canonical
- `entries_to_update` = shared keys with different values → update canonical

Swapping the first two args inverts the meaning (entries_to_add would mean "canonical has but agent doesn't" = wrong direction).

### Fix 2: OpenCode tools cleanup
- Added `post_sync_cleanup(&mut doc, mcp_server_names)` to `ConfigurationAdapter` trait (default no-op)
- OpenCode override: scans `tools` map, extracts server prefix from keys matching `name*` or `name_tool` pattern, removes entries whose prefix isn't in current `mcp` keys
- Called from `serialize_from_canonical` after mcp section replacement
- **Pitfall**: Must add method to the existing `impl ConfigurationAdapter for OpenCodeStrategy` block, not a separate one — separate blocks cause E0119

### Fix 3: Add/Remove CLI commands
- `agentalign add <NAME> --type local|remote --command "..." --url "..." --enabled --no-sync --dry-run`
- `agentalign remove <NAME> --no-sync --dry-run`
- Both modify canonical, then optionally sync to all agents
- `add` splits `--command` on whitespace (no shlex dep)

### Fix 4: Non-destructive sync
- `agentalign sync` now checks content equality before writing — skips unchanged files with `(unchanged, skipped)` message
- Avoids unnecessary file writes and transaction creation

### Fix 5: File-deletion handling
- Deleted agent configs are detected and recreated from canonical

## Manual Cleanup Performed

browsermcp removed from all 4 infected files:
1. `~/.agents/mcp_config.json` — removed `mcp.browsermcp`
2. `~/.claude/.mcp.json` — removed `mcpServers.browsermcp`
3. `~/.cursor/mcp.json` — removed `mcpServers.browsermcp`
4. `~/.config/opencode/opencode.json` — removed `mcp.browsermcp` + 7 `tools.browsermcp*` entries
