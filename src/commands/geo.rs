//! Handle the geo command functionality.
//!
//! This module contains the logic for the geo command that was previously
//! in geo/mod.rs. It handles the complete geo selection workflow including
//! test mode checks, configuration updates, and process management.

use anyhow::Result;

/// Handle the geo command from the CLI.
///
/// This function runs the geo workflow and then processes the result,
/// containing all the logic that was previously in main.rs for the geo command.
pub fn handle_geo_command(debug_enabled: bool) -> Result<crate::geo::GeoCommandResult> {
    // Check if sunsetr is already running
    // This will restore the config directory from the lock file if present
    let _running_pid = crate::io::instance::get_running_instance_pid().ok();

    // Check if test mode is active
    if crate::io::instance::is_test_mode_active() {
        log_pipe!();
        log_warning!("Cannot change location while test mode is active");
        log_indented!("Exit test mode first (press Escape in the test terminal)");
        log_end!();
        return Ok(crate::geo::GeoCommandResult::Completed);
    }

    // Run the geo workflow and process results
    match crate::geo::run_geo_workflow(debug_enabled)? {
        crate::geo::GeoSelectionResult::ConfigUpdated {
            needs_restart: true,
        } => {
            log_block_start!("Restarting sunsetr with new location...");

            // Handle existing process based on mode
            if let Ok(pid) = crate::io::instance::get_running_instance_pid() {
                if debug_enabled {
                    // For debug mode, we currently use None for previous_state to force a transition
                    // from day values. This ensures a visible smooth transition.
                    // TODO: Once IPC is implemented, query the actual current gamma values from the running process
                    let previous_state = None;

                    // Kill the existing process to take over the terminal
                    if crate::io::instance::terminate_instance(pid).is_ok() {
                        log_decorated!("Stopped existing sunsetr instance.");

                        // Clean up the lock file since the killed process can't do it
                        let runtime_dir =
                            std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
                        let lock_path = format!("{runtime_dir}/sunsetr.lock");
                        let _ = std::fs::remove_file(&lock_path);

                        // Give it a moment to fully exit
                        std::thread::sleep(std::time::Duration::from_millis(500));

                        // Continue in the current terminal without creating a new lock
                        log_indented!("Applying new configuration...");
                        Ok(crate::geo::GeoCommandResult::RestartInDebugMode { previous_state })
                    } else {
                        log_warning!(
                            "Failed to stop existing process. You may need to manually restart sunsetr.",
                        );
                        Ok(crate::geo::GeoCommandResult::Completed)
                    }
                } else {
                    // For non-debug mode, send SIGUSR2 to reload configuration
                    #[cfg(debug_assertions)]
                    eprintln!("DEBUG: Sending SIGUSR2 to PID: {pid}");

                    match crate::io::instance::send_reload_signal(pid) {
                        Ok(()) => {
                            #[cfg(debug_assertions)]
                            eprintln!("DEBUG: SIGUSR2 sent successfully to PID: {pid}");

                            log_decorated!("Sent reload signal to existing sunsetr instance.");
                            log_indented!("Configuration will be reloaded automatically.");
                            log_end!();
                            Ok(crate::geo::GeoCommandResult::Completed)
                        }
                        Err(e) => {
                            #[cfg(debug_assertions)]
                            eprintln!("DEBUG: Failed to send SIGUSR2 to PID {pid}: {e}");

                            log_warning!("Failed to signal existing process: {e}");
                            log_indented!("You may need to manually restart sunsetr.");
                            Ok(crate::geo::GeoCommandResult::Completed)
                        }
                    }
                }
            } else {
                log_warning!(
                    "Could not find running sunsetr process. You may need to manually restart sunsetr.",
                );
                Ok(crate::geo::GeoCommandResult::Completed)
            }
        }
        crate::geo::GeoSelectionResult::ConfigUpdated {
            needs_restart: false,
        } => {
            // This shouldn't happen in current implementation, but handle it gracefully
            log_decorated!("Configuration updated.");
            Ok(crate::geo::GeoCommandResult::Completed)
        }
        crate::geo::GeoSelectionResult::StartNew { debug } => {
            // Start sunsetr with the new configuration
            if debug {
                // Run in foreground with debug mode, needs lock creation
                log_indented!("Starting sunsetr with selected location...");
                Ok(crate::geo::GeoCommandResult::StartNewInDebugMode)
            } else {
                // Spawn in background and exit
                crate::io::instance::spawn_background_instance(debug)?;
                log_end!();
                Ok(crate::geo::GeoCommandResult::Completed)
            }
        }
        crate::geo::GeoSelectionResult::Cancelled => {
            log_decorated!("City selection cancelled.");
            log_end!();
            Ok(crate::geo::GeoCommandResult::Completed)
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
