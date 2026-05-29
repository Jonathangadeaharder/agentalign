use agentalign::sync::transaction;
use agentalign::shared::models::{CanonicalWorkspaceState, McpServerDefinition, TransportType};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

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
enum MagicAction {
    /// Enable magic mode (install LaunchAgent)
    On,
    /// Disable magic mode (remove LaunchAgent)
    Off,
    /// Show magic mode status
    Status,
}

/// Locate known agent config directories on this system.
fn discover_agent_configs() -> Vec<(&'static str, PathBuf)> {
    let home = dirs::home_dir().expect("HOME must be set");
    let mut found = Vec::new();

    let paths: Vec<(&str, PathBuf)> = vec![
        ("claude", home.join(".claude").join(".mcp.json")),
        ("cursor", home.join(".cursor").join("mcp.json")),
        (
            "gemini",
            home.join(".gemini").join("config").join("mcp_config.json"),
        ),
        (
            "opencode",
            home.join(".config").join("opencode").join("opencode.json"),
        ),
    ];

    for (name, path) in paths {
        if path.exists() {
            found.push((name, path));
        }
    }

    found
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Migrate { dry_run } => {
            let home = dirs::home_dir().expect("HOME must be set");
            let agents_dir = home.join(".agents");

            if dry_run {
                println!("[DRY RUN] Would scan and migrate agent configs into: {}", agents_dir.display());
            } else {
                fs::create_dir_all(&agents_dir).expect("Failed to create ~/.agents/");
                fs::create_dir_all(agents_dir.join("skills")).ok();
                fs::create_dir_all(agents_dir.join("backups")).ok();
                println!("Created ~/.agents/ directory structure.");
            }

            let discovered = discover_agent_configs();
            if discovered.is_empty() {
                println!("No existing agent configs found. Nothing to migrate.");
                return;
            }

            println!("Discovered {} agent config(s):", discovered.len());
            let mut merged = serde_json::Map::new();

            for (agent, path) in &discovered {
                println!("  {} -> {}", agent, path.display());
                if !dry_run {
                    let raw = fs::read_to_string(path).unwrap_or_default();
                    let agent_type = agentalign::mcp::factory::AgentType::from_name(agent)
                        .expect("Unknown agent type");
                    let strategy = agentalign::mcp::factory::McpFormatFactory::from_agent(agent_type);
                    if let Ok(canonical) = strategy.deserialize_to_canonical(&raw, &home) {
                        if let Some(servers) = canonical
                            .get("mcp")
                            .and_then(|v| v.as_object())
                        {
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
                            let def = serde_json::from_value(v)
                                .unwrap_or_else(|_| McpServerDefinition {
                                    transport: agentalign::shared::models::TransportType::Local,
                                    command: None,
                                    url: None,
                                    headers: None,
                                    env: None,
                                    enabled: None,
                                    extra: HashMap::new(),
                                });
                            (k, def)
                        })
                        .collect(),
                };
                let json = serde_json::to_string_pretty(&canonical)
                    .expect("Failed to serialize canonical config");
                let mcp_path = agents_dir.join("mcp_config.json");
                fs::write(&mcp_path, &json)
                    .expect("Failed to write canonical MCP config");
                println!("Wrote canonical config: {}", mcp_path.display());
                println!("Migration complete. Run `agentalign sync` to push to all agents.");
            }
        }

        Commands::Sync { dry_run } => {
            let home = dirs::home_dir().expect("HOME must be set");
            let agents_dir = home.join(".agents");
            let canonical_path = agents_dir.join("mcp_config.json");

            if !canonical_path.exists() {
                eprintln!(
                    "No canonical config found at {}. Run `agentalign migrate` first.",
                    canonical_path.display()
                );
                return;
            }

            let raw = fs::read_to_string(&canonical_path)
                .expect("Failed to read canonical config");
            let canonical: CanonicalWorkspaceState = serde_json::from_str(&raw)
                .expect("Failed to parse canonical config");

            if dry_run {
                println!("[DRY RUN] Would push canonical config to all configured agents.");
                println!("  Servers in canonical: {}", canonical.mcp.len());
                return;
            }

            // Build output for each agent
            let agents: Vec<(&str, agentalign::mcp::factory::AgentType)> = vec![
                ("Claude", agentalign::mcp::factory::AgentType::Claude),
                ("Cursor", agentalign::mcp::factory::AgentType::Cursor),
                ("Gemini", agentalign::mcp::factory::AgentType::Gemini),
                ("OpenCode", agentalign::mcp::factory::AgentType::OpenCode),
            ];

            for (label, agent_type) in &agents {
                let strategy = agentalign::mcp::factory::McpFormatFactory::from_agent(*agent_type);
                let target_path = strategy.target_config_path(&home);
                let parent = target_path.parent().unwrap();
                fs::create_dir_all(parent).ok();

                // Convert CanonicalWorkspaceState to JsonValue for the strategy
                let state_json = serde_json::to_value(&canonical)
                    .expect("Failed to serialize canonical state");

                    match strategy.serialize_from_canonical(&state_json, &home) {
                    Ok(output) => {
                        // Non-destructive: skip if content unchanged
                        if target_path.exists() {
                            if let Ok(existing) = fs::read_to_string(&target_path) {
                                if existing == output {
                                    println!("  {} -> {} (unchanged, skipped)", label, target_path.display());
                                    continue;
                                }
                            }
                        }

                        // Create transaction and write
                        let tx = transaction::create_transaction(label, &target_path);
                        match tx {
                            Ok(tx) => {
                                fs::write(&target_path, &output)
                                    .expect("Failed to write config");
                                transaction::finalize_transaction(
                                    &tx,
                                    output.as_bytes(),
                                )
                                .ok();
                                println!("  {} -> {} ({} servers)", label, target_path.display(), canonical.mcp.len());
                            }
                            Err(e) => {
                                eprintln!("  {} error: {}", label, e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  {} serialize error: {}", label, e);
                    }
                }
            }
            // Heal instruction file symlinks after MCP sync
            match agentalign::instructions::heal_all(&home) {
                Ok(fixed) => {
                    if fixed > 0 {
                        println!("  instruction symlinks healed: {}", fixed);
                    }
                }
                Err(e) => {
                    eprintln!("  instruction symlink error: {}", e);
                }
            }

            println!("Sync complete.");
        }

        Commands::Add {
            name,
            r#type,
            command,
            url,
            enabled,
            no_sync,
            dry_run,
        } => {
            let home = dirs::home_dir().expect("HOME must be set");
            let agents_dir = home.join(".agents");
            let canonical_path = agents_dir.join("mcp_config.json");

            if !canonical_path.exists() {
                eprintln!("No canonical config. Run `agentalign migrate` first.");
                std::process::exit(1);
            }

            let raw = fs::read_to_string(&canonical_path).expect("Failed to read canonical config");
            let mut canonical: CanonicalWorkspaceState =
                serde_json::from_str(&raw).expect("Failed to parse canonical config");

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
                let json =
                    serde_json::to_string_pretty(&canonical).expect("Failed to serialize");
                fs::write(&canonical_path, &json).expect("Failed to write canonical config");
                println!("Added '{}' to canonical config.", name);

                if !no_sync {
                    // Re-run sync logic inline
                    let agents: Vec<(&str, agentalign::mcp::factory::AgentType)> = vec![
                        ("claude", agentalign::mcp::factory::AgentType::Claude),
                        ("cursor", agentalign::mcp::factory::AgentType::Cursor),
                        ("gemini", agentalign::mcp::factory::AgentType::Gemini),
                        ("opencode", agentalign::mcp::factory::AgentType::OpenCode),
                        ("codex", agentalign::mcp::factory::AgentType::Codex),
                    ];
                    let state_json =
                        serde_json::to_value(&canonical).expect("Failed to serialize state");
                    for (label, agent_type) in &agents {
                        let strategy =
                            agentalign::mcp::factory::McpFormatFactory::from_agent(*agent_type);
                        let target_path = strategy.target_config_path(&home);
                        if let Some(parent) = target_path.parent() {
                            fs::create_dir_all(parent).ok();
                        }
                        match strategy.serialize_from_canonical(&state_json, &home) {
                            Ok(output) => {
                                let tx = transaction::create_transaction(label, &target_path);
                                match tx {
                                    Ok(tx) => {
                                        fs::write(&target_path, &output)
                                            .expect("Failed to write config");
                                        transaction::finalize_transaction(&tx, output.as_bytes())
                                            .ok();
                                        println!(
                                            "  {} -> {} ({} servers)",
                                            label,
                                            target_path.display(),
                                            canonical.mcp.len()
                                        );
                                    }
                                    Err(e) => eprintln!("  {} tx error: {}", label, e),
                                }
                            }
                            Err(e) => eprintln!("  {} serialize error: {}", label, e),
                        }
                    }
                    println!("Sync complete.");
                }
            }
        }

        Commands::Remove {
            name,
            no_sync,
            dry_run,
        } => {
            let home = dirs::home_dir().expect("HOME must be set");
            let agents_dir = home.join(".agents");
            let canonical_path = agents_dir.join("mcp_config.json");

            if !canonical_path.exists() {
                eprintln!("No canonical config. Run `agentalign migrate` first.");
                std::process::exit(1);
            }

            let raw = fs::read_to_string(&canonical_path).expect("Failed to read canonical config");
            let mut canonical: CanonicalWorkspaceState =
                serde_json::from_str(&raw).expect("Failed to parse canonical config");

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
                let json =
                    serde_json::to_string_pretty(&canonical).expect("Failed to serialize");
                fs::write(&canonical_path, &json).expect("Failed to write canonical config");
                println!("Removed '{}' from canonical config.", name);

                if !no_sync {
                    let agents: Vec<(&str, agentalign::mcp::factory::AgentType)> = vec![
                        ("claude", agentalign::mcp::factory::AgentType::Claude),
                        ("cursor", agentalign::mcp::factory::AgentType::Cursor),
                        ("gemini", agentalign::mcp::factory::AgentType::Gemini),
                        ("opencode", agentalign::mcp::factory::AgentType::OpenCode),
                        ("codex", agentalign::mcp::factory::AgentType::Codex),
                    ];
                    let state_json =
                        serde_json::to_value(&canonical).expect("Failed to serialize state");
                    for (label, agent_type) in &agents {
                        let strategy =
                            agentalign::mcp::factory::McpFormatFactory::from_agent(*agent_type);
                        let target_path = strategy.target_config_path(&home);
                        if let Some(parent) = target_path.parent() {
                            fs::create_dir_all(parent).ok();
                        }
                        match strategy.serialize_from_canonical(&state_json, &home) {
                            Ok(output) => {
                                let tx = transaction::create_transaction(label, &target_path);
                                match tx {
                                    Ok(tx) => {
                                        fs::write(&target_path, &output)
                                            .expect("Failed to write config");
                                        transaction::finalize_transaction(&tx, output.as_bytes())
                                            .ok();
                                        println!(
                                            "  {} -> {} ({} servers)",
                                            label,
                                            target_path.display(),
                                            canonical.mcp.len()
                                        );
                                    }
                                    Err(e) => eprintln!("  {} tx error: {}", label, e),
                                }
                            }
                            Err(e) => eprintln!("  {} serialize error: {}", label, e),
                        }
                    }
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

        Commands::Magic { action } => {
            match action {
                MagicAction::On => {
                    if let Err(e) = agentalign::magic::enable() {
                        eprintln!("Failed to enable magic mode: {}", e);
                        std::process::exit(1);
                    }
                }
                MagicAction::Off => {
                    if let Err(e) = agentalign::magic::disable() {
                        eprintln!("Failed to disable magic mode: {}", e);
                        std::process::exit(1);
                    }
                }
                MagicAction::Status => {
                    if let Err(e) = agentalign::magic::status() {
                        eprintln!("Failed to get status: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }

        Commands::Watch => {
            if let Err(e) = agentalign::watch::run_daemon() {
                eprintln!("Watch daemon error: {}", e);
                std::process::exit(1);
            }
        }
    }
}
