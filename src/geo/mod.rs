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

#[derive(Debug)]
pub enum GeoSelectionResult {
    Updated,
    Cancelled,
}

/// Run the interactive geo location workflow.
///
/// `target` names the config to write (the default config or a preset) and
/// skips the which-config picker. None runs the picker.
pub fn run_geo_workflow(
    debug_enabled: bool,
    target: Option<String>,
) -> anyhow::Result<GeoSelectionResult> {
    GeoWorkflow::new(debug_enabled, target).run()
}
