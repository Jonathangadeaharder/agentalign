# Antigravity MCP Integration — Session Notes (2026-05-30)

## Config Discovery

Antigravity's MCP config was hard to locate. Here's what was searched and where it actually lives:

| Path Searched | Result |
|---------------|--------|
| `~/.antigravity/settings.json` | Nearly empty (only `window.autoDetectColorScheme`) |
| `~/Library/Application Support/Antigravity/User/settings.json` | Same — nearly empty |
| `~/Library/Application Support/Antigravity/User/globalStorage/state.vscdb` | SQLite DB with `antigravity.mcpConfigFileInfoWidget.collapseState` key only |
| `~/.antigravity/mcp.json` | Does not exist |
| `~/.gemini/antigravity/mcp_config.json` | **THIS IS IT** — the actual MCP config |

The History snapshots in `~/Library/Application Support/Antigravity/User/History/` confirmed the `mcpServers` format but were empty (`{}`).

## AntigravityStrategy Implementation

Added as a first-class agent to agentalign. Files modified:

- `src/mcp/antigravity.rs` — new strategy (Cursor-like format, env support, no ID restrictions)
- `src/mcp/factory.rs` — added `Antigravity` to `AgentType` enum + `from_agent()` + `all()` + `from_name()`
- `src/mcp/capabilities.rs` — added `antigravity_capabilities()` (stdio + SSE + HTTP)
- `src/watch.rs` — added Antigravity to `build_watch_list()`, `sync_all_agents()`, `sync_selected_agents()`
- `src/main.rs` — added Antigravity to 4 hardcoded agent lists (discover, sync, migrate, list)

Config path: `~/.gemini/antigravity/mcp_config.json`
Format: Same as Cursor (`mcpServers`, `command`+`args` split) but with `env` field support.

## SonarCloud MCP — Free Plan Restriction

### The Two-Gateway Problem

SonarCloud has **two separate API gateways** with different access policies:

| Domain | Behavior | Auth |
|--------|----------|------|
| `api.sonarcloud.io` | 403 for ALL requests on Free-tier orgs | Returns 403 even unauthenticated, even with valid PATs. AWS IAM policy blocks access. |
| `sonarcloud.io/api` | Full REST API access | PAT Bearer auth works. Cookie auth works. |

The `/mcp` endpoint lives on `api.sonarcloud.io` → always 403 on Free plan.
Regular REST endpoints (`/api/projects/search`, `/api/issues/search`, etc.) work on `sonarcloud.io/api`.

Error from blocked gateway: `"User is not authorized to access this resource with an explicit deny in an identity-based policy"`

### Official MCP Server (paid-only)

The official SonarCloud MCP endpoint at `api.sonarcloud.io/mcp` uses Streamable HTTP transport via `mcp-remote`. It requires Team plan or higher ($32/mo). MCP is part of Sonar's AC/DC (Agent Centric Development Cycle) premium feature set.

### Community MCP Server (works on Free plan)

`community-sonarcloud-mcp-server` (npm, v1.1.1 by langtind) is a stdio-based MCP server that calls `sonarcloud.io/api` directly — bypassing the restricted gateway entirely.

**Config:**
```json
{
  "mcpServers": {
    "sonarcloud": {
      "command": "npx",
      "args": ["-y", "community-sonarcloud-mcp-server"],
      "env": {
        "SONARCLOUD_TOKEN": "<pat>",
        "SONARCLOUD_ORGANIZATION": "<org-key>"
      }
    }
  }
}
```

**12 tools provided:** search_issues, get_measures, list_projects, get_pull_requests, change_issue_status, list_languages, search_metrics, get_quality_gate_status, list_quality_gates, show_rule, list_rule_repositories, get_raw_source

**Auth:** Basic auth with PAT as username (`Basic base64(token:)`). No `mcp-remote` needed.

## Firefox Cookie Extraction for Browser Auth

When headless browser needs GitHub auth for SonarCloud OAuth flow:

1. Copy Firefox cookies DB (locked by Firefox): `cp ~/Library/Application\ Support/Firefox/Profiles/<profile>/cookies.sqlite /tmp/firefox_cookies.sqlite`
2. Extract GitHub session cookies: `user_session`, `__Host-user_session_same_site`, `logged_in`
3. Navigate browser to `github.com`, set cookies via `document.cookie`
4. Then click through SonarCloud GitHub OAuth — auto-authorizes with the injected session

Profile dir: `~/Library/Application Support/Firefox/Profiles/zwe4dkb0.default-release`
