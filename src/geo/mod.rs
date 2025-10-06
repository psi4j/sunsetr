//! Geographic location-based sunrise/sunset calculations.
//!
//! This module provides comprehensive geographic functionality for the sunsetr application,
//! enabling location-aware color temperature transitions based on local solar events.
//!
//! ## Module Structure
//!
//! - [`city_selector`]: Interactive city selection with fuzzy search across 10,000+ cities
//! - [`display`]: Formatting and display utilities for solar calculations
//! - [`solar`]: Astronomical calculations for sunrise/sunset with extreme latitude handling
//! - [`times`]: Transition time management with full timezone and date context
//! - [`timezone`]: Automatic location detection based on system timezone settings
//! - [`workflow`]: Orchestration logic for the geo command workflow
//!
//! ## Key Features
//!
//! - **Interactive city selection**: Users can search and select from a comprehensive
//!   database of world cities for precise coordinate determination
//! - **Automatic location detection**: Falls back to timezone-based detection when
//!   manual selection is not desired
//! - **Solar calculations**: Precise sunrise/sunset times with enhanced twilight
//!   transitions using custom elevation angles (+10° to -2°)
//! - **Extreme latitude handling**: Automatic fallback for polar regions where
//!   standard astronomical calculations fail
//! - **Timezone-aware transitions**: Properly handles transitions across date boundaries
//!   and different timezones, displaying both local and coordinate times when they differ

pub mod city_selector;
pub mod display;
pub mod solar;
pub mod times;
pub mod timezone;
pub mod workflow;

// Re-exports for public API
pub use city_selector::select_city_interactive;
pub use display::log_solar_debug_info;
pub use times::GeoTimes;
pub use timezone::detect_coordinates_from_timezone;
pub use workflow::{ConfigTarget, GeoWorkflow};

#[cfg(test)]
mod tests;

/// Result of the geo selection workflow.
#[derive(Debug)]
pub enum GeoSelectionResult {
    /// Configuration was updated, instance needs restart
    ConfigUpdated { needs_restart: bool },
    /// No instance running, start new with given debug mode
    StartNew { debug: bool },
    /// User cancelled the selection
    Cancelled,
}

/// Result of the geo command execution.
///
/// This enum communicates to the CLI dispatcher what action should be taken
/// after the geo selection completes, enabling proper coordination between
/// the interactive city selector and the main application lifecycle.
#[derive(Debug)]
pub enum GeoCommandResult {
    /// Restart the application in debug mode without creating a new lock.
    ///
    /// The previous state will be used to ensure smooth color temperature
    /// transitions when restarting after configuration changes.
    RestartInDebugMode {
        previous_state: Option<crate::core::period::Period>,
    },
    /// Start a new instance in debug mode with lock creation
    StartNewInDebugMode,
    /// Command completed successfully, no further action needed
    Completed,
}

/// Run the geographic location selection workflow.
///
/// This function provides the main entry point for the geo command,
/// orchestrating the entire selection process including:
/// - Checking for running instances
/// - Detecting active presets
/// - Running interactive city selection
/// - Updating configuration
///
/// # Arguments
/// * `debug_enabled` - Whether debug mode is enabled for verbose output
///
/// # Returns
/// * `Ok(GeoSelectionResult)` - The result of the selection workflow
/// * `Err(_)` - If the workflow encounters an error
pub fn run_geo_workflow(debug_enabled: bool) -> anyhow::Result<GeoSelectionResult> {
    GeoWorkflow::new(debug_enabled).run()
}
