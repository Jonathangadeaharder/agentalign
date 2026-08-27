//! File watcher daemon for bidirectional magic sync.
//!
//! Uses the `notify` crate with FSEvents on macOS to watch all agent config
//! paths. On change detection, debounces 500ms, then determines if the change
//! was a user edit (sync to all agents) or our own write (skip).
//!
//! Bidirectional logic:
//! - Canonical changed → regenerate all agents
//! - Agent config changed → compute delta vs canonical → apply add/remove/update
//!   to canonical → propagate canonical to other agents

use crate::instructions;
use crate::mcp::factory::{AgentRegistry, AgentType, McpFormatFactory};
use crate::rules;
use crate::shared::config;
use crate::shared::models::CanonicalWorkspaceState;
use crate::skills;
use crate::state::SyncState;
use crate::sync::delta_merger;
use crate::sync::transaction;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Debounce window for file change events.
const DEBOUNCE_MS: u64 = 500;

/// IDs for instruction symlink watch entries (not MCP).
const INSTRUCTION_PREFIX: &str = "instr-";

/// IDs for skills directory watch entries.
const SKILLS_PREFIX: &str = "skills-";

/// IDs for rules directory watch entries.
const RULES_PREFIX: &str = "rules-";

/// Load the local entries protection set from ~/.agents/local_entries.json.
fn load_local_entries(agents_dir: &Path) -> HashSet<String> {
    config::load_local_entries(agents_dir).unwrap_or_else(|e| {
        eprintln!("[watch] failed to load local_entries.json: {}", e);
        HashSet::new()
    })
}

/// Watcher entry: maps a file path to its agent identifier.
struct WatchEntry {
    id: String,
    agent_type: Option<AgentType>,
    path: PathBuf,
}

/// Build the list of files to watch.
fn build_watch_list(home: &Path) -> Vec<WatchEntry> {
    let agents_dir = home.join(".agents");
    let mut entries = vec![
        WatchEntry {
            id: "canonical".to_string(),
            agent_type: None,
            path: agents_dir.join("mcp_config.json"),
        },
        // Canonical instruction file
        WatchEntry {
            id: "canonical-instructions".to_string(),
            agent_type: None,
            path: instructions::canonical_path(home),
        },
    ];

    // Use AgentRegistry for MCP config paths
    let descriptors = AgentRegistry::synced_agents(home);
    for descriptor in &descriptors {
        entries.push(WatchEntry {
            id: descriptor.agent_type.as_str().to_string(),
            agent_type: Some(descriptor.agent_type),
            path: descriptor.config_path.clone(),
        });
    }

    // Instruction symlink paths (for detecting breakage)
    for descriptor in &descriptors {
        if let Some(ref instr_path) = descriptor.instruction_path {
            entries.push(WatchEntry {
                id: format!("{}{}", INSTRUCTION_PREFIX, descriptor.agent_type.as_str()),
                agent_type: None,
                path: instr_path.clone(),
            });
        }
    }

    // Skills directories (canonical + per-agent)
    let canonical_skills = skills::canonical_skills_dir(home);
    entries.push(WatchEntry {
        id: "canonical-skills".to_string(),
        agent_type: None,
        path: canonical_skills,
    });

    for descriptor in &descriptors {
        if let Some(ref skills_dir) = descriptor.skills_dir {
            entries.push(WatchEntry {
                id: format!("{}{}", SKILLS_PREFIX, descriptor.agent_type.as_str()),
                agent_type: None,
                path: skills_dir.clone(),
            });
        }
    }

    // Rules directories (Cursor .mdc + Claude .md) — detect manual edits/deletes
    // so they get regenerated from AGENTS.md like skills/instructions do.
    entries.push(WatchEntry {
        id: format!("{}cursor", RULES_PREFIX),
        agent_type: None,
        path: rules::cursor_rules_dir(home),
    });
    entries.push(WatchEntry {
        id: format!("{}claude", RULES_PREFIX),
        agent_type: None,
        path: rules::claude_rules_dir(home),
    });

    entries
}

