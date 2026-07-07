use crate::agents::canonical::ParsedAgentFile;
use crate::agents::codex::CodexAgentStrategy;
use crate::mcp::factory::AgentType;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub mod canonical;
pub mod claude;
pub mod codex;
pub mod cursor;
pub mod gemini;
pub mod grok;
pub mod opencode;
pub mod zcode;

pub trait SubagentStrategy {
    fn agent_type(&self) -> AgentType;
    fn agents_dir(&self, home: &Path) -> std::path::PathBuf;
    fn format_agent(&self, agent: &ParsedAgentFile) -> anyhow::Result<String>;

    /// Override to customize per-agent target path.
    /// Default: `<agents_dir>/<agent_name>.md`
    fn agent_target_path(&self, agents_dir: &Path, agent_name: &str) -> std::path::PathBuf {
        agents_dir.join(format!("{}.md", agent_name))
    }

    /// Hook called after all agents are synced for this strategy.
    /// Use for additional side-effects (e.g., writing runtime configs).
    fn post_sync(&self, _home: &Path, _agents: &[ParsedAgentFile]) -> anyhow::Result<()> {
        Ok(())
    }
}

pub struct SubagentRegistry;

impl SubagentRegistry {
    pub fn synced_strategies() -> Vec<Box<dyn SubagentStrategy>> {
        vec![
            Box::new(opencode::OpenCodeAgentStrategy),
            Box::new(claude::ClaudeAgentStrategy),
            Box::new(gemini::GeminiAgentStrategy),
            Box::new(CodexAgentStrategy),
            Box::new(cursor::CursorAgentStrategy),
            Box::new(zcode::ZCodeAgentStrategy),
            Box::new(grok::GrokAgentStrategy),
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentManifest {
    #[serde(flatten)]
    pub entries: HashMap<String, HashSet<String>>,
}

impl AgentManifest {
    fn path(home: &Path) -> std::path::PathBuf {
        home.join(".agents").join("agents").join(".manifest.json")
    }

    fn load(home: &Path) -> Self {
        let path = Self::path(home);
        if path.exists() {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(manifest) = serde_json::from_str(&data) {
                    return manifest;
                }
            }
        }
        Self::default()
    }

    fn save(&self, home: &Path) -> anyhow::Result<()> {
        let path = Self::path(home);
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, data)?;
        Ok(())
    }

    fn record(&mut self, agent_type: &str, agent_name: &str) {
        self.entries
            .entry(agent_type.to_string())
            .or_default()
            .insert(agent_name.to_string());
    }

    fn synced_names(&self, agent_type: &str) -> HashSet<String> {
        self.entries.get(agent_type).cloned().unwrap_or_default()
    }
}

pub fn sync_agents(home: &Path, dry_run: bool) -> anyhow::Result<usize> {
    let agents = canonical::load_all_agents(home)?;
    if agents.is_empty() {
        println!("  no canonical agents found — skipping agent sync");
        return Ok(0);
    }

    let strategies = SubagentRegistry::synced_strategies();
    let mut synced = 0usize;
    let mut manifest = AgentManifest::load(home);
    let canonical_names: HashSet<String> = agents.iter().map(|a| a.name.clone()).collect();

    for strategy in &strategies {
        let agents_dir = strategy.agents_dir(home);
        let agent_type_str = strategy.agent_type().as_str();

        if dry_run {
            println!(
                "  [DRY RUN] {} -> {} ({} agents)",
                agent_type_str,
                agents_dir.display(),
                agents.len()
            );
            continue;
        }

        ensure_dir_all(&agents_dir)?;

        for agent in &agents {
            let output = strategy.format_agent(agent)?;
            let target = strategy.agent_target_path(&agents_dir, &agent.name);

            // Ensure parent directory exists (some strategies use nested paths).
            // ensure_dir_all also clears broken symlinks left behind when an
            // agent has no matching skill directory (e.g. ~/.codex/skills/<name>
            // pointing at a missing ~/.agents/skills/<name>).
            if let Some(parent) = target.parent() {
                ensure_dir_all(parent)?;
            }

            if target.exists() {
                if let Ok(existing) = std::fs::read_to_string(&target) {
                    if existing == output {
                        manifest.record(agent_type_str, &agent.name);
                        continue;
                    }
                }
            }

            std::fs::write(&target, &output)?;
            manifest.record(agent_type_str, &agent.name);
            synced += 1;
            println!(
                "  {} agent {} -> {}",
                agent_type_str,
                agent.name,
                target.display()
            );
        }

        let previously_synced = manifest.synced_names(agent_type_str);
        let to_remove: Vec<String> = previously_synced
            .difference(&canonical_names)
            .cloned()
            .collect();

        for orphan in &to_remove {
            let orphan_path = strategy.agent_target_path(&agents_dir, orphan);
            if orphan_path.exists() && std::fs::remove_file(&orphan_path).is_ok() {
                synced += 1;
                println!(
                    "  removed synced orphan: {} agent {}",
                    agent_type_str, orphan
                );
            }
        }

        if let Some(entry) = manifest.entries.get_mut(agent_type_str) {
            *entry = canonical_names.clone();
        }

        // Post-sync hook (e.g., Gemini writes runtime agent.json configs)
        strategy.post_sync(home, &agents)?;
    }

    if !dry_run {
        manifest.save(home)?;
    }

    Ok(synced)
}

/// Like `std::fs::create_dir_all`, but transparently removes broken symlinks
/// that block directory creation.
///
/// `create_dir_all` fails with `AlreadyExists` when an ancestor of `path` is
/// a dangling symlink: it tries `create_dir(ancestor)`, gets `EEXIST`, then
/// cannot `metadata` the link to confirm it is a directory, so it returns the
/// error. This happens in agent sync when an agent has a canonical `.md`
/// definition but no matching skill directory — `~/.codex/skills/<name>` is
/// a symlink to a missing `~/.agents/skills/<name>`, and writing
/// `<name>/agents/openai.yaml` inside it fails.
///
/// We walk up `path`'s ancestors, remove any dangling symlinks, then retry.
/// Real directories and valid symlinks are left untouched.
fn ensure_dir_all(path: &Path) -> std::io::Result<()> {
    match std::fs::create_dir_all(path) {
        Ok(()) => Ok(()),
        Err(first) => {
            let mut fixed = false;
            let mut cur = Some(path);
            while let Some(p) = cur {
                if is_broken_symlink(p) {
                    std::fs::remove_file(p)?;
                    fixed = true;
                }
                cur = p.parent();
            }
            if fixed {
                std::fs::create_dir_all(path)
            } else {
                Err(first)
            }
        }
    }
}

fn is_broken_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
        && std::fs::metadata(path)
            .err()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
}

