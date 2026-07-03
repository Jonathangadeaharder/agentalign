use agentalign::mcp::factory::{AgentRegistry, McpFormatFactory};
use agentalign::shared::config;
use agentalign::shared::models::{CanonicalWorkspaceState, McpServerDefinition, TransportType};
use agentalign::sync::transaction;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Parser)]
#[command(name = "agentalign", about = "Agent Configuration Unification Engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan existing agent configs into ~/.agents/
    Migrate {
        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Push canonical config to all agents
    Sync {
        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Manage canonical subagent definitions
    Agents {
        #[command(subcommand)]
        action: AgentAction,
    },
    /// Add an MCP server to canonical config and propagate
    Add {
        /// Server name
        name: String,

        /// Server type: local or remote
        #[arg(long, default_value = "local")]
        r#type: String,

        /// Command for local servers (e.g., "npx @pkg/mcp")
        #[arg(long)]
        command: Option<String>,

        /// URL for remote servers
        #[arg(long)]
        url: Option<String>,

        /// Whether server is enabled
        #[arg(long, default_value = "true")]
        enabled: bool,

        /// Don't sync to agents after adding
        #[arg(long)]
        no_sync: bool,

        /// Preview without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove an MCP server from canonical config and propagate
    Remove {
        /// Server name to remove
        name: String,

        /// Don't sync to agents after removing
        #[arg(long)]
        no_sync: bool,

        /// Preview removal without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Roll back the last sync transaction
    Restore {
        /// Rollback specific agent (all agents if omitted)
        #[arg(long)]
        agent: Option<String>,

        /// Rollback specific transaction by UUID
        #[arg(long)]
        id: Option<String>,

        /// Show transaction history
        #[arg(long)]
        list: bool,
    },
    /// Toggle automatic bidirectional sync (magic mode)
    Magic {
        #[command(subcommand)]
        action: MagicAction,
    },
    /// Run the file watcher daemon (used by LaunchAgent)
    Watch,
}

#[derive(Subcommand)]
enum AgentAction {
    /// List canonical subagent definitions
    List,
    /// Sync subagent definitions to all tools
    Sync {
        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum MagicAction {
    /// Enable magic mode (install LaunchAgent)
    On,
    /// Disable magic mode (remove LaunchAgent)
    Off,
    /// Show magic mode status
    Status,
}

/// Re-merge local-entry servers from the existing agent config file after
/// serialization. This prevents sync from removing agent-local servers that
/// are not in canonical but are in local_entries.json.
///
/// Handles both JSON (mcp/mcpServers keys) and TOML (Codex) configs.
fn preserve_local_entries(
    output: &str,
    target_path: &Path,
    local_entries: &std::collections::HashSet<String>,
) -> String {
    if local_entries.is_empty() || !target_path.exists() {
        return output.to_string();
    }

    let extension = target_path.extension().and_then(|e| e.to_str()).unwrap_or("");

    match extension {
        "toml" => preserve_local_entries_toml(output, target_path, local_entries),
        _ => preserve_local_entries_json(output, target_path, local_entries),
    }
}

/// Preserve local entries for JSON configs (Claude, Cursor, Gemini, OpenCode, Antigravity).
fn preserve_local_entries_json(
    output: &str,
    target_path: &Path,
    local_entries: &std::collections::HashSet<String>,
) -> String {
    let existing: serde_json::Value = match fs::read_to_string(target_path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or(serde_json::json!({})),
        Err(_) => return output.to_string(),
    };

    let mut new_doc: serde_json::Value =
        serde_json::from_str(output).unwrap_or(serde_json::json!({}));

    // Try both "mcp" and "mcpServers" keys
    for key in &["mcp", "mcpServers"] {
        if let Some(existing_servers) = existing.get(key).and_then(|v| v.as_object()) {
            if let Some(new_servers) = new_doc.get_mut(key).and_then(|v| v.as_object_mut()) {
                for (name, value) in existing_servers {
                    if local_entries.contains(name) && !new_servers.contains_key(name) {
                        new_servers.insert(name.clone(), value.clone());
                    }
                }
            }
        }
    }

    serde_json::to_string_pretty(&new_doc).unwrap_or_else(|_| output.to_string())
}

/// Preserve local entries for TOML configs (Codex).
/// Parses the existing TOML, extracts local-entry servers from the [mcp] table,
/// and re-merges them into the serialized output.
fn preserve_local_entries_toml(
    output: &str,
    target_path: &Path,
    local_entries: &std::collections::HashSet<String>,
) -> String {
    let existing_raw = match fs::read_to_string(target_path) {
        Ok(raw) => raw,
        Err(_) => return output.to_string(),
    };

    let existing_doc: toml_edit::DocumentMut = existing_raw.parse().unwrap_or_default();

    let mut output_doc: toml_edit::DocumentMut = output.parse().unwrap_or_else(|_| toml_edit::DocumentMut::new());

    // Find the [mcp] table in existing, re-merge local entries into output
    if let Some(existing_mcp) = existing_doc.get("mcp").and_then(|v| v.as_table()) {
        if let Some(output_mcp) = output_doc.get_mut("mcp").and_then(|v| v.as_table_mut()) {
            for (name, value) in existing_mcp {
                if local_entries.contains(name) && !output_mcp.contains_key(name) {
                    output_mcp.insert(name, value.clone());
                }
            }
        }
    }

    output_doc.to_string()
}

/// Push canonical config to all synced agents with transaction tracking.
///
/// Single shared implementation used by `sync`, `add`, and `remove` commands.
/// Applies agent_skip filtering and preserves local entries.
fn push_to_agents(canonical: &CanonicalWorkspaceState, home: &Path) -> Result<()> {
    push_to_agents_impl(canonical, home, false)
}

/// Push canonical config to all synced agents with dry-run support.
fn push_to_agents_impl(
    canonical: &CanonicalWorkspaceState,
    home: &Path,
    dry_run: bool,
) -> Result<()> {
    let agents = AgentRegistry::synced_agents(home);
    let agents_dir = home.join(".agents");
    let local_entries = config::load_local_entries(&agents_dir)?;
    let skip_map = config::load_agent_skip(&agents_dir)?;

    for descriptor in &agents {
        // Filter out skipped servers for this agent
        let filtered = config::filter_skipped(&canonical.mcp, descriptor.label, &skip_map);
        let filtered_canonical = CanonicalWorkspaceState { mcp: filtered };
        let strategy = McpFormatFactory::from_agent(descriptor.agent_type);
        let target_path = &descriptor.config_path;

        // Validate before writing
        if let Err(e) = strategy.validate(&filtered_canonical) {
            eprintln!("  {} validation error: {}", descriptor.label, e);
            continue;
        }

        let state_json = serde_json::to_value(&filtered_canonical)
            .context("Failed to serialize canonical state")?;

        match strategy.serialize_from_canonical(&state_json, home) {
            Ok(output) => {
                // Dry-run: preview only, no filesystem mutations
                if dry_run {
                    println!(
                        "  {} -> {} ({} servers) [DRY RUN]",
                        descriptor.label,
                        target_path.display(),
                        filtered_canonical.mcp.len()
                    );
                    continue;
                }

                // Create parent dirs (not in dry-run)
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("Failed to create dir for {}", descriptor.label))?;
                }

                // Preserve local entries from existing file
                let output = preserve_local_entries(&output, target_path, &local_entries);

                // Non-destructive: skip if content unchanged
                if target_path.exists() {
                    if let Ok(existing) = fs::read_to_string(target_path) {
                        if existing == output {
                            println!(
                                "  {} -> {} (unchanged, skipped)",
                                descriptor.label,
                                target_path.display()
                            );
                            continue;
                        }
                    }
                }

                // Create transaction and write
                match transaction::create_transaction(descriptor.label, target_path) {
                    Ok(tx) => {
                        fs::write(target_path, &output)
                            .with_context(|| format!("Failed to write {}", target_path.display()))?;
                        if let Err(e) = transaction::finalize_transaction(&tx, output.as_bytes()) {
                            eprintln!("  {} tx finalize error: {}", descriptor.label, e);
                        }
                        println!(
                            "  {} -> {} ({} servers)",
                            descriptor.label,
                            target_path.display(),
                            filtered_canonical.mcp.len()
                        );
                    }
                    Err(e) => {
                        eprintln!("  {} tx error: {}", descriptor.label, e);
                    }
                }
            }
            Err(e) => {
                eprintln!("  {} serialize error: {}", descriptor.label, e);
            }
        }
    }

    // Heal instruction file symlinks (skip in dry-run)
    if !dry_run {
        match agentalign::instructions::heal_all(home) {
            Ok(fixed) => {
                if fixed > 0 {
                    println!("  instruction symlinks healed: {}", fixed);
                }
            }
            Err(e) => {
                eprintln!("  instruction symlink error: {}", e);
            }
        }

        // Heal skills symlinks
        match agentalign::skills::heal_all(home) {
            Ok(fixed) => {
                if fixed > 0 {
                    println!("  skills symlinks healed: {}", fixed);
                }
            }
            Err(e) => {
                eprintln!("  skills symlink error: {}", e);
            }
        }
    }

    // Sync subagent definitions
    match agentalign::agents::sync_agents(home, dry_run) {
        Ok(count) => {
            if count > 0 {
                println!("  agents synced: {}", count);
            }
        }
        Err(e) => {
            eprintln!("  agent sync error: {}", e);
        }
    }

    // Sync AGENTS.md sections into Cursor + Claude rules
    match agentalign::rules::sync_rules(home, dry_run) {
        Ok(count) => {
            if count > 0 {
                println!("  rules synced: {}", count);
            }
        }
        Err(e) => {
            eprintln!("  rules sync error: {}", e);
        }
    }

    Ok(())
}

/// Load and parse the canonical config from disk.
fn load_canonical(home: &Path) -> Result<CanonicalWorkspaceState> {
    let canonical_path = home.join(".agents").join("mcp_config.json");
    if !canonical_path.exists() {
        anyhow::bail!(
            "No canonical config at {}. Run `agentalign migrate` first.",
            canonical_path.display()
        );
    }
    let raw = fs::read_to_string(&canonical_path)
        .context("Failed to read canonical config")?;
    let canonical: CanonicalWorkspaceState = serde_json::from_str(&raw)
        .context("Failed to parse canonical config")?;
    Ok(canonical)
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let home = dirs::home_dir().context("HOME environment variable must be set")?;

    match cli.command {
        Commands::Migrate { dry_run } => {
            let agents_dir = home.join(".agents");

            if dry_run {
                println!(
                    "[DRY RUN] Would scan and migrate agent configs into: {}",
                    agents_dir.display()
                );
            } else {
                fs::create_dir_all(&agents_dir).context("Failed to create ~/.agents/")?;
                fs::create_dir_all(agents_dir.join("skills")).ok();
                fs::create_dir_all(agents_dir.join("backups")).ok();
                println!("Created ~/.agents/ directory structure.");
            }

            let discovered = AgentRegistry::discovered_agents(&home);
            if discovered.is_empty() {
                println!("No existing agent configs found. Nothing to migrate.");
                return Ok(());
            }

            println!("Discovered {} agent config(s):", discovered.len());
            let mut merged = serde_json::Map::new();

            for descriptor in &discovered {
                println!(
                    "  {} -> {}",
                    descriptor.label,
                    descriptor.config_path.display()
                );
                if !dry_run {
                    let raw = fs::read_to_string(&descriptor.config_path).unwrap_or_default();
                    let strategy = McpFormatFactory::from_agent(descriptor.agent_type);
                    if let Ok(canonical) = strategy.deserialize_to_canonical(&raw, &home) {
                        if let Some(servers) = canonical.get("mcp").and_then(|v| v.as_object()) {
                            for (k, v) in servers {
                                merged.insert(k.clone(), v.clone());
                            }
                        }
                    }
                }
            }

            if !dry_run {
                let canonical = CanonicalWorkspaceState {
                    mcp: merged
                        .into_iter()
                        .map(|(k, v)| {
                            let def = serde_json::from_value(v).unwrap_or_else(|_| {
                                McpServerDefinition {
                                    transport: TransportType::Local,
                                    command: None,
                                    url: None,
                                    headers: None,
                                    env: None,
                                    enabled: None,
                                    extra: HashMap::new(),
                                }
                            });
                            (k, def)
                        })
                        .collect(),
                };
                let json = serde_json::to_string_pretty(&canonical)
                    .context("Failed to serialize canonical config")?;
                let mcp_path = agents_dir.join("mcp_config.json");
                fs::write(&mcp_path, &json)
                    .context("Failed to write canonical MCP config")?;
                println!("Wrote canonical config: {}", mcp_path.display());
                println!(
                    "Migration complete. Run `agentalign sync` to push to all agents."
                );
            }

            // Collect orphan skills from tool directories into canonical store
            if dry_run {
                println!("\n[DRY RUN] Would scan for orphan skills in tool directories...");
            } else {
                match agentalign::skills::collect_orphan_skills(&home) {
                    Ok(count) if count > 0 => {
                        println!("Collected {} orphan skill(s) into canonical store.", count);
                    }
                    Ok(_) => {
                        println!("No orphan skills found.");
                    }
                    Err(e) => {
                        eprintln!("Skill collection error: {}", e);
                    }
                }
            }
        }

        Commands::Sync { dry_run } => {
            let canonical = load_canonical(&home)?;

            if dry_run {
                println!("[DRY RUN] Would push canonical config to all configured agents.");
                println!("  Servers in canonical: {}", canonical.mcp.len());
                push_to_agents_impl(&canonical, &home, true)?;
                return Ok(());
            }

            push_to_agents(&canonical, &home)?;
            println!("Sync complete.");
        }

        Commands::Agents { action } => match action {
            AgentAction::List => {
                let agents = agentalign::agents::canonical::load_all_agents(&home)?;
                if agents.is_empty() {
                    println!("No canonical agents found in ~/.agents/agents/");
                } else {
                    for agent in &agents {
                        println!(
                            "  {} — {} (model: {})",
                            agent.name,
                            agent.frontmatter.description,
                            agent.frontmatter.model.as_deref().unwrap_or("default")
                        );
                    }
                }
            }
            AgentAction::Sync { dry_run } => {
                let count = agentalign::agents::sync_agents(&home, dry_run)?;
                if dry_run {
                    println!("[DRY RUN] Would sync agents to all tools.");
                } else {
                    println!("Agents synced: {}", count);
                }
            }
        },

        Commands::Add {
            name,
            r#type,
            command,
            url,
            enabled,
            no_sync,
            dry_run,
        } => {
            let mut canonical = load_canonical(&home)?;

            if canonical.mcp.contains_key(&name) {
                eprintln!("Server '{}' already exists in canonical config.", name);
                std::process::exit(1);
            }

            let transport = match r#type.as_str() {
                "local" => TransportType::Local,
                "remote" => TransportType::Remote,
                other => {
                    eprintln!("Unknown type '{}'. Use 'local' or 'remote'.", other);
                    std::process::exit(1);
                }
            };

            let def = McpServerDefinition {
                transport,
                command: command.map(|c| c.split_whitespace().map(String::from).collect()),
                url,
                headers: None,
                env: None,
                enabled: Some(enabled),
                extra: HashMap::new(),
            };

            println!("+ {} ({})", name, r#type);

            if dry_run {
                println!("[DRY RUN] Would add server '{}' to canonical config.", name);
            } else {
                canonical.mcp.insert(name.clone(), def);
                let json = serde_json::to_string_pretty(&canonical)
                    .context("Failed to serialize")?;
                let canonical_path = home.join(".agents").join("mcp_config.json");
                fs::write(&canonical_path, &json)
                    .context("Failed to write canonical config")?;
                println!("Added '{}' to canonical config.", name);

                if !no_sync {
                    push_to_agents(&canonical, &home)?;
                    println!("Sync complete.");
                }
            }
        }

        Commands::Remove {
            name,
            no_sync,
            dry_run,
        } => {
            let mut canonical = load_canonical(&home)?;

            if !canonical.mcp.contains_key(&name) {
                eprintln!("Server '{}' not found in canonical config.", name);
                std::process::exit(1);
            }

            println!("- {}", name);

            if dry_run {
                println!(
                    "[DRY RUN] Would remove server '{}' from canonical config.",
                    name
                );
            } else {
                canonical.mcp.remove(&name);
                let json = serde_json::to_string_pretty(&canonical)
                    .context("Failed to serialize")?;
                let canonical_path = home.join(".agents").join("mcp_config.json");
                fs::write(&canonical_path, &json)
                    .context("Failed to write canonical config")?;
                println!("Removed '{}' from canonical config.", name);

                if !no_sync {
                    push_to_agents(&canonical, &home)?;
                    println!("Sync complete.");
                }
            }
        }

        Commands::Restore { agent, id, list } => {
            if list {
                match transaction::handle_list(agent.as_deref()) {
                    Ok(transactions) => {
                        if transactions.is_empty() {
                            println!("No transactions found.");
                        } else {
                            println!(
                                "{:<38} {:<10} {:<12} {:<50} {:<10}",
                                "ID", "Agent", "Timestamp", "Target Path", "Status"
                            );
                            println!("{}", "-".repeat(130));
                            for tx in &transactions {
                                let status = match tx.status {
                                    agentalign::shared::models::TransactionStatus::Pending => {
                                        "pending"
                                    }
                                    agentalign::shared::models::TransactionStatus::Committed => {
                                        "committed"
                                    }
                                    agentalign::shared::models::TransactionStatus::RolledBack => {
                                        "rolled_back"
                                    }
                                };
                                println!(
                                    "{:<38} {:<10} {:<12} {:<50} {:<10}",
                                    tx.id, tx.agent, tx.timestamp, tx.target_path, status
                                );
                            }
                        }
                    }
                    Err(e) => eprintln!("Error listing transactions: {}", e),
                }
            } else if let Some(tx_id) = id {
                match transaction::handle_rollback_by_id(&tx_id) {
                    Ok(()) => println!("Done."),
                    Err(e) => eprintln!("Error: {}", e),
                }
            } else {
                match transaction::handle_rollback(agent.as_deref()) {
                    Ok(count) => {
                        if count > 0 {
                            println!("Rolled back {} transaction(s).", count);
                        } else {
                            println!("No transactions to roll back.");
                        }
                    }
                    Err(e) => eprintln!("Error: {}", e),
                }
            }
        }

        Commands::Magic { action } => match action {
            MagicAction::On => {
                agentalign::magic::enable()?;
            }
            MagicAction::Off => {
                agentalign::magic::disable()?;
            }
            MagicAction::Status => {
                agentalign::magic::status()?;
            }
        },

        Commands::Watch => {
            agentalign::watch::run_daemon()?;
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}
