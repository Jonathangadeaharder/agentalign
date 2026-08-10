use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::fs;
use uuid::Uuid;

use crate::shared::models::{SyncTransaction, TransactionStatus};

use super::cache;

/// Create a new transaction: backup the original file, compute checksum_before,
/// and record the transaction as Pending in the cache.
///
/// After calling this, the caller should write the new file content,
/// then call `finalize_transaction` (with checksum_after) or `rollback_transaction`.
pub fn create_transaction(agent: &str, target_path: &Path) -> Result<SyncTransaction> {
    let id = Uuid::new_v4().to_string();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("Time went backwards")?
        .as_secs() as i64;

    // Read original file and compute SHA-256 checksum. Record existence
    // explicitly: an empty `original_content` is ambiguous (the file may have
    // been absent OR present-but-empty), and rollback needs to tell them apart.
    let existed = target_path.exists();
    let original_content = if existed {
        fs::read(target_path).context("Failed to read target file for backup")?
    } else {
        Vec::new()
    };
    let checksum_before = hex::encode(Sha256::digest(&original_content));

    // Create backup directory
    let home = crate::shared::home_dir()?;
    let backup_dir = home.join(".agents").join("backups");
    fs::create_dir_all(&backup_dir).context("Failed to create backup directory")?;
    // Use the transaction UUID in the backup filename to guarantee uniqueness.
    let backup_filename = format!("{}_{}.bak", agent, id);
    let backup_path = backup_dir.join(&backup_filename);

    // Write backup of original file (only if it existed)
    if !original_content.is_empty() {
        fs::write(&backup_path, &original_content)
            .context("Failed to write backup file")?;
    }

    let tx = SyncTransaction {
        id,
        timestamp,
        agent: agent.to_string(),
        target_path: target_path.to_string_lossy().to_string(),
        backup_path: backup_path.to_string_lossy().to_string(),
        existed,
        checksum_before,
        checksum_after: String::new(),
        status: TransactionStatus::Pending,
    };

    cache::save_transaction(&tx).context("Failed to save transaction to cache")?;
    Ok(tx)
}

/// Finalize a transaction by recording the checksum_after and marking it Committed.
/// Call this after successfully writing new content to the target file.
pub fn finalize_transaction(tx: &SyncTransaction, written_content: &[u8]) -> Result<()> {
    let checksum_after = hex::encode(Sha256::digest(written_content));

    // Update checksum_after in cache
    cache::update_checksum_after(&tx.id, &checksum_after)
        .context("Failed to update checksum_after in cache")?;

    // Mark as committed
    cache::update_transaction_status(&tx.id, TransactionStatus::Committed)
        .context("Failed to commit transaction")?;

    Ok(())
}

/// Roll back a transaction, restoring the target to its pre-sync state and
/// marking it RolledBack. The pre-sync state is recovered from `tx.existed`
/// and the backup file:
/// - target did not exist before → remove the file the sync created;
/// - target existed but was empty (no backup written) → restore empty content;
/// - target existed with content → restore the backup, verifying its checksum.
pub fn rollback_transaction(tx: &SyncTransaction) -> Result<()> {
    let target_path = Path::new(&tx.target_path);
    let backup_path = Path::new(&tx.backup_path);

    // The target did not exist before this sync: undo the creation.
    if !tx.existed {
        if target_path.exists() {
            fs::remove_file(target_path)
                .context("Failed to remove created file during rollback")?;
        }
        cache::update_transaction_status(&tx.id, TransactionStatus::RolledBack)
            .context("Failed to update transaction status to RolledBack")?;
        return Ok(());
    }

    // The target existed but was empty, so no backup was written: restore empty.
    if !backup_path.exists() {
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)
                .context("Failed to create parent directories for restore")?;
        }
        fs::write(target_path, []).context("Failed to restore empty file")?;
        cache::update_transaction_status(&tx.id, TransactionStatus::RolledBack)
            .context("Failed to update transaction status to RolledBack")?;
        return Ok(());
    }

    // The target existed with content: restore from backup, verifying integrity.
    let backup_content =
        fs::read(backup_path).context("Failed to read backup file for rollback")?;

    let backup_checksum = hex::encode(Sha256::digest(&backup_content));
    if backup_checksum != tx.checksum_before {
        bail!(
            "Backup checksum mismatch for transaction {}: expected {}, got {}",
            tx.id,
            tx.checksum_before,
            backup_checksum
        );
    }

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).context("Failed to create parent directories for restore")?;
    }
    fs::write(target_path, &backup_content).context("Failed to restore original file")?;

    cache::update_transaction_status(&tx.id, TransactionStatus::RolledBack)
        .context("Failed to update transaction status to RolledBack")?;

    Ok(())
}

/// Get the latest (most recent) committed or pending transaction for an agent.
pub fn get_latest_transaction(agent: &str) -> Result<Option<SyncTransaction>> {
    let mut all = cache::load_cache()?;
    all.retain(|tx| tx.agent == agent);
    // Filter to pending or committed only (not already rolled back)
    all.retain(|tx| {
        matches!(tx.status, TransactionStatus::Pending | TransactionStatus::Committed)
    });
    all.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
    Ok(all.into_iter().next())
}