/// Run the file watcher daemon. Blocks until interrupted.
pub fn run_daemon() -> anyhow::Result<()> {
    let home = crate::shared::home_dir()?;
    let agents_dir = home.join(".agents");
    let canonical_path = agents_dir.join("mcp_config.json");

    if !canonical_path.exists() {
        anyhow::bail!(
            "No canonical config found at {}. Run `agentalign migrate` first.",
            canonical_path.display()
        );
    }

    // Heal instruction symlinks on startup
    match instructions::heal_all(&home) {
        Ok(fixed) => {
            if fixed > 0 {
                eprintln!("  instruction symlinks healed: {}", fixed);
            }
        }
        Err(e) => {
            eprintln!("  instruction symlink error: {}", e);
        }
    }

    // Heal skills symlinks on startup
    match skills::heal_all(&home) {
        Ok(fixed) => {
            if fixed > 0 {
                eprintln!("  skills symlinks healed: {}", fixed);
            }
        }
        Err(e) => {
            eprintln!("  skills symlink error: {}", e);
        }
    }

    // Sync AGENTS.md sections into Cursor + Claude rules on startup
    match rules::sync_rules(&home, false) {
        Ok(fixed) => {
            if fixed > 0 {
                eprintln!("  rules synced: {}", fixed);
            }
        }
        Err(e) => {
            eprintln!("  rules sync error: {}", e);
        }
    }

    let entries = build_watch_list(&home);
    let mut state = SyncState::load(&agents_dir);

    // Initialize hashes for all watched files
    for entry in &entries {
        state.update_hash(&entry.id, &entry.path);
    }
    state.save(&agents_dir)?;

    let (tx, rx) = channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        Config::default(),
    )?;

    // Watch all parent directories (notify watches directories, not individual files)
    let mut watched_dirs = std::collections::HashSet::new();
    for entry in &entries {
        if let Some(parent) = entry.path.parent() {
            if watched_dirs.insert(parent.to_path_buf()) {
                watcher.watch(parent, RecursiveMode::NonRecursive)?;
            }
        }
    }

    // Signal handling for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .ok();

    eprintln!("agentalign watch daemon started");
    eprintln!("Watching {} paths:", entries.len());
    for entry in &entries {
        eprintln!("  {} -> {}", entry.id, entry.path.display());
    }

    let mut last_event = Instant::now();
    let mut pending_sync = false;

    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(event)) => {
                if is_relevant_event(&event) {
                    last_event = Instant::now();
                    pending_sync = true;
                }
            }
            Ok(Err(e)) => {
                eprintln!("Watch error: {}", e);
            }
            Err(_) => {
                // timeout — check if debounce window has passed
                if pending_sync && last_event.elapsed() >= Duration::from_millis(DEBOUNCE_MS) {
                    pending_sync = false;
                    if let Err(e) = process_changes(&home, &agents_dir, &entries, &mut state) {
                        eprintln!("Sync error: {}", e);
                    }
                }
            }
        }
    }

    eprintln!("agentalign watch daemon shutting down");
    state.save(&agents_dir)?;
    Ok(())
}

/// Check if a notify event is relevant (file modified/created/removed).
fn is_relevant_event(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
    )
}

