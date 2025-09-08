//! Handle the geo command functionality.
//!
//! This module contains the logic for the geo command that was previously
//! in geo/mod.rs. It handles the complete geo selection workflow including
//! test mode checks, configuration updates, and process management.

use anyhow::Result;

/// Handle the geo command from the CLI.
///
/// This function delegates to handle_geo_selection and then processes the result,
/// containing all the logic that was previously in main.rs for the geo command.
pub fn handle_geo_command(debug_enabled: bool) -> Result<crate::geo::GeoCommandResult> {
    // Check if test mode is active
    let test_lock_path = "/tmp/sunsetr-test.lock";

    // Check if lock file exists and if the PID in it is still running
    if let Ok(contents) = std::fs::read_to_string(test_lock_path)
        && let Ok(lock_pid) = contents.trim().parse::<u32>()
    {
        // Check if the process that created the lock is still running
        let proc_path = format!("/proc/{}", lock_pid);
        if std::path::Path::new(&proc_path).exists() {
            // Process is still running, test mode is active
            log_pipe!();
            log_warning!("Cannot change location while test mode is active");
            log_indented!("Exit test mode first (press Escape in the test terminal)");
            log_end!();
            return Ok(crate::geo::GeoCommandResult::Completed);
        } else {
            // Process is dead, remove stale lock file
            let _ = std::fs::remove_file(test_lock_path);
        }
    }

    // Delegate to geo module and handle result
    match crate::geo::handle_geo_selection(debug_enabled)? {
        crate::geo::GeoSelectionResult::ConfigUpdated {
            needs_restart: true,
        } => {
            log_block_start!("Restarting sunsetr with new location...");

            // Handle existing process based on mode
            if let Ok(pid) = crate::utils::get_running_sunsetr_pid() {
                if debug_enabled {
                    // For debug mode, we currently use None for previous_state to force a transition
                    // from day values. This ensures a visible smooth transition.
                    // TODO: Once IPC is implemented, query the actual current gamma values from the running process
                    let previous_state = None;

                    // Kill the existing process to take over the terminal
                    if crate::utils::kill_process(pid) {
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
                    use nix::sys::signal::{Signal, kill};
                    use nix::unistd::Pid;

                    #[cfg(debug_assertions)]
                    eprintln!("DEBUG: Sending SIGUSR2 to PID: {pid}");

                    match kill(Pid::from_raw(pid as i32), Signal::SIGUSR2) {
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
                crate::utils::spawn_background_process(debug)?;
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
