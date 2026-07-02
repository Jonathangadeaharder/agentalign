# Cursor Feature Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend agentalign to sync skills, agent definitions, instruction files (as rules), and hooks to Cursor, achieving feature parity with Claude/OpenCode/Gemini/Codex.

**Architecture:** Three new sync targets for Cursor: (1) skills via symlinks into `~/.cursor/skills/` (same pattern as Claude/Gemini/Codex), (2) agent definitions via a new `CursorAgentStrategy` that reuses Claude's markdown+frontmatter format into `~/.cursor/agents/`, (3) instruction files via a new `rules` module that splits `~/.agents/AGENTS.md` into ~10 thematic `.cursor/rules/*.mdc` files. MCP already synced via `ClaudeStrategy { is_cursor: true }`. Hooks generated as `~/.cursor/hooks.json` + shell scripts in `~/.agents/hooks/`.

**Tech Stack:** Rust (existing agentalign crate), serde_yaml for .mdc frontmatter, std::os::unix::fs::symlink for skills, shell scripts for hooks.

---

## File Structure

### New Files

| File | Responsibility |
|------|---------------|
| `src/agents/cursor.rs` | `CursorAgentStrategy` — reuses Claude's format, targets `~/.cursor/agents/` |
| `src/rules/mod.rs` | Cursor rules sync — parses AGENTS.md sections, generates `.mdc` files in `~/.cursor/rules/` |
| `~/.agents/hooks/git-guard.sh` | Pre-tool hook blocking destructive git commands |
| `~/.agents/hooks/biome-check.sh` | After-edit hook running biome check on TS/Svelte files |
| `~/.agents/hooks/typecheck.sh` | After-edit hook running tsc --noEmit on TS/Svelte files |
| `~/.cursor/hooks.json` | Cursor hooks configuration referencing the scripts above |
| `~/.cursor/rules/*.mdc` | 10 generated rule files from AGENTS.md split |

### Modified Files

| File | Changes |
|------|---------|
| `src/mcp/factory.rs` | Update `AgentRegistry::synced_agents()` — add `skills_dir` and `rules_dir` to Cursor descriptor |
| `src/agents/mod.rs` | Add `CursorAgentStrategy` to `SubagentRegistry::synced_strategies()`; add `pub mod cursor;` |
| `src/skills/mod.rs` | Add Cursor entry to `registry()` |
| `src/watch.rs` | Add rules directory watching for Cursor (auto-derived from AgentRegistry) |
| `src/lib.rs` | Add `pub mod rules;` |
| `src/main.rs` | Call `rules::sync_all(home)` in `push_to_agents_impl()` after skills sync |

---

## Part 1: agentalign Rust Code Changes

### 1.1 AgentDescriptor Update (`src/mcp/factory.rs:110-116`)

The Cursor descriptor currently has `instruction_path: None` and `skills_dir: None`. Update to:

```rust
AgentDescriptor {
    agent_type: AgentType::Cursor,
    label: "Cursor",
    config_path: home.join(".cursor").join("mcp.json"),
    instruction_path: None, // Cursor uses rules/*.mdc, not a single instruction file
    skills_dir: Some(home.join(".cursor").join("skills")),
},
```

**Rationale:** `instruction_path` stays `None` because Cursor doesn't read a single AGENTS.md file. The new `rules` module handles instruction sync separately. `skills_dir` is set to `~/.cursor/skills/` for symlink-based skills sync.

**No new field needed on AgentDescriptor.** The watch daemon already iterates `AgentRegistry::synced_agents()` for `skills_dir` and auto-discovers them. Adding `skills_dir` here is sufficient — no changes to `watch.rs` needed for skills watching.

### 1.2 Skills Registry Update (`src/skills/mod.rs:35-49`)

Add Cursor to the `registry()` function:

```rust
fn registry(home: &Path) -> Vec<SkillsEntry> {
    vec![
        SkillsEntry {
            agent: "claude",
            skills_dir: home.join(".claude").join("skills"),
        },
        SkillsEntry {
            agent: "cursor",
            skills_dir: home.join(".cursor").join("skills"),
        },
        SkillsEntry {
            agent: "gemini",
            skills_dir: home.join(".gemini").join("skills"),
        },
        SkillsEntry {
            agent: "codex",
            skills_dir: home.join(".codex").join("skills"),
        },
    ]
}
```

**Safety:** The 4 existing OpenSpec skills in `~/.cursor/skills/` are real directories, not symlinks. `heal_skill()` handles `ReplacedByDir` state by backing up and replacing. BUT `heal_all()` only processes canonical skills (from `~/.agents/skills/`), and the OpenSpec skills are NOT in the canonical store. They will be left untouched. No name conflicts exist between canonical skills and OpenSpec skills (verified: canonical has `agent-browser`, `audit`, `caveman`, etc.; Cursor has `openspec-apply-change`, `openspec-archive-change`, `openspec-explore`, `openspec-propose`).

The `~/.cursor/skills-cursor/` directory (built-in Cursor skills) is never touched because it's not in the registry.

**Test update:** The test `test_heal_all` in `skills/mod.rs:378` asserts `fixed == 4` (for 4 agents × 1 skill). After adding Cursor, it will be `fixed == 5`. Update the assertion.

