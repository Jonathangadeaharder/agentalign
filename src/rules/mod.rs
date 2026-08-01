//! Rules generation from AGENTS.md for Cursor and Claude Code.
//!
//! Cursor and Claude Code both support path-scoped rules that load conditionally
//! based on file patterns. This module reads the canonical AGENTS.md, splits it
//! into thematic rules, and writes:
//! - Cursor: `.mdc` files in `~/.cursor/rules/` (frontmatter: globs, alwaysApply)
//! - Claude: `.md` files in `~/.claude/rules/` (frontmatter: paths as YAML list)
//!
//! OpenCode, Gemini, and Codex don't have path-scoped rules — they use the
//! symlinked AGENTS.md from the instructions module.
//!
//! Called by:
//! - `agentalign sync` (after MCP + instruction + skills + agent sync)
//! - `agentalign watch` (on daemon startup and on AGENTS.md changes)

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A single rule definition shared across both Cursor and Claude.
struct RuleDef {
    /// Base filename without extension (e.g., "git-workflow").
    basename: &'static str,
    description: &'static str,
    always_apply: bool,
    globs: Option<&'static str>,
    section_numbers: &'static [usize],
}

/// The thematic rule mapping: AGENTS.md sections → rule files.
const RULE_DEFS: &[RuleDef] = &[
    RuleDef {
        basename: "git-workflow",
        description: "Git workflow standards - branching, PR flow, review, merge. NEVER commit/push to main/master.",
        always_apply: true,
        globs: None,
        section_numbers: &[1],
    },
    RuleDef {
        basename: "python-stack",
        description: "Python stack: uv, ruff, pyright. Delegate Python to python-uv subagent.",
        always_apply: false,
        globs: Some("**/*.py"),
        section_numbers: &[2],
    },
    RuleDef {
        basename: "nodejs-stack",
        description: "Node.js/JS stack: pnpm, biome, tsc, volta. NEVER npm/yarn/npx.",
        always_apply: false,
        globs: Some("**/*.{ts,js,mts,mjs,svelte,json5}"),
        section_numbers: &[3, 4],
    },
    RuleDef {
        basename: "tdd-quality",
        description: "TDD & Quality Assurance: Red-Green-Refactor, 90% branch coverage, vitest+stryker, Playwright.",
        always_apply: true,
        globs: None,
        section_numbers: &[5],
    },
    RuleDef {
        basename: "pr-resolution",
        description: "PR resolution workflow: comments, CI, merge conflicts. Severity levels.",
        always_apply: true,
        globs: None,
        section_numbers: &[6],
    },
    RuleDef {
        basename: "communication-persona",
        description: "Caveman communication mode: terse, technical, no fluff. Active every response.",
        always_apply: true,
        globs: None,
        section_numbers: &[7],
    },
    RuleDef {
        basename: "semantic-test-review",
        description: "Semantic test review: anti-patterns, mocking budget, framework-specific checks.",
        always_apply: false,
        globs: Some("**/*.test.{ts,js,py},**/*.spec.{ts,js}"),
        section_numbers: &[8],
    },
    RuleDef {
        basename: "ci-cd-protocol",
        description: "CI/CD wait protocol (gh-wait) and quality gates (pr-gate, merge-gate, PR-Agent).",
        always_apply: false,
        globs: Some("**/.github/workflows/*"),
        section_numbers: &[9, 10],
    },
    RuleDef {
        basename: "deployment-strategy",
        description: "Deployment: DigitalOcean droplet, Coolify, AI_PROVIDER env-driven AI strategy.",
        always_apply: false,
        globs: Some("**/{Dockerfile,docker-compose*,coolify*,.env*}"),
        section_numbers: &[11],
    },
    RuleDef {
        basename: "mandatory-services",
        description: "Mandatory services: SonarCloud, PGlite+Drizzle, bcrypt, Pino. Every project MUST integrate.",
        always_apply: true,
        globs: None,
        section_numbers: &[12],
    },
    RuleDef {
        basename: "engineering-discipline",
        description: "Engineering discipline: verify-before-act, instruction fidelity, never assume, complete-before-claim, fd/rg, no fabricated APIs, craftsman persona, structural hygiene, cache policy, tool management, docs paths, subagents.",
        always_apply: true,
        globs: None,
        section_numbers: &[13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24],
    },
];

