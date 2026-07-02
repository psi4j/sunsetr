//! Application constants and default values for sunsetr.
//!
//! This module contains all the configuration defaults, validation limits,
//! and operational constants used throughout the application.

use crate::config::Backend;

// # Application Configuration Defaults

pub const DEFAULT_BACKEND: Backend = Backend::Auto;

pub const DEFAULT_SMOOTHING: bool = true;
pub const DEFAULT_STARTUP_DURATION_SEC: f64 = 0.5;
pub const DEFAULT_SHUTDOWN_DURATION_SEC: f64 = 0.5;
pub const DEFAULT_ADAPTIVE_INTERVAL_MS: u64 = 1;
pub const DEFAULT_SUNSET: &str = "19:00:00";
pub const DEFAULT_SUNRISE: &str = "06:00:00";
pub const DEFAULT_NIGHT_TEMP: u32 = 3300;
pub const DEFAULT_DAY_TEMP: u32 = 6500;
pub const DEFAULT_NIGHT_GAMMA: f64 = 90.0;
pub const DEFAULT_DAY_GAMMA: f64 = 100.0;
pub const DEFAULT_TRANSITION_DURATION_MIN: u64 = 45;
pub const DEFAULT_UPDATE_INTERVAL_SEC: u64 = 60;
pub const DEFAULT_TRANSITION_MODE: &str = "geo";
pub const FALLBACK_DEFAULT_TRANSITION_MODE: &str = "finish_by";

// # Validation Limits

// Smooth transition limits (preferred)
pub const MINIMUM_SMOOTH_TRANSITION_DURATION_SEC: f64 = 0.0;
pub const MAXIMUM_SMOOTH_TRANSITION_DURATION_SEC: f64 = 60.0;

pub const MINIMUM_ADAPTIVE_INTERVAL_MS: u64 = 1; // milliseconds (1000fps theoretical max)
pub const MAXIMUM_ADAPTIVE_INTERVAL_MS: u64 = 1000; // milliseconds (1 second max)

// Temperature limits (Kelvin scale)
pub const MINIMUM_TEMP: u32 = 1000;
pub const MAXIMUM_TEMP: u32 = 20000;

// Gamma limits (percentage)
pub const MINIMUM_GAMMA: f64 = 10.0;
pub const MAXIMUM_GAMMA: f64 = 200.0;

// Transition duration limits
pub const MINIMUM_TRANSITION_DURATION_MIN: u64 = 5;
pub const MAXIMUM_TRANSITION_DURATION_MIN: u64 = 120;

// Update interval limits
pub const MINIMUM_UPDATE_INTERVAL_SEC: u64 = 10;
pub const MAXIMUM_UPDATE_INTERVAL_SEC: u64 = 300;

// # Test Constants
#[cfg(test)]
pub mod test_constants {
    use super::*;

    pub const TEST_STANDARD_SUNSET: &str = "19:00:00";
    pub const TEST_STANDARD_SUNRISE: &str = "06:00:00";
    pub const TEST_STANDARD_TRANSITION_DURATION: u64 = 30;
    pub const TEST_STANDARD_UPDATE_INTERVAL: u64 = 60;
    pub const TEST_STANDARD_NIGHT_TEMP: u32 = DEFAULT_NIGHT_TEMP;
    pub const TEST_STANDARD_DAY_TEMP: u32 = DEFAULT_DAY_TEMP;
    pub const TEST_STANDARD_NIGHT_GAMMA: f64 = DEFAULT_NIGHT_GAMMA;
    pub const TEST_STANDARD_DAY_GAMMA: f64 = DEFAULT_DAY_GAMMA;
    pub const TEST_STANDARD_MODE: &str = DEFAULT_TRANSITION_MODE;
}
