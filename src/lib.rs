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
//! - **`time_state`**: Time-based state transitions and calculations
//! - **`display_state`**: Runtime state queries for external tools
//!
//! ### Infrastructure
//! - **`signals`**: Unix signal handling with message passing
//! - **`dbus`**: System event monitoring (sleep/resume, display changes)
//! - **`logger`**: Custom logging macros (special thanks to the hyprsunset devs)
//! - **`utils`**: Shared utilities and helpers

// Import macros from logger module for use in all submodules
#[macro_use]
pub mod logger;

// Public API modules
pub mod args;
pub mod backend;
pub mod commands;
pub mod config;
pub mod constants;
pub mod display_state;
pub mod geo;
pub mod signals;
pub mod smooth_transitions;
pub mod state;
pub mod time_source;
pub mod time_state;
pub mod utils;

// Internal modules
mod core;
pub(crate) mod dbus;
pub mod simulate;
mod sunsetr;

// Re-export for binary
pub use geo::GeoCommandResult;
pub use sunsetr::Sunsetr;