### 1.3 Cursor Agent Strategy (`src/agents/cursor.rs` — new file)

Cursor uses the same markdown + YAML frontmatter format as Claude (`name`, `description`, `model`, `tools`). The only difference is the target directory (`~/.cursor/agents/` vs `~/.claude/agents/`).

```rust
use crate::agents::canonical::ParsedAgentFile;
use crate::agents::SubagentStrategy;
use crate::mcp::factory::AgentType;
use serde_yaml::Mapping;
use serde_yaml::Value as YamlValue;
use std::path::Path;

pub struct CursorAgentStrategy;

impl SubagentStrategy for CursorAgentStrategy {
    fn agent_type(&self) -> AgentType {
        AgentType::Cursor
    }

    fn agents_dir(&self, home: &Path) -> std::path::PathBuf {
        home.join(".cursor").join("agents")
    }

    fn format_agent(&self, agent: &ParsedAgentFile) -> anyhow::Result<String> {
        // Cursor uses the same frontmatter format as Claude:
        // name, description, model, tools
        // Cursor does NOT support: mode, permission, color
        let mut frontmatter = Mapping::new();

        frontmatter.insert(
            YamlValue::String("name".into()),
            YamlValue::String(agent.name.clone()),
        );

        frontmatter.insert(
            YamlValue::String("description".into()),
            YamlValue::String(agent.frontmatter.description.clone()),
        );

        if let Some(ref model) = agent.frontmatter.model {
            frontmatter.insert(
                YamlValue::String("model".into()),
                YamlValue::String(model.clone()),
            );
        }

        if !agent.frontmatter.tools.is_empty() {
            let tools_str = agent.frontmatter.tools.join(", ");
            frontmatter.insert(
                YamlValue::String("tools".into()),
                YamlValue::String(tools_str),
            );
        }

        let yaml_str = serde_yaml::to_string(&frontmatter)?;
        let yaml_trimmed = yaml_str.trim_end_matches('\n');

        Ok(format!("---\n{}\n---\n\n{}", yaml_trimmed, agent.body))
    }
}
```

**Design decision:** Reuse the format logic from `ClaudeAgentStrategy` rather than sharing the struct. Cursor may diverge in the future (e.g., different frontmatter fields). Copy-paste is cleaner here than a shared base — YAGNI says don't abstract until a second divergence point appears.

**Alternative considered:** Make `ClaudeAgentStrategy` generic with a target directory parameter. Rejected because `ClaudeAgentStrategy` has `agent_type()` returning `AgentType::Claude`, and changing this would break the trait contract. A separate strategy is cleaner.

### 1.4 SubagentRegistry Update (`src/agents/mod.rs:35-43`)

Add `CursorAgentStrategy` to the registry:

```rust
impl SubagentRegistry {
    pub fn synced_strategies() -> Vec<Box<dyn SubagentStrategy>> {
        vec![
            Box::new(opencode::OpenCodeAgentStrategy),
            Box::new(claude::ClaudeAgentStrategy),
            Box::new(cursor::CursorAgentStrategy),
            Box::new(gemini::GeminiAgentStrategy),
            Box::new(CodexAgentStrategy),
        ]
    }
}
```

Add module declaration at top of `src/agents/mod.rs`:

```rust
pub mod cursor;
```

### 1.5 Rules Module (`src/rules/mod.rs` — new file)

This module splits `~/.agents/AGENTS.md` into thematic `.mdc` rule files in `~/.cursor/rules/`.

**Architecture:**
1. `RULE_GROUPS` — static array mapping AGENTS.md section numbers to rule file names, frontmatter, and descriptions
2. `parse_sections()` — parses AGENTS.md by splitting on `## N. Title` headings
3. `generate_mdc()` — for each rule group, concatenates section content and wraps in `.mdc` frontmatter
4. `sync_all()` — orchestrates parse → generate → write, with manifest-based orphan cleanup
5. `heal_all()` — alias for `sync_all()` (called by main.rs and watch.rs)

