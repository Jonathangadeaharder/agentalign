use agentalign_shared::models::SecretMapping;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

const SENSITIVE_KEYWORDS: &[&str] = &["key", "secret", "token", "password", "api", "auth"];
const PLACEHOLDER_PREFIX: &str = "ENV_AGENTALIGN_SECRET_";

/// Check if a field name contains any sensitive keyword (case-insensitive).
fn is_sensitive_field(name: &str) -> bool {
    let lower = name.to_lowercase();
    SENSITIVE_KEYWORDS.iter().any(|kw| lower.contains(kw))
}

/// Generate a deterministic placeholder suffix from a value.
/// Uses SHA-256 truncated to first 16 hex chars.
fn hash_value(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..8]) // 8 bytes = 16 hex chars
}

/// Build the placeholder string from a hash suffix.
fn make_placeholder(suffix: &str) -> String {
    format!("${{{}{}}}", PLACEHOLDER_PREFIX, suffix)
}

/// Build the keychain label from a hash suffix.
fn make_keychain_label(suffix: &str) -> String {
    format!("{}{}", PLACEHOLDER_PREFIX, suffix)
}

/// Result of extracting secrets from a JSON value tree.
#[derive(Debug, Clone)]
pub struct SplitResult {
    /// The sanitized value with placeholders instead of secrets.
    pub sanitized: serde_json::Value,
    /// Mapping entries describing each extracted secret.
    pub secrets: Vec<SecretMapping>,
}

/// Recursively scan a `serde_json::Value` tree for sensitive fields,
/// replace their values with deterministic `${ENV_AGENTALIGN_SECRET_*}`
/// placeholders, and return the sanitized tree plus secret mappings.
///
/// # Arguments
/// * `value` - The JSON value to scan
/// * `target_agent` - Agent identifier for the mapping entries
/// * `path_prefix` - Accumulated JSON path segments (internal use)
pub fn split_secrets(
    value: &serde_json::Value,
    target_agent: &str,
    path_prefix: &[String],
) -> SplitResult {
    match value {
        serde_json::Value::Object(map) => {
            let mut sanitized = serde_json::Map::new();
            let mut all_secrets = Vec::new();

            for (key, val) in map {
                let mut path = path_prefix.to_vec();
                path.push(key.clone());

                if let serde_json::Value::String(s) = val {
                    if is_sensitive_field(key) {
                        // Extract this secret
                        let suffix = hash_value(s);
                        let placeholder = make_placeholder(&suffix);
                        let keychain_label = make_keychain_label(&suffix);

                        sanitized.insert(key.clone(), serde_json::Value::String(placeholder.clone()));

                        all_secrets.push(SecretMapping {
                            placeholder,
                            keychain_label,
                            target_agent: target_agent.to_string(),
                            original_path: path,
                        });
                        continue;
                    }
                }

                // Recurse into nested values
                let nested = split_secrets(val, target_agent, &path);
                sanitized.insert(key.clone(), nested.sanitized);
                all_secrets.extend(nested.secrets);
            }

            SplitResult {
                sanitized: serde_json::Value::Object(sanitized),
                secrets: all_secrets,
            }
        }
        serde_json::Value::Array(arr) => {
            let mut sanitized = Vec::new();
            let mut all_secrets = Vec::new();

            for (i, val) in arr.iter().enumerate() {
                let mut path = path_prefix.to_vec();
                path.push(format!("[{}]", i));

                let nested = split_secrets(val, target_agent, &path);
                sanitized.push(nested.sanitized);
                all_secrets.extend(nested.secrets);
            }

            SplitResult {
                sanitized: serde_json::Value::Array(sanitized),
                secrets: all_secrets,
            }
        }
        // Primitives (strings, numbers, booleans, null) — pass through unchanged
        _ => SplitResult {
            sanitized: value.clone(),
            secrets: Vec::new(),
        },
    }
}

/// Apply secret mapping to a JSON value: replace placeholders back with actual values.
/// This is the inverse of `split_secrets`.
pub fn apply_secret_mappings(
    value: &serde_json::Value,
    secret_values: &HashMap<String, String>,
) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (key, val) in map {
                let resolved = if let serde_json::Value::String(s) = val {
                    if is_placeholder(s) {
                        // Look up the placeholder in our secret values map
                        if let Some(actual) = secret_values.get(s) {
                            serde_json::Value::String(actual.clone())
                        } else {
                            // Placeholder not found; keep it as-is
                            val.clone()
                        }
                    } else {
                        apply_secret_mappings(val, secret_values)
                    }
                } else {
                    apply_secret_mappings(val, secret_values)
                };
                result.insert(key.clone(), resolved);
            }
            serde_json::Value::Object(result)
        }
        serde_json::Value::Array(arr) => {
            let result: Vec<_> = arr
                .iter()
                .map(|v| apply_secret_mappings(v, secret_values))
                .collect();
            serde_json::Value::Array(result)
        }
        _ => value.clone(),
    }
}

