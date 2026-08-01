//! Symlink guard module for agent instruction files.
//!
//! Maintains `~/.agents/AGENTS.md` as the single source of truth for agent
//! instruction files. All tool-specific files (CLAUDE.md, GEMINI.md, CODEX.md,
//! QWEN.md, AGENTS.md) are symlinks pointing to the canonical file.
//!
//! The `verify()` and `heal()` functions detect and fix broken, missing, or
//! regular-file-replaced symlinks. This module is called by:
//! - `agentalign sync` (after MCP sync)
//! - `agentalign watch` (on daemon startup and on file change events)

use std::fs;
#[cfg(unix)]
use std::os::unix;
use std::path::{Path, PathBuf};

use chrono::Utc;

/// A symlink entry in the instruction file registry.
pub struct InstructionEntry {
    /// Human-readable agent name (for logging).
    pub agent: &'static str,
    /// Path to the tool-specific instruction file (the symlink target).
    pub symlink_path: PathBuf,
}

/// Build the registry of instruction file symlinks.
fn registry(home: &Path) -> Vec<InstructionEntry> {
    vec![
        InstructionEntry {
            agent: "opencode",
            symlink_path: home.join(".config").join("opencode").join("AGENTS.md"),
        },
        InstructionEntry {
            agent: "claude",
            symlink_path: home.join(".claude").join("CLAUDE.md"),
        },
        InstructionEntry {
            agent: "gemini",
            symlink_path: home.join(".gemini").join("GEMINI.md"),
        },
        InstructionEntry {
            agent: "codex",
            symlink_path: home.join(".codex").join("CODEX.md"),
        },
        InstructionEntry {
            agent: "zcode",
            symlink_path: home.join(".zcode").join("AGENTS.md"),
        },
        InstructionEntry {
            agent: "grok",
            symlink_path: home.join(".grok").join("AGENTS.md"),
        },
        // Qwen Code (Gemini CLI fork) reads QWEN.md, analog of GEMINI.md.
        InstructionEntry {
            agent: "qwen",
            symlink_path: home.join(".qwen").join("QWEN.md"),
        },
    ]
}

/// Get the canonical instruction file path.
pub fn canonical_path(home: &Path) -> PathBuf {
    home.join(".agents").join("AGENTS.md")
}

/// Symlink state for a single entry.
#[derive(Debug, Clone, PartialEq)]
pub enum SymlinkState {
    /// Symlink is correct (points to canonical).
    Ok,
    /// Path does not exist at all.
    Missing,
    /// Path exists as a symlink but points to a different target.
    WrongTarget { current_target: PathBuf },
    /// Path exists as a regular file (overwritten).
    ReplacedByFile,
}

impl InstructionEntry {
    /// Check the state of this instruction symlink.
    pub fn verify(&self, canonical_path: &Path) -> SymlinkState {
        let path = &self.symlink_path;

        if !path.exists() && !path.symlink_metadata_ok() {
            return SymlinkState::Missing;
        }

        // Check if it's a symlink
        match fs::read_link(path) {
            Ok(target) => {
                // Resolve the target path (handle relative symlinks)
                let resolved = if target.is_relative() {
                    if let Some(parent) = path.parent() {
                        parent.join(&target)
                    } else {
                        target
                    }
                } else {
                    target
                };

                if resolved == canonical_path || resolved == *canonical_path {
                    SymlinkState::Ok
                } else {
                    SymlinkState::WrongTarget {
                        current_target: resolved,
                    }
                }
            }
            Err(_) => {
                // Not a symlink — must be a regular file (or broken symlink which exists() returns false for)
                if path.exists() {
                    SymlinkState::ReplacedByFile
                } else {
                    SymlinkState::Missing
                }
            }
        }
    }

