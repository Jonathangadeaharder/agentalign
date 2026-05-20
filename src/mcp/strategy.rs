//! Re-exports and helper utilities for the ~McpFormatStrategy~ trait.

pub use agentalign_shared::traits::ConfigurationAdapter;
pub use agentalign_shared::traits::McpFormatStrategy;

use agentalign_shared::error::Result;
use agentalign_shared::models::CanonicalWorkspaceState;

/// Validate a canonical workspace state against ALL registered strategies.
/// Returns Ok(()) only if every strategy validates successfully.
pub fn validate_all(
    state: &CanonicalWorkspaceState,
    strategies: &[Box<dyn McpFormatStrategy>],
) -> Result<()> {
    for strategy in strategies {
        strategy.validate(state)?;
    }
    Ok(())
}
