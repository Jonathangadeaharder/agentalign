//! Skills directory syncing module.
//!
//! Maintains `~/.agents/skills/` as the canonical source of truth for agent
//! skill directories. Each skill subdirectory (e.g., `~/.agents/skills/diagnose/`)
//! is symlinked into per-agent skills directories:
//! - `~/.claude/skills/diagnose` → `~/.agents/skills/diagnose`
//! - `~/.gemini/config/skills/diagnose` → `~/.agents/skills/diagnose`
//! - `~/.codex/skills/diagnose` → `~/.agents/skills/diagnose`
//! - `~/.qwen/skills/diagnose` → `~/.agents/skills/diagnose`
//!
//! Real directories (pre-agentalign) are backed up and replaced with symlinks.
//! Called by:
//! - `agentalign sync` (after MCP + instruction sync)
//! - `agentalign watch` (on daemon startup and on skills dir changes)

use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::Utc;

const GENERATED_COMMAND_MARKER: &str = "<!-- agentalign: skill command -->";

/// Create a directory symlink at `link` pointing to `canonical`.
///
/// Windows needs `symlink_dir` plus Developer Mode or elevation.
fn create_skill_symlink(canonical: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(canonical, link)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(canonical, link)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (canonical, link);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "symlinks are not supported on this platform",
        ))
    }
}

/// Link `link` to `canonical`, falling back to a junction. Returns the mechanism used.
///
/// Windows refuses directory symlinks without Developer Mode or elevation, but a
/// junction to a directory on the same volume needs neither and resolves the same.
fn create_skill_link(canonical: &Path, link: &Path) -> std::io::Result<&'static str> {
    match create_skill_symlink(canonical, link) {
        Ok(()) => Ok("symlink"),
        Err(symlink_err) => {
            #[cfg(windows)]
            {
                junction::create(canonical, link)
                    .map(|()| "junction")
                    .map_err(|_| symlink_err)
            }
            #[cfg(not(windows))]
            {
                Err(symlink_err)
            }
        }
    }
}

/// Remove a symlink that points at a directory.
///
/// Windows directory symlinks are removed with `remove_dir`, not `remove_file`.
fn remove_symlink(link: &Path) -> std::io::Result<()> {
    if link.is_dir() {
        std::fs::remove_dir(link)
    } else {
        std::fs::remove_file(link)
    }
}

fn prune_dangling(skills_dir: &Path, canonical_dir: &Path) -> std::io::Result<usize> {
    let mut pruned = 0;
    for entry in std::fs::read_dir(skills_dir)? {
        let path = entry?.path();
        let Ok(target) = std::fs::read_link(&path) else {
            continue;
        };
        let resolved = if target.is_absolute() {
            target
        } else {
            skills_dir.join(target)
        };
        if resolved.starts_with(canonical_dir) && !resolved.exists() {
            #[cfg(windows)]
            std::fs::remove_dir(&path)?;
            #[cfg(not(windows))]
            std::fs::remove_file(&path)?;
            pruned += 1;
        }
    }
    Ok(pruned)
}

/// Sibling path used to park a real directory while its symlink is created.
fn parked_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".agentalign-old");
    path.with_file_name(name)
}

/// Get the canonical skills directory path.
pub fn canonical_skills_dir(home: &Path) -> PathBuf {
    home.join(".agents").join("skills")
}

/// Per-agent skills directory descriptor.
pub struct SkillsEntry {
    /// Human-readable agent name.
    pub agent: &'static str,
    /// Path to the agent's skills directory.
    pub skills_dir: PathBuf,
}

