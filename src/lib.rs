//! # Sunsetr
//!
//! Automatic blue light filter for Hyprland, Niri, and everything Wayland.
//!
//! Sunsetr provides smooth color temperature and gamma transitions based on time of day,
//! with support for multiple backends: native Hyprland CTM control, hyprsunset daemon,
//! and generic Wayland compositors (via wlr-gamma-control-unstable-v1 protocol).
//!
//! ## Architecture
//!
//! - **backend**: Backend abstraction and implementations (Hyprland and Wayland)
//! - **config**: Configuration loading, validation, and default generation
//! - **constants**: Application-wide constants and defaults  
//! - **logger**: Structured logging with visual formatting
//! - **startup_transition**: Smooth transitions when the application starts
//! - **time_state**: Time-based state calculations and transition logic
//! - **utils**: Utility functions for interpolation and version handling

// Import macros from logger module for use in all submodules
#[macro_use]
pub mod logger;

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

// Re-export important types for easier access
pub use backend::{BackendType, ColorTemperatureBackend, create_backend, detect_backend};
pub use config::Config;
pub use display_state::DisplayState;
pub use logger::Log;
pub use time_state::{TimeState, get_transition_state, time_until_next_event};
