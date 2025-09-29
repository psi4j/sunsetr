//! # Sunsetr Library
//!
//! Internal library for the Sunsetr binary application
//!
//! This library exists to enable testing of complex internals and provide clean separation
//! between CLI dispatch (main.rs) and application logic.
//!
//! ## Architecture
//!
//! The library is organized into several layers:
//!
//! - **Entry Point**: `Sunsetr` struct provides the main application API with resource management
//! - **Core Logic**: Internal `Core` module contains the main loop and state management
//! - **Backends**: `backend` module with Hyprland and Wayland compositor support
//! - **Configuration**: `config` module for TOML-based settings with hot-reload
//! - **Commands**: `commands` module for CLI subcommands (reload, test, preset, geo, etc.)
//! - **Geographic**: `geo` module for sunrise/sunset calculations and city selection
//! - **State Management**: `time_state` for time-based transitions, `display_state` quering
//!   runtime state
//! - **Infrastructure**: Signal handling, D-Bus monitoring, logging, and utilities

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
