//! Application constants and default values for sunsetr.
//!
//! This module contains all the configuration defaults, validation limits,
//! and operational constants used throughout the application.

use crate::config::Backend;

// # Application Configuration Defaults
// These values are used when config options are not specified by the user

pub const DEFAULT_START_HYPRSUNSET: bool = true;
pub const DEFAULT_BACKEND: Backend = Backend::Auto; // Auto-detect backend

// New smoothing constants (preferred)
pub const DEFAULT_SMOOTHING: bool = true;
pub const DEFAULT_STARTUP_DURATION: f64 = 0.5; // seconds (supports decimals like 0.5)
pub const DEFAULT_SHUTDOWN_DURATION: f64 = 0.5; // seconds (supports decimals like 0.5)

// Legacy constants (deprecated - use smoothing constants instead)
pub const DEFAULT_STARTUP_TRANSITION: bool = DEFAULT_SMOOTHING; // deprecated - use DEFAULT_SMOOTHING
pub const DEFAULT_STARTUP_TRANSITION_DURATION: f64 = DEFAULT_STARTUP_DURATION; // deprecated - use DEFAULT_STARTUP_DURATION
pub const DEFAULT_ADAPTIVE_INTERVAL: u64 = 1; // milliseconds minimum between updates
pub const DEFAULT_SUNSET: &str = "19:00:00";
pub const DEFAULT_SUNRISE: &str = "06:00:00";
pub const DEFAULT_NIGHT_TEMP: u32 = 3300; // Kelvin - warm, comfortable for night viewing
pub const DEFAULT_DAY_TEMP: u32 = 6500; // Kelvin - close to natural sunlight
pub const DEFAULT_NIGHT_GAMMA: f32 = 90.0; // Slightly dimmed for night (percentage)
pub const DEFAULT_DAY_GAMMA: f32 = 100.0; // Full brightness for day (percentage)
pub const DEFAULT_TRANSITION_DURATION: u64 = 45; // minutes - gradual change
pub const DEFAULT_UPDATE_INTERVAL: u64 = 60; // seconds - how often to update during transitions
pub const DEFAULT_TRANSITION_MODE: &str = "geo"; // Geographic location-based transitions
pub const FALLBACK_DEFAULT_TRANSITION_MODE: &str = "finish_by"; // Fallback when default mode fails

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
pub const MINIMUM_TEMP: u32 = 1000; // Very warm candlelight-like
pub const MAXIMUM_TEMP: u32 = 20000; // Very cool blue light

// Gamma limits (percentage of full brightness)
pub const MINIMUM_GAMMA: f32 = 10.0; // Complete darkness (not recommended)
pub const MAXIMUM_GAMMA: f32 = 100.0; // Full brightness

// Transition duration limits
pub const MINIMUM_TRANSITION_DURATION: u64 = 5; // minutes (prevents too-rapid changes)
pub const MAXIMUM_TRANSITION_DURATION: u64 = 120; // minutes (2 hours max)

// Update interval limits
pub const MINIMUM_UPDATE_INTERVAL: u64 = 10; // seconds (prevents excessive CPU usage)
pub const MAXIMUM_UPDATE_INTERVAL: u64 = 300; // seconds (5 minutes max for responsive transitions)

// # Operational Timing Constants
// Internal timing values for application operation

// Time anomaly detection thresholds for suspend/resume and clock change scenarios
pub const SLEEP_DETECTION_THRESHOLD_SECS: u64 = 300; // 5 minutes - detect definite system sleep/resume
pub const SHORT_SUSPEND_THRESHOLD_SECS: u64 = 30; // 30 seconds - detect brief suspends or delays
pub const DST_TRANSITION_THRESHOLD_SECS: u64 = 3900; // 65 minutes - allow for DST transitions (1 hour + buffer)
pub const CLOCK_DRIFT_THRESHOLD_SECS: u64 = 5; // 5 seconds - ignore small backwards time jumps (NTP corrections)

// Application operation timing
pub const COMMAND_DELAY_MS: u64 = 100; // Delay between hyprsunset commands to prevent conflicts

// # Transition Curve Constants
// Bezier curve control points for smooth sunrise/sunset transitions
//
// The transition uses a cubic Bezier curve to create natural-looking changes
// that start slowly, accelerate through the middle, and slow down at the end.
// This avoids sudden jumps at transition boundaries.
//
// The curve is defined by four points:
// - P0 = (0, 0) - Start point (implicit)
// - P1 = (P1X, P1Y) - First control point
// - P2 = (P2X, P2Y) - Second control point
// - P3 = (1, 1) - End point (implicit)
//
// Recommended values:
// - For gentle S-curve: P1=(0.25, 0.0), P2=(0.75, 1.0)
// - For steeper curve: P1=(0.42, 0.0), P2=(0.58, 1.0)
// - For linear-like: P1=(0.33, 0.33), P2=(0.67, 0.67)
// - For ease-in only (slow start, fast end): P1=(0.42, 0.0), P2=(1.0, 1.0)

pub const BEZIER_P1X: f32 = 0.33; // X coordinate of first control point (0.0 to 0.5)
pub const BEZIER_P1Y: f32 = 0.07; // Y coordinate of first control point (typically 0.0)
pub const BEZIER_P2X: f32 = 0.33; // X coordinate of second control point (0.5 to 1.0)
pub const BEZIER_P2Y: f32 = 1.0; // Y coordinate of second control point (typically 1.0)

// # Socket Communication Constants
// Settings for hyprsunset IPC communication

pub const SOCKET_TIMEOUT_MS: u64 = 1000; // 1 second timeout for socket operations
pub const SOCKET_BUFFER_SIZE: usize = 1024; // Buffer size for socket communication

// # User Interface Constants
// Visual display settings

pub const PROGRESS_BAR_WIDTH: usize = 30; // Characters width for progress bar display

// # Retry and Recovery Constants
// Error handling and resilience settings

pub const MAX_RETRIES: u32 = 3; // Maximum attempts for failed operations
pub const RETRY_DELAY_MS: u64 = 1000; // Delay between retry attempts
pub const SOCKET_RECOVERY_DELAY_MS: u64 = 5000; // Wait time when hyprsunset becomes unavailable

// # Exit Codes
// Standard exit codes for process termination

pub const EXIT_FAILURE: i32 = 1; // General failure

// # Test Constants
// Common values used in tests for consistency
#[cfg(test)]
pub mod test_constants {
    use super::*;

    pub const TEST_STANDARD_SUNSET: &str = "19:00:00";
    pub const TEST_STANDARD_SUNRISE: &str = "06:00:00";
    pub const TEST_STANDARD_TRANSITION_DURATION: u64 = 30; // minutes
    pub const TEST_STANDARD_UPDATE_INTERVAL: u64 = 60; // seconds
    pub const TEST_STANDARD_NIGHT_TEMP: u32 = DEFAULT_NIGHT_TEMP; // 3300K
    pub const TEST_STANDARD_DAY_TEMP: u32 = DEFAULT_DAY_TEMP; // 6500K
    pub const TEST_STANDARD_NIGHT_GAMMA: f32 = DEFAULT_NIGHT_GAMMA; // 90.0%
    pub const TEST_STANDARD_DAY_GAMMA: f32 = DEFAULT_DAY_GAMMA; // 100.0%
    pub const TEST_STANDARD_MODE: &str = DEFAULT_TRANSITION_MODE; // "geo"
}
