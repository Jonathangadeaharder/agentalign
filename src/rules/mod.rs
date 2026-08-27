//! Path-scoped rule generation from the canonical AGENTS.md.
//!
//! One concept, three native formats. Every `## ` section of AGENTS.md becomes a
//! rule; the `SCOPES` table decides which sections are path-scoped:
//! - Cursor: `.mdc` in `~/.cursor/rules/` (frontmatter: `globs`, `alwaysApply`)
//! - Claude: `.md` in `~/.claude/rules/` (frontmatter: `paths` as a YAML list)
//! - Copilot: `.instructions.md` in VS Code's user prompts dir (`applyTo`)
//!
//! A target that already receives the whole AGENTS.md through the instructions
//! module gets only the scoped rules, so the unscoped sections are not loaded
//! twice. Cursor has no user-level instruction file and gets the full set.
//!
//! Called by:
//! - `agentalign sync` (after MCP + instruction + skills + agent sync)
//! - `agentalign watch` (on daemon startup and on AGENTS.md changes)

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Path scopes for the sections that apply to specific file types.
///
/// Keys are heading slugs. A section not listed here is always-apply. A key that
/// matches no section warns, so renaming a heading cannot silently unscope it.
const SCOPES: &[(&str, &str)] = &[
    (
        "cpp-header-and-precompiled-header-policy",
        "**/*.{h,hpp,hxx,inl,cpp,cxx,cc}",
    ),
    (
        "build-invocation-msbuild",
        "**/*.{sln,vcxproj,props,targets}",
    ),
    ("powershell-style", "**/*.ps1"),
    ("debugging-turbomed-dumps", "**/*.dmp"),
];

/// A rule derived from one AGENTS.md section.
struct Rule {
    /// Filename stem, the slugified heading.
    basename: String,
    /// Heading text, used as the rule description.
    title: String,
    /// Path scope, or `None` for an always-apply rule.
    globs: Option<&'static str>,
    /// The section text, heading included.
    body: String,
}

/// Turn a heading into a filename stem.
fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;

    for ch in title.to_lowercase().replace("c++", "cpp").chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(ch);
        } else {
            pending_dash = true;
        }
    }

    slug
}

/// Split AGENTS.md into its `## ` sections, heading text paired with body text.
///
/// Lines inside a fenced code block are never treated as headings, so a code
/// sample containing a `## `-prefixed comment does not start a section.
fn split_sections(content: &str) -> Vec<(String, String)> {
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut in_code_block = false;

    for line in content.lines() {
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
        } else if !in_code_block {
            if let Some(title) = line.strip_prefix("## ") {
                sections.push((title.trim().to_string(), String::new()));
                continue;
            }
        }

        if let Some((_, body)) = sections.last_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }

    sections
}

/// Derive the rule set from AGENTS.md, warning on `SCOPES` keys that match nothing.
fn derive_rules(content: &str) -> Vec<Rule> {
    let rules: Vec<Rule> = split_sections(content)
        .into_iter()
        .filter(|(_, body)| !body.trim().is_empty())
        .map(|(title, body)| {
            let basename = slugify(&title);
            let globs = SCOPES
                .iter()
                .find(|(slug, _)| *slug == basename)
                .map(|(_, globs)| *globs);
            Rule {
                body: format!("## {}\n{}", title, body),
                basename,
                title,
                globs,
            }
        })
        .collect();

    for (slug, _) in SCOPES {
        if !rules.iter().any(|rule| rule.basename == *slug) {
            eprintln!(
                "  warning: scope '{}' matches no AGENTS.md heading — it is now always-apply",
                slug
            );
        }
    }

    rules
}

/// Format a Cursor `.mdc` file.
fn format_cursor_mdc(rule: &Rule) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("description: {}\n", yaml_quote(&rule.title)));

    if let Some(globs) = rule.globs {
        out.push_str(&format!("globs: {}\n", yaml_quote(globs)));
    }

    out.push_str(&format!("alwaysApply: {}\n", rule.globs.is_none()));
    out.push_str("---\n\n");
    out.push_str(&rule.body);
    out
}

/// Format a Claude Code `.md` rule file.
///
/// Claude uses `paths` (a YAML list) instead of Cursor's single `globs` string,
/// and has no `alwaysApply`: a rule without `paths` loads unconditionally.
fn format_claude_md(rule: &Rule) -> String {
    let mut out = String::from("---\n");

    if let Some(globs) = rule.globs {
        out.push_str("paths:\n");
        for pattern in split_glob_patterns(globs) {
            out.push_str(&format!("  - {}\n", yaml_quote(pattern)));
        }
    }

    out.push_str("---\n\n");
    out.push_str(&rule.body);
    out
}

