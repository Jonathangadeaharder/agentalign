//! Pre-write validation for MCP server definitions.
//!
//! Checks that a canonical workspace state can be safely serialised
//! for the target agent without loss or rejection.

use agentalign_shared::error::{AdapterError, Result};
use agentalign_shared::models::{CanonicalWorkspaceState, TransportType};

/// Validate server IDs for forbidden characters.
pub fn check_forbidden_chars(
    state: &CanonicalWorkspaceState,
    forbidden: &[char],
) -> Result<()> {
    for name in state.mcp.keys() {
        if let Some(ch) = name.chars().find(|c| forbidden.contains(c)) {
            return Err(AdapterError::Validation(format!(
                "Server ID '{}' contains forbidden character '{}'",
                name, ch
            )));
        }
    }
    Ok(())
}

/// Validate server IDs do not exceed max length.
pub fn check_max_id_length(
    state: &CanonicalWorkspaceState,
    max: usize,
) -> Result<()> {
    for name in state.mcp.keys() {
        if name.len() > max {
            return Err(AdapterError::Validation(format!(
                "Server ID '{}' exceeds max length of {}",
                name, max
            )));
        }
    }
    Ok(())
}

/// Validate that file paths in `command` exist (stdio servers).
pub fn check_stdio_paths(state: &CanonicalWorkspaceState) -> Result<()> {
    for (name, server) in &state.mcp {
        if server.transport != TransportType::Local {
            continue;
        }
        if let Some(cmd) = &server.command {
            if cmd.is_empty() {
                return Err(AdapterError::Validation(format!(
                    "Server '{}' has empty command list",
                    name
                )));
            }
        }
    }
    Ok(())
}

