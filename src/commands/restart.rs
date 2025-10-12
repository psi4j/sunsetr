//! Implementation of the restart command.
//!
//! This command stops a running sunsetr instance and starts a fresh one,
//! providing a solution for manual state recovery and backend reinitialization.

use anyhow::Result;

/// Result of a restart command operation.
#[derive(Debug, PartialEq)]
pub enum RestartResult {
    /// Successfully restarted existing instance
    Restarted,
    /// Started fresh instance (no existing instance found)
    StartedFresh,
}

/// Handle the restart command using stop-wait-start sequence.
pub fn handle_restart_command(instant: bool, debug_enabled: bool, background: bool) -> Result<()> {
    log_version!();

    // Check if test mode is active
    if crate::io::instance::is_test_mode_active() {
        log_pipe!();
        log_warning!("Cannot restart while test mode is active");
        log_indented!("Exit test mode first (press Escape in the test terminal)");
        log_end!();
        return Ok(());
    }

    match crate::io::instance::get_running_instance_pid() {
        Ok(pid) => {
            // Existing process - stop it, wait for termination, then start fresh
            log_pipe!();
            log_info!("Restarting sunsetr instance (PID: {})...", pid);

            // Step 1: Send appropriate termination signal
            let termination_result = if instant {
                // For instant restart, signal the process to skip smooth shutdown
                match crate::io::instance::send_instant_shutdown_signal(pid) {
                    Ok(()) => {
                        if debug_enabled {
                            log_pipe!();
                            log_debug!("Instant shutdown signal sent successfully");
                        }
                        Ok(())
                    }
                    Err(e) => {
                        log_warning!("Failed to send instant shutdown signal: {}", e);
                        if debug_enabled {
                            log_pipe!();
                            log_debug!("Falling back to normal termination...");
                        }
                        // Fall back to normal termination
                        crate::io::instance::terminate_instance(pid)
                    }
                }
            } else {
                // Normal restart - use standard termination
                match crate::io::instance::terminate_instance(pid) {
                    Ok(()) => {
                        if debug_enabled {
                            log_pipe!();
                            log_debug!("Termination signal sent successfully");
                        }
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            };

            match termination_result {
                Ok(()) => {
                    // Step 2: Wait for process to actually terminate

                    // Calculate timeout using same logic as stop command
                    // Load config to check shutdown duration and smoothing settings
                    let (total_timeout_ms, show_shutdown_message) =
                        match crate::config::Config::load() {
                            Ok(config) => {
                                let resolved_backend = crate::backend::detect_backend(&config)
                                    .unwrap_or(crate::backend::BackendType::Wayland);
                                let backend_supports_smoothing = matches!(
                                    resolved_backend,
                                    crate::backend::BackendType::Wayland
                                );
                                let smoothing_enabled = config
                                    .smoothing
                                    .unwrap_or(crate::common::constants::DEFAULT_SMOOTHING);
                                let shutdown_duration = config
                                    .shutdown_duration
                                    .unwrap_or(crate::common::constants::DEFAULT_SHUTDOWN_DURATION);

                                let base_timeout_ms = 3000u64;
                                let additional_timeout_ms = if backend_supports_smoothing
                                    && smoothing_enabled
                                    && shutdown_duration >= 0.1
                                {
                                    (shutdown_duration * 1000.0) as u64
                                } else {
                                    0
                                };
                                let total = base_timeout_ms + additional_timeout_ms;
                                let show_msg = backend_supports_smoothing
                                    && smoothing_enabled
                                    && shutdown_duration >= 0.1;
                                (total, show_msg)
                            }
                            Err(_) => {
                                // Fallback to base timeout if config load fails
                                (3000u64, false)
                            }
                        };

                    // Show shutdown message if smooth transition is active
                    if show_shutdown_message && debug_enabled {
                        log_indented!("Shutting down...");
                    }

                    let max_attempts = total_timeout_ms / 100; // 100ms intervals
                    let mut attempts = 0;

                    while attempts < max_attempts {
                        if !crate::io::instance::is_instance_running(pid) {
                            if debug_enabled {
                                log_pipe!();
                                log_debug!("Previous instance stopped successfully");
                            }
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        attempts += 1;
                    }

                    if crate::io::instance::is_instance_running(pid) {
                        log_pipe!();
                        log_warning!("Previous instance did not stop within timeout");
                        log_indented!("Continuing with restart anyway...");
                    }
                }
                Err(e) => {
                    log_pipe!();
                    log_warning!("Failed to terminate instance: {}", e);
                    log_indented!("The process may no longer be running, continuing...");
                }
            }

            // Step 3: Start fresh instance
            if debug_enabled {
                log_pipe!();
                log_debug!("Starting fresh instance...");
            }
        }
        Err(_) => {
            // No existing process - start fresh (foreground by default)
            if debug_enabled {
                log_pipe!();
                log_debug!("No running instance found, starting fresh...");
            }
        }
    }

    // Check if instant flag is used with non-Wayland backend (provide helpful guidance)
    if instant {
        // Load config to detect backend
        match crate::config::Config::load() {
            Ok(config) => {
                match crate::backend::detect_backend(&config) {
                    Ok(backend_type) => {
                        if !matches!(backend_type, crate::backend::BackendType::Wayland) {
                            log_pipe!();
                            log_warning!(
                                "The --instant flag has no effect with Hyprland-based backends"
                            );
                            log_indented!(
                                "Hyprland handles color temperature transitions natively"
                            );
                            log_indented!(
                                "To disable smooth transitions, set 'ctm_animations = 0' in hyprland.conf"
                            );
                        }
                    }
                    Err(_) => {
                        // Backend detection failed, continue without warning
                    }
                }
            }
            Err(_) => {
                // Config load failed, continue without warning
            }
        }
    }

    // Start new instance (unified path for both cases)
    let sunsetr = crate::Sunsetr::new(debug_enabled).without_headers();
    let sunsetr = if instant {
        // Skip all smooth transitions for instant restart
        sunsetr.bypass_smoothing()
    } else {
        sunsetr
    };
    let sunsetr = if background {
        // Run in background mode
        sunsetr.background()
    } else {
        sunsetr
    };

    sunsetr.run() // Return early to avoid duplicate log_end!()
}

/// Display usage help for the restart command (--help flag)
pub fn show_usage() {
    log_version!();
    log_block_start!("Usage: sunsetr restart [--instant]");
    log_block_start!("Description:");
    log_indented!("Stop running instance and start fresh for state recovery");
    log_pipe!();
    log_info!("For detailed help with examples, try: sunsetr help restart");
    log_end!();
}

/// Display detailed help for the restart command (help subcommand)
pub fn display_help() {
    log_version!();
    log_block_start!("restart - Stop and start fresh for state recovery");
    log_block_start!("Usage: sunsetr restart [--instant]");
    log_block_start!("Description:");
    log_indented!("Stops the running sunsetr instance and starts a fresh one,");
    log_indented!("providing a solution for state recovery and backend issues.");
    log_block_start!("Options:");
    log_indented!("--instant, -i    Skip smooth transitions for immediate effect");
    log_block_start!("Process:");
    log_indented!("1. Terminates running instance gracefully");
    log_indented!("2. Waits for confirmation of shutdown");
    log_indented!("3. Starts fresh instance with clean state");
    log_block_start!("Examples:");
    log_indented!("# Standard restart with smooth transition");
    log_indented!("sunsetr restart");
    log_pipe!();
    log_indented!("# Instant restart for state recovery");
    log_indented!("sunsetr restart --instant");
    log_end!();
}
