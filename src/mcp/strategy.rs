//! Re-exports and helper utilities for the ~McpFormatStrategy~ trait.

pub use crate::shared::traits::ConfigurationAdapter;
pub use crate::shared::traits::McpFormatStrategy;

use crate::shared::error::Result;
use crate::shared::models::CanonicalWorkspaceState;

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
