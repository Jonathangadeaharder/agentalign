use std::path::PathBuf;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, AdapterError>;

#[derive(Error, Debug)]
pub enum AdapterError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(String),

    #[error("Unsupported transport type: {0}")]
    UnsupportedTransport(String),

    #[error("Client capability mismatch: {transport} not supported by {client}")]
    CapabilityMismatch { client: String, transport: String },

    #[error("Key collision for server '{server}': {a} vs {b}")]
    KeyCollision { server: String, a: String, b: String },

    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Keychain error: {0}")]
    Keychain(String),

    #[error("Transaction error: {0}")]
    Transaction(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("{0}")]
    Other(String),
}

#[derive(Error, Debug)]
pub enum TrimError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Process inspection error: {0}")]
    ProcessInspection(String),

    #[error("Safety gate: {0} is protected and cannot be pruned")]
    SafetyGate(String),

    #[error("Backup error: {0}")]
    Backup(String),

    #[error("{0}")]
    Other(String),
}
