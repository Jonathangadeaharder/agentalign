use anyhow::{Context, Result};
use keyring::Entry;

const SERVICE_NAME: &str = "agentalign";

/// Store a secret in the OS keychain.
/// Uses `keyring` crate with service name "agentalign" and the label as account name.
pub fn store_secret(label: &str, value: &str) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, label)
        .with_context(|| format!("Failed to create keychain entry for '{label}'"))?;
    entry
        .set_password(value)
        .with_context(|| format!("Failed to store secret '{label}' in keychain"))?;
    Ok(())
}

/// Retrieve a secret from the OS keychain.
/// Returns the stored value, or an error if the entry doesn't exist.
pub fn get_secret(label: &str) -> Result<String> {
    let entry = Entry::new(SERVICE_NAME, label)
        .with_context(|| format!("Failed to create keychain entry for '{label}'"))?;
    let value = entry
        .get_password()
        .with_context(|| format!("Failed to retrieve secret '{label}' from keychain"))?;
    Ok(value)
}

/// Delete a secret from the OS keychain.
pub fn delete_secret(label: &str) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, label)
        .with_context(|| format!("Failed to create keychain entry for '{label}'"))?;
    entry
        .delete_credential()
        .with_context(|| format!("Failed to delete secret '{label}' from keychain"))?;
    Ok(())
}

/// Check if a secret exists in the keychain.
pub fn secret_exists(label: &str) -> bool {
    get_secret(label).is_ok()
}

/// Fallback: try keychain first, then local store.
/// Returns `Ok(Some(value))` if found in either, `Ok(None)` if not in either.
pub fn resolve_secret(label: &str, local: &crate::migration::local_json::LocalSecretStore) -> Option<String> {
    // Try OS keychain first
    if let Ok(value) = get_secret(label) {
        return Some(value);
    }
    // Fall back to local store
    local.secrets.get(label).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keyring crate's mock tests require platform-specific mocking.
    /// These tests are integration-level; for CI without a keychain,
    /// we test the fallback path instead.
    #[test]
    fn test_service_name_constant() {
        assert_eq!(SERVICE_NAME, "agentalign");
    }

    #[test]
    fn test_fallback_resolve_none() {
        let local = crate::migration::local_json::LocalSecretStore::default();
        let result = resolve_secret("nonexistent_label", &local);
        assert!(result.is_none());
    }

    #[test]
    fn test_fallback_resolve_local() {
        let mut local = crate::migration::local_json::LocalSecretStore::default();
        local.secrets.insert("test_label".into(), "test_value".into());
        let result = resolve_secret("test_label", &local);
        assert_eq!(result, Some("test_value".to_string()));
    }
}