/// Build the registry of per-agent skills directories.
fn registry(home: &Path) -> Vec<SkillsEntry> {
    vec![
        SkillsEntry {
            agent: "claude",
            skills_dir: home.join(".claude").join("skills"),
        },
        // Antigravity CLI discovers skills by recursively scanning
        // ~/.gemini/config/ for skills/*/SKILL.md (same as plugins).
        // Using ~/.gemini/config/skills/ so agy finds the canonical skills.
        SkillsEntry {
            agent: "gemini",
            skills_dir: home.join(".gemini").join("config").join("skills"),
        },
        SkillsEntry {
            agent: "codex",
            skills_dir: home.join(".codex").join("skills"),
        },
        SkillsEntry {
            agent: "cursor",
            skills_dir: home.join(".cursor").join("skills"),
        },
        // OpenCode does NOT read ~/.agents/skills natively — it needs
        // symlinks in its own skills directory.
        SkillsEntry {
            agent: "opencode",
            skills_dir: home.join(".config").join("opencode").join("skills"),
        },
        // Grok CLI has its own skills dir at ~/.grok/skills/.
        SkillsEntry {
            agent: "grok",
            skills_dir: home.join(".grok").join("skills"),
        },
        // Qwen Code (Gemini CLI fork) reads skills from ~/.qwen/skills/.
        SkillsEntry {
            agent: "qwen",
            skills_dir: home.join(".qwen").join("skills"),
        },
        // ZCode reads ~/.agents/skills natively (per the zcode-guide
        // discovery order), so it is intentionally omitted here — no
        // symlinks needed. AGENTS.md instructions are also read from
        // ~/.zcode/AGENTS.md natively.
    ]
}

/// Symlink state for a single skill in a single agent.
#[derive(Debug, Clone, PartialEq)]
pub enum SkillState {
    /// Symlink points to correct canonical skill.
    Ok,
    /// Skill does not exist in this agent's skills dir.
    Missing,
    /// Symlink exists but points to wrong target.
    WrongTarget { current_target: PathBuf },
    /// Real directory exists instead of symlink.
    ReplacedByDir,
}

/// Check the state of a single skill symlink.
pub fn verify_skill(
    agent_skills_dir: &Path,
    skill_name: &str,
    canonical_skill: &Path,
) -> SkillState {
    let path = agent_skills_dir.join(skill_name);

    // Check if it's a symlink
    match std::fs::read_link(&path) {
        Ok(target) => {
            let resolved = if target.is_relative() {
                if let Some(parent) = path.parent() {
                    parent.join(&target)
                } else {
                    target
                }
            } else {
                target
            };

            if resolved == canonical_skill {
                SkillState::Ok
            } else {
                SkillState::WrongTarget {
                    current_target: resolved,
                }
            }
        }
        Err(_) => {
            if path.is_dir() {
                SkillState::ReplacedByDir
            } else if path.exists() {
                // Exists as file, not dir — treat as replaced
                SkillState::ReplacedByDir
            } else {
                // Check if it's a broken symlink
                match std::fs::symlink_metadata(&path) {
                    Ok(_) => SkillState::WrongTarget {
                        current_target: PathBuf::from("(broken)"),
                    },
                    Err(_) => SkillState::Missing,
                }
            }
        }
    }
}

/// Heal a single skill symlink. Returns true if a change was made.
pub fn heal_skill(
    agent_skills_dir: &Path,
    skill_name: &str,
    canonical_skill: &Path,
    agent_name: &str,
    backup_dir: &Path,
) -> anyhow::Result<bool> {
    let state = verify_skill(agent_skills_dir, skill_name, canonical_skill);

    let skill_path = agent_skills_dir.join(skill_name);

    match state {
        SkillState::Ok => return Ok(false),
        SkillState::Missing => {
            // Ensure parent directory exists
            std::fs::create_dir_all(agent_skills_dir)?;
        }
        SkillState::WrongTarget { .. } => {
            remove_symlink(&skill_path)?;
        }
        SkillState::ReplacedByDir => {
            // Backup the real directory/file before replacing with symlink
            std::fs::create_dir_all(backup_dir)?;

            let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
            let backup_name = format!("{}-skill-{}-{}.bak", agent_name, skill_name, timestamp);
            let backup_path = backup_dir.join(&backup_name);

            if skill_path.is_dir() {
                // Copy directory contents for backup, then remove original
                copy_dir_recursive(&skill_path, &backup_path)?;
                std::fs::remove_dir_all(&skill_path)?;
            } else {
                // It's a regular file
                let content = std::fs::read(&skill_path)?;
                std::fs::write(&backup_path, &content)?;
                std::fs::remove_file(&skill_path)?;
            }

            eprintln!(
                "  backed up {}/{} -> {}",
                agent_name,
                skill_name,
                backup_path.display()
            );
        }
    }

    let mechanism = create_skill_link(canonical_skill, &skill_path).with_context(|| {
        format!(
            "failed to link {} -> {}{}",
            skill_path.display(),
            canonical_skill.display(),
            crate::instructions::symlink_hint()
        )
    })?;

    eprintln!(
        "  {} skill {} -> {} ({})",
        agent_name,
        skill_name,
        canonical_skill.display(),
        mechanism
    );

    Ok(true)
}

