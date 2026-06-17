//! Geographic location-based sunrise/sunset calculations.
//!
//! Location-aware color temperature transitions driven by local solar events.

pub mod city_selector;
pub mod display;
pub mod solar;
pub mod times;
pub mod timezone;
pub mod workflow;

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
    /// Configuration was updated.
    Updated,
    /// User cancelled the selection.
    Cancelled,
}

/// Run the geographic location selection workflow.
///
/// This function provides the main entry point for the geo command,
/// orchestrating the entire selection process including:
/// - Detecting active presets
/// - Running interactive city selection
/// - Updating configuration
///
/// # Arguments
/// * `debug_enabled` - Whether debug mode is enabled for verbose output
/// * `target` - Explicit config target (default or a preset name), bypassing the picker
///
/// # Returns
/// * `Ok(GeoSelectionResult)` - The result of the selection workflow
/// * `Err(_)` - If the workflow encounters an error
pub fn run_geo_workflow(
    debug_enabled: bool,
    target: Option<String>,
) -> anyhow::Result<GeoSelectionResult> {
    GeoWorkflow::new(debug_enabled, target).run()
}
