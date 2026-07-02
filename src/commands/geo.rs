//! Interactive city selection that writes the chosen coordinates to the configuration.

use anyhow::Result;

pub fn handle_geo_command(debug_enabled: bool, target: Option<String>) -> Result<()> {
    if crate::io::instance::is_test_mode_active() {
        log_error_end!(
            "Cannot change location while test mode is active\n   Exit test mode first (press Escape in the test terminal)"
        );
        return Ok(());
    }

    match crate::geo::run_geo_workflow(debug_enabled, target)? {
        crate::geo::GeoSelectionResult::Updated => {
            log_block_start!("Configuration updated.");
            log_end!();
            Ok(())
        }
        crate::geo::GeoSelectionResult::Cancelled => {
            log_block_start!("City selection cancelled.");
            log_end!();
            Ok(())
        }
    }
}

pub fn show_usage() {
    log_version!();
    log_block_start!("Usage: sunsetr geo [OPTIONS]");
    log_block_start!("Options:");
    log_indented!("-t, --target <name>  Target configuration to update");
    log_indented!("                     'default' = base configuration");
    log_indented!("                     <name> = named preset");
    log_pipe!();
    log_info!("For detailed help with examples, try: sunsetr help geo");
    log_end!();
}

pub fn display_help() {
    log_version!();
    log_block_start!("Interactive city selection for geographic mode");
    log_block_start!("Usage: sunsetr geo [OPTIONS]");
    log_block_start!("Options:");
    log_indented!("-t, --target <name>  Target configuration to update");
    log_indented!("                     'default' = base configuration");
    log_indented!("                     <name> = named preset");
    log_block_start!("Features:");
    log_indented!("- Search by city name (partial matching)");
    log_indented!("- Filter results by city/country");
    log_indented!("- Real-time sunrise/sunset calculations");
    log_indented!("- Privacy-focused geo.toml option");
    log_block_start!("Interactive Controls:");
    log_indented!("- Type to search for cities");
    log_indented!("- Arrow keys to navigate results");
    log_indented!("- Enter to select a city");
    log_indented!("- Escape to cancel");
    log_block_start!("Configuration:");
    log_indented!("Selected location is saved to:");
    log_indented!("- geo.toml (if it exists), gitignored for privacy");
    log_indented!("- config.toml (otherwise), the standard location");
    log_indented!("Use --target to update the default config or a named preset.");
    log_block_start!("Examples:");
    log_indented!("# Basic city selection");
    log_indented!("sunsetr geo");
    log_pipe!();
    log_indented!("# With debug output for troubleshooting");
    log_indented!("sunsetr --debug geo");
    log_pipe!();
    log_indented!("# Update a specific preset's coordinates");
    log_indented!("sunsetr geo --target gaming");
    log_pipe!();
    log_indented!("# Update the default config in a custom base directory");
    log_indented!("sunsetr --config ~/.dotfiles/sunsetr/ geo --target default");
    log_end!();
}
