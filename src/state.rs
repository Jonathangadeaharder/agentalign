//! Sync state tracking for loop prevention.
//!
//! Stores content hashes of all watched files in `~/.agents/.sync_state.json`.
//! When the watcher detects a change, it compares the current hash against the
//! stored hash to determine if the change was caused by our own write (skip)
//! or by a user edit (process).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Sync state persisted to disk.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SyncState {
    /// Map of file identifier → content hash.
    pub hashes: HashMap<String, String>,
    /// ISO 8601 timestamp of last sync.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync: Option<String>,
}

impl SyncState {
    /// Load state from `~/.agents/.sync_state.json`.
    pub fn load(agents_dir: &Path) -> Self {
        let path = agents_dir.join(".sync_state.json");
        if path.exists() {
            fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Save state to disk.
    pub fn save(&self, agents_dir: &Path) -> anyhow::Result<()> {
        let path = agents_dir.join(".sync_state.json");
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&path, json)?;
        Ok(())
    }

    /// Compute SHA-256 hash of file contents.
    pub fn compute_hash(path: &Path) -> Option<String> {
        fs::read(path).ok().map(|bytes| {
            let hash = Sha256::digest(&bytes);
            format!("{:x}", hash)
        })
    }

    /// Check if a file's current hash matches the stored hash.
    pub fn is_unchanged(&self, id: &str, path: &Path) -> bool {
        let current = Self::compute_hash(path);
        let stored = self.hashes.get(id);
        match (current, stored) {
            (Some(c), Some(s)) => c == *s,
            _ => false,
        }
    }

    /// Update stored hash for a file.
    pub fn update_hash(&mut self, id: &str, path: &Path) {
        if let Some(hash) = Self::compute_hash(path) {
            self.hashes.insert(id.to_string(), hash);
        }
    }

    /// Update stored hash from raw content.
    pub fn update_hash_from_bytes(&mut self, id: &str, bytes: &[u8]) {
        let hash = Sha256::digest(bytes);
        self.hashes
            .insert(id.to_string(), format!("{:x}", hash));
    }

    /// Set last sync timestamp to now.
    pub fn touch(&mut self) {
        self.last_sync = Some(chrono::Utc::now().to_rfc3339());
    }
}

/// Get the canonical state path.
pub fn canonical_path(agents_dir: &Path) -> PathBuf {
    agents_dir.join("mcp_config.json")
}
