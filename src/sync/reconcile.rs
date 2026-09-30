use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::mcp::factory::McpFormatFactory;
use crate::mcp::factory::{AgentDescriptor, AgentRegistry};
use crate::shared::config;
use crate::shared::models::{CanonicalWorkspaceState, McpServerDefinition};
use crate::sync::{overlay, transaction};

#[derive(Default, Serialize, Deserialize)]
pub struct ReconciledState {
    #[serde(default)]
    pub mcp: HashMap<String, McpServerDefinition>,
    #[serde(default)]
    pub agents: BTreeMap<String, BTreeMap<String, McpServerDefinition>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub excluded: BTreeMap<String, HashSet<String>>,
    #[serde(default, flatten)]
    pub other: BTreeMap<String, Value>,
}

#[derive(Default, Serialize, Deserialize)]
struct PublishedState {
    agents: BTreeMap<String, HashMap<String, McpServerDefinition>>,
}

pub fn collect(home: &Path) -> Result<ReconciledState> {
    let path = home.join(".agents/mcp_config.json");
    if !path.exists() {
        anyhow::bail!(
            "No canonical config at {}. Run `agentalign migrate` first.",
            path.display()
        );
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut state: ReconciledState = serde_json::from_str(&raw)?;
    let published_path = home.join(".agents/mcp_published.json");
    let published: PublishedState = if published_path.exists() {
        serde_json::from_str(&fs::read_to_string(&published_path)?)?
    } else {
        PublishedState::default()
    };
    let has_published = published_path.exists();
    let local_entries = config::load_local_entries(&home.join(".agents"))?;

    for descriptor in AgentRegistry::synced_agents(home) {
        if !descriptor.config_path.exists() {
            continue;
        }
        let raw = fs::read_to_string(&descriptor.config_path)?;
        let agent = descriptor.agent_type.as_str();
        if agent != "codex" && agent != "grok" {
            let value: Value = serde_json::from_str(&raw)?;
            let section = match agent {
                "opencode" => "mcp",
                "vscode" => "servers",
                "zcode" => "mcp",
                _ => "mcpServers",
            };
            let present = if agent == "zcode" {
                value
                    .get("mcp")
                    .and_then(|mcp| mcp.get("servers"))
                    .is_some()
            } else {
                value.get(section).is_some()
            };
            if !present {
                continue;
            }
        }
        let strategy = McpFormatFactory::from_agent(descriptor.agent_type);
        let parsed = strategy
            .deserialize_to_canonical(&raw, home)
            .with_context(|| format!("parse {}", descriptor.config_path.display()))?;
        let servers = parsed
            .get("mcp")
            .and_then(Value::as_object)
            .with_context(|| format!("missing MCP map in {}", descriptor.config_path.display()))?;
        let restored = strategy.serialize_from_canonical(&parsed, home)?;
        let original_mcp = match agent {
            "opencode" => serde_json::from_str::<Value>(&raw)?.get("mcp").cloned(),
            "zcode" => serde_json::from_str::<Value>(&raw)?
                .get("mcp")
                .and_then(|mcp| mcp.get("servers"))
                .cloned(),
            "vscode" => serde_json::from_str::<Value>(&raw)?.get("servers").cloned(),
            "codex" | "grok" => None,
            _ => serde_json::from_str::<Value>(&raw)?
                .get("mcpServers")
                .cloned(),
        };
        let restored_mcp = match agent {
            "opencode" => serde_json::from_str::<Value>(&restored)?
                .get("mcp")
                .cloned(),
            "zcode" => serde_json::from_str::<Value>(&restored)?
                .get("mcp")
                .and_then(|mcp| mcp.get("servers"))
                .cloned(),
            "vscode" => serde_json::from_str::<Value>(&restored)?
                .get("servers")
                .cloned(),
            "codex" | "grok" => None,
            _ => serde_json::from_str::<Value>(&restored)?
                .get("mcpServers")
                .cloned(),
        };
        if original_mcp != restored_mcp {
            anyhow::bail!(
                "{} native MCP settings cannot be collected without loss",
                descriptor.label
            );
        }
        let observed: HashSet<&str> = servers.keys().map(String::as_str).collect();
        if !has_published && !state.agents.contains_key(agent) {
            for name in state.mcp.keys() {
                if !observed.contains(name.as_str()) {
                    state
                        .excluded
                        .entry(agent.to_owned())
                        .or_default()
                        .insert(name.clone());
                }
            }
        }
        if let Some(published) = published.agents.get(agent) {
            for name in published.keys() {
                if !observed.contains(name.as_str()) {
                    state
                        .excluded
                        .entry(agent.to_owned())
                        .or_default()
                        .insert(name.clone());
                }
            }
        }
        let variants = state.agents.entry(agent.to_owned()).or_default();
        variants.retain(|name, _| observed.contains(name.as_str()));
        let excluded = state.excluded.entry(agent.to_owned()).or_default();
        excluded.retain(|name| !observed.contains(name.as_str()));
        for (name, value) in servers {
            let definition: McpServerDefinition = serde_json::from_value(value.clone())
                .with_context(|| format!("invalid {} server {}", descriptor.label, name))?;
            let last = published
                .agents
                .get(agent)
                .and_then(|published| published.get(name));
            if last == Some(&definition)
                && state.mcp.contains_key(name)
                && !local_entries.contains(name)
            {
                variants.remove(name);
                continue;
            }
            if last == Some(&definition)
                && !state.mcp.contains_key(name)
                && !variants.contains_key(name)
            {
                continue;
            }
            variants.insert(name.clone(), definition);
        }
    }
    Ok(state)
}

pub fn view(state: &ReconciledState, agent: &str) -> CanonicalWorkspaceState {
    let mut mcp = state.mcp.clone();
    if let Some(excluded) = state.excluded.get(agent) {
        mcp.retain(|name, _| !excluded.contains(name));
    }
    if let Some(variants) = state.agents.get(agent) {
        mcp.extend(
            variants
                .iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        );
    }
    CanonicalWorkspaceState { mcp }
}

pub fn save(home: &Path, state: &ReconciledState) -> Result<()> {
    let path = home.join(".agents/mcp_config.json");
    let body = serde_json::to_string_pretty(state)?;
    if fs::read_to_string(&path).is_ok_and(|current| current == body) {
        return Ok(());
    }
    fs::write(path, body)?;
    Ok(())
}

pub fn prepare(
    home: &Path,
    state: &ReconciledState,
) -> Result<Vec<(AgentDescriptor, String, usize)>> {
    let skip = config::load_agent_skip(&home.join(".agents"))?;
    AgentRegistry::synced_agents(home)
        .into_iter()
        .map(|descriptor| {
            let mut view = view(state, descriptor.agent_type.as_str());
            view.mcp = config::filter_skipped(&view.mcp, descriptor.label, &skip);
            let strategy = McpFormatFactory::from_agent(descriptor.agent_type);
            strategy.validate(&view)?;
            let output = strategy.serialize_from_canonical(&serde_json::to_value(&view)?, home)?;
            let output = overlay::overlay_onto_existing(&output, &descriptor.config_path);
            let rendered = strategy
                .deserialize_to_canonical(&output, home)
                .with_context(|| format!("verify {} output", descriptor.label))?;
            let servers = rendered
                .get("mcp")
                .and_then(Value::as_object)
                .with_context(|| format!("missing MCP map in {} output", descriptor.label))?;
            for (name, expected) in &view.mcp {
                let actual = servers
                    .get(name)
                    .with_context(|| format!("{} dropped {}", descriptor.label, name))?;
                let actual: McpServerDefinition = serde_json::from_value(actual.clone())?;
                if &actual != expected {
                    anyhow::bail!(
                        "{} cannot round-trip MCP server {} without losing settings",
                        descriptor.label,
                        name
                    );
                }
            }
            Ok((descriptor, output, view.mcp.len()))
        })
        .collect()
}

pub fn sync(home: &Path, dry_run: bool) -> Result<Vec<(String, usize)>> {
    let state = collect(home)?;
    let outputs = prepare(home, &state)?;
    let summary = outputs
        .iter()
        .map(|(descriptor, _, count)| (descriptor.label.to_owned(), *count))
        .collect();
    if dry_run {
        return Ok(summary);
    }
    let canonical_path = home.join(".agents/mcp_config.json");
    let published_path = home.join(".agents/mcp_published.json");
    let previous_canonical = fs::read(&canonical_path)?;
    let previous_published = if published_path.exists() {
        Some(fs::read(&published_path)?)
    } else {
        None
    };
    let mut written = Vec::new();
    let write_result = (|| -> Result<()> {
        save(home, &state)?;
        for (descriptor, output, _) in outputs {
            if fs::read_to_string(&descriptor.config_path).is_ok_and(|current| current == output) {
                continue;
            }
            if let Some(parent) = descriptor.config_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let tx = transaction::create_transaction(descriptor.label, &descriptor.config_path)?;
            written.push(tx.clone());
            fs::write(&descriptor.config_path, &output)?;
            transaction::finalize_transaction(&tx, output.as_bytes())?;
        }
        let skip = config::load_agent_skip(&home.join(".agents"))?;
        let agents = AgentRegistry::synced_agents(home)
            .into_iter()
            .map(|descriptor| {
                let agent = descriptor.agent_type.as_str().to_owned();
                let visible = view(&state, &agent);
                (
                    agent,
                    config::filter_skipped(&visible.mcp, descriptor.label, &skip),
                )
            })
            .collect();
        let published = PublishedState { agents };
        fs::write(&published_path, serde_json::to_string_pretty(&published)?)?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let mut rollback_error = None;
        for tx in written.iter().rev() {
            if let Err(err) = transaction::rollback_transaction(tx) {
                rollback_error = Some(err);
            }
        }
        if let Err(err) = fs::write(canonical_path, previous_canonical) {
            rollback_error = Some(err.into());
        }
        if let Some(previous) = previous_published {
            if let Err(err) = fs::write(published_path, previous) {
                rollback_error = Some(err.into());
            }
        } else if published_path.exists() {
            if let Err(err) = fs::remove_file(published_path) {
                rollback_error = Some(err.into());
            }
        }
        if let Some(err) = rollback_error {
            return Err(error.context(format!("rollback failed: {err}")));
        }
        return Err(error);
    }
    Ok(summary)
}