/// Check if a string is a placeholder.
#[inline]
pub fn is_placeholder(s: &str) -> bool {
    s.starts_with("${") && s.contains(PLACEHOLDER_PREFIX) && s.ends_with('}')
}

/// Extract the hash suffix from a placeholder string.
/// e.g., `${ENV_AGENTALIGN_SECRET_abc123}` -> `abc123`
pub fn placeholder_suffix(placeholder: &str) -> Option<&str> {
    let prefix = format!("${{{}", PLACEHOLDER_PREFIX);
    placeholder
        .strip_prefix(&prefix)
        .and_then(|s| s.strip_suffix('}'))
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_is_sensitive_field() {
        assert!(is_sensitive_field("api_key"));
        assert!(is_sensitive_field("API_KEY"));
        assert!(is_sensitive_field("Authorization"));
        assert!(is_sensitive_field("secret_token"));
        assert!(is_sensitive_field("password"));
        assert!(is_sensitive_field("openai_api_key"));
        assert!(!is_sensitive_field("name"));
        assert!(!is_sensitive_field("description"));
        assert!(!is_sensitive_field("enabled"));
        assert!(!is_sensitive_field("command"));
    }

    #[test]
    fn test_hash_deterministic() {
        let a = hash_value("my-secret-value");
        let b = hash_value("my-secret-value");
        assert_eq!(a, b, "hash must be deterministic");
        assert_eq!(a.len(), 16, "hash should be 16 hex chars");
    }

    #[test]
    fn test_different_values_different_hashes() {
        let a = hash_value("secret-1");
        let b = hash_value("secret-2");
        assert_ne!(a, b, "different values must produce different hashes");
    }

    #[test]
    fn test_make_placeholder() {
        let p = make_placeholder("abc123");
        assert_eq!(p, "${ENV_AGENTALIGN_SECRET_abc123}");
    }

    #[test]
    fn test_placeholder_roundtrip() {
        let p = "${ENV_AGENTALIGN_SECRET_abc123}";
        assert!(is_placeholder(p));
        assert_eq!(placeholder_suffix(p), Some("abc123"));
    }

    #[test]
    fn test_is_placeholder_non_matches() {
        assert!(!is_placeholder("${OTHER_VAR}"));
        assert!(!is_placeholder("plain text"));
        assert!(!is_placeholder("${ENV_OTHER_SECRET_x}"));
    }

    #[test]
    fn test_split_simple_object() {
        let value = json!({
            "name": "my-server",
            "api_key": "sk-0123456789abcdef",
            "url": "https://example.com",
            "secret": "hunter2"
        });

        let result = split_secrets(&value, "opencode", &[]);

        // Sanitized should have placeholders for api_key and secret
        assert_eq!(result.secrets.len(), 2);

        let api_key_mapping = result
            .secrets
            .iter()
            .find(|m| m.original_path.last() == Some(&"api_key".to_string()))
            .expect("api_key should be captured");
        assert_eq!(api_key_mapping.target_agent, "opencode");
        assert!(api_key_mapping.placeholder.starts_with("${ENV_AGENTALIGN_SECRET_"));
        assert_eq!(api_key_mapping.original_path, vec!["api_key"]);

        let secret_mapping = result
            .secrets
            .iter()
            .find(|m| m.original_path.last() == Some(&"secret".to_string()))
            .expect("secret should be captured");
        assert!(secret_mapping.placeholder.starts_with("${ENV_AGENTALIGN_SECRET_"));

        // Sanitized output
        let sanitized = result.sanitized;
        assert_eq!(sanitized["name"], "my-server");
        assert_eq!(sanitized["url"], "https://example.com");
        assert!(sanitized["api_key"].as_str().unwrap().starts_with("${ENV_AGENTALIGN_SECRET_"));
        assert!(sanitized["secret"].as_str().unwrap().starts_with("${ENV_AGENTALIGN_SECRET_"));
    }

    #[test]
    fn test_split_nested() {
        let value = json!({
            "mcp": {
                "supabase": {
                    "headers": {
                        "Authorization": "Bearer eyJhbGciOiJIUzI1NiJ9.test"
                    },
                    "env": {
                        "SUPABASE_API_KEY": "sbp_abc123",
                        "SUPABASE_URL": "https://project.supabase.co"
                    }
                }
            }
        });

        let result = split_secrets(&value, "opencode", &[]);

        assert_eq!(result.secrets.len(), 2);

        // Authorization header
        let auth = result
            .secrets
            .iter()
            .find(|m| m.original_path.contains(&"Authorization".to_string()))
            .expect("Authorization should be captured");
        assert_eq!(
            auth.original_path,
            vec!["mcp", "supabase", "headers", "Authorization"]
        );

        // API key in env
        let api = result
            .secrets
            .iter()
            .find(|m| m.original_path.contains(&"SUPABASE_API_KEY".to_string()))
            .expect("SUPABASE_API_KEY should be captured");
        assert_eq!(
            api.original_path,
            vec!["mcp", "supabase", "env", "SUPABASE_API_KEY"]
        );

        // URL should NOT be captured
        let sanitized = result.sanitized;
        assert_eq!(
            sanitized["mcp"]["supabase"]["env"]["SUPABASE_URL"],
            "https://project.supabase.co"
        );
    }

    #[test]
    fn test_split_idempotent() {
        let value = json!({
            "api_key": "sk-abcdef",
            "secret": "hunter2"
        });

        let a = split_secrets(&value, "opencode", &[]);
        let b = split_secrets(&value, "opencode", &[]);

        assert_eq!(a.secrets.len(), b.secrets.len());
        for (ma, mb) in a.secrets.iter().zip(b.secrets.iter()) {
            assert_eq!(ma.placeholder, mb.placeholder, "placeholders must be deterministic");
            assert_eq!(
                ma.original_path, mb.original_path,
                "paths must be identical"
            );
        }
        assert_eq!(a.sanitized, b.sanitized, "sanitized output must be identical");
    }

    #[test]
    fn test_split_no_sensitive_fields() {
        let value = json!({
            "name": "server",
            "url": "http://example.com",
            "enabled": true,
            "count": 42
        });

        let result = split_secrets(&value, "test", &[]);
        assert!(result.secrets.is_empty());
        assert_eq!(result.sanitized, value);
    }

    #[test]
    fn test_split_array() {
        let value = json!([
            {"name": "server1", "token": "abc123"},
            {"name": "server2", "api_key": "def456"}
        ]);

        let result = split_secrets(&value, "test", &[]);
        assert_eq!(result.secrets.len(), 2);
    }

    #[test]
    fn test_apply_secret_mappings() {
        let sanitized = json!({
            "name": "server",
            "api_key": "${ENV_AGENTALIGN_SECRET_abc123}",
            "url": "https://example.com"
        });

        let mut secrets = HashMap::new();
        secrets.insert(
            "${ENV_AGENTALIGN_SECRET_abc123}".to_string(),
            "sk-real-value".to_string(),
        );

        let resolved = apply_secret_mappings(&sanitized, &secrets);
        assert_eq!(resolved["api_key"], "sk-real-value");
        assert_eq!(resolved["name"], "server");
        assert_eq!(resolved["url"], "https://example.com");
    }

    #[test]
    fn test_apply_secret_mappings_missing() {
        let sanitized = json!({
            "api_key": "${ENV_AGENTALIGN_SECRET_unknown}"
        });

        let secrets = HashMap::new();
        let resolved = apply_secret_mappings(&sanitized, &secrets);
        // Unknown placeholder should be kept as-is
        assert_eq!(
            resolved["api_key"],
            "${ENV_AGENTALIGN_SECRET_unknown}"
        );
    }

    #[test]
    fn test_placeholder_suffix_none() {
        assert!(placeholder_suffix("not a placeholder").is_none());
        assert!(placeholder_suffix("${NOT_PREFIX}").is_none());
    }

    #[test]
    fn test_split_with_path_prefix() {
        let value = json!({"api_key": "sk-test"});
        let result = split_secrets(&value, "opencode", &["agents".to_string(), "opencode".to_string()]);
        assert_eq!(result.secrets.len(), 1);
        assert_eq!(
            result.secrets[0].original_path,
            vec!["agents", "opencode", "api_key"]
        );
    }
}