    /// Heal this instruction symlink.
    ///
    /// Returns `true` if a change was made, `false` if already correct.
    pub fn heal(&self, canonical_path: &Path) -> anyhow::Result<bool> {
        let state = self.verify(canonical_path);

        match state {
            SymlinkState::Ok => return Ok(false),
            SymlinkState::Missing => {
                // Ensure parent directory exists
                if let Some(parent) = self.symlink_path.parent() {
                    fs::create_dir_all(parent)?;
                }
            }
            SymlinkState::WrongTarget { current_target: _ } => {
                // Remove the wrong symlink
                fs::remove_file(&self.symlink_path)?;
            }
            SymlinkState::ReplacedByFile => {
                // Backup the file before replacing
                let backup_dir = canonical_path
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("canonical path has no parent"))?
                    .join("backups");
                fs::create_dir_all(&backup_dir)?;

                let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
                let backup_name = format!("{}-AGENTS.md-{}.bak", self.agent, timestamp);
                let backup_path = backup_dir.join(&backup_name);

                // Read existing content for backup
                let content = fs::read_to_string(&self.symlink_path)?;
                fs::write(&backup_path, &content)?;

                // Remove the regular file
                fs::remove_file(&self.symlink_path)?;

                println!(
                    "  backed up {} -> {}",
                    self.symlink_path.display(),
                    backup_path.display()
                );
            }
        }

        // Create the symlink
        #[cfg(unix)]
        unix::fs::symlink(canonical_path, &self.symlink_path)?;
        #[cfg(not(unix))]
        {
            eprintln!(
                "  warning: symlinks not supported on this platform, skipping {}",
                self.agent
            );
            return Ok(false);
        }

        println!(
            "  {} -> {} (symlink to canonical)",
            self.agent,
            self.symlink_path.display()
        );

        Ok(true)
    }
}

/// Helper trait extension for Path to check symlink metadata without following.
trait PathSymlinkExt {
    fn symlink_metadata_ok(&self) -> bool;
}

impl PathSymlinkExt for Path {
    fn symlink_metadata_ok(&self) -> bool {
        fs::symlink_metadata(self).is_ok()
    }
}

/// Verify all instruction symlinks and return a report.
pub fn verify_all(home: &Path) -> Vec<(&'static str, SymlinkState)> {
    let canonical = canonical_path(home);
    let entries = registry(home);
    let mut results = Vec::new();

    for entry in &entries {
        let state = entry.verify(&canonical);
        results.push((entry.agent, state));
    }

    results
}

/// Heal all instruction symlinks. Returns the number of symlinks that were fixed.
///
/// Prints progress to stdout. Silent if all OK.
/// Warns and returns 0 if canonical instruction file is missing (does not bail).
pub fn heal_all(home: &Path) -> anyhow::Result<usize> {
    let canonical = canonical_path(home);

    if !canonical.exists() {
        eprintln!(
            "  warning: canonical instruction file not found at {} — skipping instruction sync",
            canonical.display()
        );
        return Ok(0);
    }

    let entries = registry(home);
    let mut fixed = 0usize;

    for entry in &entries {
        if entry.heal(&canonical)? {
            fixed += 1;
        }
    }

    Ok(fixed)
}

