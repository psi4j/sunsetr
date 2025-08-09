//! # Sunsetr
//!
//! A sophisticated sunrise/sunset transition manager for Hyprland and Wayland compositors.
//!
//! Sunsetr provides smooth color temperature and gamma transitions based on time of day,
//! with support for multiple backends: Hyprland (via hyprsunset) and generic Wayland
//! compositors (via wlr-gamma-control-unstable-v1 protocol).
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

pub mod backend;
pub mod commands;
pub mod config;
pub mod constants;
pub mod geo;
pub mod logger;
pub mod signals;
pub mod startup_transition;
pub mod time_source;
pub mod time_state;
pub mod utils;

// Re-export important types for easier access
pub use backend::{BackendType, ColorTemperatureBackend, create_backend, detect_backend};
pub use config::Config;
pub use logger::{Log, LogLevel};
pub use time_state::{TimeState, TransitionState, get_transition_state, time_until_next_event};
