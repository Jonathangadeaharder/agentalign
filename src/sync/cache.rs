use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use std::fs;
use toml_edit::{table as tbl_value, value, DocumentMut, InlineTable, Item};

use crate::shared::models::{SyncTransaction, TransactionStatus};

fn cache_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot find home directory")?;
    let dir = home.join(".agents");
    fs::create_dir_all(&dir).context("Failed to create ~/.agents directory")?;
    Ok(dir.join("cache.toml"))
}

/// Load all transactions from the cache file.
pub fn load_cache() -> Result<Vec<SyncTransaction>> {
    let path = cache_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path).context("Failed to read cache.toml")?;
    let doc = content
        .parse::<DocumentMut>()
        .context("Failed to parse cache.toml")?;

    let mut transactions = Vec::new();
    if let Some(txns_table) = doc.get("transactions").and_then(|t| t.as_table()) {
        for (tx_id, entry) in txns_table.iter() {
            if let Some(tx) = parse_transaction_item(tx_id, entry) {
                transactions.push(tx);
            }
        }
    }
    Ok(transactions)
}

fn parse_transaction_item(id: &str, item: &Item) -> Option<SyncTransaction> {
    if let Some(table) = item.as_table() {
        return Some(parse_transaction_table(id, table));
    }

    item.as_inline_table()
        .map(|inline| parse_transaction_inline(id, inline))
}