```rust
//! Cursor rules sync module.
//!
//! Splits ~/.agents/AGENTS.md into thematic .mdc rule files in ~/.cursor/rules/.
//! Each rule file has YAML frontmatter (description, globs, alwaysApply) and
//! contains the concatenated content of its mapped AGENTS.md sections.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

/// A rule group: maps AGENTS.md section numbers to a single .mdc file.
struct RuleGroup {
    /// Output filename without extension (e.g., "git-workflow").
    name: &'static str,
    /// Human-readable description for frontmatter.
    description: &'static str,
    /// Glob patterns for conditional activation. Empty = alwaysApply.
    globs: &'static [&'static str],
    /// AGENTS.md section numbers to include.
    sections: &'static [u8],
}

/// The 10-rule split mapping.
static RULE_GROUPS: &[RuleGroup] = &[
    RuleGroup {
        name: "git-workflow",
        description: "Git workflow, branch naming, PR review flow, PR resolution, and merge rules",
        globs: &[],
        sections: &[1, 6],
    },
    RuleGroup {
        name: "python-stack",
        description: "Python toolchain: uv, ruff, pyright. Delegate to python-uv subagent.",
        globs: &["**/*.py", "**/pyproject.toml", "**/requirements*.txt", "**/setup.cfg"],
        sections: &[2],
    },
    RuleGroup {
        name: "nodejs-stack",
        description: "Node.js stack: pnpm, biome, tsc, fnm. Never npm/yarn/nvm.",
        globs: &["**/*.ts", "**/*.js", "**/*.mjs", "**/*.cjs", "**/*.svelte", "**/package.json", "**/tsconfig.json"],
        sections: &[3, 4],
    },
    RuleGroup {
        name: "tdd-quality",
        description: "TDD, coverage thresholds, mutation testing, semantic test review anti-patterns.",
        globs: &["**/*test*", "**/*spec*", "**/vitest.config.*", "**/pytest.ini", "**/conftest.py"],
        sections: &[5, 8],
    },
    RuleGroup {
        name: "cicd-protocol",
        description: "CI/CD wait protocol (gh-wait), quality gates, PR-Agent, branch protection.",
        globs: &[".github/workflows/**", "**/.pr_agent.toml", "**/sonar-project.properties"],
        sections: &[9, 10],
    },
    RuleGroup {
        name: "deployment-strategy",
        description: "Deployment (DigitalOcean + Coolify), AI provider strategy, env-driven model switching.",
        globs: &["**/Dockerfile", "**/docker-compose*", "**/.env*", "**/ai.ts"],
        sections: &[11],
    },
    RuleGroup {
        name: "mandatory-services",
        description: "Required services: SonarCloud, PGlite+Drizzle, bcrypt, Pino. Never alternatives.",
        globs: &["**/schema.ts", "**/telemetry.ts", "**/auth/**", "**/sonar-project.properties", "**/drizzle.config.*"],
        sections: &[12],
    },
    RuleGroup {
        name: "communication-persona",
        description: "Caveman communication mode, honest craftsman persona, YAGNI, Boy Scout, dialectic triad.",
        globs: &[],
        sections: &[7, 23],
    },
    RuleGroup {
        name: "subagents-tools",
        description: "Subagent delegation, tool management, cache policy, documentation paths.",
        globs: &[],
        sections: &[13, 14, 15, 16],
    },
    RuleGroup {
        name: "engineering-discipline",
        description: "Verify before acting, instruction fidelity, never assume state, complete before done, file search, no fabricated APIs, structural hygiene.",
        globs: &[],
        sections: &[17, 18, 19, 20, 21, 22, 24],
    },
];
```

**Parsing logic:** Split AGENTS.md content on the regex pattern `^## \d+\. ` to extract sections. Each section includes everything from its heading to the next heading (or EOF). Map sections to rule groups by their number.

**Manifest:** Track generated `.mdc` files in `~/.cursor/rules/.manifest.json` (same pattern as `AgentManifest` in `agents/mod.rs`). On sync, remove orphan `.mdc` files that are no longer in the manifest.

**Sync flow:**
1. Read `~/.agents/AGENTS.md`
2. Parse into sections (HashMap<u8, String>)
3. For each RuleGroup: concatenate section content, wrap in frontmatter, write to `~/.cursor/rules/<name>.mdc`
4. Load manifest, compare with generated set, remove orphans
5. Save manifest

**`.mdc` file format:**
```markdown
---
description: <description from RuleGroup>
alwaysApply: true
---

<concatenated section content>
```

Or for glob-matched rules:
```markdown
---
description: <description from RuleGroup>
globs:
  - "**/*.py"
  - "**/pyproject.toml"
alwaysApply: false
---

<concatenated section content>
```

### 1.6 Module Registration (`src/lib.rs`)

Add:
```rust
pub mod rules;
```

### 1.7 Sync Integration (`src/main.rs`)

In `push_to_agents_impl()` (line ~330), after skills sync and before agent sync, add:

```rust
// Sync Cursor rules (AGENTS.md → .mdc files)
match agentalign::rules::sync_all(home, dry_run) {
    Ok(count) => {
        if count > 0 {
            println!("  cursor rules synced: {}", count);
        }
    }
    Err(e) => {
        eprintln!("  cursor rules sync error: {}", e);
    }
}
```

### 1.8 Watch Daemon Integration (`src/watch.rs`)

The watch daemon already watches the canonical instructions file (`canonical-instructions` entry at line 62). When AGENTS.md changes, `process_changes()` detects the change at line 248-252 but currently only logs "symlinks already reflect". Add rules sync trigger:

In `process_changes()`, after the canonical-instructions check block (line 248-252), add:

```rust
if entry.id == "canonical-instructions" {
    if !state.is_unchanged(&entry.id, &entry.path) {
        eprintln!("[watch] canonical instructions changed -> symlinks already reflect");
        // Also sync Cursor rules
        if let Err(e) = crate::rules::sync_all(home, false) {
            eprintln!("[watch] cursor rules sync error: {}", e);
        }
        state.update_hash(&entry.id, &entry.path);
    }
}
```

Also add `~/.cursor/rules/` to the watch list in `build_watch_list()`:

```rust
entries.push(WatchEntry {
    id: "cursor-rules".to_string(),
    agent_type: None,
    path: home.join(".cursor").join("rules"),
});
```