/// Get transaction history for an agent, newest first.
pub fn get_transaction_history(agent: &str) -> Result<Vec<SyncTransaction>> {
    let mut all = cache::load_cache()?;
    all.retain(|tx| tx.agent == agent);
    all.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
    Ok(all)
}

/// Get all transactions (across all agents), newest first.
pub fn get_all_transactions() -> Result<Vec<SyncTransaction>> {
    let mut all = cache::load_cache()?;
    all.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
    Ok(all)
}

/// Rollback the latest transaction for a specific agent (or all agents if None).
/// Returns the number of transactions rolled back.
pub fn handle_rollback(agent: Option<&str>) -> Result<usize> {
    let transactions = if let Some(agent_name) = agent {
        get_latest_transaction(agent_name)?.into_iter().collect()
    } else {
        // Rollback latest for each agent
        let mut all = cache::load_cache()?;
        all.retain(|tx| {
            matches!(tx.status, TransactionStatus::Pending | TransactionStatus::Committed)
        });
        // Group by agent, take latest per agent
        let mut seen = std::collections::HashSet::new();
        all.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
        all.into_iter()
            .filter(|tx| {
                if seen.contains(&tx.agent) {
                    false
                } else {
                    seen.insert(tx.agent.clone());
                    true
                }
            })
            .collect::<Vec<_>>()
    };

    if transactions.is_empty() {
        return Ok(0);
    }

    let count = transactions.len();
    for tx in &transactions {
        rollback_transaction(tx)?;
        println!("  ✓ Rolled back {} (agent: {}, target: {})", tx.id, tx.agent, tx.target_path);
    }

    Ok(count)
}

/// Rollback a specific transaction by its UUID.
pub fn handle_rollback_by_id(tx_id: &str) -> Result<()> {
    let all = cache::load_cache()?;
    let tx = all
        .into_iter()
        .find(|t| t.id == tx_id)
        .ok_or_else(|| anyhow::anyhow!("Transaction '{}' not found", tx_id))?;

    rollback_transaction(&tx)?;
    println!("  ✓ Rolled back transaction {} (agent: {}, target: {})", tx.id, tx.agent, tx.target_path);
    Ok(())
}

/// List transactions, optionally filtered by agent.
pub fn handle_list(agent: Option<&str>) -> Result<Vec<SyncTransaction>> {
    if let Some(agent_name) = agent {
        get_transaction_history(agent_name)
    } else {
        get_all_transactions()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard};
    use tempfile::TempDir;

    /// `create_transaction`/`rollback_transaction` resolve the cache and backup
    /// directories from the process HOME, so round-trip tests redirect HOME to
    /// an isolated sandbox. HOME is process-global, so these tests serialise on
    /// a mutex and restore the original value on drop.
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    struct TempHome {
        _tmp: TempDir,
        home: PathBuf,
        original: Option<OsString>,
        _guard: MutexGuard<'static, ()>,
    }

    impl TempHome {
        fn new() -> Self {
            let guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let tmp = TempDir::new().expect("create temp dir");
            let home = tmp.path().to_path_buf();
            let original = std::env::var_os("HOME");
            std::env::set_var("HOME", &home);
            Self { _tmp: tmp, home, original, _guard: guard }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original {
                Some(home) => std::env::set_var("HOME", home),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    /// Drive a full create → write → finalize → rollback cycle, reloading the
    /// transaction from the cache before rollback so the persisted `existed`
    /// flag (not just the in-memory copy) is exercised.
    fn round_trip(target: &Path, new_content: &[u8]) -> SyncTransaction {
        let tx = create_transaction("TestAgent", target).expect("create transaction");
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(target, new_content).expect("write synced content");
        finalize_transaction(&tx, new_content).expect("finalize transaction");

        let reloaded = get_latest_transaction("TestAgent")
            .expect("load cache")
            .expect("transaction present");
        rollback_transaction(&reloaded).expect("rollback transaction");
        reloaded
    }

    #[test]
    fn rollback_removes_file_created_by_sync() {
        let th = TempHome::new();
        let target = th.home.join(".config").join("agent").join("config.json");
        assert!(!target.exists());

        let tx = round_trip(&target, b"{\"mcp\":{}}");

        assert!(!tx.existed, "target did not exist before sync");
        assert!(!target.exists(), "rollback must remove the created file");
    }

    #[test]
    fn rollback_restores_empty_file_overwritten_by_sync() {
        let th = TempHome::new();
        let target = th.home.join("config.json");
        fs::write(&target, []).expect("create empty target");

        let tx = round_trip(&target, b"{\"mcp\":{}}");

        assert!(tx.existed, "target existed (empty) before sync");
        assert!(target.exists(), "rollback must keep the existing file");
        assert_eq!(fs::read(&target).expect("read target"), b"", "empty content restored");
    }

    #[test]
    fn rollback_restores_original_non_empty_file() {
        let th = TempHome::new();
        let target = th.home.join("config.json");
        fs::write(&target, b"original content").expect("seed original");

        let tx = round_trip(&target, b"overwritten");

        assert!(tx.existed, "target existed before sync");
        assert_eq!(
            fs::read(&target).expect("read target"),
            b"original content",
            "original content restored"
        );
    }
}