/// Heal all skills for all agents. Returns the number of changes made.
pub fn heal_all(home: &Path) -> anyhow::Result<usize> {
    let canonical_dir = canonical_skills_dir(home);
    if let Err(e) = collect_opencode_commands_as_skills(home) {
        eprintln!("  warning: failed to import opencode commands: {}", e);
    }

    if !canonical_dir.exists() {
        eprintln!(
            "  warning: canonical skills dir not found at {} — skipping skills sync",
            canonical_dir.display()
        );
        return Ok(0);
    }

    // Discover canonical skills
    let canonical_skills: Vec<String> = std::fs::read_dir(&canonical_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            e.file_name()
                .into_string()
                .ok()
                .filter(|n| !n.starts_with('.'))
        })
        .collect();

    let commands = sync_opencode_skill_commands(home, &canonical_skills)?;

    let entries = registry(home);
    let backup_dir = home.join(".agents").join("backups");
    let mut fixed = commands;

    for entry in &entries {
        // Ensure agent skills dir exists
        std::fs::create_dir_all(&entry.skills_dir)?;
        fixed += prune_dangling(&entry.skills_dir, &canonical_dir)?;

        for skill_name in &canonical_skills {
            let canonical_skill_path = canonical_dir.join(skill_name);
            match heal_skill(
                &entry.skills_dir,
                skill_name,
                &canonical_skill_path,
                entry.agent,
                &backup_dir,
            ) {
                Ok(changed) => {
                    if changed {
                        fixed += 1;
                    }
                }
                Err(e) => {
                    eprintln!("  error healing {}/{}: {}", entry.agent, skill_name, e);
                }
            }
        }
    }

    Ok(fixed)
}