And handle it in `process_changes()`:
```rust
} else if entry.id == "cursor-rules" {
    if !state.is_unchanged(&entry.id, &entry.path) {
        eprintln!("[watch] cursor rules dir changed -> regenerating from canonical");
        if let Err(e) = crate::rules::sync_all(home, false) {
            eprintln!("[watch] cursor rules sync error: {}", e);
        }
        state.update_hash(&entry.id, &entry.path);
    }
}
```

---

## Part 2: AGENTS.md Split into 10 Thematic Rules

Each rule is a `.mdc` file in `~/.cursor/rules/`. Below is the frontmatter and content summary for each.

### Rule 1: `git-workflow.mdc`

```yaml
---
description: Git workflow, branch naming, PR review flow, PR resolution, and merge rules
alwaysApply: true
---
```

**Content:** AGENTS.md sections 1 (Version Control & Git Workflow) + 6 (PR Resolution).
- Never commit to main/master
- Branch naming: feature/name, fix/name, chore/name
- PR review flow: create PR → wait CodeRabbit → evaluate findings → merge
- PR resolution: review comments → fix CI → merge conflicts
- Severity levels: crit/warn/nit
- Merge: `gh pr merge --squash --admin`

### Rule 2: `python-stack.mdc`

```yaml
---
description: Python toolchain: uv, ruff, pyright. Delegate to python-uv subagent.
globs:
  - "**/*.py"
  - "**/pyproject.toml"
  - "**/requirements*.txt"
  - "**/setup.cfg"
alwaysApply: false
---
```

**Content:** AGENTS.md section 2 (Python Stack).
- Delegate Python to `python-uv` subagent
- uv toolchain authoritative for TDD, coverage, PR gates

### Rule 3: `nodejs-stack.mdc`

```yaml
---
description: Node.js stack: pnpm, biome, tsc, fnm. Never npm/yarn/nvm.
globs:
  - "**/*.ts"
  - "**/*.js"
  - "**/*.mjs"
  - "**/*.cjs"
  - "**/*.svelte"
  - "**/package.json"
  - "**/tsconfig.json"
alwaysApply: false
---
```

**Content:** AGENTS.md sections 3 (Node.js & JS Stack) + 4 (Node Version Management).
- Package mgmt: pnpm (never npm/yarn)
- Install: pnpm install / pnpm add
- Ephemeral: pnpm dlx (never npx)
- Lint/format: biome (never eslint/prettier)
- Type check: tsc --noEmit
- Node version: fnm (never nvm/n)

### Rule 4: `tdd-quality.mdc`

```yaml
---
description: TDD, coverage thresholds, mutation testing, semantic test review anti-patterns.
globs:
  - "**/*test*"
  - "**/*spec*"
  - "**/vitest.config.*"
  - "**/pytest.ini"
  - "**/conftest.py"
alwaysApply: false
---
```

**Content:** AGENTS.md sections 5 (TDD & QA) + 8 (Semantic Test Review).
- Tests before implementation (Red-Green-Refactor)
- Branch coverage min 90%
- Node.js: vitest + @vitest/coverage-v8 (never jest/mocha)
- Mutation: @stryker-mutator/core
- E2E: Playwright (never Cypress/Selenium)
- Semantic test review anti-patterns: overmocking, implementation coupling, weak assertions, mirror tests
- Framework-specific flags (Pytest, Vitest, Cargo, Playwright)
- Mocking budget: volatile OK, stable = crit
- CI gate: fail if >3 impl-coupled tests / mock ratio >50% stable deps

### Rule 5: `cicd-protocol.mdc`

```yaml
---
description: CI/CD wait protocol (gh-wait), quality gates, PR-Agent, branch protection.
globs:
  - ".github/workflows/**"
  - "**/.pr_agent.toml"
  - "**/sonar-project.properties"
alwaysApply: false
---
```

**Content:** AGENTS.md sections 9 (CI/CD Wait Protocol) + 10 (Quality Gates & PR-Agent).
- gh-wait usage: pr, ci, coderabbit, reviews, mergeable
- Never manual poll or blind sleep
- Required workflows: pr-gate.yml, merge-gate.yml, pr-agent.yml
- PR-Agent Pattern C (slash-command-only)
- SvelteKit PR gate: pnpm lint, pnpm check, pnpm vitest run --coverage, pnpm build
- Branch protection setup

### Rule 6: `deployment-strategy.mdc`

```yaml
---
description: Deployment (DigitalOcean + Coolify), AI provider strategy, env-driven model switching.
globs:
  - "**/Dockerfile"
  - "**/docker-compose*"
  - "**/.env*"
  - "**/ai.ts"
alwaysApply: false
---
```

**Content:** AGENTS.md section 11 (Deployment & AI Strategy).
- Hosting: DigitalOcean Premium Droplet (2 vCPU/4GB, ~$24/mo)
- Deployment: Coolify → GitHub
- AI_PROVIDER env var: mini/openrouter/groq/ollama
- src/lib/ai.ts reads AI_PROVIDER
- Dev: ollama locally ($0)

### Rule 7: `mandatory-services.mdc`

```yaml
---
description: Required services: SonarCloud, PGlite+Drizzle, bcrypt, Pino. Never alternatives.
globs:
  - "**/schema.ts"
  - "**/telemetry.ts"
  - "**/auth/**"
  - "**/sonar-project.properties"
  - "**/drizzle.config.*"
alwaysApply: false
---
```

