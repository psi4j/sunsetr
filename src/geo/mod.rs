//! Geographic location-based sunrise/sunset calculations.
//!
//! Location-aware color temperature transitions driven by local solar events.

pub mod city_selector;
pub mod display;
pub mod geoclue;
pub mod solar;
pub mod times;
pub mod timezone;
pub mod workflow;

pub use city_selector::select_city_interactive;
pub use display::log_solar_debug_info;
pub use timezone::detect_coordinates_from_timezone;
pub use workflow::GeoWorkflow;

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

/// Clamp a latitude to 65 degrees, warning with `source` naming where it
/// came from. Beyond that the sun stays up or down for part of the year
/// and the transitions stop making sense.
pub fn cap_latitude(latitude: f64, source: &str) -> f64 {
    if latitude.abs() <= 65.0 {
        return latitude;
    }
    let hemisphere = if latitude >= 0.0 { "N" } else { "S" };
    log_pipe!();
    log_warning!(
        "⚠️ Latitude capped at 65°{hemisphere} ({source} {:.4}°{hemisphere})",
        latitude.abs()
    );
    log_indented!("Are you researching extremophile bacteria under the ice caps?");
    log_indented!("Consider using manual sunset/sunrise times for more sensible transitions.");
    65.0 * latitude.signum()
}
