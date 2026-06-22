# SonarCloud Community MCP Server Setup (Free Plan)

## Problem

SonarCloud's official MCP endpoint (`api.sonarcloud.io/mcp`) returns 403 on Free-tier orgs due to IAM policy. Even regular REST API calls to `api.sonarcloud.io` are blocked. The working gateway is `sonarcloud.io/api`.

## Solution: community-sonarcloud-mcp-server

npm package `community-sonarcloud-mcp-server` (by langtind, v1.1.1) — stdio-based MCP server that calls `sonarcloud.io/api` directly.

### Canonical Config

```json
{
  "type": "local",
  "command": ["npx", "-y", "community-sonarcloud-mcp-server"],
  "env": {
    "SONARCLOUD_TOKEN": "<personal-access-token>",
    "SONARCLOUD_ORGANIZATION": "<org-key>"
  }
}
```

### Agent-Specific Formats

**OpenCode** (type=stdio, command=string, env as array):
```json
{
  "type": "stdio",
  "command": "npx",
  "args": ["-y", "community-sonarcloud-mcp-server"],
  "env": ["SONARCLOUD_TOKEN=<pat>", "SONARCLOUD_ORGANIZATION=<org>"]
}
```

**Claude/Cursor/Antigravity** (command+args split, env object):
```json
{
  "command": "npx",
  "args": ["-y", "community-sonarcloud-mcp-server"],
  "env": {
    "SONARCLOUD_TOKEN": "<pat>",
    "SONARCLOUD_ORGANIZATION": "<org>"
  }
}
```

### Token Generation

1. Go to `https://sonarcloud.io/account/security` (must be logged in)
2. Type a name (e.g., "antigravity-mcp")
3. Click Generate
4. Copy the 40-char hex token immediately (shown once)

### Token Verification

```bash
curl -s -H "Authorization: Bearer <token>" "https://sonarcloud.io/api/authentication/validate"
# Expected: {"valid":true}
# NOTE: Use sonarcloud.io/api, NOT api.sonarcloud.io (which always returns 403)
```

### Current Token (2026-05-30)

Token name: `antigravity-mcp`, 40-char hex. Valid on `sonarcloud.io/api`. Stored in `~/.zshenv` as `SONAR_TOKEN`.

### Tools Provided (12)

search_issues, get_measures, list_projects, get_pull_requests, change_issue_status, list_languages, search_metrics, get_quality_gate_status, list_quality_gates, show_rule, list_rule_repositories, get_raw_source

### Auth Mechanism

Basic auth with PAT as username, empty password: `Authorization: Basic base64(token:)`