/// Process all changed files and sync as needed.
///
/// Uses delta_merger for proper add/update/remove detection instead of
/// naive additive merge. Deletions in agent configs propagate to canonical
/// and then to all other agents. Instruction symlink breakage is detected
/// and healed independently.
fn process_changes(
    home: &Path,
    agents_dir: &Path,
    entries: &[WatchEntry],
    state: &mut SyncState,
) -> anyhow::Result<()> {
    let mut changed_canonical = false;
    let mut changed_agents: Vec<(String, AgentType, PathBuf)> = Vec::new();
    let mut deleted_agents: Vec<(String, AgentType, PathBuf)> = Vec::new();
    let mut instr_events: Vec<String> = Vec::new();
    let mut skills_events = false;
    let mut rules_events = false;

    // Detect which files changed (including deleted)
    for entry in entries {
        if entry.id == "canonical-instructions" {
            if !state.is_unchanged(&entry.id, &entry.path) {
                eprintln!("[watch] canonical instructions changed -> symlinks already reflect; regenerating rules");
                rules_events = true;
                state.update_hash(&entry.id, &entry.path);
            }
        } else if entry.id == "canonical-skills" {
            if !state.is_unchanged(&entry.id, &entry.path) {
                eprintln!("[watch] canonical skills changed -> healing symlinks");
                skills_events = true;
                state.update_hash(&entry.id, &entry.path);
            }
        } else if entry.id.starts_with(INSTRUCTION_PREFIX) {
            if !entry.path.exists() || !state.is_unchanged(&entry.id, &entry.path) {
                let agent = entry
                    .id
                    .strip_prefix(INSTRUCTION_PREFIX)
                    .unwrap_or(&entry.id)
                    .to_string();
                instr_events.push(agent);
            }
        } else if entry.id.starts_with(SKILLS_PREFIX) {
            if !state.is_unchanged(&entry.id, &entry.path) {
                eprintln!("[watch] agent skills dir changed -> healing");
                skills_events = true;
                state.update_hash(&entry.id, &entry.path);
            }
        } else if entry.id.starts_with(RULES_PREFIX) {
            if !state.is_unchanged(&entry.id, &entry.path) {
                eprintln!("[watch] rules dir changed -> regenerating");
                rules_events = true;
                state.update_hash(&entry.id, &entry.path);
            }
        } else if entry.id == "canonical" {
            if !entry.path.exists() || !state.is_unchanged(&entry.id, &entry.path) {
                if entry.path.exists() {
                    changed_canonical = true;
                } else {
                    eprintln!("[watch] WARNING: canonical config deleted -> skipping");
                }
            }
        } else if let Some(agent_type) = entry.agent_type {
            if !entry.path.exists() {
                deleted_agents.push((entry.id.clone(), agent_type, entry.path.clone()));
            } else if !state.is_unchanged(&entry.id, &entry.path) {
                changed_agents.push((entry.id.clone(), agent_type, entry.path.clone()));
            }
        }
    }

    // Heal broken instruction symlinks
    for agent in &instr_events {
        eprintln!("[watch] instruction symlink for {} changed -> healing", agent);
        if let Err(e) = instructions::heal_one(home, agent) {
            eprintln!("  instruction heal error for {}: {}", agent, e);
        }
    }

    // Heal skills if needed
    if skills_events {
        match skills::heal_all(home) {
            Ok(fixed) => {
                if fixed > 0 {
                    eprintln!("  skills symlinks healed: {}", fixed);
                }
            }
            Err(e) => {
                eprintln!("  skills heal error: {}", e);
            }
        }
    }

    // Regenerate Cursor + Claude rules if AGENTS.md changed
    if rules_events {
        match rules::sync_rules(home, false) {
            Ok(fixed) => {
                if fixed > 0 {
                    eprintln!("  rules regenerated: {}", fixed);
                }
            }
            Err(e) => {
                eprintln!("  rules sync error: {}", e);
            }
        }
    }

    // Recreate any deleted agent configs from canonical
    if !deleted_agents.is_empty() {
        let canonical_raw = std::fs::read_to_string(agents_dir.join("mcp_config.json"))?;
        let canonical: CanonicalWorkspaceState = serde_json::from_str(&canonical_raw)?;
        for (id, _agent_type, _path) in &deleted_agents {
            eprintln!("[watch] {} config deleted -> recreating from canonical", id);
        }
        if !changed_canonical && changed_agents.is_empty() && instr_events.is_empty() {
            sync_all_agents(home, agents_dir, &canonical, state)?;
            state.touch();
            state.save(agents_dir)?;
            return Ok(());
        }
    }

    if !changed_canonical && changed_agents.is_empty() && instr_events.is_empty() {
        return Ok(());
    }

    // If only instruction/skills/rules events, no MCP sync needed
    if !changed_canonical && changed_agents.is_empty() && deleted_agents.is_empty() && (!instr_events.is_empty() || skills_events || rules_events) {
        state.touch();
        state.save(agents_dir)?;
        return Ok(());
    }

    // Load canonical
    let canonical_raw = std::fs::read_to_string(agents_dir.join("mcp_config.json"))?;
    let mut canonical: CanonicalWorkspaceState = serde_json::from_str(&canonical_raw)?;

    if changed_canonical {
        eprintln!("[watch] canonical changed -> regenerating all agents");
        sync_all_agents(home, agents_dir, &canonical, state)?;
        state.update_hash("canonical", &agents_dir.join("mcp_config.json"));
    } else {
        let local_entries = load_local_entries(agents_dir);

        for (id, agent_type, path) in &changed_agents {
            eprintln!("[watch] {} changed -> computing delta to canonical", id);
            let raw = std::fs::read_to_string(path)?;
            let strategy = McpFormatFactory::from_agent(*agent_type);

            if let Ok(parsed) = strategy.deserialize_to_canonical(&raw, home) {
                if let Some(agent_servers) = parsed.get("mcp").and_then(|v| v.as_object()) {
                    let canonical_json = serde_json::to_value(&canonical)?;
                    let canonical_servers_json = canonical_json
                        .get("mcp")
                        .cloned()
                        .unwrap_or(serde_json::json!({}));

                    let delta = delta_merger::compute_delta(
                        &canonical_servers_json,
                        &serde_json::Value::Object(agent_servers.clone()),
                        &local_entries,
                    )?;

                    for key in &delta.entries_to_remove {
                        canonical.mcp.remove(key);
                        eprintln!("  - {} (removed from canonical)", key);
                    }

                    for key in &delta.entries_to_add {
                        if let Some(v) = agent_servers.get(key) {
                            let def = serde_json::from_value(v.clone()).unwrap_or_else(|_| {
                                crate::shared::models::McpServerDefinition {
                                    transport: crate::shared::models::TransportType::Local,
                                    command: None,
                                    url: None,
                                    headers: None,
                                    env: None,
                                    enabled: None,
                                    extra: HashMap::new(),
                                }
                            });
                            canonical.mcp.insert(key.clone(), def);
                            eprintln!("  + {} (added to canonical)", key);
                        }
                    }

                    for key in &delta.entries_to_update {
                        // Guard: prevent lossy format-conversion round-trips.
                        // When the watcher reverse-merges an agent config back to canonical,
                        // the agent's serialized form may have lost fields (e.g., OpenCode
                        // splits canonical command:["npx","-y","pkg"] into command:"npx" + args:["-y","pkg"]).
                        // Skip the update only when canonical has richer data (command/url)
                        // AND the incoming agent entry is missing that data — meaning the
                        // agent's format conversion stripped it. This allows legitimate
                        // user edits (e.g. changing a URL) to propagate to canonical.
                        if let Some(existing) = canonical.mcp.get(key) {
                            let canonical_has_data = existing.command.is_some() || existing.url.is_some();
                            if canonical_has_data {
                                // Check if the incoming agent entry is lossy
                                let agent_has_data = agent_servers
                                    .get(key)
                                    .and_then(|v| v.as_object())
                                    .map(|obj| {
                                        obj.contains_key("command")
                                            || obj.contains_key("url")
                                            || obj.contains_key("args")
                                    })
                                    .unwrap_or(false);

                                if !agent_has_data {
                                    eprintln!("  ~ {} (skipped — agent entry is lossy)", key);
                                    continue;
                                }
                            }
                        }
                        if let Some(v) = agent_servers.get(key) {
                            let def = serde_json::from_value(v.clone()).unwrap_or_else(|_| {
                                crate::shared::models::McpServerDefinition {
                                    transport: crate::shared::models::TransportType::Local,
                                    command: None,
                                    url: None,
                                    headers: None,
                                    env: None,
                                    enabled: None,
                                    extra: HashMap::new(),
                                }
                            });
                            canonical.mcp.insert(key.clone(), def);
                            eprintln!("  ~ {} (updated in canonical)", key);
                        }
                    }
                }
            } else {
                eprintln!("  warning: failed to parse {} config, skipping delta", id);
            }
        }

        // Write updated canonical
        let canonical_json = serde_json::to_string_pretty(&canonical)?;
        std::fs::write(agents_dir.join("mcp_config.json"), &canonical_json)?;
        state.update_hash_from_bytes("canonical", canonical_json.as_bytes());

        // Regenerate all agents EXCEPT the ones that changed
        let skip: HashSet<String> =
            changed_agents.iter().map(|(id, _, _)| id.clone()).collect();
        sync_selected_agents(home, agents_dir, &canonical, state, &skip)?;
    }

    state.touch();
    state.save(agents_dir)?;
    Ok(())
}

