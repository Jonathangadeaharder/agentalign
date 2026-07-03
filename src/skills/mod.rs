//! Skills directory syncing module.
//!
//! Maintains `~/.agents/skills/` as the canonical source of truth for agent
//! skill directories. Each skill subdirectory (e.g., `~/.agents/skills/diagnose/`)
//! is symlinked into per-agent skills directories:
//! - `~/.claude/skills/diagnose` → `~/.agents/skills/diagnose`
//! - `~/.gemini/skills/diagnose` → `~/.agents/skills/diagnose`
//! - `~/.codex/skills/diagnose` → `~/.agents/skills/diagnose`
//!
//! Real directories (pre-agentalign) are backed up and replaced with symlinks.
//! Called by:
//! - `agentalign sync` (after MCP + instruction sync)
//! - `agentalign watch` (on daemon startup and on skills dir changes)

#[cfg(unix)]
use std::os::unix;
use std::path::{Path, PathBuf};

use chrono::Utc;

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
        SkillsEntry {
            agent: "gemini",
            skills_dir: home.join(".gemini").join("skills"),
        },
        SkillsEntry {
            agent: "codex",
            skills_dir: home.join(".codex").join("skills"),
        },
        SkillsEntry {
            agent: "cursor",
            skills_dir: home.join(".cursor").join("skills"),
        },
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
#[cfg(unix)]
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
            // Remove wrong symlink
            std::fs::remove_file(&skill_path)?;
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

    // Create symlink
    unix::fs::symlink(canonical_skill, &skill_path)?;

    eprintln!(
        "  {} skill {} -> {} (symlink)",
        agent_name,
        skill_name,
        canonical_skill.display()
    );

    Ok(true)
}

/// Non-unix fallback: just report that symlinks aren't supported.
#[cfg(not(unix))]
pub fn heal_skill(
    _agent_skills_dir: &Path,
    _skill_name: &str,
    _canonical_skill: &Path,
    _agent_name: &str,
    _backup_dir: &Path,
) -> anyhow::Result<bool> {
    eprintln!("  warning: symlinks not supported on this platform");
    Ok(false)
}

/// Heal all skills for all agents. Returns the number of changes made.
pub fn heal_all(home: &Path) -> anyhow::Result<usize> {
    let canonical_dir = canonical_skills_dir(home);

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

    if canonical_skills.is_empty() {
        return Ok(0);
    }

    let entries = registry(home);
    let backup_dir = home.join(".agents").join("backups");
    let mut fixed = 0usize;

    for entry in &entries {
        // Ensure agent skills dir exists
        std::fs::create_dir_all(&entry.skills_dir).ok();

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
                    eprintln!(
                        "  error healing {}/{}: {}",
                        entry.agent, skill_name, e
                    );
                }
            }
        }
    }

    Ok(fixed)
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

    std::fs::create_dir_all(&entry.skills_dir).ok();

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

    #[cfg(unix)]
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
                fs::symlink_metadata(&link).unwrap().file_type().is_symlink(),
                "should be symlink for {}",
                entry.agent
            );
        }
    }

    #[cfg(unix)]
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
        assert!(
            fs::symlink_metadata(&claude_skill)
                .unwrap()
                .file_type()
                .is_symlink()
        );

        // Verify backup was created
        let backup_dir = home.join(".agents").join("backups");
        let backups: Vec<_> = fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!backups.is_empty(), "expected backup of replaced dir");
    }

    #[cfg(unix)]
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
    fn test_heal_one_unknown_agent() {
        let (_tmp, home) = setup();
        let result = heal_one(&home, "nonexistent");
        assert!(result.is_err());
    }
}
