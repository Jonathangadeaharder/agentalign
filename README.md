# agentalign

Rust CLI for aligning AI agent instructions across projects. Tracks, migrates, and synchronizes agent skill configs (OpenSpec, AGENTS.md, MCP servers).

## Stack

Rust (edition 2021), MIT licensed.

## Build

```bash
cargo build
cargo test
cargo clippy -- -D warnings
```

## Structure

```
src/
├── main.rs        # CLI entrypoint
├── lib.rs         # Core logic
├── instructions/  # Instruction alignment
├── mcp/           # MCP server management
├── migration/     # Migration logic
├── skills/        # Skill management
├── sync/          # Cross-project sync
├── tracking/      # State tracking
├── watch.rs       # File watcher
├── magic.rs       # Heuristics
└── shared/        # Shared types
```
