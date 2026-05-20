use agentalign_shared::models::{ClientCapabilities, PlaceholderStyle};

/// Returns the capability matrix for Claude Desktop.
pub fn claude_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        name: "claude".into(),
        supports_stdio: true,
        supports_sse: true,
        supports_http: true,
        supports_env_section: false,
        placeholder_style: PlaceholderStyle::EnvDollarBrace,
        max_id_length: None,
        forbidden_id_chars: vec![],
        requires_security_sandbox: false,
    }
}

/// Returns the capability matrix for Cursor.
pub fn cursor_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        name: "cursor".into(),
        supports_stdio: true,
        supports_sse: true,
        supports_http: false,
        supports_env_section: false,
        placeholder_style: PlaceholderStyle::DollarBrace,
        max_id_length: Some(64),
        forbidden_id_chars: vec!['/', '\\'],
        requires_security_sandbox: false,
    }
}

/// Returns the capability matrix for VS Code.
pub fn vscode_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        name: "vscode".into(),
        supports_stdio: true,
        supports_sse: true,
        supports_http: true,
        supports_env_section: false,
        placeholder_style: PlaceholderStyle::EnvDollarBrace,
        max_id_length: None,
        forbidden_id_chars: vec![],
        requires_security_sandbox: false,
    }
}

/// Returns the capability matrix for Copilot CLI.
pub fn copilot_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        name: "copilot".into(),
        supports_stdio: true,
        supports_sse: true,
        supports_http: false,
        supports_env_section: true,
        placeholder_style: PlaceholderStyle::Dollar,
        max_id_length: None,
        forbidden_id_chars: vec![],
        requires_security_sandbox: false,
    }
}

/// Returns the capability matrix for Windsurf.
pub fn windsurf_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        name: "windsurf".into(),
        supports_stdio: true,
        supports_sse: true,
        supports_http: true,
        supports_env_section: false,
        placeholder_style: PlaceholderStyle::DollarBrace,
        max_id_length: None,
        forbidden_id_chars: vec![],
        requires_security_sandbox: true,
    }
}

/// Returns the capability matrix for Zed.
pub fn zed_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        name: "zed".into(),
        supports_stdio: true,
        supports_sse: true,
        supports_http: false,
        supports_env_section: true,
        placeholder_style: PlaceholderStyle::EnvDollarBrace,
        max_id_length: None,
        forbidden_id_chars: vec![],
        requires_security_sandbox: false,
    }
}

/// Returns the capability matrix for Gemini / Antigravity.
pub fn gemini_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        name: "gemini".into(),
        supports_stdio: true,
        supports_sse: true,
        supports_http: true,
        supports_env_section: false,
        placeholder_style: PlaceholderStyle::Dollar,
        max_id_length: None,
        forbidden_id_chars: vec![],
        requires_security_sandbox: false,
    }
}

/// Returns the capability matrix for Codex CLI.
pub fn codex_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        name: "codex".into(),
        supports_stdio: true,
        supports_sse: false,
        supports_http: false,
        supports_env_section: true,
        placeholder_style: PlaceholderStyle::DollarBrace,
        max_id_length: None,
        forbidden_id_chars: vec!['/', '.'],
        requires_security_sandbox: false,
    }
}