/// Validate remote server URLs are present and non-empty.
pub fn check_remote_urls(state: &CanonicalWorkspaceState) -> Result<()> {
    for (name, server) in &state.mcp {
        if server.transport != TransportType::Remote {
            continue;
        }
        match &server.url {
            None => {
                return Err(AdapterError::Validation(format!(
                    "Remote server '{}' is missing a URL",
                    name
                )));
            }
            Some(u) if u.is_empty() => {
                return Err(AdapterError::Validation(format!(
                    "Remote server '{}' has an empty URL",
                    name
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Validate only stdio transport is used when SSE is not supported.
pub fn check_transport_support(
    state: &CanonicalWorkspaceState,
    supports_sse: bool,
    supports_http: bool,
) -> Result<()> {
    for (name, server) in &state.mcp {
        match server.transport {
            TransportType::Remote if !supports_sse && !supports_http => {
                return Err(AdapterError::Validation(format!(
                    "Server '{}' uses remote transport but target does not support SSE or HTTP",
                    name
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Validate TOML table key restrictions for server IDs.
pub fn check_toml_key_safety(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(AdapterError::Validation(
            "Server ID cannot be empty".into(),
        ));
    }
    if name.contains('[') || name.contains(']') || name.contains('.') {
        return Err(AdapterError::Validation(format!(
            "Server ID '{}' contains characters unsafe for TOML table keys",
            name
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentalign_shared::models::{CanonicalWorkspaceState, McpServerDefinition, TransportType};
    use std::collections::HashMap;

    fn make_state(name: &str, transport: TransportType) -> CanonicalWorkspaceState {
        let mut mcp = HashMap::new();
        mcp.insert(
            name.into(),
            McpServerDefinition {
                transport,
                command: Some(vec!["npx".into()]),
                url: Some("http://localhost".into()),
                headers: None,
                env: None,
                enabled: None,
                extra: HashMap::new(),
            },
        );
        CanonicalWorkspaceState { mcp }
    }

    fn state_with_cmd(name: &str, cmd: Option<Vec<String>>) -> CanonicalWorkspaceState {
        let mut mcp = HashMap::new();
        mcp.insert(
            name.into(),
            McpServerDefinition {
                transport: TransportType::Local,
                command: cmd,
                url: None,
                headers: None,
                env: None,
                enabled: None,
                extra: HashMap::new(),
            },
        );
        CanonicalWorkspaceState { mcp }
    }

    #[test]
    fn test_forbidden_chars() {
        let state = make_state("bad/name", TransportType::Local);
        assert!(check_forbidden_chars(&state, &['/', '\\']).is_err());
        assert!(check_forbidden_chars(&state, &['x']).is_ok());
    }

    #[test]
    fn test_forbidden_chars_multi_level() {
        let mut mcp = HashMap::new();
        mcp.insert(
            "a\\b".into(),
            McpServerDefinition {
                transport: TransportType::Local,
                command: Some(vec!["npx".into()]),
                url: None, headers: None, env: None, enabled: None,
                extra: HashMap::new(),
            },
        );
        mcp.insert(
            "c/d".into(),
            McpServerDefinition {
                transport: TransportType::Local,
                command: Some(vec!["npx".into()]),
                url: None, headers: None, env: None, enabled: None,
                extra: HashMap::new(),
            },
        );
        let state = CanonicalWorkspaceState { mcp };
        assert!(check_forbidden_chars(&state, &['/', '\\']).is_err());
    }

    #[test]
    fn test_max_id_length() {
        let state = make_state(&"a".repeat(65), TransportType::Local);
        assert!(check_max_id_length(&state, 64).is_err());
        assert!(check_max_id_length(&state, 128).is_ok());
    }

    #[test]
    fn test_max_id_length_exact_boundary() {
        let state = make_state(&"a".repeat(64), TransportType::Local);
        assert!(check_max_id_length(&state, 64).is_ok());
    }

    #[test]
    fn test_transport_support() {
        let state = make_state("remote", TransportType::Remote);
        assert!(check_transport_support(&state, false, false).is_err());
        assert!(check_transport_support(&state, true, false).is_ok());
    }

    #[test]
    fn test_transport_support_local_ok() {
        let state = make_state("local", TransportType::Local);
        assert!(check_transport_support(&state, false, false).is_ok());
    }

    #[test]
    fn test_transport_support_http_fallback() {
        let state = make_state("remote", TransportType::Remote);
        assert!(check_transport_support(&state, false, true).is_ok());
    }

    #[test]
    fn test_stdio_paths_empty_cmd() {
        let state = state_with_cmd("test", Some(vec![]));
        assert!(check_stdio_paths(&state).is_err());
    }

    #[test]
    fn test_stdio_paths_remote_skipped() {
        let mut mcp = HashMap::new();
        mcp.insert(
            "remote".into(),
            McpServerDefinition {
                transport: TransportType::Remote,
                command: None,
                url: Some("http://localhost".into()),
                headers: None, env: None, enabled: None,
                extra: HashMap::new(),
            },
        );
        let state = CanonicalWorkspaceState { mcp };
        assert!(check_stdio_paths(&state).is_ok());
    }

    #[test]
    fn test_stdio_paths_no_cmd() {
        let state = state_with_cmd("test", None);
        assert!(check_stdio_paths(&state).is_ok());
    }

    #[test]
    fn test_stdio_paths_valid() {
        let state = state_with_cmd("test", Some(vec!["npx".into(), "@browsermcp/mcp".into()]));
        assert!(check_stdio_paths(&state).is_ok());
    }

    #[test]
    fn test_remote_urls_valid() {
        let state = make_state("remote", TransportType::Remote);
        assert!(check_remote_urls(&state).is_ok());
    }

    #[test]
    fn test_remote_urls_missing() {
        let mut mcp = HashMap::new();
        mcp.insert(
            "remote".into(),
            McpServerDefinition {
                transport: TransportType::Remote,
                command: None,
                url: None,
                headers: None, env: None, enabled: None,
                extra: HashMap::new(),
            },
        );
        let state = CanonicalWorkspaceState { mcp };
        assert!(check_remote_urls(&state).is_err());
    }

    #[test]
    fn test_remote_urls_empty() {
        let mut mcp = HashMap::new();
        mcp.insert(
            "remote".into(),
            McpServerDefinition {
                transport: TransportType::Remote,
                command: None,
                url: Some(String::new()),
                headers: None, env: None, enabled: None,
                extra: HashMap::new(),
            },
        );
        let state = CanonicalWorkspaceState { mcp };
        assert!(check_remote_urls(&state).is_err());
    }

    #[test]
    fn test_remote_urls_local_skipped() {
        let state = make_state("local", TransportType::Local);
        assert!(check_remote_urls(&state).is_ok());
    }

    #[test]
    fn test_toml_key_safety_empty() {
        assert!(check_toml_key_safety("").is_err());
    }

    #[test]
    fn test_toml_key_safety_brackets() {
        assert!(check_toml_key_safety("server[1]").is_err());
        assert!(check_toml_key_safety("server.1").is_err());
    }

    #[test]
    fn test_toml_key_safety_valid() {
        assert!(check_toml_key_safety("my-server").is_ok());
        assert!(check_toml_key_safety("my_server").is_ok());
        assert!(check_toml_key_safety("server-123").is_ok());
    }

    #[test]
    fn test_toml_key_safety_mixed_valid() {
        assert!(check_toml_key_safety("a_b-c").is_ok());
    }
}
