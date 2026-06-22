//! Shared config helpers: local entries protection + agent skip lists.

use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;

const LOCAL_ENTRIES_FILE: &str = "local_entries.json";
const AGENT_SKIP_FILE: &str = "agent_skip.json";

/// Load the local entries protection set from ~/.agents/local_entries.json.
/// Keys listed here are preserved even if absent from canonical.
///
/// Returns an empty set if the file doesn't exist. Returns an error if the
/// file exists but cannot be read or parsed — a corrupt protection list
/// must not silently become an empty set (that would cause data loss).
pub fn load_local_entries(agents_dir: &Path) -> Result<HashSet<String>> {
    let path = agents_dir.join(LOCAL_ENTRIES_FILE);
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let entries: HashSet<String> = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(entries)
}

/// Load the per-agent skip list from ~/.agents/agent_skip.json.
/// Returns a map of agent label → set of server names to skip during sync.
///
/// Returns an empty map if the file doesn't exist. Returns an error if the
/// file exists but cannot be read or parsed.
pub fn load_agent_skip(agents_dir: &Path) -> Result<HashMap<String, HashSet<String>>> {
    let path = agents_dir.join(AGENT_SKIP_FILE);
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let skip_map: HashMap<String, HashSet<String>> = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(skip_map)
}

/// Filter canonical state to exclude servers skipped for a specific agent.
pub fn filter_skipped<T: Clone>(
    canonical_mcp: &std::collections::HashMap<String, T>,
    agent_label: &str,
    skip_map: &HashMap<String, HashSet<String>>,
) -> std::collections::HashMap<String, T> {
    if let Some(skip_set) = skip_map.get(agent_label) {
        let mut filtered = canonical_mcp.clone();
        filtered.retain(|k, _| !skip_set.contains(k));
        filtered
    } else {
        canonical_mcp.clone()
    }
}
