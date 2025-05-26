//! # Sunsetr
//! 
//! A sophisticated sunrise/sunset transition manager for Hyprland.
//! 
//! Sunsetr provides smooth color temperature and gamma transitions based on time of day,
//! integrating with hyprsunset to automatically adjust display colors for better eye comfort.
//! 
//! ## Architecture
//! 
//! - **config**: Configuration loading, validation, and default generation
//! - **constants**: Application-wide constants and defaults  
//! - **hyprsunset**: Client for communicating with the hyprsunset daemon
//! - **logger**: Structured logging with visual formatting
//! - **startup_transition**: Smooth transitions when the application starts
//! - **time_state**: Time-based state calculations and transition logic
//! - **utils**: Utility functions for interpolation and version handling

pub mod config;
pub mod constants;
pub mod hyprsunset;
pub mod logger;
pub mod startup_transition;
pub mod time_state;
pub mod utils;

// Re-export important types for easier access
pub use config::Config;
pub use hyprsunset::HyprsunsetClient;
pub use logger::{Log, LogLevel};
pub use time_state::{TimeState, TransitionState, get_transition_state, time_until_next_event};
