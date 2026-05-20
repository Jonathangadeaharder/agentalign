pub mod keychain;

use agentalign_shared::error::{AdapterError, Result};

/// Abstraction for secret storage. Production uses OS keychain; tests use in-memory HashMap.
pub trait SecretVault {
    fn store_secret(&self, label: &str, value: &str) -> Result<()>;
    fn get_secret(&self, label: &str) -> Result<String>;
}

/// Production binding to the native OS keyring via keyring crate.
pub struct OsKeyringVault;

impl SecretVault for OsKeyringVault {
    fn store_secret(&self, label: &str, value: &str) -> Result<()> {
        keychain::store_secret(label, value)
            .map_err(|e| AdapterError::Other(e.to_string()))
    }
    fn get_secret(&self, label: &str) -> Result<String> {
        keychain::get_secret(label)
            .map_err(|e| AdapterError::Other(e.to_string()))
    }
}

/// In-memory vault for testing. Never touches the OS keychain.
pub struct InMemoryVault {
    secrets: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl InMemoryVault {
    pub fn new() -> Self {
        Self {
            secrets: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

impl SecretVault for InMemoryVault {
    fn store_secret(&self, label: &str, value: &str) -> Result<()> {
        let mut map = self.secrets.lock().unwrap();
        map.insert(label.to_string(), value.to_string());
        Ok(())
    }
    fn get_secret(&self, label: &str) -> Result<String> {
        let map = self.secrets.lock().unwrap();
        map.get(label)
            .cloned()
            .ok_or_else(|| AdapterError::Other(format!("Secret '{}' not found", label)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_vault_roundtrip() {
        let vault = InMemoryVault::new();
        vault.store_secret("test-key", "test-value").unwrap();
        assert_eq!(vault.get_secret("test-key").unwrap(), "test-value");
    }

    #[test]
    fn test_in_memory_vault_missing_key() {
        let vault = InMemoryVault::new();
        assert!(vault.get_secret("nonexistent").is_err());
    }

    #[test]
    fn test_in_memory_vault_overwrite() {
        let vault = InMemoryVault::new();
        vault.store_secret("key", "v1").unwrap();
        vault.store_secret("key", "v2").unwrap();
        assert_eq!(vault.get_secret("key").unwrap(), "v2");
    }

    #[test]
    fn test_os_keyring_vault_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<OsKeyringVault>();
    }
}
