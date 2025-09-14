//! Command-line command handlers for sunsetr.
//!
//! This module contains implementations for one-shot CLI commands like reload and test.
//! Each command is implemented in its own submodule to keep the code organized and maintainable.

pub mod geo;
pub mod preset;
pub mod reload;
pub mod test;

// Re-export from signals for backward compatibility (used by signals module)
// pub use crate::signals::TestModeParams;
