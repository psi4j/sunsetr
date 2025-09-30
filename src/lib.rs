//! # Sunsetr Library
//!
//! Internal library for the Sunsetr binary application.
//!
//! This library implements the library-with-thin-binary pattern, where the main.rs file
//! serves as a minimal CLI dispatcher while all application logic resides in the library.
//!
//! **Note**: This is an internal library for the Sunsetr binary, not a public API
//! for external consumption. The public interface may change between versions.
//!
//! ## Architecture
//!
//! The library is organized into several layers:
//!
//! ### Application Layer
//! - **`Sunsetr`**: Main entry point with builder pattern for configuration
//!   - Manages RAII resources (terminal guard, lock files)
//!   - Orchestrates backend creation and dependency injection
//!   - Creates and executes the Core
//!
//! ### Core Logic
//! - **`core`** (internal): Encapsulates all application state and main loop
//!   - Manages color temperature transitions
//!   - Handles signal processing and config reloads
//!   - Implements smooth state changes based on time
//!
//! ### Domain Modules
//! - **`backend`**: Compositor support (Hyprland, Wayland)
//! - **`config`**: TOML configuration with validation and hot-reload
//! - **`commands`**: CLI subcommands (reload, test, preset, geo, etc.)
//! - **`geo`**: Geolocation-based calculations for sunset/sunrise times
//! - **`state`**: State management (period, display, preset states)
//!
//! ### Infrastructure
//! - **`common`**: Shared utilities, constants, and logging
//! - **`io`**: External I/O operations (signals, D-Bus, lock files)
//! - **`time`**: Time source abstraction and simulation

// IMPORTANT: Common module with logger macros must be first
#[macro_use]
pub mod common; // Contains logger with macros

// Entry Points (stay at root for clarity)
pub mod args; // CLI argument definitions
mod sunsetr; // Application coordinator
pub use sunsetr::Sunsetr;

// Core Business Logic
pub(crate) mod core; // Core state machine and smoothing

// State Management
pub mod state; // Period (time), display, and preset states

// Domain Modules (already well-organized)
pub mod backend; // Compositor backends
pub mod commands; // CLI command handlers
pub mod config; // Configuration management
pub mod geo; // Geographic calculations

// Utility Modules
pub(crate) mod io;
pub mod time; // Time source and simulation // External I/O operations (dbus, signals, lock)

// Re-exports for convenience and backward compatibility
pub use common::{constants, utils};

// State re-exports
pub use state::display::DisplayState;
pub use state::period::{
    Period, StateChange, get_transition_state, log_state_announcement, should_update_state,
    time_until_next_event, time_until_transition_end,
};

// Core re-exports
pub use core::smoothing as smooth_transitions;
pub use core::smoothing::SmoothTransition;

// Time re-exports
pub use time::simulate;
pub use time::source as time_source;

// I/O re-exports
pub use io::signals;

// Geo re-exports
pub use geo::GeoCommandResult;
pub use geo::times::GeoTimes;