/// Heal a single instruction symlink by agent name.
pub fn heal_one(home: &Path, agent: &str) -> anyhow::Result<bool> {
    let canonical = canonical_path(home);

    if !canonical.exists() {
        eprintln!(
            "  warning: canonical instruction file not found at {} — skipping",
            canonical.display()
        );
        return Ok(false);
    }

    let entries = registry(home);
    let entry = entries
        .iter()
        .find(|e| e.agent == agent)
        .ok_or_else(|| anyhow::anyhow!("Unknown agent: {}", agent))?;

    entry.heal(&canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (TempDir, PathBuf, Vec<InstructionEntry>) {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let canonical = canonical_path(&home);

        // Create canonical file
        fs::create_dir_all(canonical.parent().unwrap()).unwrap();
        fs::write(&canonical, b"# Canonical instruction file").unwrap();

        let entries = registry(&home);
        (tmp, canonical, entries)
    }

    #[test]
    fn test_missing_symlink() {
        let (_tmp, canonical, _entries) = setup();
        let entry = InstructionEntry {
            agent: "test",
            symlink_path: PathBuf::from("/nonexistent/path/CLAUDE.md"),
        };
        assert_eq!(entry.verify(&canonical), SymlinkState::Missing);
    }

    #[test]
    fn test_correct_symlink() {
        let (_tmp, canonical, entries) = setup();
        let entry = &entries[0];

        // Create parent and symlink
        fs::create_dir_all(entry.symlink_path.parent().unwrap()).unwrap();
        unix::fs::symlink(&canonical, &entry.symlink_path).unwrap();

        assert_eq!(entry.verify(&canonical), SymlinkState::Ok);
    }

    #[test]
    fn test_wrong_symlink_target() {
        let (_tmp, canonical, entries) = setup();
        let entry = &entries[0];

        // Create the parent directory first
        fs::create_dir_all(entry.symlink_path.parent().unwrap()).unwrap();
        // Create a different file to symlink to
        let wrong_target = entry.symlink_path.parent().unwrap().join("wrong.md");
        fs::write(&wrong_target, b"wrong content").unwrap();

        unix::fs::symlink(&wrong_target, &entry.symlink_path).unwrap();

        assert_eq!(
            entry.verify(&canonical),
            SymlinkState::WrongTarget {
                current_target: wrong_target
            }
        );
    }

    #[test]
    fn test_replaced_by_file() {
        let (_tmp, canonical, entries) = setup();
        let entry = &entries[0];

        // Create a regular file where the symlink should be
        fs::create_dir_all(entry.symlink_path.parent().unwrap()).unwrap();
        fs::write(&entry.symlink_path, b"user-edited content").unwrap();

        assert_eq!(entry.verify(&canonical), SymlinkState::ReplacedByFile);
    }

    #[test]
    fn test_heal_missing() {
        let (_tmp, canonical, entries) = setup();
        let entry = &entries[0];

        let changed = entry.heal(&canonical).unwrap();
        assert!(changed);
        assert_eq!(entry.verify(&canonical), SymlinkState::Ok);
    }

    #[test]
    fn test_heal_wrong_target() {
        let (_tmp, canonical, entries) = setup();
        let entry = &entries[0];

        fs::create_dir_all(entry.symlink_path.parent().unwrap()).unwrap();
        let wrong_target = entry.symlink_path.parent().unwrap().join("wrong.md");
        fs::write(&wrong_target, b"wrong content").unwrap();
        unix::fs::symlink(&wrong_target, &entry.symlink_path).unwrap();

        let changed = entry.heal(&canonical).unwrap();
        assert!(changed);
        assert_eq!(entry.verify(&canonical), SymlinkState::Ok);
    }

    #[test]
    fn test_heal_replaced_by_file() {
        let (_tmp, canonical, entries) = setup();
        let entry = &entries[0];

        fs::create_dir_all(entry.symlink_path.parent().unwrap()).unwrap();
        fs::write(&entry.symlink_path, b"user-edited content").unwrap();

        let changed = entry.heal(&canonical).unwrap();
        assert!(changed);
        assert_eq!(entry.verify(&canonical), SymlinkState::Ok);

        // Verify backup was created
        let backup_dir = canonical.parent().unwrap().join("backups");
        let backups: Vec<_> = fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!backups.is_empty(), "expected at least one backup file");
    }

    #[test]
    fn test_heal_all() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let canonical = canonical_path(&home);

        fs::create_dir_all(canonical.parent().unwrap()).unwrap();
        fs::write(&canonical, b"# Canonical instruction file").unwrap();

        // None of the symlinks exist yet
        let fixed = heal_all(&home).unwrap();
        assert_eq!(fixed, 7); // opencode, claude, gemini, codex, zcode, grok, qwen

        // Second heal should be a no-op
        let fixed2 = heal_all(&home).unwrap();
        assert_eq!(fixed2, 0);
    }

    #[test]
    fn test_heal_all_warns_if_canonical_missing() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");

        // Should return Ok(0) and warn, not error
        let result = heal_all(&home);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_heal_one_warns_if_canonical_missing() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");

        let result = heal_one(&home, "claude");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), false);
    }

    #[test]
    fn test_heal_one_unknown_agent() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let canonical = canonical_path(&home);

        // Create canonical so we get past the missing-file check
        fs::create_dir_all(canonical.parent().unwrap()).unwrap();
        fs::write(&canonical, b"# Canonical instruction file").unwrap();

        let result = heal_one(&home, "nonexistent");
        assert!(result.is_err());
    }
}
