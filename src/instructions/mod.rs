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
use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::Utc;

/// Create a symlink at `link` pointing to `canonical`.
///
/// Windows needs `symlink_file` plus Developer Mode or elevation.
pub(crate) fn create_symlink(canonical: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(canonical, link)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(canonical, link)
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

/// Platform-specific hint appended to symlink creation failures.
pub(crate) fn symlink_hint() -> &'static str {
    if cfg!(windows) {
        " (Windows requires Developer Mode or an elevated shell to create symlinks)"
    } else {
        ""
    }
}

/// Compare two paths, tolerating the `\\?\` prefix Windows `read_link` returns.
fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// A symlink entry in the instruction file registry.
pub struct InstructionEntry {
    /// Human-readable agent name (for logging).
    pub agent: &'static str,
    /// Path to the tool-specific instruction file (the symlink target).
    pub symlink_path: PathBuf,
}

/// VS Code's user prompts directory, where Copilot reads `*.instructions.md`.
fn vscode_prompts_dir(home: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    let base = home.join("AppData").join("Roaming").join("Code");
    #[cfg(target_os = "macos")]
    let base = home
        .join("Library")
        .join("Application Support")
        .join("Code");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = home.join(".config").join("Code");

    base.join("User").join("prompts")
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
        // VS Code Copilot applies every *.instructions.md in the user prompts
        // directory; the canonical file carries the `applyTo` frontmatter.
        InstructionEntry {
            agent: "copilot-vscode",
            symlink_path: vscode_prompts_dir(home).join("agents.instructions.md"),
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

                if same_path(&resolved, canonical_path) {
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
        if state == SymlinkState::Ok {
            return Ok(false);
        }

        if let Some(parent) = self.symlink_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Stage the link beside the target and rename it into place, so a platform
        // that cannot create symlinks never destroys the existing instruction file.
        let staged = staging_path(&self.symlink_path);
        let _ = fs::remove_file(&staged);
        create_symlink(canonical_path, &staged).with_context(|| {
            format!(
                "cannot symlink {} -> {}{}",
                self.symlink_path.display(),
                canonical_path.display(),
                symlink_hint()
            )
        })?;

        if state == SymlinkState::ReplacedByFile {
            match self.backup(canonical_path) {
                Ok(backup_path) => println!(
                    "  backed up {} -> {}",
                    self.symlink_path.display(),
                    backup_path.display()
                ),
                Err(e) => {
                    let _ = fs::remove_file(&staged);
                    return Err(e);
                }
            }
        }

        // fs::rename replaces an existing file or symlink on both Unix and Windows.
        if let Err(e) = fs::rename(&staged, &self.symlink_path) {
            let _ = fs::remove_file(&staged);
            return Err(anyhow::Error::new(e).context(format!(
                "cannot move the new symlink into place at {}",
                self.symlink_path.display()
            )));
        }

        println!(
            "  {} -> {} (symlink to canonical)",
            self.agent,
            self.symlink_path.display()
        );

        Ok(true)
    }

    /// Copy the current instruction file into `~/.agents/backups/`, returning its path.
    fn backup(&self, canonical_path: &Path) -> anyhow::Result<PathBuf> {
        let backup_dir = canonical_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("canonical path has no parent"))?
            .join("backups");
        fs::create_dir_all(&backup_dir)?;

        let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
        let backup_path =
            backup_dir.join(format!("{}-AGENTS.md-{}.bak", self.agent, timestamp));
        fs::copy(&self.symlink_path, &backup_path).with_context(|| {
            format!("cannot back up {}", self.symlink_path.display())
        })?;
        Ok(backup_path)
    }
}

/// Sibling path used to stage a new symlink before renaming it over the target.
fn staging_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".agentalign-new");
    target.with_file_name(name)
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

    /// Create a symlink in tests on whichever platform is running.
    fn link(target: &Path, at: &Path) {
        create_symlink(target, at).expect(
            "test symlink creation failed; on Windows enable Developer Mode or run elevated",
        );
    }

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
        link(&canonical, &entry.symlink_path);

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

        link(&wrong_target, &entry.symlink_path);

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
        link(&wrong_target, &entry.symlink_path);

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
    fn test_heal_preserves_non_utf8_content_in_backup() {
        let (_tmp, canonical, entries) = setup();
        let entry = &entries[0];
        let raw: &[u8] = &[b'#', b' ', 0xff, 0xfe, b' ', b'n', b'o', b't', b'e', b's'];

        fs::create_dir_all(entry.symlink_path.parent().unwrap()).unwrap();
        fs::write(&entry.symlink_path, raw).unwrap();

        entry.heal(&canonical).unwrap();

        let backup_dir = canonical.parent().unwrap().join("backups");
        let backup = fs::read_dir(&backup_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(fs::read(&backup).unwrap(), raw);
    }

    #[test]
    fn test_heal_is_idempotent() {
        let (_tmp, canonical, entries) = setup();
        let entry = &entries[0];

        assert!(entry.heal(&canonical).unwrap());
        assert!(
            !entry.heal(&canonical).unwrap(),
            "a healed symlink must verify as Ok, otherwise every sync rewrites it"
        );
    }

    #[test]
    fn test_heal_leaves_no_staging_file() {
        let (_tmp, canonical, entries) = setup();
        let entry = &entries[0];

        entry.heal(&canonical).unwrap();

        let staged = staging_path(&entry.symlink_path);
        assert!(
            fs::symlink_metadata(&staged).is_err(),
            "staging file {} was left behind",
            staged.display()
        );
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
        assert_eq!(fixed, registry(&home).len());

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
        assert!(!result.unwrap());
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