/// Extract a section from AGENTS.md by its section number.
/// Sections are delimited by `## N. ` headings. Lines inside fenced code
/// blocks (` ``` `) are never treated as headings, so a code sample that
/// happens to contain a `## `-prefixed comment doesn't truncate the section.
fn extract_section(content: &str, section_num: usize) -> Option<String> {
    let heading_prefix = format!("## {}. ", section_num);
    let lines: Vec<&str> = content.lines().collect();
    let mut start_idx = None;
    let mut end_idx = lines.len();
    let mut in_code_block = false;

    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        if line.starts_with(&heading_prefix) {
            start_idx = Some(i);
        } else if start_idx.is_some() && line.starts_with("## ") {
            end_idx = i;
            break;
        }
    }

    let start = start_idx?;
    let section_lines = &lines[start..end_idx];
    let section_text = section_lines.join("\n");

    if section_text.trim().is_empty() {
        None
    } else {
        Some(section_text)
    }
}

/// Build the body content for a rule from its section numbers.
fn build_body(content: &str, rule: &RuleDef) -> Option<String> {
    let mut body_parts = Vec::new();

    for &section_num in rule.section_numbers {
        if let Some(section) = extract_section(content, section_num) {
            if !body_parts.is_empty() {
                body_parts.push(String::new());
            }
            body_parts.push(section);
        }
    }

    if body_parts.is_empty() {
        None
    } else {
        Some(body_parts.join("\n"))
    }
}

/// Format a Cursor `.mdc` file from a rule definition and its content.
fn format_cursor_mdc(rule: &RuleDef, body: &str) -> String {
    let mut fm = String::from("---\n");
    fm.push_str(&format!("description: {}\n", yaml_quote(rule.description)));

    if let Some(globs) = rule.globs {
        fm.push_str(&format!("globs: {}\n", yaml_quote(globs)));
    }

    fm.push_str(&format!("alwaysApply: {}\n", rule.always_apply));
    fm.push_str("---\n\n");

    format!("{}{}", fm, body)
}

/// Wrap a value in a double-quoted YAML scalar, escaping backslashes and
/// double quotes. Plain (unquoted) scalars break when the value contains
/// `: ` (e.g. a description like "TDD & Quality Assurance: ...") since that
/// reads as a nested mapping to most YAML parsers.
fn yaml_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Format a Claude Code `.md` rule file from a rule definition and its content.
/// Claude uses `paths` (YAML list) instead of `globs` (single string).
/// No `alwaysApply` field — rules without `paths` load unconditionally.
fn format_claude_md(rule: &RuleDef, body: &str) -> String {
    let mut fm = String::from("---\n");

    if let Some(globs) = rule.globs {
        fm.push_str("paths:\n");
        let patterns = split_glob_patterns(globs);
        for pattern in patterns {
            fm.push_str(&format!("  - \"{}\"\n", pattern));
        }
    }

    fm.push_str("---\n\n");

    format!("{}{}", fm, body)
}

/// Split a comma-separated glob string into individual patterns,
/// respecting brace expansion (commas inside `{}` are part of the pattern).
fn split_glob_patterns(globs: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut brace_depth = 0;
    let mut start = 0;

    for (i, ch) in globs.char_indices() {
        match ch {
            '{' => brace_depth += 1,
            '}' => {
                if brace_depth > 0 {
                    brace_depth -= 1;
                }
            }
            ',' if brace_depth == 0 => {
                let pattern = globs[start..i].trim();
                if !pattern.is_empty() {
                    result.push(pattern);
                }
                start = i + 1;
            }
            _ => {}
        }
    }

    let last = globs[start..].trim();
    if !last.is_empty() {
        result.push(last);
    }

    result
}