/// Sync all agents from canonical with transaction tracking.
fn sync_all_agents(
    home: &Path,
    agents_dir: &Path,
    canonical: &CanonicalWorkspaceState,
    state: &mut SyncState,
) -> anyhow::Result<()> {
    let descriptors = AgentRegistry::synced_agents(home);
    let skip_map = config::load_agent_skip(agents_dir).unwrap_or_else(|e| {
        eprintln!("[watch] failed to load agent_skip.json: {}", e);
        std::collections::HashMap::new()
    });

    for descriptor in &descriptors {
        let filtered_mcp = config::filter_skipped(&canonical.mcp, descriptor.label, &skip_map);
        let filtered = CanonicalWorkspaceState { mcp: filtered_mcp };
        let state_json = serde_json::to_value(&filtered)?;
        let strategy = McpFormatFactory::from_agent(descriptor.agent_type);
        let target_path = &descriptor.config_path;
        let id = descriptor.agent_type.as_str();

        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("Failed to create dir for {}", id))?;
        }

        match strategy.serialize_from_canonical(&state_json, home) {
            Ok(output) => {
                let output = crate::sync::overlay::overlay_onto_existing(&output, target_path);
                if should_write(target_path, &output) {
                    match transaction::create_transaction(id, target_path) {
                        Ok(tx) => {
                            std::fs::write(target_path, &output)?;
                            if let Err(e) = transaction::finalize_transaction(&tx, output.as_bytes()) {
                                eprintln!("  {} tx finalize error: {}", id, e);
                            }
                            eprintln!("  {} -> {}", id, target_path.display());
                        }
                        Err(e) => {
                            eprintln!("  {} tx error: {}", id, e);
                            std::fs::write(target_path, &output)?;
                            eprintln!("  {} -> {} (no tx)", id, target_path.display());
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("  {} serialize error: {}", id, e);
            }
        }
    }

    // Update hashes from actual file contents
    for descriptor in &descriptors {
        let target_path = &descriptor.config_path;
        let id = descriptor.agent_type.as_str();
        state.update_hash(id, target_path);
    }

    Ok(())
}

/// Sync agents except those in the skip set, with transaction tracking.
fn sync_selected_agents(
    home: &Path,
    agents_dir: &Path,
    canonical: &CanonicalWorkspaceState,
    state: &mut SyncState,
    skip: &HashSet<String>,
) -> anyhow::Result<()> {
    let descriptors = AgentRegistry::synced_agents(home);
    let skip_map = config::load_agent_skip(agents_dir).unwrap_or_else(|e| {
        eprintln!("[watch] failed to load agent_skip.json: {}", e);
        std::collections::HashMap::new()
    });

    for descriptor in &descriptors {
        let id = descriptor.agent_type.as_str();

        if skip.contains(id) {
            continue;
        }

        let filtered_mcp = config::filter_skipped(&canonical.mcp, descriptor.label, &skip_map);
        let filtered = CanonicalWorkspaceState { mcp: filtered_mcp };
        let state_json = serde_json::to_value(&filtered)?;
        let strategy = McpFormatFactory::from_agent(descriptor.agent_type);
        let target_path = &descriptor.config_path;

        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("Failed to create dir for {}", id))?;
        }

        match strategy.serialize_from_canonical(&state_json, home) {
            Ok(output) => {
                let output = crate::sync::overlay::overlay_onto_existing(&output, target_path);
                if should_write(target_path, &output) {
                    match transaction::create_transaction(id, target_path) {
                        Ok(tx) => {
                            std::fs::write(target_path, &output)?;
                            if let Err(e) = transaction::finalize_transaction(&tx, output.as_bytes()) {
                                eprintln!("  {} tx finalize error: {}", id, e);
                            }
                            eprintln!("  {} -> {}", id, target_path.display());
                        }
                        Err(e) => {
                            eprintln!("  {} tx error: {}", id, e);
                            std::fs::write(target_path, &output)?;
                            eprintln!("  {} -> {} (no tx)", id, target_path.display());
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("  {} serialize error: {}", id, e);
            }
        }
    }

    // Update hashes from actual file contents for ALL agents including skipped
    for descriptor in &descriptors {
        let target_path = &descriptor.config_path;
        let id = descriptor.agent_type.as_str();
        state.update_hash(id, target_path);
    }

    Ok(())
}

/// Check if file needs writing (content differs or doesn't exist).
fn should_write(path: &Path, new_content: &str) -> bool {
    if !path.exists() {
        return true;
    }
    match std::fs::read_to_string(path) {
        Ok(existing) => existing != new_content,
        Err(_) => true,
    }
}

use anyhow::Context;