fn sync_opencode_skill_commands(home: &Path, canonical_skills: &[String]) -> anyhow::Result<usize> {
    let opencode = home.join(".config").join("opencode");
    if !opencode.is_dir() {
        return Ok(0);
    }
    let commands_dir = opencode.join("command");
    let legacy_dir = opencode.join("commands");
    let configured: std::collections::HashSet<String> =
        match std::fs::read_to_string(opencode.join("opencode.json")) {
            Ok(raw) => serde_json::from_str::<serde_json::Value>(&raw)?
                .get("command")
                .and_then(|commands| commands.as_object())
                .map(|commands| commands.keys().cloned().collect())
                .unwrap_or_default(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Default::default(),
            Err(e) => return Err(e.into()),
        };
    let mut changed = 0;

    for skill_name in canonical_skills {
        if configured.contains(skill_name)
            || ["init", "undo", "redo", "share", "help"].contains(&skill_name.as_str())
        {
            continue;
        }
        if ["command", "commands"].iter().any(|dir| {
            let path = opencode.join(dir).join(format!("{skill_name}.md"));
            path.is_file()
                && std::fs::read_to_string(path)
                    .is_ok_and(|content| !content.contains(GENERATED_COMMAND_MARKER))
        }) {
            continue;
        }
        let skill_file = canonical_skills_dir(home).join(skill_name).join("SKILL.md");
        let Ok(skill) = std::fs::read_to_string(skill_file) else {
            continue;
        };
        let Some(frontmatter) = skill
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---"))
        else {
            continue;
        };
        let Ok(metadata) = serde_yaml::from_str::<serde_yaml::Value>(frontmatter.0) else {
            continue;
        };
        let Some(description) = metadata.get("description").and_then(|value| value.as_str()) else {
            continue;
        };
        let command_path = commands_dir.join(format!("{skill_name}.md"));
        let existing = match std::fs::read_to_string(&command_path) {
            Ok(content) => Some(content),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if existing
            .as_ref()
            .is_some_and(|text| !text.contains(GENERATED_COMMAND_MARKER))
        {
            continue;
        }
        let content = format!(
            "---\ndescription: {}\n---\n\n{}\nLoad the `{}` skill and follow its instructions. User request: $ARGUMENTS\n",
            serde_json::to_string(description)?, GENERATED_COMMAND_MARKER, skill_name
        );
        if existing.as_deref() != Some(&content) {
            std::fs::create_dir_all(&commands_dir)?;
            std::fs::write(command_path, content)?;
            changed += 1;
        }
    }

    if commands_dir.is_dir() {
        for entry in std::fs::read_dir(&commands_dir)? {
            let entry = entry?;
            let path = entry.path();
            let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if path.is_file()
                && path.extension().is_some_and(|extension| extension == "md")
                && (!canonical_skills.iter().any(|skill| skill == name)
                    || configured.contains(name))
                && std::fs::read_to_string(&path)?.contains(GENERATED_COMMAND_MARKER)
            {
                std::fs::remove_file(path)?;
                changed += 1;
            }
        }
    }
    if legacy_dir.is_dir() {
        for entry in std::fs::read_dir(&legacy_dir)? {
            let path = entry?.path();
            if path.is_file()
                && path.extension().is_some_and(|extension| extension == "md")
                && std::fs::read_to_string(&path)?.contains(GENERATED_COMMAND_MARKER)
            {
                std::fs::remove_file(path)?;
                changed += 1;
            }
        }
    }
    Ok(changed)
}

/// Import top-level OpenCode command markdown files into the canonical skills store.
///
/// OpenCode commands live as `~/.config/opencode/commands/<name>.md`, while
/// Codex and the other agents consume skill directories with `SKILL.md`.
/// Creating the canonical skill lets the existing symlink sync distribute the
/// command everywhere without teaching every agent about OpenCode's command path.
pub fn collect_opencode_commands_as_skills(home: &Path) -> anyhow::Result<usize> {
    let commands_dir = home.join(".config").join("opencode").join("commands");

    if !commands_dir.exists() {
        return Ok(0);
    }

    let canonical_dir = canonical_skills_dir(home);
    let mut collected = 0usize;

    for entry in std::fs::read_dir(&commands_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        if std::fs::read_to_string(&path)?.contains(GENERATED_COMMAND_MARKER) {
            continue;
        }

        let skill_name = match path.file_stem().and_then(|s| s.to_str()) {
            Some(name) if !name.starts_with('.') && !name.is_empty() => name,
            _ => continue,
        };

        let skill_dir = canonical_dir.join(skill_name);
        let skill_file = skill_dir.join("SKILL.md");
        if skill_file.exists() {
            continue;
        }

        match import_opencode_command_as_skill(&path, &skill_dir, &skill_file, skill_name) {
            Ok(()) => {
                collected += 1;
                eprintln!(
                    "  collected opencode command {} -> canonical skill",
                    skill_name
                );
            }
            Err(e) => eprintln!("  error importing opencode command {}: {}", skill_name, e),
        }
    }

    Ok(collected)
}

fn import_opencode_command_as_skill(
    command_path: &Path,
    skill_dir: &Path,
    skill_file: &Path,
    skill_name: &str,
) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(command_path)?;
    std::fs::create_dir_all(skill_dir)?;
    std::fs::write(skill_file, as_skill_markdown(skill_name, &content))?;
    Ok(())
}

fn as_skill_markdown(skill_name: &str, content: &str) -> String {
    if let Some(rest) = content.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let frontmatter = &rest[..end];
            if frontmatter.lines().any(|line| line.starts_with("name:")) {
                return content.to_string();
            }

            let body = &rest[end..];
            return format!("---\nname: {}\n{}{}", skill_name, frontmatter, body);
        }
    }

    format!(
        "---\nname: {}\ndescription: Imported OpenCode command\n---\n\n{}",
        skill_name, content
    )
}