/// Write rules to a target directory with a given formatter and extension.
/// Returns the number of rules written (changed or new).
fn write_rules(
    content: &str,
    rules_dir: &Path,
    extension: &str,
    formatter: fn(&RuleDef, &str) -> String,
    agent_label: &str,
    dry_run: bool,
) -> anyhow::Result<usize> {
    if dry_run {
        println!(
            "  [DRY RUN] {} rules -> {} ({} rules)",
            agent_label,
            rules_dir.display(),
            RULE_DEFS.len()
        );
        return Ok(RULE_DEFS.len());
    }

    std::fs::create_dir_all(rules_dir)?;
    let mut written = 0usize;
    let mut current_files: HashSet<String> = HashSet::new();

    for rule in RULE_DEFS {
        let body = match build_body(content, rule) {
            Some(b) => b,
            None => {
                eprintln!(
                    "  warning: no content found for rule {} (sections {:?})",
                    rule.basename, rule.section_numbers
                );
                continue;
            }
        };

        let filename = format!("{}.{}", rule.basename, extension);
        current_files.insert(filename.clone());

        let file_content = formatter(rule, &body);
        let target = rules_dir.join(&filename);

        if target.exists() {
            if let Ok(existing) = std::fs::read_to_string(&target) {
                if existing == file_content {
                    continue;
                }
            }
        }

        std::fs::write(&target, &file_content)?;
        written += 1;
        println!("  {} rule {} -> {}", agent_label, filename, target.display());
    }

    written += remove_orphaned_rules(rules_dir, &current_files, agent_label)?;

    Ok(written)
}

/// Manifest filename tracking which rule files agentalign generated in a rules dir.
const MANIFEST_FILENAME: &str = ".manifest.json";

/// Remove rule files that were generated by a previous sync but are no longer
/// produced (e.g. a `RuleDef` was renamed or dropped). Returns the number removed.
fn remove_orphaned_rules(
    rules_dir: &Path,
    current_files: &HashSet<String>,
    agent_label: &str,
) -> anyhow::Result<usize> {
    let manifest_path = rules_dir.join(MANIFEST_FILENAME);
    let previous: HashSet<String> = std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let mut removed = 0usize;
    for orphan in previous.difference(current_files) {
        let orphan_path = rules_dir.join(orphan);
        if orphan_path.exists() && std::fs::remove_file(&orphan_path).is_ok() {
            removed += 1;
            println!(
                "  removed orphaned {} rule {} -> {}",
                agent_label,
                orphan,
                orphan_path.display()
            );
        }
    }

    let data = serde_json::to_string_pretty(current_files)?;
    std::fs::write(&manifest_path, data)?;

    Ok(removed)
}

/// Get the target rules directory for Cursor.
pub fn cursor_rules_dir(home: &Path) -> PathBuf {
    home.join(".cursor").join("rules")
}

/// Get the target rules directory for Claude Code.
pub fn claude_rules_dir(home: &Path) -> PathBuf {
    home.join(".claude").join("rules")
}

