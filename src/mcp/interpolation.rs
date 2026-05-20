//! Environment variable interpolation normaliser & secret placeholder resolution.
//!
//! ## Dialect normalization
//! Translates between canonical `${VAR}` and per-agent dialects:
//! - `$VAR`           – bare dollar (Copilot)
//! - `${VAR}`         – canonical / dollar-brace
//! - `${env:VAR}`     – VS Code env-dedicated placeholder
//! - `${input:name}`  – VS Code input prompt (preserved, never generated)
//! - `${VAR:-default}`– default-value suffix
//!
//! ## Secret placeholder resolution
//! Resolves `${ENV_AGENTALIGN_SECRET_*}` placeholders injected by the
//! secret-splitting migration step back to actual secret values.

use agentalign_shared::models::PlaceholderStyle;
use std::collections::HashMap;

// ─── Dialect Normalization ────────────────────────────────────────────────

/// Normalise a single value string from canonical `${VAR}` into the target dialect.
pub fn normalize_value(value: &str, style: &PlaceholderStyle) -> String {
    use PlaceholderStyle::*;
    match style {
        DollarBrace => value.to_string(),
        Dollar => dollar_brace_to_bare(value),
        EnvDollarBrace => dollar_brace_to_env(value),
        InputDollarBrace => value.to_string(), // input placeholders are never generated
    }
}

/// Normalise an entire env map from canonical into the target dialect.
pub fn normalize_env_map(
    env: &HashMap<String, String>,
    style: &PlaceholderStyle,
) -> HashMap<String, String> {
    env.iter()
        .map(|(k, v)| (k.clone(), normalize_value(v, style)))
        .collect()
}

/// Convert `${VAR}` / `${VAR:-default}` → `$VAR`.
/// Default values are dropped (only Copilot CLI uses bare vars).
fn dollar_brace_to_bare(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'
            let mut name = String::new();
            let mut has_default = false;

            loop {
                match chars.next() {
                    Some('}') => break,
                    Some(':') if chars.peek() == Some(&'-') => {
                        has_default = true;
                        chars.next(); // consume '-'
                        break;
                    }
                    Some(ch) => name.push(ch),
                    None => break,
                }
            }

            if has_default {
                // skip default value
                while let Some(ch) = chars.next() {
                    if ch == '}' {
                        break;
                    }
                }
            }

            result.push('$');
            result.push_str(&name);
        } else {
            result.push(c);
        }
    }

    result
}

/// Convert `$VAR` → `${env:VAR}`.
fn bare_to_env_dollar_brace(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' {
            let mut name = String::new();
            // Check for `{` indicating `${VAR}` syntax
            if chars.peek() == Some(&'{') {
                chars.next();
                for ch in chars.by_ref() {
                    if ch == '}' || ch == ':' {
                        if ch == ':' {
                            // skip default
                            for rest in chars.by_ref() {
                                if rest == '}' {
                                    break;
                                }
                            }
                        }
                        break;
                    }
                    name.push(ch);
                }
                // already in ${VAR} form, convert to ${env:VAR}
                result.push_str("${env:");
                result.push_str(&name);
                result.push('}');
            } else {
                // bare $VAR
                while let Some(&ch) = chars.peek() {
                    if ch.is_alphanumeric() || ch == '_' {
                        name.push(ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                result.push_str("${env:");
                result.push_str(&name);
                result.push('}');
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Convert `${VAR}` → `${env:VAR}` (for existing dollar-brace syntax).
fn dollar_brace_to_env(input: &str) -> String {
    bare_to_env_dollar_brace(input)
}

// ─── Secret Placeholder Resolution ────────────────────────────────────────

/// The prefix used in placeholder strings.
const PLACEHOLDER_PREFIX: &str = "${ENV_AGENTALIGN_SECRET_";

/// Find all unique `${ENV_AGENTALIGN_SECRET_*}` placeholder references
/// in a JSON value tree (recursive).
///
/// Returns a `Vec<String>` of complete placeholder strings (e.g.,
/// `"${ENV_AGENTALIGN_SECRET_abc123}"`).
pub fn extract_placeholders(value: &serde_json::Value) -> Vec<String> {
    let mut placeholders = Vec::new();
    collect_placeholders(value, &mut placeholders);
    placeholders.sort();
    placeholders.dedup();
    placeholders
}

fn collect_placeholders(value: &serde_json::Value, acc: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for val in map.values() {
                collect_placeholders(val, acc);
            }
        }
        serde_json::Value::Array(arr) => {
            for val in arr {
                collect_placeholders(val, acc);
            }
        }
        serde_json::Value::String(s) => {
            scan_string_for_placeholders(s, acc);
        }
        _ => {}
    }
}

fn scan_string_for_placeholders(s: &str, acc: &mut Vec<String>) {
    let mut start = 0;
    while let Some(pos) = s[start..].find(PLACEHOLDER_PREFIX) {
        let actual_start = start + pos;
        // Find the closing brace
        if let Some(end) = s[actual_start..].find('}') {
            let placeholder = &s[actual_start..=actual_start + end];
            acc.push(placeholder.to_string());
            start = actual_start + end + 1;
        } else {
            break;
        }
    }
}

/// Resolve all `${ENV_AGENTALIGN_SECRET_*}` placeholders in a JSON value tree
/// by replacing them with actual values from the provided map.
///
/// The map key should be the full placeholder string (e.g.,
/// `"${ENV_AGENTALIGN_SECRET_abc123}"`), and the value is the secret.
///
/// Placeholders not found in the map are kept as-is.
pub fn resolve_placeholders(
    value: &serde_json::Value,
    secrets: &HashMap<String, String>,
) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (key, val) in map {
                result.insert(key.clone(), resolve_placeholders(val, secrets));
            }
            serde_json::Value::Object(result)
        }
        serde_json::Value::Array(arr) => {
            let result: Vec<_> = arr
                .iter()
                .map(|v| resolve_placeholders(v, secrets))
                .collect();
            serde_json::Value::Array(result)
        }
        serde_json::Value::String(s) => {
            // Scan for placeholders and replace
            let mut result = String::new();
            let mut last_end = 0;
            let mut found = false;

            let mut start = 0;
            while let Some(pos) = s[start..].find(PLACEHOLDER_PREFIX) {
                found = true;
                let actual_start = start + pos;
                // Append text before this placeholder
                result.push_str(&s[last_end..actual_start]);

                if let Some(end) = s[actual_start..].find('}') {
                    let placeholder = &s[actual_start..=actual_start + end];
                    let resolved = secrets
                        .get(placeholder)
                        .cloned()
                        .unwrap_or_else(|| placeholder.to_string());
                    result.push_str(&resolved);
                    last_end = actual_start + end + 1;
                    start = last_end;
                } else {
                    break;
                }
            }

            if found {
                result.push_str(&s[last_end..]);
                serde_json::Value::String(result)
            } else {
                value.clone()
            }
        }
        _ => value.clone(),
    }
}