/// Heal skills for a single agent by name.
pub fn heal_one(home: &Path, agent: &str) -> anyhow::Result<usize> {
    let canonical_dir = canonical_skills_dir(home);

    if !canonical_dir.exists() {
        eprintln!(
            "  warning: canonical skills dir not found at {} — skipping",
            canonical_dir.display()
        );
        return Ok(0);
    }

    let entries = registry(home);
    let entry = entries
        .iter()
        .find(|e| e.agent == agent)
        .ok_or_else(|| anyhow::anyhow!("Unknown agent for skills: {}", agent))?;

    let canonical_skills: Vec<String> = std::fs::read_dir(&canonical_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();

    let backup_dir = home.join(".agents").join("backups");
    let mut fixed = 0usize;

    std::fs::create_dir_all(&entry.skills_dir)?;
    fixed += prune_dangling(&entry.skills_dir, &canonical_dir)?;

    for skill_name in &canonical_skills {
        let canonical_skill_path = canonical_dir.join(skill_name);
        if heal_skill(
            &entry.skills_dir,
            skill_name,
            &canonical_skill_path,
            entry.agent,
            &backup_dir,
        )? {
            fixed += 1;
        }
    }

    Ok(fixed)
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Collect orphan skills from all tool directories into the canonical store.
///
/// Scans every agent's skills directory for **real directories** (not symlinks)
/// that don't exist in the canonical store (`~/.agents/skills/`). Moves each
/// orphan to the canonical store, then replaces the original with a symlink.
/// This is the "reverse sync" — `heal_all` pushes canonical → tools, this
/// collects tools → canonical.
///
/// Called by `agentalign migrate`.
pub fn collect_orphan_skills(home: &Path) -> anyhow::Result<usize> {
    let canonical_dir = canonical_skills_dir(home);
    std::fs::create_dir_all(&canonical_dir)?;

    // Build set of skills already in canonical store
    let canonical_names: std::collections::HashSet<String> = std::fs::read_dir(&canonical_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| !n.starts_with('.'))
                .collect()
        })
        .unwrap_or_default();

    let entries = registry(home);
    let mut collected = 0usize;

    for entry in &entries {
        if !entry.skills_dir.exists() {
            continue;
        }

        for dir_entry in std::fs::read_dir(&entry.skills_dir)? {
            let dir_entry = dir_entry?;
            let path = dir_entry.path();

            // Skip files (only collect directories)
            if !path.is_dir() {
                continue;
            }

            // Skip symlinks (already linked to canonical)
            if std::fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
            {
                continue;
            }

            let name = match dir_entry.file_name().into_string() {
                Ok(n) if !n.starts_with('.') => n,
                _ => continue,
            };

            // Skip if already in canonical store
            if canonical_names.contains(&name) {
                continue;
            }

            // Copy into the canonical store, then park the original next to
            // itself so a failed symlink can be undone instead of losing it.
            let canonical_path = canonical_dir.join(&name);
            copy_dir_recursive(&path, &canonical_path)?;

            let parked = parked_path(&path);
            std::fs::rename(&path, &parked)?;
            if let Err(e) = create_skill_link(&canonical_path, &path) {
                std::fs::rename(&parked, &path)?;
                return Err(anyhow::Error::new(e).context(format!(
                    "failed to link {} -> {}{}",
                    path.display(),
                    canonical_path.display(),
                    crate::instructions::symlink_hint()
                )));
            }
            std::fs::remove_dir_all(&parked)?;

            collected += 1;
            eprintln!(
                "  collected skill {} from {} -> canonical",
                name, entry.agent
            );
        }
    }

    // Now heal all symlinks so the moved skills are linked back everywhere
    if collected > 0 {
        heal_all(home)?;
    }

    Ok(collected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let canonical = canonical_skills_dir(&home);
        fs::create_dir_all(&canonical).unwrap();
        (tmp, home)
    }

    #[test]
    fn test_heal_all_empty_canonical() {
        let (_tmp, home) = setup();
        let fixed = heal_all(&home).unwrap();
        assert_eq!(fixed, 0);
    }

    #[test]
    fn test_heal_all_missing_canonical() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("nonexistent");
        let fixed = heal_all(&home).unwrap();
        assert_eq!(fixed, 0);
    }

    #[test]
    fn test_heal_creates_symlinks() {
        let (_tmp, home) = setup();
        let canonical = canonical_skills_dir(&home);

        // Create a canonical skill
        let skill_dir = canonical.join("test-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "# Test").unwrap();

        let fixed = heal_all(&home).unwrap();
        assert!(fixed > 0, "expected at least one symlink created");

        // Verify symlinks exist
        let entries = registry(&home);
        for entry in &entries {
            let link = entry.skills_dir.join("test-skill");
            assert!(link.exists(), "symlink should exist for {}", entry.agent);
            assert!(
                fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "should be symlink for {}",
                entry.agent
            );
        }
    }

    #[test]
    fn test_heal_replaces_real_dir() {
        let (_tmp, home) = setup();
        let canonical = canonical_skills_dir(&home);

        // Create canonical skill
        let skill_dir = canonical.join("my-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "# Canonical").unwrap();

        // Create a real directory in claude skills (pre-agentalign)
        let claude_skill = home.join(".claude").join("skills").join("my-skill");
        fs::create_dir_all(&claude_skill).unwrap();
        fs::write(claude_skill.join("SKILL.md"), "# Old content").unwrap();

        let fixed = heal_all(&home).unwrap();
        assert!(fixed > 0);

        // Verify it's now a symlink
        assert!(fs::symlink_metadata(&claude_skill)
            .unwrap()
            .file_type()
            .is_symlink());

        // Verify backup was created
        let backup_dir = home.join(".agents").join("backups");
        let backups: Vec<_> = fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!backups.is_empty(), "expected backup of replaced dir");
    }

    #[test]
    fn test_heal_idempotent() {
        let (_tmp, home) = setup();
        let canonical = canonical_skills_dir(&home);

        let skill_dir = canonical.join("idempotent-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "# Test").unwrap();

        let fixed1 = heal_all(&home).unwrap();
        assert!(fixed1 > 0);

        // Second heal should be no-op
        let fixed2 = heal_all(&home).unwrap();
        assert_eq!(fixed2, 0);
    }

    #[test]
    fn test_heal_imports_opencode_command_as_codex_skill() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let commands_dir = home.join(".config").join("opencode").join("commands");
        fs::create_dir_all(&commands_dir).unwrap();
        fs::write(
            commands_dir.join("cook.md"),
            "---\ndescription: Full cook cycle\nagent: build\n---\n\nRun the cook workflow.",
        )
        .unwrap();

        let fixed = heal_all(&home).unwrap();

        let canonical_skill = canonical_skills_dir(&home).join("cook").join("SKILL.md");
        let codex_link = home.join(".codex").join("skills").join("cook");
        let skill_markdown = fs::read_to_string(&canonical_skill).unwrap();

        assert!(fixed > 0, "expected imported command to be linked");
        assert!(
            canonical_skill.exists(),
            "canonical cook skill should exist"
        );
        assert!(
            skill_markdown.contains("name: cook"),
            "import should add missing skill name"
        );
        assert!(
            fs::symlink_metadata(&codex_link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "codex should receive a symlink to the imported command skill"
        );
        assert_eq!(
            fs::read_link(codex_link).unwrap(),
            canonical_skills_dir(&home).join("cook")
        );
    }

    #[test]
    fn test_heal_continues_when_opencode_commands_cannot_be_scanned() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let canonical = canonical_skills_dir(&home);
        let skill_dir = canonical.join("still-heals");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "# Test").unwrap();

        let opencode_dir = home.join(".config").join("opencode");
        fs::create_dir_all(&opencode_dir).unwrap();
        fs::write(opencode_dir.join("commands"), "not a directory").unwrap();

        let fixed = heal_all(&home).unwrap();
        let codex_link = home.join(".codex").join("skills").join("still-heals");

        assert!(fixed > 0, "canonical skill should still be linked");
        assert!(
            fs::symlink_metadata(&codex_link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "codex should still receive canonical skills"
        );
    }

    #[test]
    fn canonical_skill_is_invokable_as_opencode_command() {
        let (_tmp, home) = setup();
        fs::create_dir_all(home.join(".config/opencode")).unwrap();
        let skill_dir = canonical_skills_dir(&home).join("rewrite-history");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: rewrite-history\ndescription: Rewrite branch history\n---\n\n# Instructions").unwrap();

        heal_all(&home).unwrap();

        let command = fs::read_to_string(home.join(".config/opencode/command/rewrite-history.md")).unwrap();
        assert!(command.contains("description: \"Rewrite branch history\""));
        assert!(command.contains("Load the `rewrite-history` skill"));
        assert!(command.contains("$ARGUMENTS"));
    }

    #[test]
    fn existing_opencode_command_is_not_replaced_by_skill() {
        let (_tmp, home) = setup();
        let skill_dir = canonical_skills_dir(&home).join("research");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: research\ndescription: Research\n---\n").unwrap();
        let commands = home.join(".config/opencode/commands");
        fs::create_dir_all(&commands).unwrap();
        fs::write(commands.join("research.md"), "Custom research command").unwrap();

        heal_all(&home).unwrap();

        assert_eq!(fs::read_to_string(commands.join("research.md")).unwrap(), "Custom research command");
        assert!(!home.join(".config/opencode/command/research.md").exists());
    }

    #[test]
    fn configured_opencode_command_takes_precedence_over_skill() {
        let (_tmp, home) = setup();
        let skill_dir = canonical_skills_dir(&home).join("research");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: research\ndescription: Research\n---\n").unwrap();
        let opencode = home.join(".config/opencode");
        fs::create_dir_all(&opencode).unwrap();
        fs::write(opencode.join("opencode.json"), r#"{"command":{"research":{"template":"Custom"}}}"#).unwrap();

        heal_all(&home).unwrap();

        assert!(!opencode.join("commands/research.md").exists());
    }

    #[test]
    fn configured_opencode_command_removes_previous_generated_wrapper() {
        let (_tmp, home) = setup();
        let skill_dir = canonical_skills_dir(&home).join("research");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: research\ndescription: Research\n---\n").unwrap();
        let opencode = home.join(".config/opencode");
        fs::create_dir_all(&opencode).unwrap();
        heal_all(&home).unwrap();
        let generated = opencode.join("command/research.md");
        assert!(generated.exists());
        fs::write(opencode.join("opencode.json"), r#"{"command":{"research":{"template":"Custom"}}}"#).unwrap();

        heal_all(&home).unwrap();

        assert!(!generated.exists());
    }

    #[test]
    fn generated_command_tracks_skill_and_is_removed_with_it() {
        let (_tmp, home) = setup();
        let skill_dir = canonical_skills_dir(&home).join("audit");
        fs::create_dir_all(&skill_dir).unwrap();
        let skill = skill_dir.join("SKILL.md");
        fs::write(&skill, "---\nname: audit\ndescription: First description\n---\n").unwrap();
        heal_all(&home).unwrap();

        fs::write(&skill, "---\nname: audit\ndescription: Second description\n---\n").unwrap();
        heal_all(&home).unwrap();

        let command = home.join(".config/opencode/command/audit.md");
        assert!(fs::read_to_string(&command).unwrap().contains("description: \"Second description\""));

        fs::remove_dir_all(skill_dir).unwrap();
        heal_all(&home).unwrap();

        assert!(!command.exists());
    }

    #[test]
    fn managed_plural_commands_migrate_without_importing_or_overwriting_user_files() {
        let (_tmp, home) = setup();
        let skill_dir = canonical_skills_dir(&home).join("audit");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: audit\ndescription: Audit\n---\n").unwrap();
        let plural = home.join(".config/opencode/commands");
        fs::create_dir_all(&plural).unwrap();
        fs::write(plural.join("audit.md"), format!("{}\nold wrapper", GENERATED_COMMAND_MARKER)).unwrap();
        fs::write(plural.join("mine.md"), "Custom command").unwrap();

        heal_all(&home).unwrap();

        assert!(!plural.join("audit.md").exists());
        assert_eq!(fs::read_to_string(plural.join("mine.md")).unwrap(), "Custom command");
        assert!(home.join(".config/opencode/command/audit.md").exists());
        assert!(canonical_skills_dir(&home).join("mine").join("SKILL.md").exists());
    }

    #[test]
    fn deleted_canonical_skill_prunes_only_its_dangling_links() {
        let (_tmp, home) = setup();
        let canonical = canonical_skills_dir(&home);
        let skills = home.join(".config/opencode/skills");
        fs::create_dir_all(&skills).unwrap();
        create_skill_link(&canonical.join("gone"), &skills.join("gone")).unwrap();
        create_skill_link(&home.join("elsewhere"), &skills.join("foreign")).unwrap();

        heal_all(&home).unwrap();

        assert!(fs::symlink_metadata(skills.join("gone")).is_err());
        assert!(fs::symlink_metadata(skills.join("foreign")).is_ok());
    }

    #[test]
    fn test_skill_link_resolves_without_symlink_privilege() {
        let tmp = TempDir::new().unwrap();
        let canonical = tmp.path().join("canonical-skill");
        let link = tmp.path().join("linked-skill");
        fs::create_dir_all(&canonical).unwrap();
        fs::write(canonical.join("SKILL.md"), "# Canonical").unwrap();

        let mechanism = create_skill_link(&canonical, &link).unwrap();

        assert!(mechanism == "symlink" || mechanism == "junction");
        assert_eq!(
            fs::canonicalize(fs::read_link(&link).unwrap()).unwrap(),
            fs::canonicalize(&canonical).unwrap()
        );
        assert_eq!(
            fs::read_to_string(link.join("SKILL.md")).unwrap(),
            "# Canonical"
        );
    }

    #[test]
    fn test_heal_one_unknown_agent() {
        let (_tmp, home) = setup();
        let result = heal_one(&home, "nonexistent");
        assert!(result.is_err());
    }
}