/// Sync AGENTS.md sections into Cursor `.mdc` and Claude `.md` rules.
/// Returns the total number of rules written across both targets.
pub fn sync_rules(home: &Path, dry_run: bool) -> anyhow::Result<usize> {
    let agents_md_path = home.join(".agents").join("AGENTS.md");

    if !agents_md_path.exists() {
        eprintln!(
            "  warning: canonical AGENTS.md not found at {} — skipping rules sync",
            agents_md_path.display()
        );
        return Ok(0);
    }

    let content = std::fs::read_to_string(&agents_md_path)?;
    let mut total_written = 0usize;

    let cursor_dir = cursor_rules_dir(home);
    total_written += write_rules(
        &content,
        &cursor_dir,
        "mdc",
        format_cursor_mdc,
        "cursor",
        dry_run,
    )?;

    let claude_dir = claude_rules_dir(home);
    total_written += write_rules(
        &content,
        &claude_dir,
        "md",
        format_claude_md,
        "claude",
        dry_run,
    )?;

    Ok(total_written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (TempDir, std::path::PathBuf) {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let agents_dir = home.join(".agents");
        fs::create_dir_all(&agents_dir).unwrap();

        let agents_md = agents_dir.join("AGENTS.md");
        fs::write(
            &agents_md,
            "# AI Agent Execution & Tooling Rules\n\n## 1. Git Workflow\n- NEVER push to main\n\n## 2. Python Stack\n- Use uv\n\n## 3. Node.js Stack\n- Use pnpm\n\n## 4. Node Version\n- Use volta\n\n## 5. TDD\n- Red-green-refactor\n\n## 6. PR Resolution\n- Resolve comments\n\n## 7. Caveman Mode\n- Terse\n\n## 8. Test Review\n- Check overmocking\n\n## 9. CI/CD Protocol\n- Use gh-wait\n\n## 10. Quality Gates\n- pr-gate.yml\n\n## 11. Deployment\n- DigitalOcean\n\n## 12. Mandatory Services\n- SonarCloud\n\n## 13. Tool Management\n- Re-install after edit\n\n## 14. Cache Policy\n- Never clear pnpm cache\n\n## 15. Subagents\n- vision, python-uv\n\n## 16. Documentation Paths\n- docs/adrs/\n\n## 17. Verify Before Acting\n- Read before edit\n\n## 18. Instruction Fidelity\n- Follow steps in order\n\n## 19. Never Assume State\n- Read it first\n\n## 20. Complete Before Claiming Done\n- Implement ALL features\n\n## 21. File Search\n- Use fd/rg\n\n## 22. No Fabricated APIs\n- Read the source\n\n## 23. Honest Craftsman\n- YAGNI\n\n## 24. Structural Hygiene\n- No god packages\n",
        )
        .unwrap();

        (tmp, home)
    }

    #[test]
    fn test_sync_rules_creates_cursor_files() {
        let (_tmp, home) = setup();
        let count = sync_rules(&home, false).unwrap();
        assert!(count > 0, "expected rules to be written");

        let rules_dir = cursor_rules_dir(&home);
        assert!(rules_dir.exists());

        let git_rule = rules_dir.join("git-workflow.mdc");
        assert!(git_rule.exists());

        let content = fs::read_to_string(&git_rule).unwrap();
        assert!(content.contains("description:"));
        assert!(content.contains("alwaysApply: true"));
        assert!(content.contains("## 1. Git Workflow"));
        assert!(content.contains("NEVER push to main"));
    }

    #[test]
    fn test_sync_rules_creates_claude_files() {
        let (_tmp, home) = setup();
        sync_rules(&home, false).unwrap();

        let rules_dir = claude_rules_dir(&home);
        assert!(rules_dir.exists());

        let git_rule = rules_dir.join("git-workflow.md");
        assert!(git_rule.exists());

        let content = fs::read_to_string(&git_rule).unwrap();
        assert!(content.starts_with("---\n"));
        assert!(!content.contains("alwaysApply"));
        assert!(!content.contains("globs"));
        assert!(content.contains("## 1. Git Workflow"));
        assert!(content.contains("NEVER push to main"));
    }

    #[test]
    fn test_sync_rules_dry_run() {
        let (_tmp, home) = setup();
        let count = sync_rules(&home, true).unwrap();
        assert_eq!(count, RULE_DEFS.len() * 2);

        assert!(!cursor_rules_dir(&home).exists());
        assert!(!claude_rules_dir(&home).exists());
    }

    #[test]
    fn test_sync_rules_idempotent() {
        let (_tmp, home) = setup();
        let count1 = sync_rules(&home, false).unwrap();
        assert!(count1 > 0);

        let count2 = sync_rules(&home, false).unwrap();
        assert_eq!(count2, 0, "second sync should write nothing");
    }

    #[test]
    fn test_removes_orphaned_rule_file() {
        let (_tmp, home) = setup();
        sync_rules(&home, false).unwrap();

        // Simulate a rule basename that existed in a previous version of
        // RULE_DEFS but was since renamed/removed from the code.
        let rules_dir = cursor_rules_dir(&home);
        let stale = rules_dir.join("old-rule.mdc");
        fs::write(&stale, "stale content").unwrap();

        let manifest_path = rules_dir.join(".manifest.json");
        let mut manifest: std::collections::HashSet<String> =
            serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
        manifest.insert("old-rule.mdc".to_string());
        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        sync_rules(&home, false).unwrap();

        assert!(!stale.exists(), "orphaned rule file should be removed");
        assert!(
            rules_dir.join("git-workflow.mdc").exists(),
            "current rules should be untouched"
        );
    }

    #[test]
    fn test_always_apply_globs_invariant() {
        for rule in RULE_DEFS {
            assert_eq!(
                rule.always_apply,
                rule.globs.is_none(),
                "rule '{}' breaks the always_apply/globs invariant: \
                 format_claude_md() derives Claude's always-apply behavior solely \
                 from globs.is_none(), so always_apply must stay in lockstep with it",
                rule.basename
            );
        }
    }

    #[test]
    fn test_multi_section_rule_cursor() {
        let (_tmp, home) = setup();
        sync_rules(&home, false).unwrap();

        let nodejs_rule = cursor_rules_dir(&home).join("nodejs-stack.mdc");
        let content = fs::read_to_string(&nodejs_rule).unwrap();
        assert!(content.contains("## 3. Node.js Stack"));
        assert!(content.contains("## 4. Node Version"));
    }

    #[test]
    fn test_multi_section_rule_claude() {
        let (_tmp, home) = setup();
        sync_rules(&home, false).unwrap();

        let nodejs_rule = claude_rules_dir(&home).join("nodejs-stack.md");
        let content = fs::read_to_string(&nodejs_rule).unwrap();
        assert!(content.contains("## 3. Node.js Stack"));
        assert!(content.contains("## 4. Node Version"));
    }

    #[test]
    fn test_cursor_frontmatter_is_valid_yaml_with_colon_in_description() {
        let (_tmp, home) = setup();
        sync_rules(&home, false).unwrap();

        // tdd-quality's description contains "Assurance: Red-Green-Refactor",
        // an unquoted ": " inside a plain YAML scalar — previously invalid.
        let tdd_rule = cursor_rules_dir(&home).join("tdd-quality.mdc");
        let content = fs::read_to_string(&tdd_rule).unwrap();
        let frontmatter = content
            .strip_prefix("---\n")
            .and_then(|s| s.split_once("\n---\n"))
            .map(|(fm, _)| fm)
            .expect("frontmatter delimiters");

        let parsed: serde_yaml::Value =
            serde_yaml::from_str(frontmatter).expect("frontmatter must be valid YAML");
        assert_eq!(
            parsed["description"].as_str().unwrap(),
            "TDD & Quality Assurance: Red-Green-Refactor, 90% branch coverage, vitest+stryker, Playwright."
        );
    }

    #[test]
    fn test_cursor_globs_in_frontmatter() {
        let (_tmp, home) = setup();
        sync_rules(&home, false).unwrap();

        let python_rule = cursor_rules_dir(&home).join("python-stack.mdc");
        let content = fs::read_to_string(&python_rule).unwrap();
        assert!(content.contains("globs: \"**/*.py\""));
        assert!(content.contains("alwaysApply: false"));
    }

    #[test]
    fn test_claude_paths_in_frontmatter() {
        let (_tmp, home) = setup();
        sync_rules(&home, false).unwrap();

        let python_rule = claude_rules_dir(&home).join("python-stack.md");
        let content = fs::read_to_string(&python_rule).unwrap();
        assert!(content.contains("paths:"));
        assert!(content.contains("\"**/*.py\""));
    }

    #[test]
    fn test_claude_always_apply_has_no_frontmatter() {
        let (_tmp, home) = setup();
        sync_rules(&home, false).unwrap();

        let git_rule = claude_rules_dir(&home).join("git-workflow.md");
        let content = fs::read_to_string(&git_rule).unwrap();
        let fm_end = content.find("---\n\n").unwrap();
        let frontmatter = &content[..fm_end];
        assert!(
            !frontmatter.contains("paths"),
            "always-apply rules should have empty frontmatter (no paths)"
        );
        assert!(!frontmatter.contains("alwaysApply"));
        assert!(!frontmatter.contains("globs"));
    }

    #[test]
    fn test_claude_multi_pattern_globs() {
        let (_tmp, home) = setup();
        sync_rules(&home, false).unwrap();

        let test_rule = claude_rules_dir(&home).join("semantic-test-review.md");
        let content = fs::read_to_string(&test_rule).unwrap();
        assert!(content.contains("paths:"));
        assert!(content.contains("\"**/*.test.{ts,js,py}\""));
        assert!(content.contains("\"**/*.spec.{ts,js}\""));
    }

    #[test]
    fn test_missing_canonical() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let result = sync_rules(&home, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_extract_section() {
        let content = "## 1. First\ncontent A\n\n## 2. Second\ncontent B\n";
        let section = extract_section(content, 1).unwrap();
        assert!(section.contains("First"));
        assert!(section.contains("content A"));
        assert!(!section.contains("Second"));

        let section2 = extract_section(content, 2).unwrap();
        assert!(section2.contains("Second"));
        assert!(section2.contains("content B"));
    }

    #[test]
    fn test_extract_section_ignores_headings_inside_code_blocks() {
        let content = "## 1. First\n\
                        ```bash\n\
                        ## this looks like a heading but is inside a fence\n\
                        echo hi\n\
                        ```\n\
                        content A\n\
                        \n\
                        ## 2. Second\n\
                        content B\n";

        let section = extract_section(content, 1).unwrap();
        assert!(section.contains("this looks like a heading but is inside a fence"));
        assert!(section.contains("content A"));
        assert!(!section.contains("Second"));
    }
}