**Content:** AGENTS.md section 12 (Mandatory Services).
- SonarCloud: sonar-project.properties at root
- PGlite + Drizzle ORM: schema in src/lib/db/schema.ts
- bcrypt: bcryptjs, salt rounds 12, auth in src/lib/auth/
- Pino: structured JSON logger in src/lib/telemetry.ts

### Rule 8: `communication-persona.mdc`

```yaml
---
description: Caveman communication mode, honest craftsman persona, YAGNI, Boy Scout, dialectic triad.
alwaysApply: true
---
```

**Content:** AGENTS.md sections 7 (Caveman Communication Mode) + 23 (Honest Software Craftsman & Critical Persona).
- Terse communication, drop filler/articles
- Pattern: [thing] [action] [reason]. [next step]
- Professional critic, no yes-man, no cheerleader
- YAGNI: delete dead code
- Boy Scout: leave cleaner
- Git = truth
- Dialectic triad: Advocate/Critic/Judge
- Self-critique loop (7 questions)

### Rule 9: `subagents-tools.mdc`

```yaml
---
description: Subagent delegation, tool management, cache policy, documentation paths.
alwaysApply: true
---
```

**Content:** AGENTS.md sections 13 (Tool & Executable Management) + 14 (Cache & Cleanup Policy) + 15 (Subagents) + 16 (Documentation Paths).
- Re-install executables after source changes
- Never clear expensive caches (huggingface, mathlib, uv, etc.)
- Subagents: vision, python-uv, svelte-file-editor
- Swap models via ~/.agents/agents/ frontmatter + agentalign agents sync
- Documentation paths: docs/adrs/, docs/specs/, docs/plans/

### Rule 10: `engineering-discipline.mdc`

```yaml
---
description: Verify before acting, instruction fidelity, never assume state, complete before done, file search, no fabricated APIs, structural hygiene.
alwaysApply: true
---
```

**Content:** AGENTS.md sections 17-22 + 24 (Verify Before Acting, Instruction Fidelity, Never Assume State, Complete Before Claiming Done, File Search & Content Search, No Fabricated APIs, Structural Hygiene).
- Read → verify → act pattern
- Execute user workflows step-by-step, never skip/reorder
- Never assume file content/API shape/project structure
- Implement ALL requested features, partial = not done
- File search: fd (never glob tool)
- Content search: rg (never grep tool)
- Never call non-existent APIs, read source first
- Structural hygiene: cohesive packages, intention-revealing names, domain-oriented, max depth 4

---

## Part 3: Hooks Design

### 3.1 Hooks Configuration (`~/.cursor/hooks.json`)

```json
{
  "preToolUse": [
    {
      "type": "command",
      "command": "bash $HOME/.agents/hooks/git-guard.sh",
      "matcher": "Bash"
    }
  ],
  "afterFileEdit": [
    {
      "type": "command",
      "command": "bash $HOME/.agents/hooks/biome-check.sh",
      "matcher": "**/*.{ts,js,mjs,cjs,svelte}"
    },
    {
      "type": "command",
      "command": "bash $HOME/.agents/hooks/typecheck.sh",
      "matcher": "**/*.{ts,svelte}"
    }
  ]
}
```

**Note:** Cursor hooks receive a JSON payload on stdin. The exact schema is not fully documented but follows the pattern:
```json
{
  "tool": "Bash",
  "params": {
    "command": "git push --force"
  }
}
```

The hook script can return JSON to block the action:
```json
{
  "blocked": true,
  "message": "Destructive git command blocked: git push --force"
}
```

Or empty/no output to allow. If the hook exits non-zero, the action is blocked.

### 3.2 Hook Scripts

#### `~/.agents/hooks/git-guard.sh`

**Purpose:** Block destructive git commands before they execute.

**Destructive patterns to block:**
- `git push --force` / `git push -f` (to non-feature branches)
- `git push --force-with-lease` (to main/master — allowed to feature branches)
- `git reset --hard` (if uncommitted changes exist)
- `git clean -fdx` / `git clean -fd`
- `git branch -D` (force delete)
- `git checkout -- .` / `git restore .` (discard all changes)
- `git commit --amend` (to main/master)
- Any `git` command with `--no-verify` flag

**Script logic:**
1. Read JSON from stdin
2. Extract `params.command` field
3. Check against blocklist patterns (regex)
4. If matched: print JSON `{"blocked": true, "message": "..."}` and exit 1
5. If clean: exit 0 with no output

```bash
#!/usr/bin/env bash
set -euo pipefail

# Read JSON payload from stdin
INPUT=$(cat)

# Extract command from JSON (using python3 for reliable parsing)
CMD=$(echo "$INPUT" | python3 -c "
import json, sys
try:
    data = json.load(sys.stdin)
    print(data.get('params', {}).get('command', ''))
except:
    print('')
" 2>/dev/null)

# Destructive patterns
DESTRUCTIVE_PATTERNS=(
    'git push (--force|-f)(?!.*--force-with-lease)'
    'git push.*--force.*main'
    'git push.*--force.*master'
    'git reset --hard'
    'git clean -fd'
    'git branch -D'
    'git checkout -- \.'
    'git restore \.'
    'git commit --amend.*main'
    'git commit --amend.*master'
    'git.*--no-verify'
)

for pattern in "${DESTRUCTIVE_PATTERNS[@]}"; do
    if echo "$CMD" | grep -qE "$pattern"; then
        echo "{\"blocked\": true, \"message\": \"Blocked by git-guard: matched '$pattern'\"}"
        exit 1
    fi
done

exit 0
```