fn parse_transaction_table(id: &str, table: &toml_edit::Table) -> SyncTransaction {
    SyncTransaction {
        id: id.to_string(),
        timestamp: table
            .get("timestamp")
            .and_then(|v| v.as_integer())
            .unwrap_or(0),
        agent: table
            .get("agent")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        target_path: table
            .get("target_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        backup_path: table
            .get("backup_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        checksum_before: table
            .get("checksum_before")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        checksum_after: table
            .get("checksum_after")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        status: match table.get("status").and_then(|v| v.as_str()) {
            Some("pending") => TransactionStatus::Pending,
            Some("committed") => TransactionStatus::Committed,
            Some("rolled_back") => TransactionStatus::RolledBack,
            _ => TransactionStatus::Pending,
        },
    }
}

fn parse_transaction_inline(id: &str, table: &InlineTable) -> SyncTransaction {
    SyncTransaction {
        id: id.to_string(),
        timestamp: table
            .get("timestamp")
            .and_then(|v| v.as_integer())
            .unwrap_or(0),
        agent: table
            .get("agent")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        target_path: table
            .get("target_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        backup_path: table
            .get("backup_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        checksum_before: table
            .get("checksum_before")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        checksum_after: table
            .get("checksum_after")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        status: match table.get("status").and_then(|v| v.as_str()) {
            Some("pending") => TransactionStatus::Pending,
            Some("committed") => TransactionStatus::Committed,
            Some("rolled_back") => TransactionStatus::RolledBack,
            _ => TransactionStatus::Pending,
        },
    }
}

/// Save a transaction to the cache (insert or update).
pub fn save_transaction(tx: &SyncTransaction) -> Result<()> {
    let path = cache_path()?;
    let mut doc = if path.exists() {
        let content = fs::read_to_string(&path)?;
        content
            .parse::<DocumentMut>()
            .context("Failed to parse cache.toml")?
    } else {
        DocumentMut::new()
    };

    // Ensure [transactions] table exists
    if !doc.contains_key("transactions") {
        doc.insert("transactions", tbl_value());
    }

    // Build inline table for the transaction entry.
    // InlineTable::insert expects Value, not Item.
    let mut inline = InlineTable::new();
    inline.insert("timestamp", tx.timestamp.into());
    inline.insert("agent", tx.agent.as_str().into());
    inline.insert("target_path", tx.target_path.as_str().into());
    inline.insert("backup_path", tx.backup_path.as_str().into());
    inline.insert("checksum_before", tx.checksum_before.as_str().into());
    inline.insert("checksum_after", tx.checksum_after.as_str().into());
    let status_str = match tx.status {
        TransactionStatus::Pending => "pending",
        TransactionStatus::Committed => "committed",
        TransactionStatus::RolledBack => "rolled_back",
    };
    inline.insert("status", status_str.into());

    // Wrap in an Item::Value so we can assign to the document
    doc["transactions"][&tx.id] = Item::Value(toml_edit::Value::InlineTable(inline));

    fs::write(&path, doc.to_string()).context("Failed to write cache.toml")?;
    Ok(())
}

/// Set a string field in a toml_edit::Table, inserting or overwriting.
fn set_str_field(table: &mut toml_edit::Table, key: &str, val: &str) {
    table.insert(key, value(val));
}

fn set_transaction_str_field(entry: &mut Item, key: &str, val: &str) -> Result<()> {
    if let Some(table) = entry.as_table_mut() {
        set_str_field(table, key, val);
        return Ok(());
    }

    if let Some(inline) = entry.as_inline_table_mut() {
        inline.insert(key, val.into());
        return Ok(());
    }

    bail!("Transaction entry is not a table")
}

/// Update the status of a specific transaction in the cache.
pub fn update_transaction_status(id: &str, status: TransactionStatus) -> Result<()> {
    let path = cache_path()?;
    if !path.exists() {
        bail!("Cache file not found — no transactions exist");
    }
    let content = fs::read_to_string(&path)?;
    let mut doc = content
        .parse::<DocumentMut>()
        .context("Failed to parse cache.toml")?;

    let status_str = match status {
        TransactionStatus::Pending => "pending",
        TransactionStatus::Committed => "committed",
        TransactionStatus::RolledBack => "rolled_back",
    };

    if let Some(txns) = doc.get_mut("transactions").and_then(|t| t.as_table_mut()) {
        if let Some(tx_entry) = txns.get_mut(id) {
            set_transaction_str_field(tx_entry, "status", status_str)
                .with_context(|| format!("Transaction entry '{}' is not a table", id))?;
        } else {
            bail!("Transaction '{}' not found in cache", id);
        }
    } else {
        bail!("No transactions table in cache");
    }

    fs::write(&path, doc.to_string()).context("Failed to write cache.toml")?;
    Ok(())
}

/// Update the checksum_after field of a transaction in the cache.
pub fn update_checksum_after(id: &str, checksum: &str) -> Result<()> {
    let path = cache_path()?;
    if !path.exists() {
        bail!("Cache file not found");
    }
    let content = fs::read_to_string(&path)?;
    let mut doc = content
        .parse::<DocumentMut>()
        .context("Failed to parse cache.toml")?;

    if let Some(txns) = doc.get_mut("transactions").and_then(|t| t.as_table_mut()) {
        if let Some(tx_entry) = txns.get_mut(id) {
            set_transaction_str_field(tx_entry, "checksum_after", checksum)
                .with_context(|| format!("Transaction entry '{}' is not a table", id))?;
        } else {
            bail!("Transaction '{}' not found in cache", id);
        }
    } else {
        bail!("No transactions table in cache");
    }

    fs::write(&path, doc.to_string()).context("Failed to write cache.toml")?;
    Ok(())
}

/// Clear the entire cache file.
pub fn clear_cache() -> Result<()> {
    let path = cache_path()?;
    if path.exists() {
        fs::remove_file(&path).context("Failed to remove cache.toml")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inline_transaction() -> Item {
        let mut inline = InlineTable::new();
        inline.insert("timestamp", 42.into());
        inline.insert("agent", "Codex".into());
        inline.insert("target_path", "/tmp/codex.toml".into());
        inline.insert("backup_path", "/tmp/codex.bak".into());
        inline.insert("checksum_before", "before".into());
        inline.insert("checksum_after", "".into());
        inline.insert("status", "pending".into());
        Item::Value(toml_edit::Value::InlineTable(inline))
    }

    #[test]
    fn parses_saved_inline_transaction_entries() {
        let tx = parse_transaction_item("tx-1", &inline_transaction()).unwrap();

        assert_eq!(tx.id, "tx-1");
        assert_eq!(tx.agent, "Codex");
        assert_eq!(tx.timestamp, 42);
        assert_eq!(tx.status, TransactionStatus::Pending);
    }

    #[test]
    fn updates_saved_inline_transaction_entries() {
        let mut entry = inline_transaction();

        set_transaction_str_field(&mut entry, "checksum_after", "after").unwrap();
        set_transaction_str_field(&mut entry, "status", "committed").unwrap();

        let tx = parse_transaction_item("tx-1", &entry).unwrap();
        assert_eq!(tx.checksum_after, "after");
        assert_eq!(tx.status, TransactionStatus::Committed);
    }
}
