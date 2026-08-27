pub mod config;
pub mod error;
pub mod models;
pub mod paths;
pub mod traits;

use std::path::PathBuf;

/// Resolve the home directory agentalign operates on.
///
/// `AGENTALIGN_HOME` overrides it; `dirs::home_dir()` ignores `HOME`/`USERPROFILE`
/// on Windows, so an override is the only way to sandbox a run or a test.
pub fn home_dir() -> anyhow::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("AGENTALIGN_HOME") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine the home directory; set AGENTALIGN_HOME"))
}