#### `~/.agents/hooks/biome-check.sh`

**Purpose:** Run biome check on the edited file.

**Script logic:**
1. Read JSON from stdin
2. Extract file path from `params.filePath` or `params.path`
3. Check if `biome.json` or `biome.jsonc` exists in project root
4. If biome config exists: run `pnpm dlx @biomejs/biome check --write <file>`
5. If no biome config: exit 0 (skip)
6. If biome fails: print error JSON, exit 0 (non-blocking — don't block edits, just warn)

```bash
#!/usr/bin/env bash
set -euo pipefail

INPUT=$(cat)

FILE_PATH=$(echo "$INPUT" | python3 -c "
import json, sys
try:
    data = json.load(sys.stdin)
    print(data.get('params', {}).get('filePath', data.get('params', {}).get('path', '')))
except:
    print('')
" 2>/dev/null)

if [ -z "$FILE_PATH" ] || [ ! -f "$FILE_PATH" ]; then
    exit 0
fi

# Find project root (nearest biome.json or package.json)
DIR=$(dirname "$FILE_PATH")
while [ "$DIR" != "/" ]; do
    if [ -f "$DIR/biome.json" ] || [ -f "$DIR/biome.jsonc" ]; then
        cd "$DIR"
        pnpm dlx @biomejs/biome check --write "$FILE_PATH" 2>/dev/null || true
        break
    fi
    DIR=$(dirname "$DIR")
done

exit 0
```

#### `~/.agents/hooks/typecheck.sh`

**Purpose:** Run tsc --noEmit after .ts/.svelte file edits.

**Script logic:**
1. Read JSON from stdin
2. Extract file path
3. Find tsconfig.json in project root
4. If tsconfig exists: run `pnpm exec tsc --noEmit` in background (non-blocking)
5. If no tsconfig: exit 0

```bash
#!/usr/bin/env bash
set -euo pipefail

INPUT=$(cat)

FILE_PATH=$(echo "$INPUT" | python3 -c "
import json, sys
try:
    data = json.load(sys.stdin)
    print(data.get('params', {}).get('filePath', data.get('params', {}).get('path', '')))
except:
    print('')
" 2>/dev/null)

if [ -z "$FILE_PATH" ]; then
    exit 0
fi

# Find project root (nearest tsconfig.json)
DIR=$(dirname "$FILE_PATH")
while [ "$DIR" != "/" ]; do
    if [ -f "$DIR/tsconfig.json" ]; then
        cd "$DIR"
        # Non-blocking: run in background, log to file
        (pnpm exec tsc --noEmit 2>&1 | tail -5) || true
        break
    fi
    DIR=$(dirname "$DIR")
done

exit 0
```

### 3.3 Hooks in agentalign

Hooks are NOT synced by agentalign automatically. They are one-time setup. The `~/.agents/hooks/` scripts are part of the canonical store and should be manually created (or scripted in the sync execution plan below).

The `~/.cursor/hooks.json` file should be generated by a future `agentalign hooks sync` command, but for now it is a manual one-time setup (see Part 5: Sync Execution Plan).

**Future enhancement:** Add a `hooks` module to agentalign that syncs `~/.agents/hooks/*.sh` to `~/.cursor/hooks.json` + `~/.agents/hooks/`. Out of scope for this plan.

---

## Part 4: Project-Level vs User-Level Config Split

### 4.1 User-Level Config (applies to ALL projects)

| Config | Path | Source |
|--------|------|--------|
| MCP servers | `~/.cursor/mcp.json` | agentalign sync (already working) |
| Skills | `~/.cursor/skills/*/` | agentalign skills sync (symlinks from `~/.agents/skills/`) |
| Agents | `~/.cursor/agents/*.md` | agentalign agents sync (from `~/.agents/agents/*.md`) |
| Rules | `~/.cursor/rules/*.mdc` | agentalign rules sync (from `~/.agents/AGENTS.md`) |
| Hooks | `~/.cursor/hooks.json` | Manual one-time setup |
| Hook scripts | `~/.agents/hooks/*.sh` | Manual one-time setup |

### 4.2 Project-Level Config for vidiomtm/Vidiom

**Path:** `~/projects/vidiomtm/Vidiom/.cursor/`

| Config | Path | Content | Rationale |
|--------|------|---------|-----------|
| Rules | `.cursor/rules/project-conventions.mdc` | SvelteKit monorepo conventions, pnpm workspace rules, Docker setup, Makefile targets | Project-specific, not in AGENTS.md |
| MCP | `.cursor/mcp.json` (optional) | Project-specific MCP servers only | If vidiomtm needs servers not in canonical |
| Agents | `.cursor/agents/` (empty) | None needed | All agents are user-level |
| Skills | `.cursor/skills/` (empty) | None needed | All skills are user-level |
| Hooks | `.cursor/hooks.json` (optional) | Empty or project-specific overrides | User-level hooks sufficient for vidiomtm |

**Project-level rule example (`.cursor/rules/project-conventions.mdc`):**

```yaml
---
description: Vidiom project-specific conventions: monorepo structure, Docker, Makefile
globs:
  - "**/*"
alwaysApply: false
---
```

Content would include:
- Monorepo structure: `apps/` for SvelteKit apps, `infra/` for Docker/infra
- Package manager: pnpm workspaces (`pnpm -r`)
- Docker: `docker-compose.yml` for dev, `docker-compose.prod.yml` for prod
- Makefile targets: `make dev`, `make test`, `make build`
- Biome config: `biome.json` at root
- No `.cursor/agents/` or `.cursor/skills/` — all user-level

### 4.3 Cursor Priority Resolution

Cursor resolves config with project-over-user priority:
1. Project-level `.cursor/rules/*.mdc` override user-level `~/.cursor/rules/*.mdc` (by filename)
2. Project-level `.cursor/agents/*.md` override user-level `~/.cursor/agents/*.md` (by filename)
3. Project-level `.cursor/skills/*/` override user-level `~/.cursor/skills/*/` (by skill name)
4. MCP: project `.cursor/mcp.json` merges with user `~/.cursor/mcp.json`

**Implication:** agentalign syncs to user-level only. Project-level config is managed manually per-project. This matches how Claude works (CLAUDE.md at project root overrides ~/.claude/CLAUDE.md).

---

## Part 5: Sync Execution Plan

### Phase 1: Code Implementation

Execute in this order (each produces independently testable changes):

1. **Create `src/agents/cursor.rs`** — `CursorAgentStrategy` with tests
2. **Update `src/agents/mod.rs`** — Add `pub mod cursor;` and register strategy
3. **Run `cargo test`** — Verify agent strategy tests pass
4. **Update `src/skills/mod.rs`** — Add Cursor to registry, fix test assertion
5. **Run `cargo test`** — Verify skills tests pass
6. **Update `src/mcp/factory.rs`** — Add `skills_dir` to Cursor descriptor
7. **Create `src/rules/mod.rs`** — Rules sync module with tests
8. **Update `src/lib.rs`** — Add `pub mod rules;`
9. **Update `src/main.rs`** — Call `rules::sync_all()` in `push_to_agents_impl()`
10. **Update `src/watch.rs`** — Add rules watching and sync trigger
11. **Run `cargo test`** — All tests pass
12. **Run `cargo build`** — Compiles cleanly
13. **Install:** `cargo install --path ~/projects/agent-utils/agentalign --force`

### Phase 2: Hooks Setup (one-time)

14. **Create `~/.agents/hooks/` directory:** `mkdir -p ~/.agents/hooks`
15. **Write hook scripts:** `git-guard.sh`, `biome-check.sh`, `typecheck.sh`
16. **Make executable:** `chmod +x ~/.agents/hooks/*.sh`
17. **Write `~/.cursor/hooks.json`** — Reference the hook scripts

### Phase 3: Sync Execution

18. **Dry run:** `agentalign sync --dry-run`
    - Verify output shows Cursor targets for MCP, skills, agents, rules
    - Verify no errors
19. **Full sync:** `agentalign sync`
    - MCP: `~/.cursor/mcp.json` (already working, no change expected)
    - Skills: symlinks created in `~/.cursor/skills/` for each canonical skill
    - Agents: `~/.cursor/agents/*.md` created for each canonical agent
    - Rules: `~/.cursor/rules/*.mdc` created (10 files)
    - Instruction symlinks: healed for OpenCode, Claude, Gemini, Codex
20. **Verify agent sync:** `agentalign agents sync`
    - Confirm Cursor agents written to `~/.cursor/agents/`

### Phase 4: Project-Level Config (vidiomtm/Vidiom)

21. **Create `.cursor/rules/` in project:** `mkdir -p ~/projects/vidiomtm/Vidiom/.cursor/rules`
22. **Write project conventions rule:** `.cursor/rules/project-conventions.mdc`
23. **No project-level agents/skills/hooks needed** — user-level is sufficient

---

## Part 6: Verification Steps

### 6.1 Skills Verification

```bash
# Verify canonical skills are symlinked into Cursor
for skill in $(ls ~/.agents/skills/); do
    link=~/.cursor/skills/$skill
    if [ -L "$link" ]; then
        echo "OK: $skill -> $(readlink $link)"
    else
        echo "FAIL: $skill not symlinked"
    fi
done

# Verify OpenSpec skills are untouched (still real directories)
for skill in openspec-apply-change openspec-archive-change openspec-explore openspec-propose; do
    if [ -d ~/.cursor/skills/$skill ] && [ ! -L ~/.cursor/skills/$skill ]; then
        echo "OK: $skill preserved as real directory"
    else
        echo "FAIL: $skill was modified"
    fi
done

# Verify built-in skills untouched
ls ~/.cursor/skills-cursor/ | wc -l  # Should be 19 (unchanged)
```

### 6.2 Agents Verification

```bash
# Verify canonical agents synced to Cursor
for agent in $(ls ~/.agents/agents/*.md 2>/dev/null); do
    name=$(basename "$agent")
    target=~/.cursor/agents/$name
    if [ -f "$target" ]; then
        echo "OK: $name synced"
        # Verify frontmatter format matches Cursor (name, description, model, tools)
        head -20 "$target"
    else
        echo "FAIL: $name not synced"
    fi
done

# Verify manifest updated
cat ~/.cursor/agents/.manifest.json 2>/dev/null || echo "No manifest (may be in ~/.agents/agents/)"
```

### 6.3 Rules Verification

```bash
# Verify 10 .mdc files exist
ls ~/.cursor/rules/*.mdc | wc -l  # Should be 10

# Verify each file has valid frontmatter
for rule in ~/.cursor/rules/*.mdc; do
    echo "=== $(basename $rule) ==="
    head -10 "$rule"
    echo
done

# Verify content matches AGENTS.md sections
# Check that git-workflow.mdc contains both §1 and §6 content
grep -c "PR Review Flow" ~/.cursor/rules/git-workflow.mdc  # Should be > 0 (from §1)
grep -c "PR Resolution" ~/.cursor/rules/git-workflow.mdc   # Should be > 0 (from §6)

# Verify manifest exists
cat ~/.cursor/rules/.manifest.json 2>/dev/null
```

### 6.4 Hooks Verification

```bash
# Verify hooks.json is valid JSON
python3 -c "import json; json.load(open('$HOME/.cursor/hooks.json'))" && echo "OK: valid JSON"

# Verify hook scripts exist and are executable
for script in git-guard.sh biome-check.sh typecheck.sh; do
    if [ -x ~/.agents/hooks/$script ]; then
        echo "OK: $script executable"
    else
        echo "FAIL: $script missing or not executable"
    fi
done

# Test git-guard blocks destructive commands
echo '{"tool":"Bash","params":{"command":"git push --force origin main"}}' | bash ~/.agents/hooks/git-guard.sh
# Expected: exit 1, JSON with "blocked": true

# Test git-guard allows safe commands
echo '{"tool":"Bash","params":{"command":"git status"}}' | bash ~/.agents/hooks/git-guard.sh
# Expected: exit 0
```

### 6.5 MCP Verification (existing, should be unchanged)

```bash
# Verify Cursor MCP config still valid
python3 -c "import json; data=json.load(open('$HOME/.cursor/mcp.json')); print(f'{len(data.get(\"mcpServers\",{}))} servers')"
```

### 6.6 Watch Daemon Verification

```bash
# Start watch daemon
agentalign watch &

# Modify AGENTS.md (add a test line, then revert)
echo "<!-- test -->" >> ~/.agents/AGENTS.md

# Wait for debounce (500ms + processing)
sleep 2

# Verify rules regenerated
ls ~/.cursor/rules/*.mdc | wc -l  # Should still be 10

# Revert AGENTS.md
git checkout ~/.agents/AGENTS.md  # or manual revert

# Stop daemon
kill %1
```

### 6.7 Integration Test

```bash
# Full sync + verify
agentalign sync --dry-run  # Preview
agentalign sync            # Execute
agentalign agents sync     # Agents only

# Verify all 4 sync domains for Cursor:
echo "=== MCP ==="
python3 -c "import json; print(len(json.load(open('$HOME/.cursor/mcp.json')).get('mcpServers',{})), 'servers')"
echo "=== Skills ==="
ls -la ~/.cursor/skills/ | grep -c "^l"  # Count symlinks
echo "=== Agents ==="
ls ~/.cursor/agents/*.md 2>/dev/null | wc -l
echo "=== Rules ==="
ls ~/.cursor/rules/*.mdc 2>/dev/null | wc -l
```

---

## Appendix: Risk Assessment

### Risk 1: `~/.cursor/rules/` user-level support undocumented
**Impact:** Rules may not load from `~/.cursor/rules/`
**Mitigation:** Test in Cursor before relying on it. Fallback: create a single `~/.cursor/rules/agents-md.mdc` with `alwaysApply: true` wrapping full AGENTS.md content (no split). This is simpler but loses the glob-matching benefit.

### Risk 2: Hooks JSON schema undocumented
**Impact:** Hook scripts may not receive expected JSON or block correctly
**Mitigation:** Start with non-blocking hooks (exit 0 always, log to file). Capture actual stdin payloads. Then enable blocking once schema confirmed.

### Risk 3: OpenSpec skills conflict
**Impact:** `heal_skill()` backs up and replaces real directories
**Probability:** Very low — verified no name conflicts between canonical skills and OpenSpec skills
**Mitigation:** Already safe. `heal_all()` only processes canonical skills. OpenSpec skills are not in canonical store, so never touched.

### Risk 4: Cursor agent format divergence
**Impact:** Cursor may require different frontmatter fields than Claude
**Mitigation:** `CursorAgentStrategy` is a separate struct. If format diverges, only `cursor.rs` changes. No impact on other strategies.

### Risk 5: Rules module parsing fragility
**Impact:** AGENTS.md heading format changes break section parsing
**Mitigation:** Parser uses regex `^## \d+\. ` which matches the established pattern. Add a test with the actual AGENTS.md content. If headings change, update `RULE_GROUPS` section numbers.
