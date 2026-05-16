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
//! - **`commands`**: CLI subcommands (restart, stop, test, preset, geo, etc.)
//! - **`geo`**: Geolocation-based calculations for sunset/sunrise times
//! - **`state`**: Display, preset, and IPC state management
//!
//! ### Infrastructure
//! - **`common`**: Shared utilities, constants, and logging
//! - **`io`**: External I/O operations (signals, D-Bus, lock files)
//! - **`time`**: Time source abstraction and simulation

// IMPORTANT: `common` must be declared first so its logger macros are in
// unqualified scope for every module below (`#[macro_use]` textual scoping).
#[macro_use]
pub mod common;

// Entry points
pub mod args;
mod sunsetr;

// Core logic
pub(crate) mod core;

// Domain modules
pub mod backend;
pub mod commands;
pub mod config;
pub mod geo;
pub mod state;

// Infrastructure
pub(crate) mod io;
pub mod time;

// Crate-root facade. The binary, integration tests, and doctests reach the
// `pub(crate)` `core` module through these; the `pub mod` tree is used
// directly otherwise.
pub use common::utils;
pub use core::period::time_until_next_event;
pub use sunsetr::Sunsetr;
