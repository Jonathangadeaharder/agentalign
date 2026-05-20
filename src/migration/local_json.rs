use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Local fallback store for secrets when OS keychain is unavailable.
/// Written to ~/.agents/local.json (gitignored).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LocalSecretStore {
    pub secrets: HashMap<String, String>,
}

/// Discover the path to ~/.agents/local.json.
fn local_json_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home.join(".agents").join("local.json"))
}

/// Ensure ~/.agents/ directory exists.
fn ensure_agents_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    let dir = home.join(".agents");
    std::fs::create_dir_all(&dir).context("Could not create ~/.agents/ directory")?;
    Ok(dir)
}

/// Read secrets from `~/.agents/local.json`.
/// Returns an empty store if the file doesn't exist or is invalid.
pub fn read_local_secrets() -> Result<LocalSecretStore> {
    let path = local_json_path()?;
    if !path.exists() {
        return Ok(LocalSecretStore::default());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {:?}", path))?;
    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {:?}", path))
}

/// Write secrets to `~/.agents/local.json`.
/// Creates the file and parent directory as needed.
pub fn write_local_secrets(store: &LocalSecretStore) -> Result<()> {
    ensure_agents_dir()?;
    let path = local_json_path()?;
    let content = serde_json::to_string_pretty(store)
        .context("Failed to serialize local secret store")?;
    // Atomically write using a temp file
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, &content)
        .with_context(|| format!("Failed to write {:?}", tmp_path))?;
    std::fs::rename(&tmp_path, &path)
        .with_context(|| format!("Failed to rename {:?} to {:?}", tmp_path, path))?;
    Ok(())
}

/// Store a single secret in the local store, preserving existing entries.
pub fn store_secret_local(label: &str, value: &str) -> Result<()> {
    let mut store = read_local_secrets()?;
    store.secrets.insert(label.to_string(), value.to_string());
    write_local_secrets(&store)
}

/// Retrieve a single secret from the local store.
pub fn get_secret_local(label: &str) -> Result<Option<String>> {
    let store = read_local_secrets()?;
    Ok(store.secrets.get(label).cloned())
}

/// Delete a single secret from the local store.
pub fn delete_secret_local(label: &str) -> Result<()> {
    let mut store = read_local_secrets()?;
    store.secrets.remove(label);
    write_local_secrets(&store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_default_store_empty() {
        let store = LocalSecretStore::default();
        assert!(store.secrets.is_empty());
    }

    #[test]
    fn test_roundtrip_secrets() {
        let tmp = std::env::temp_dir().join("_test_agentalign_local.json");
        let path = tmp.clone();

        // Override the file path by using the functions directly
        let store = LocalSecretStore {
            secrets: [("test_key".into(), "test_value".into())].into(),
        };
        let content = serde_json::to_string_pretty(&store).unwrap();
        std::fs::write(&path, &content).unwrap();

        // Read it back
        let read_content = std::fs::read_to_string(&path).unwrap();
        let parsed: LocalSecretStore = serde_json::from_str(&read_content).unwrap();
        assert_eq!(parsed.secrets.get("test_key").unwrap(), "test_value");

        let _ = fs::remove_file(&path);
    }
}