/// Build a lookup map from keychain-label-to-value entries.
/// Maps placeholder strings (e.g., `${ENV_AGENTALIGN_SECRET_abc}`)
/// to their actual secret values.
pub fn build_secret_map(
    keychain_label_to_value: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for (label, value) in keychain_label_to_value {
        let placeholder = format!("${{{}}}", label);
        map.insert(placeholder, value.clone());
    }
    map
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Dialect Normalisation ──

    #[test]
    fn test_dollar_brace_to_bare() {
        assert_eq!(dollar_brace_to_bare("${HOME}"), "$HOME");
        assert_eq!(dollar_brace_to_bare("${PATH}"), "$PATH");
        assert_eq!(dollar_brace_to_bare("prefix_${VAR}_suffix"), "prefix_$VAR_suffix");
    }

    #[test]
    fn test_dollar_brace_to_bare_with_default() {
        assert_eq!(dollar_brace_to_bare("${HOME:-/tmp}"), "$HOME");
        assert_eq!(dollar_brace_to_bare("${PORT:-8080}"), "$PORT");
    }

    #[test]
    fn test_bare_to_env_dollar_brace() {
        assert_eq!(bare_to_env_dollar_brace("$HOME"), "${env:HOME}");
        assert_eq!(bare_to_env_dollar_brace("prefix_$VAR"), "prefix_${env:VAR}");
    }

    #[test]
    fn test_dollar_brace_to_env() {
        assert_eq!(dollar_brace_to_env("${HOME}"), "${env:HOME}");
        assert_eq!(dollar_brace_to_env("${PATH}"), "${env:PATH}");
    }

    #[test]
    fn test_normalize_value_identity() {
        assert_eq!(normalize_value("${HOME}", &PlaceholderStyle::DollarBrace), "${HOME}");
    }

    #[test]
    fn test_normalize_env_map() {
        let mut env = HashMap::new();
        env.insert("HOME".into(), "/Users/test".into());
        env.insert("API_KEY".into(), "${GLOBAL_KEY}".into());

        let result = normalize_env_map(&env, &PlaceholderStyle::Dollar);
        assert_eq!(result.get("HOME").unwrap(), "/Users/test");
        assert_eq!(result.get("API_KEY").unwrap(), "$GLOBAL_KEY");
    }

    // ── Secret Placeholder Resolution ──

    #[test]
    fn test_extract_placeholders_simple() {
        let value = json!({
            "api_key": "${ENV_AGENTALIGN_SECRET_abc123}"
        });
        let mut placeholders = extract_placeholders(&value);
        placeholders.sort();
        assert_eq!(placeholders, vec!["${ENV_AGENTALIGN_SECRET_abc123}"]);
    }

    #[test]
    fn test_extract_placeholders_nested() {
        let value = json!({
            "mcp": {
                "headers": {
                    "Authorization": "${ENV_AGENTALIGN_SECRET_def456}"
                },
                "env": {
                    "API_KEY": "${ENV_AGENTALIGN_SECRET_abc123}"
                }
            }
        });
        let mut placeholders = extract_placeholders(&value);
        placeholders.sort();
        assert_eq!(
            placeholders,
            vec![
                "${ENV_AGENTALIGN_SECRET_abc123}",
                "${ENV_AGENTALIGN_SECRET_def456}"
            ]
        );
    }

    #[test]
    fn test_extract_placeholders_none() {
        let value = json!({"name": "server", "url": "http://example.com"});
        assert!(extract_placeholders(&value).is_empty());
    }

    #[test]
    fn test_extract_placeholders_embedded() {
        let value = json!({
            "config": "prefix_${ENV_AGENTALIGN_SECRET_abc123}_suffix"
        });
        let mut placeholders = extract_placeholders(&value);
        placeholders.sort();
        assert_eq!(placeholders, vec!["${ENV_AGENTALIGN_SECRET_abc123}"]);
    }

    #[test]
    fn test_resolve_placeholders_basic() {
        let value = json!({
            "api_key": "${ENV_AGENTALIGN_SECRET_abc123}",
            "url": "https://example.com"
        });

        let mut secrets = HashMap::new();
        secrets.insert(
            "${ENV_AGENTALIGN_SECRET_abc123}".to_string(),
            "sk-real-key".to_string(),
        );

        let resolved = resolve_placeholders(&value, &secrets);
        assert_eq!(resolved["api_key"], "sk-real-key");
        assert_eq!(resolved["url"], "https://example.com");
    }

    #[test]
    fn test_resolve_placeholders_nested() {
        let value = json!({
            "mcp": {
                "headers": {
                    "Authorization": "${ENV_AGENTALIGN_SECRET_def456}"
                }
            }
        });

        let mut secrets = HashMap::new();
        secrets.insert(
            "${ENV_AGENTALIGN_SECRET_def456}".to_string(),
            "Bearer my-token".to_string(),
        );

        let resolved = resolve_placeholders(&value, &secrets);
        assert_eq!(resolved["mcp"]["headers"]["Authorization"], "Bearer my-token");
    }

    #[test]
    fn test_resolve_placeholders_embedded() {
        let value = json!({
            "config": "prefix_${ENV_AGENTALIGN_SECRET_abc123}_suffix"
        });

        let mut secrets = HashMap::new();
        secrets.insert(
            "${ENV_AGENTALIGN_SECRET_abc123}".to_string(),
            "MIDDLE".to_string(),
        );

        let resolved = resolve_placeholders(&value, &secrets);
        assert_eq!(resolved["config"], "prefix_MIDDLE_suffix");
    }

    #[test]
    fn test_resolve_placeholders_missing() {
        let value = json!({
            "api_key": "${ENV_AGENTALIGN_SECRET_unknown}"
        });

        let secrets = HashMap::new();
        let resolved = resolve_placeholders(&value, &secrets);
        assert_eq!(resolved["api_key"], "${ENV_AGENTALIGN_SECRET_unknown}");
    }

    #[test]
    fn test_resolve_placeholders_empty() {
        let value = json!({"name": "server"});
        let secrets = HashMap::new();
        let resolved = resolve_placeholders(&value, &secrets);
        assert_eq!(resolved, value);
    }

    #[test]
    fn test_resolve_placeholders_array() {
        let value = json!([
            "${ENV_AGENTALIGN_SECRET_a}",
            "${ENV_AGENTALIGN_SECRET_b}",
            "plain"
        ]);

        let mut secrets = HashMap::new();
        secrets.insert("${ENV_AGENTALIGN_SECRET_a}".to_string(), "val_a".to_string());
        secrets.insert("${ENV_AGENTALIGN_SECRET_b}".to_string(), "val_b".to_string());

        let resolved = resolve_placeholders(&value, &secrets);
        assert_eq!(resolved[0], "val_a");
        assert_eq!(resolved[1], "val_b");
        assert_eq!(resolved[2], "plain");
    }

    #[test]
    fn test_extract_placeholders_array() {
        let value = json!([
            "${ENV_AGENTALIGN_SECRET_a}",
            "${ENV_AGENTALIGN_SECRET_b}"
        ]);
        let mut placeholders = extract_placeholders(&value);
        placeholders.sort();
        assert_eq!(
            placeholders,
            vec![
                "${ENV_AGENTALIGN_SECRET_a}",
                "${ENV_AGENTALIGN_SECRET_b}"
            ]
        );
    }

    #[test]
    fn test_build_secret_map() {
        let mut raw = HashMap::new();
        raw.insert(
            "ENV_AGENTALIGN_SECRET_abc".to_string(),
            "secret-value".to_string(),
        );
        let map = build_secret_map(&raw);
        assert_eq!(
            map.get("${ENV_AGENTALIGN_SECRET_abc}"),
            Some(&"secret-value".to_string())
        );
    }
}
