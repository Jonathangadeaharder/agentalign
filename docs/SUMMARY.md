# agentalign-agenttrim

Two Rust CLIs for MCP config unification and telemetry-driven pruning.

## Repo

`~/projects/agentalign-agenttrim/`

## Architecture

- **agentalign** (`agentalign/src/`): config unification engine. Subcommands: `migrate`, `sync`, `restore`. Uses strategy + factory for multi-agent format translation. Canonical format = OpenCode.
- **agenttrim** (`agenttrim/src/`): telemetry-driven pruning. Subcommands: `analyze`, `prune`, `vacuum`, `status`, `watch`.
- **shared** (`shared/src/`): types + traits (`ConfigurationAdapter`, `TrimAnalyzer`, `TimeProvider`, `UsageStore`).

## Done

### Config unification (agentalign)
- 14 strategy modules for Claude, Cursor, VS Code, Copilot, Windsurf, Zed, Gemini, Codex, OpenCode.
- Secret-splitting + OS keychain (`SecretVault` trait).
- Transactional cache + rollback for sync safety.
- Delta merger for sync diffs.

### Telemetry & pruning (agenttrim)
- SQLite `usage.db` at `~/.agents/usage.db` with `usage_log` table (id, server_id, tool_name, agent, action, timestamp, byte_cost).
- `agenttrim status` — displays per-server + per-tool usage.
- `agenttrim watch` — daemon polls `~/.agents/skills/` for SKILL.md mtime changes, logs usage automatically.
- `agenttrim analyze` — scans skills + MCPs for unused items.
- `agenttrim prune` — removes with safety gates + backup + rollback.
- `TimeProvider` + `UsageStore` traits injected for testability. Mutation kill: skills 71%, validation_hook 65%.

### Infrastructure
- `TimeProvider` trait (`SystemTimeProvider` / `FrozenTimeProvider`).
- Opaque `UsageStore` trait (`RealUsageStore` / `MockUsageStore`).
- OS process mapping via `libc::sysctl` (macOS) and `/proc` (Linux).
- All 140 tests pass (71 agentalign unit + 58 agenttrim unit + 6 E2E + 5 integration).

## Key files

- `agenttrim/src/analyze/ledger_reader.rs` — SQLite schema + `log_usage()`, `get_usage_stats()`, `get_tool_usage_stats()`, `get_unused_since()`.
- `agenttrim/src/main.rs` — CLI + `run_watch()` daemon polling skills dir.
- `agenttrim/src/analyze/skills.rs` — `SkillAnalyzer` with `TimeProvider` + `UsageStore`.
- `agenttrim/src/analyze/validation_hook.rs` — `PrePurgeValidation` with `TimeProvider`.
- `agentalign/src/main.rs` — CLI: `migrate`, `sync`, `restore` only.
