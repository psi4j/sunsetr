//! Implementation of the stop command.
//!
//! This command cleanly terminates a running sunsetr instance by sending
//! SIGTERM and providing user feedback about the shutdown process.

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

                    // Wait for process to actually terminate (up to 3 seconds)
                    let mut attempts = 0;
                    const MAX_ATTEMPTS: u32 = 30; // 3 seconds with 100ms intervals

                    while attempts < MAX_ATTEMPTS {
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
                    log_warning!("Process did not terminate within 3 seconds");
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