#[cfg(test)]
mod tests {
    use super::ensure_dir_all;
    use std::path::Path;
    use tempfile::TempDir;

    /// Helper: create a broken symlink at `<root>/link` pointing to a
    /// nonexistent `<root>/missing`.
    #[cfg(unix)]
    fn make_broken_symlink(root: &Path, name: &str) -> std::path::PathBuf {
        let target = root.join("missing");
        let link = root.join(name);
        std::os::unix::fs::symlink(&target, &link).unwrap();
        link
    }

    #[test]
    fn test_ensure_dir_all_creates_missing_dir() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a").join("b");
        ensure_dir_all(&path).unwrap();
        assert!(path.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_dir_all_handles_broken_symlink_ancestor() {
        // Reproduces the production bug: a broken symlink sits where agentalign
        // needs to create a directory (e.g. ~/.codex/skills/<agent>/agents
        // when ~/.codex/skills/<agent> is a dangling symlink).
        let tmp = TempDir::new().unwrap();
        let skills = tmp.path().join("codex").join("skills");
        std::fs::create_dir_all(&skills).unwrap();

        // Broken symlink: skills/vision -> skills/missing (missing does not
        // exist).
        make_broken_symlink(&skills, "vision");

        let target = skills.join("vision").join("agents");
        // Before the fix this returned AlreadyExists (os error 17).
        ensure_dir_all(&target)
            .expect("ensure_dir_all should remove the broken symlink and create the dir");
        assert!(target.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_dir_all_preserves_valid_symlink_to_dir() {
        let tmp = TempDir::new().unwrap();
        let skills = tmp.path().join("codex").join("skills");
        std::fs::create_dir_all(&skills).unwrap();

        // Valid symlink: skills/designer -> skills/real_designer (exists).
        let real = skills.join("real_designer");
        std::fs::create_dir_all(&real).unwrap();
        let link = skills.join("designer");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let target = link.join("agents");
        ensure_dir_all(&target).unwrap();
        assert!(target.is_dir());
        // Symlink must still be a symlink, not replaced.
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
    }
}