/// Format a Copilot `.instructions.md` file.
///
/// Copilot's `applyTo` is one comma-separated glob string, and `**` means the
/// rule applies everywhere.
fn format_copilot_instructions(rule: &Rule) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("description: {}\n", yaml_quote(&rule.title)));
    out.push_str(&format!("applyTo: {}\n", yaml_quote(rule.globs.unwrap_or("**"))));
    out.push_str("---\n\n");
    out.push_str(&rule.body);
    out
}

/// Wrap a value in a double-quoted YAML scalar, escaping backslashes and
/// double quotes. A plain scalar breaks when the value contains `: `, which
/// reads as a nested mapping to most YAML parsers.
fn yaml_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
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

/// One rules destination: where to write, in which format, and how much.
struct RuleTarget {
    label: &'static str,
    dir: PathBuf,
    extension: &'static str,
    formatter: fn(&Rule) -> String,
    /// Whether unscoped sections are written here. False where the instructions
    /// module already delivers the whole AGENTS.md.
    emit_always_apply: bool,
}

/// Write a target's rules. Returns the number of files written or removed.
fn write_rules(rules: &[Rule], target: &RuleTarget, dry_run: bool) -> anyhow::Result<usize> {
    let selected: Vec<&Rule> = rules
        .iter()
        .filter(|rule| target.emit_always_apply || rule.globs.is_some())
        .collect();

    if dry_run {
        println!(
            "  [DRY RUN] {} rules -> {} ({} rules)",
            target.label,
            target.dir.display(),
            selected.len()
        );
        return Ok(selected.len());
    }

    std::fs::create_dir_all(&target.dir)?;
    let mut written = 0usize;
    let mut current_files: HashSet<String> = HashSet::new();

    for rule in selected {
        let filename = format!("{}.{}", rule.basename, target.extension);
        current_files.insert(filename.clone());

        let file_content = (target.formatter)(rule);
        let path = target.dir.join(&filename);

        if let Ok(existing) = std::fs::read_to_string(&path) {
            if existing == file_content {
                continue;
            }
        }

        std::fs::write(&path, &file_content)?;
        written += 1;
        println!("  {} rule {} -> {}", target.label, filename, path.display());
    }

    written += remove_orphaned_rules(&target.dir, &current_files, target.label)?;

    Ok(written)
}

/// Manifest filename tracking which rule files agentalign generated in a rules dir.
const MANIFEST_FILENAME: &str = ".manifest.json";

/// Remove rule files that a previous sync generated but this one no longer
/// produces. Returns the number removed.
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

/// Get the target rules directory for Copilot.
pub fn copilot_rules_dir(home: &Path) -> PathBuf {
    crate::shared::paths::vscode_prompts_dir(home)
}

/// Build the rules destinations.
fn targets(home: &Path) -> Vec<RuleTarget> {
    vec![
        // Cursor has no user-level instruction file, so it needs every section.
        RuleTarget {
            label: "cursor",
            dir: cursor_rules_dir(home),
            extension: "mdc",
            formatter: format_cursor_mdc,
            emit_always_apply: true,
        },
        RuleTarget {
            label: "claude",
            dir: claude_rules_dir(home),
            extension: "md",
            formatter: format_claude_md,
            emit_always_apply: false,
        },
        RuleTarget {
            label: "copilot",
            dir: copilot_rules_dir(home),
            extension: "instructions.md",
            formatter: format_copilot_instructions,
            emit_always_apply: false,
        },
    ]
}

