use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::fs;
use uuid::Uuid;

use agentalign_shared::models::{SyncTransaction, TransactionStatus};

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

    // Read original file and compute SHA-256 checksum
    let original_content = if target_path.exists() {
        fs::read(target_path).context("Failed to read target file for backup")?
    } else {
        Vec::new()
    };
    let checksum_before = hex::encode(Sha256::digest(&original_content));

    // Create backup directory
    let home = dirs::home_dir().context("Cannot find home directory")?;
    let backup_dir = home.join(".agents").join("backups");
    fs::create_dir_all(&backup_dir).context("Failed to create backup directory")?;
    let backup_filename = format!("{}_{}.bak", agent, timestamp);
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

/// Roll back a transaction: restore the original file from backup,
/// verify backup integrity via SHA-256, and mark as RolledBack.
pub fn rollback_transaction(tx: &SyncTransaction) -> Result<()> {
    let backup_path = Path::new(&tx.backup_path);

    // If no backup exists and original was empty (never existed), just mark rolled back
    if tx.checksum_before.is_empty() && tx.checksum_after.is_empty() && !backup_path.exists() {
        cache::update_transaction_status(&tx.id, TransactionStatus::RolledBack)?;
        return Ok(());
    }

    if !backup_path.exists() {
        bail!(
            "Backup file not found: {} — cannot roll back transaction {}",
            tx.backup_path,
            tx.id
        );
    }

    let backup_content =
        fs::read(backup_path).context("Failed to read backup file for rollback")?;

    // Verify backup integrity: checksum must match checksum_before
    let backup_checksum = hex::encode(Sha256::digest(&backup_content));
    if backup_checksum != tx.checksum_before {
        bail!(
            "Backup checksum mismatch for transaction {}: expected {}, got {}",
            tx.id,
            tx.checksum_before,
            backup_checksum
        );
    }

    // Restore the original file
    let target_path = Path::new(&tx.target_path);
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).context("Failed to create parent directories for restore")?;
    }
    fs::write(target_path, &backup_content).context("Failed to restore original file")?;

    // Mark as rolled back
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
    all.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(all.into_iter().next())
}

/// Get transaction history for an agent, newest first.
pub fn get_transaction_history(agent: &str) -> Result<Vec<SyncTransaction>> {
    let mut all = cache::load_cache()?;
    all.retain(|tx| tx.agent == agent);
    all.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(all)
}

/// Get all transactions (across all agents), newest first.
pub fn get_all_transactions() -> Result<Vec<SyncTransaction>> {
    let mut all = cache::load_cache()?;
    all.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
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
        all.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
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
