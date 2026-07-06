# agentalign

Rust CLI for AI agent config unification (MCP, agents, instructions, skills, rules) across AI coding agents.

## Repo

`~/projects/agentalign/`

## Architecture

- **agentalign** (`src/`): config unification engine. Subcommands: `migrate`, `sync`, `add`, `remove`, `restore`, `magic`, `watch`. Uses strategy + factory for multi-agent format translation. Canonical format = OpenCode-derived (`CanonicalWorkspaceState`).

### Modules

- **mcp/** — 12 strategy modules for Claude, Cursor, VS Code, Copilot, Windsurf, Zed, Gemini, Codex, OpenCode, Antigravity, ZCode, Grok. Factory pattern via `McpFormatFactory::from_agent(AgentType)`. Single source of truth: `AgentRegistry::synced_agents()`.
- **agents/** — Subagent definition sync. Canonical: `~/.agents/agents/*.md`. 7 strategies: Claude, Cursor, OpenCode, Gemini (Agy customAgent JSON), Codex (openai.yaml), ZCode, Grok. `SubagentRegistry::synced_strategies()` returns the 7 synced strategies (Antigravity not yet supported).
- **sync/** — Transactional writes (`transaction.rs`) with TOML cache (`cache.rs`) + SHA-256 checksum verification + rollback. Delta merger (`delta_merger.rs`) for bidirectional add/update/remove detection with local entries protection.
- **migration/** — Secret splitting (`secret_splitter.rs`): extracts sensitive fields → `${ENV_AGENTALIGN_SECRET_*}` placeholders. Local JSON fallback store (`local_json.rs`) at `~/.agents/local.json`.
- **tracking/** — `SecretVault` trait (`OsKeyringVault` via keyring crate / `InMemoryVault` for tests). Keychain bindings in `keychain.rs`.
- **instructions/** — Symlink guard for agent instruction files. Canonical source: `~/.agents/AGENTS.md`. Agent files (CLAUDE.md, GEMINI.md, CODEX.md, AGENTS.md) are symlinks. Detects broken/missing/replaced-by-file states and heals.
- **rules/** — Rules generation. Splits `~/.agents/AGENTS.md` into path-scoped Cursor `.mdc` files (`~/.cursor/rules/`) and Claude `.md` files (`~/.claude/rules/`). Called by `sync` and `watch` (on daemon startup and on AGENTS.md changes).
- **skills/** — Skills directory symlink syncing. Canonical: `~/.agents/skills/`. Per-agent dirs are symlinked (claude, gemini, codex, cursor, opencode, grok). Backs up real directories before replacing.
- **watch.rs** — File watcher daemon using `notify` crate. 500ms debounce. Bidirectional logic: canonical changed → regenerate all agents; agent config changed → compute delta → update canonical → propagate.
- **magic.rs** — LaunchAgent management. Installs/removes `com.agentalign.magic.plist` at `~/Library/LaunchAgents/`. Runs `agentalign watch` at login with KeepAlive.
- **state.rs** — Sync state tracking. Stores SHA-256 hashes in `~/.agents/.sync_state.json` to distinguish own writes from user edits (loop prevention).
- **shared/** — Core types (`CanonicalWorkspaceState`, `McpServerDefinition`, `SyncTransaction`, `ClientCapabilities`, etc.), traits (`ConfigurationAdapter`, `McpFormatStrategy`), and error types (`AdapterError`).

## Test Count

144 unit tests + 5 E2E tests = 149 total.

## Key Files

- `src/main.rs` — CLI: `migrate`, `sync`, `agents list`, `agents sync`, `add`, `remove`, `restore`, `magic`, `watch`.
- `src/mcp/factory.rs` — `AgentRegistry` (synced agents list) + `McpFormatFactory` (strategy factory). 12 agent types.
- `src/agents/mod.rs` — `SubagentRegistry`, `sync_agents()`. 7 synced strategies (Claude, Cursor, OpenCode, Gemini, Codex, ZCode, Grok).
- `src/agents/canonical.rs` — `load_all_agents()`, parses YAML frontmatter from `~/.agents/agents/*.md`.
- `src/sync/transaction.rs` — `create_transaction()`, `finalize_transaction()`, `rollback_transaction()`, `handle_rollback()`, `handle_list()`.
- `src/sync/cache.rs` — TOML-based transaction cache at `~/.agents/cache.toml`.
- `src/sync/delta_merger.rs` — `compute_delta()` with local entries protection set.
- `src/migration/secret_splitter.rs` — `split_secrets()`, `apply_secret_mappings()`, `is_placeholder()`.
- `src/tracking/keychain.rs` — `store_secret()`, `get_secret()`, `delete_secret()`, `resolve_secret()` (keychain → local fallback).
- `src/tracking/mod.rs` — `SecretVault` trait, `OsKeyringVault`, `InMemoryVault`.
- `src/instructions/mod.rs` — `heal_all()`, `heal_one()`, `verify_all()`. 6 agents: opencode, claude, gemini, codex, zcode, grok.
- `src/rules/mod.rs` — `sync_rules()`, `cursor_rules_dir()`, `claude_rules_dir()`. 16 tests. Splits AGENTS.md sections into Cursor `.mdc` + Claude `.md` rule files.
- `src/skills/mod.rs` — `heal_all()`, `heal_one()`. 6 agents: claude, gemini, codex, cursor, opencode, grok.
- `src/watch.rs` — `run_daemon()`. Bidirectional sync with `process_changes()`, `sync_all_agents()`, `sync_selected_agents()`.
- `src/magic.rs` — `enable()`, `disable()`, `status()`. LaunchAgent plist generation + launchctl bootstrap.
- `src/state.rs` — `SyncState` struct. `compute_hash()`, `is_unchanged()`, `update_hash()`.
- `src/shared/models.rs` — `CanonicalWorkspaceState`, `McpServerDefinition`, `TransportType`, `SyncTransaction`, `TransactionStatus`, `ClientCapabilities`, `PlaceholderStyle`, `SecretMapping`, `EnvironmentMapping`.
- `src/shared/traits.rs` — `ConfigurationAdapter`, `McpFormatStrategy`.
- `src/shared/error.rs` — `AdapterError`.
- `src/mcp/validation.rs` — `check_forbidden_chars()`, `check_max_id_length()`, `check_stdio_paths()`, `check_remote_urls()`, `check_transport_support()`, `check_toml_key_safety()`.
- `src/mcp/interpolation.rs` — `normalize_value()`, `normalize_env_map()`, `extract_placeholders()`, `resolve_placeholders()`, `build_secret_map()`.
- `src/mcp/capabilities.rs` — Per-agent `ClientCapabilities` constructors.
- `tests/e2e_cli_sync.rs` — 5 E2E tests using `assert_cmd` + `tempfile`.
