//! Application constants and default values for sunsetr.
//!
//! This module contains all the configuration defaults, validation limits,
//! and operational constants used throughout the application.

use crate::config::Backend;

// # Application Configuration Defaults
// These values are used when config options are not specified by the user

pub const DEFAULT_BACKEND: Backend = Backend::Auto;

pub const DEFAULT_SMOOTHING: bool = true;
pub const DEFAULT_STARTUP_DURATION: f64 = 0.5; // seconds
pub const DEFAULT_SHUTDOWN_DURATION: f64 = 0.5; // seconds
pub const DEFAULT_ADAPTIVE_INTERVAL: u64 = 1; // milliseconds
pub const DEFAULT_SUNSET: &str = "19:00:00";
pub const DEFAULT_SUNRISE: &str = "06:00:00";
pub const DEFAULT_NIGHT_TEMP: u32 = 3300;
pub const DEFAULT_DAY_TEMP: u32 = 6500;
pub const DEFAULT_NIGHT_GAMMA: f64 = 90.0;
pub const DEFAULT_DAY_GAMMA: f64 = 100.0;
pub const DEFAULT_TRANSITION_DURATION: u64 = 45; // minutes
pub const DEFAULT_UPDATE_INTERVAL: u64 = 60; // seconds
pub const DEFAULT_TRANSITION_MODE: &str = "geo";
pub const FALLBACK_DEFAULT_TRANSITION_MODE: &str = "finish_by";

// # hyprsunset Compatibility
// Version requirements and compatibility information

pub const REQUIRED_HYPRSUNSET_VERSION: &str = "v0.2.0"; // Minimum required version
pub const COMPATIBLE_HYPRSUNSET_VERSIONS: &[&str] = &[
    "v0.2.0", "v0.3.0",
    // Add more versions as they become available and tested
];

// # Validation Limits
// These limits ensure user inputs are within reasonable and safe ranges

// Smooth transition limits (preferred)
pub const MINIMUM_SMOOTH_TRANSITION_DURATION: f64 = 0.0; // seconds (accepts 0.0 for instant transition)
pub const MAXIMUM_SMOOTH_TRANSITION_DURATION: f64 = 60.0; // seconds (prevents excessively long startup)

// Legacy startup transition limits (deprecated - use smooth transition limits instead)
pub const MINIMUM_STARTUP_TRANSITION_DURATION: f64 = MINIMUM_SMOOTH_TRANSITION_DURATION; // deprecated
pub const MAXIMUM_STARTUP_TRANSITION_DURATION: f64 = MAXIMUM_SMOOTH_TRANSITION_DURATION; // deprecated
pub const MINIMUM_ADAPTIVE_INTERVAL: u64 = 1; // milliseconds (1000fps theoretical max)
pub const MAXIMUM_ADAPTIVE_INTERVAL: u64 = 1000; // milliseconds (1 second max)

// Temperature limits (Kelvin scale)
pub const MINIMUM_TEMP: u32 = 1000;
pub const MAXIMUM_TEMP: u32 = 20000;

// Gamma limits (percentage)
pub const MINIMUM_GAMMA: f64 = 10.0;
pub const MAXIMUM_GAMMA: f64 = 200.0;

// Transition duration limits
pub const MINIMUM_TRANSITION_DURATION: u64 = 5; // minutes
pub const MAXIMUM_TRANSITION_DURATION: u64 = 120; // minutes

// Update interval limits
pub const MINIMUM_UPDATE_INTERVAL: u64 = 10; // seconds
pub const MAXIMUM_UPDATE_INTERVAL: u64 = 300; // seconds

// # Adaptive update interval constants
// Just Noticeable Difference in mireds for adaptive interval calculation.
pub const ADAPTIVE_JND_MIREDS: f64 = 3.0;
// Just Noticeable Difference in gamma percentage points for adaptive interval calculation.
pub const ADAPTIVE_JND_GAMMA: f64 = 0.6;

// # Socket Communication Constants
// Settings for hyprsunset IPC communication

pub const SOCKET_TIMEOUT_MS: u64 = 1000;
pub const SOCKET_BUFFER_SIZE: usize = 1024;

// # User Interface Constants
// Visual display settings

pub const PROGRESS_BAR_WIDTH: usize = 30;

// # Exit Codes
// Standard exit codes for process termination

pub const EXIT_FAILURE: i32 = 1;

// # Test Constants
// Common values used in tests for consistency
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
