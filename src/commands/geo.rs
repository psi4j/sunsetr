//! Handle the geo command functionality.
//!
//! This module contains the logic for the geo command that was previously
//! in geo/mod.rs. It handles the complete geo selection workflow including
//! test mode checks, configuration updates, and process management.

use anyhow::Result;

/// Handle the geo command from the CLI.
///
/// This function runs the geo workflow and updates configuration files.
/// The geo command is now purely a configuration tool and does not spawn processes.
pub fn handle_geo_command(debug_enabled: bool) -> Result<()> {
    // Check if sunsetr is already running
    // This will restore the config directory from the lock file if present
    let _running_pid = crate::io::instance::get_running_instance_pid().ok();

    // Check if test mode is active
    if crate::io::instance::is_test_mode_active() {
        log_error_exit!(
            "Cannot change location while test mode is active\n   Exit test mode first (press Escape in the test terminal)"
        );
        return Ok(());
    }

    // Run the geo workflow and process results
    match crate::geo::run_geo_workflow(debug_enabled)? {
        crate::geo::GeoSelectionResult::ConfigUpdated { .. } => {
            log_block_start!("Configuration updated.");
            log_end!();
            Ok(())
        }
        crate::geo::GeoSelectionResult::StartNew { .. } => {
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

/// Display usage help for the geo command (--help flag)
pub fn show_usage() {
    log_version!();
    log_block_start!("Usage: sunsetr geo");
    log_block_start!("Description:");
    log_indented!("Interactive city selection for geographic-based transitions");
    log_pipe!();
    log_info!("For detailed help with examples, try: sunsetr help geo");
    log_end!();
}

/// Display detailed help for the geo command (help subcommand)
pub fn display_help() {
    log_version!();
    log_block_start!("geo - Interactive city selection for geographic mode");
    log_block_start!("Usage: sunsetr geo");
    log_block_start!("Description:");
    log_indented!("Opens an interactive city selector to configure sunsetr for");
    log_indented!("geographic-based transitions. The command searches through a");
    log_indented!("database of over 10,000 cities worldwide and automatically");
    log_indented!("calculates sunrise and sunset times for your location.");
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
    log_indented!("- geo.toml (if it exists) - for privacy using .gitignore");
    log_indented!("- config.toml (otherwise) - standard config");
    log_block_start!("Examples:");
    log_indented!("# Basic city selection");
    log_indented!("sunsetr geo");
    log_pipe!();
    log_indented!("# With debug output for troubleshooting");
    log_indented!("sunsetr --debug geo");
    log_end!();
}
