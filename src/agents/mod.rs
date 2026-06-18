use crate::agents::canonical::ParsedAgentFile;
use crate::agents::codex::CodexAgentStrategy;
use crate::mcp::factory::AgentType;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub mod canonical;
pub mod claude;
pub mod codex;
pub mod gemini;
pub mod opencode;

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
        self.entries
            .get(agent_type)
            .cloned()
            .unwrap_or_default()
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

        std::fs::create_dir_all(&agents_dir)?;

        for agent in &agents {
            let output = strategy.format_agent(agent)?;
            let target = strategy.agent_target_path(&agents_dir, &agent.name);

            // Ensure parent directory exists (some strategies use nested paths)
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
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
