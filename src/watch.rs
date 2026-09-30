//! File watcher daemon for bidirectional magic sync.
//!
//! Uses the `notify` crate with FSEvents on macOS to watch all agent config
//! paths. On change detection, debounces 500ms, then determines if the change
//! was a user edit (sync to all agents) or our own write (skip).
//!
//! Canonical and agent config changes are reconciled through the same sync path.

use crate::instructions;
use crate::mcp::factory::{AgentRegistry, AgentType};
use crate::rules;
use crate::skills;
use crate::state::SyncState;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
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

    // Rules directories — detect manual edits and deletes so they get
    // regenerated from AGENTS.md like skills and instructions do.
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
    entries.push(WatchEntry {
        id: format!("{}copilot", RULES_PREFIX),
        agent_type: None,
        path: rules::copilot_rules_dir(home),
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
    crate::sync::reconcile::sync(&home, false)?;

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

    if changed_canonical || !changed_agents.is_empty() || !deleted_agents.is_empty() {
        crate::sync::reconcile::sync(home, false)?;
        for entry in entries {
            state.update_hash(&entry.id, &entry.path);
        }
        state.touch();
        state.save(agents_dir)?;
        return Ok(());
    }

    state.touch();
    state.save(agents_dir)?;
    Ok(())
}