/// Sync AGENTS.md sections into every target's native rule format.
/// Returns the total number of rule files written across all targets.
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
    let rules = derive_rules(&content);
    let mut total_written = 0usize;

    for target in targets(home) {
        total_written += write_rules(&rules, &target, dry_run)?;
    }

    Ok(total_written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const AGENTS_MD: &str = "\
# Global working rules

## PowerShell style

Short scripts only.

## Git worktrees

At most two checkouts.

## Comment and docstring style

Three lines, optimally one.
";

    fn setup() -> (TempDir, std::path::PathBuf) {
        let tmp = TempDir::new().expect("tempdir");
        let home = tmp.path().join("home");
        let agents_dir = home.join(".agents");
        fs::create_dir_all(&agents_dir).expect("create .agents");
        fs::write(agents_dir.join("AGENTS.md"), AGENTS_MD).expect("write AGENTS.md");

        (tmp, home)
    }

    #[test]
    fn test_slugify_keeps_cpp_distinguishable_from_c() {
        assert_eq!(slugify("C++ header policy"), "cpp-header-policy");
        assert_eq!(
            slugify("MR / PR hygiene (hard rules)"),
            "mr-pr-hygiene-hard-rules"
        );
        assert_eq!(slugify("German keyboard: Ctrl+Alt"), "german-keyboard-ctrl-alt");
    }

    #[test]
    fn test_every_section_becomes_a_cursor_rule() {
        let (_tmp, home) = setup();
        sync_rules(&home, false).expect("sync");

        let dir = cursor_rules_dir(&home);
        assert!(dir.join("powershell-style.mdc").exists());
        assert!(dir.join("git-worktrees.mdc").exists());
        assert!(dir.join("comment-and-docstring-style.mdc").exists());
    }

    #[test]
    fn test_claude_receives_only_the_scoped_sections() {
        let (_tmp, home) = setup();
        sync_rules(&home, false).expect("sync");

        let dir = claude_rules_dir(&home);
        assert!(dir.join("powershell-style.md").exists());
        assert!(
            !dir.join("git-worktrees.md").exists(),
            "an always-apply section arrives through the symlinked CLAUDE.md"
        );
    }

    #[test]
    fn test_copilot_scoped_rule_carries_apply_to() {
        let (_tmp, home) = setup();
        sync_rules(&home, false).expect("sync");

        let rule = copilot_rules_dir(&home).join("powershell-style.instructions.md");
        let content = fs::read_to_string(&rule).expect("read rule");
        assert!(content.contains("applyTo: \"**/*.ps1\""));
        assert!(content.contains("Short scripts only."));
    }

    #[test]
    fn test_scoped_section_carries_its_globs_in_every_format() {
        let (_tmp, home) = setup();
        sync_rules(&home, false).expect("sync");

        let cursor = fs::read_to_string(cursor_rules_dir(&home).join("powershell-style.mdc"))
            .expect("cursor rule");
        assert!(cursor.contains("globs: \"**/*.ps1\""));
        assert!(cursor.contains("alwaysApply: false"));

        let claude = fs::read_to_string(claude_rules_dir(&home).join("powershell-style.md"))
            .expect("claude rule");
        assert!(claude.contains("paths:\n  - \"**/*.ps1\"\n"));
    }

    #[test]
    fn test_cursor_always_apply_rule_omits_globs() {
        let (_tmp, home) = setup();
        sync_rules(&home, false).expect("sync");

        let content = fs::read_to_string(cursor_rules_dir(&home).join("git-worktrees.mdc"))
            .expect("cursor rule");
        assert!(content.contains("alwaysApply: true"));
        assert!(!content.contains("globs:"));
    }

    #[test]
    fn test_frontmatter_is_valid_yaml_when_the_heading_contains_a_colon() {
        let rule = Rule {
            basename: "german-keyboard".into(),
            title: "German keyboard: Ctrl+Alt is AltGr".into(),
            globs: None,
            body: "## German keyboard: Ctrl+Alt is AltGr\n".into(),
        };

        let frontmatter = format_cursor_mdc(&rule)
            .strip_prefix("---\n")
            .and_then(|s| s.split_once("\n---\n"))
            .map(|(fm, _)| fm.to_string())
            .expect("frontmatter delimiters");

        let parsed: serde_yaml::Value =
            serde_yaml::from_str(&frontmatter).expect("frontmatter must be valid YAML");
        assert_eq!(
            parsed["description"].as_str().expect("description"),
            "German keyboard: Ctrl+Alt is AltGr"
        );
    }

    #[test]
    fn test_sync_rules_is_idempotent() {
        let (_tmp, home) = setup();
        assert!(sync_rules(&home, false).expect("first sync") > 0);

        let second = sync_rules(&home, false).expect("second sync");

        assert_eq!(second, 0, "second sync should write nothing");
    }

    #[test]
    fn test_dry_run_writes_nothing() {
        let (_tmp, home) = setup();

        let count = sync_rules(&home, true).expect("dry run");

        assert!(count > 0);
        assert!(!cursor_rules_dir(&home).exists());
        assert!(!claude_rules_dir(&home).exists());
    }

    #[test]
    fn test_removes_orphaned_rule_file() {
        let (_tmp, home) = setup();
        sync_rules(&home, false).expect("sync");

        let rules_dir = cursor_rules_dir(&home);
        let stale = rules_dir.join("old-rule.mdc");
        fs::write(&stale, "stale content").expect("write stale");

        let manifest_path = rules_dir.join(MANIFEST_FILENAME);
        let mut manifest: HashSet<String> =
            serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest"))
                .expect("parse manifest");
        manifest.insert("old-rule.mdc".to_string());
        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).expect("serialize manifest"),
        )
        .expect("write manifest");

        sync_rules(&home, false).expect("resync");

        assert!(!stale.exists(), "orphaned rule file should be removed");
        assert!(rules_dir.join("git-worktrees.mdc").exists());
    }

    #[test]
    fn test_missing_canonical_is_not_an_error() {
        let tmp = TempDir::new().expect("tempdir");

        let count = sync_rules(&tmp.path().join("home"), false).expect("sync");

        assert_eq!(count, 0);
    }

    #[test]
    fn test_headings_inside_a_fence_do_not_start_a_section() {
        let content = "## Real heading\n\
                       ```bash\n\
                       ## not a heading\n\
                       ```\n\
                       body\n";

        let sections = split_sections(content);

        assert_eq!(sections.len(), 1);
        assert!(sections[0].1.contains("## not a heading"));
    }
}
