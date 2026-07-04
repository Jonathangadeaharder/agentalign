# agentalign

Rust CLI for unifying AI agent configuration across multiple coding agents. Syncs four domains from a canonical `~/.agents/` store: MCP servers (`mcp_config.json`), agent definitions (`agents/*.md`), instruction files (`AGENTS.md` symlinks), and skills (`skills/` symlinks). Propagates to all supported agents with transactional rollback, bidirectional sync, secret splitting, and symlink healing.

## Stack

Rust (edition 2021), MIT licensed.

## Build

```bash
cargo build
cargo test
cargo clippy -- -D warnings
```

## CLI Commands

```text
agentalign migrate [--dry-run]              # Scan existing agent configs into ~/.agents/
agentalign sync [--dry-run]                 # Push canonical config to all agents
agentalign agents list                      # List canonical subagent definitions
agentalign agents sync [--dry-run]          # Sync subagent definitions to all tools
agentalign add <name>                       # Add MCP server to canonical and propagate
  --type <local|remote>  (default: local)
  --command <cmd>        (for local servers, e.g. "npx @pkg/mcp")
  --url <url>            (for remote servers)
  --enabled <bool>       (default: true)
  --no-sync              (don't propagate after adding)
  --dry-run
agentalign remove <name>                    # Remove MCP server from canonical and propagate
  --no-sync
  --dry-run
agentalign restore                          # Roll back the last sync transaction
  --agent <name>         (rollback specific agent; all if omitted)
  --id <uuid>            (rollback specific transaction)
  --list                 (show transaction history)
agentalign magic <on|off|status>            # Toggle automatic bidirectional sync (LaunchAgent)
agentalign watch                            # Run the file watcher daemon
```

## Supported Agents

| Agent | Config Path | Format |
|-------|-----------|--------|
| Claude | `~/.claude/.mcp.json` | JSON (claude-derived) |
| Cursor | `~/.cursor/mcp.json` | JSON (claude-derived, `is_cursor` flag) |
| VS Code | User settings JSON | JSON (env-section) |
| Copilot | GitHub Copilot config | JSON (`$VAR` placeholders) |
| Windsurf | Windsurf config | JSON (security sandbox) |
| Zed | Zed settings | JSON (env-section) |
| Gemini | `~/.gemini/config/mcp_config.json` | JSON (`$VAR` placeholders) |
| Codex | `~/.codex/config.toml` | TOML (restricted key chars) |
| OpenCode | `~/.config/opencode/opencode.json` | JSON (canonical format) |
| Antigravity | `~/.gemini/antigravity/mcp_config.json` | JSON |

## Key Features

- **Transactional sync**: every write is wrapped in a transaction with backup + SHA-256 checksum. `restore` rolls back.
- **Delta merger**: bidirectional sync detects adds, updates, and removals between agent configs and canonical.
- **Secret splitting**: sensitive fields (api_key, token, password, etc.) are extracted to OS keychain or `~/.agents/local.json` fallback, replaced with `${ENV_AGENTALIGN_SECRET_*}` placeholders.
- **Environment interpolation**: normalizes `${VAR}`, `$VAR`, `${env:VAR}` across agent dialects.
- **Instruction symlink healing**: `~/.agents/AGENTS.md` is the canonical source. Agent files (CLAUDE.md, GEMINI.md, CODEX.md, AGENTS.md) are symlinks.
- **Skills directory syncing**: `~/.agents/skills/` is the canonical source. Per-agent skill dirs are symlinked.
- **Magic mode**: installs a macOS LaunchAgent that runs `agentalign watch` on login with 500ms debounced bidirectional sync.
- **Local entries protection**: `~/.agents/local_entries.json` preserves user-added keys during sync.
- **Per-agent skip list**: `~/.agents/agent_skip.json` prevents specific servers from being pushed to specific agents.

## Structure

```text
src/
├── main.rs              # CLI entrypoint (clap derive)
├── lib.rs               # Module declarations
├── state.rs             # Sync state tracking (SHA-256 hashes, loop prevention)
├── magic.rs             # LaunchAgent install/uninstall/status
├── watch.rs             # File watcher daemon (notify crate, bidirectional sync)
├── agents/              # Subagent definition sync (canonical -> per-agent formats)
│   ├── mod.rs           # SubagentRegistry, sync_agents()
│   ├── canonical.rs     # Load/parse canonical agent definitions (~/.agents/agents/*.md)
│   ├── claude.rs        # ClaudeAgentStrategy: markdown with YAML frontmatter
│   ├── codex.rs         # CodexAgentStrategy: openai.yaml inside skill dirs
│   ├── cursor.rs        # CursorAgentStrategy: same format as Claude, no `color`
│   ├── gemini.rs        # GeminiAgentStrategy: Agy customAgent JSON format
│   ├── opencode.rs      # OpenCodeAgentStrategy: same format as Claude
│   └── zcode.rs         # ZCodeAgentStrategy: disallowedTools from permission
├── instructions/
│   └── mod.rs           # Instruction symlink healing (AGENTS.md -> CLAUDE.md, etc.)
├── mcp/
│   ├── mod.rs
│   ├── factory.rs       # AgentRegistry + McpFormatFactory
│   ├── strategy.rs      # validate_all helper
│   ├── canonical.rs     # Identity strategy (OpenCode format)
│   ├── validation.rs    # Pre-write validation (forbidden chars, transport, IDs)
│   ├── capabilities.rs  # Per-agent ClientCapabilities matrix
│   ├── interpolation.rs # Env var dialect normalization + secret placeholder resolution
│   ├── claude.rs        # Claude/Cursor strategy
│   ├── vscode.rs        # VS Code strategy
│   ├── copilot.rs       # Copilot CLI strategy
│   ├── windsurf.rs      # Windsurf strategy
│   ├── zed.rs           # Zed strategy
│   ├── gemini.rs        # Gemini strategy
│   ├── codex.rs         # Codex CLI (TOML) strategy
│   ├── opencode.rs      # OpenCode strategy
│   └── antigravity.rs   # Antigravity strategy
├── migration/
│   ├── mod.rs
│   ├── secret_splitter.rs  # Extract sensitive fields -> keychain placeholders
│   └── local_json.rs       # Local fallback secret store (~/.agents/local.json)
├── skills/
│   └── mod.rs           # Skills directory symlink healing
├── sync/
│   ├── mod.rs
│   ├── transaction.rs   # Transactional write with backup + rollback
│   ├── cache.rs         # TOML-based transaction cache (~/.agents/cache.toml)
│   └── delta_merger.rs  # Bidirectional add/update/remove delta computation
├── tracking/
│   ├── mod.rs           # SecretVault trait + InMemoryVault + OsKeyringVault
│   └── keychain.rs      # OS keychain bindings (keyring crate)
└── shared/
    ├── mod.rs
    ├── config.rs        # Shared config loaders (local_entries, agent_skip)
    ├── models.rs        # CanonicalWorkspaceState, McpServerDefinition, SyncTransaction, etc.
    ├── traits.rs        # ConfigurationAdapter, McpFormatStrategy traits
    └── error.rs         # AdapterError enum
```
