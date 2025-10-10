//! Implementation of the stop command.
//!
//! This command cleanly terminates a running sunsetr instance by sending
//! SIGTERM and providing user feedback about the shutdown process.

use crate::common::constants::*;
use crate::config::Config;
use anyhow::Result;

/// Result of a stop command operation.
#[derive(Debug, PartialEq)]
pub enum StopResult {
    /// Successfully signaled running instance to stop
    Stopped,
    /// No running instance found
    NoInstanceRunning,
    /// Failed to stop the instance
    Failed(String),
}

/// Handle the stop command to terminate a running sunsetr instance.
pub fn handle_stop_command(debug_enabled: bool) -> Result<()> {
    log_version!();

    // Load configuration to check smoothing settings
    let config = Config::load()?;

    // Get running instance (ignore test mode - force stop)
    match crate::io::instance::get_running_instance_pid() {
        Ok(pid) => {
            log_block_start!("Stopping sunsetr instance (PID: {})...", pid);

            match crate::io::instance::terminate_instance(pid) {
                Ok(()) => {
                    if debug_enabled {
                        log_pipe!();
                        log_debug!("SIGTERM sent to process {}", pid);
                    }

                    // Detect the backend being used
                    let resolved_backend = crate::backend::detect_backend(&config)?;

                    // Only Wayland backend supports our custom smoothing transitions
                    // (Hyprland uses native CTM animations, Hyprsunset doesn't support smoothing)
                    let backend_supports_smoothing =
                        matches!(resolved_backend, crate::backend::BackendType::Wayland);
                    let smoothing_enabled = config.smoothing.unwrap_or(DEFAULT_SMOOTHING);
                    let shutdown_duration = config
                        .shutdown_duration
                        .unwrap_or(DEFAULT_SHUTDOWN_DURATION);

                    // Show shutdown message if smooth transition is active
                    if backend_supports_smoothing && smoothing_enabled && shutdown_duration >= 0.1 {
                        log_block_start!("Shutting down...");
                    }

                    // Calculate timeout: base 3 seconds + shutdown_duration if applicable
                    let base_timeout_ms = 3000u64;
                    let additional_timeout_ms = if backend_supports_smoothing
                        && smoothing_enabled
                        && shutdown_duration >= 0.1
                    {
                        (shutdown_duration * 1000.0) as u64
                    } else {
                        0
                    };
                    let total_timeout_ms = base_timeout_ms + additional_timeout_ms;
                    let max_attempts = total_timeout_ms / 100; // 100ms intervals

                    // Hide cursor during termination wait
                    let _terminal_guard = crate::common::utils::TerminalGuard::new();

                    // Wait for process to actually terminate
                    let mut attempts = 0;

                    while attempts < max_attempts {
                        if !crate::io::instance::is_instance_running(pid) {
                            log_pipe!();
                            log_info!("Process terminated successfully");
                            log_end!();
                            return Ok(());
                        }

                        std::thread::sleep(std::time::Duration::from_millis(100));
                        attempts += 1;
                    }

                    // If we get here, the process didn't terminate within the timeout
                    log_pipe!();
                    log_warning!("Process did not terminate within the expected time");
                    log_indented!(
                        "The termination signal was sent, but the process may still be shutting down"
                    );
                    log_end!();
                }
                Err(e) => {
                    log_error_exit!("Failed to terminate instance: {}", e);
                }
            }
        }
        Err(_) => {
            log_error_exit!("sunsetr isn't running");
        }
    }
    Ok(())
}

/// Display usage help for the stop command (--help flag)
pub fn show_usage() {
    log_version!();
    log_block_start!("Usage: sunsetr stop");
    log_block_start!("Description:");
    log_indented!("Cleanly terminate the running sunsetr instance");
    log_pipe!();
    log_info!("For detailed help with examples, try: sunsetr help stop");
    log_end!();
}

/// Display detailed help for the stop command (help subcommand)
pub fn display_help() {
    log_version!();
    log_block_start!("stop - Cleanly terminate running sunsetr");
    log_block_start!("Usage: sunsetr stop");
    log_block_start!("Description:");
    log_indented!("Sends a termination signal to the running sunsetr instance,");
    log_indented!("allowing it to shut down gracefully and reset display gamma.");
    log_indented!("Waits up to 3 seconds to confirm the process actually terminates.");
    log_block_start!("Process:");
    log_indented!("1. Locates the running sunsetr process");
    log_indented!("2. Sends SIGTERM for graceful shutdown");
    log_indented!("3. Waits for confirmation that the process terminated");
    log_indented!("4. Reports successful termination and gamma reset");
    log_block_start!("Examples:");
    log_indented!("# Stop running sunsetr");
    log_indented!("sunsetr stop");
    log_pipe!();
    log_indented!("# Stop with debug output");
    log_indented!("sunsetr --debug stop");
    log_end!();
}
