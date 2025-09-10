//! Signal handling and inter-process communication for sunsetr.
//!
//! This module provides signal-based communication between sunsetr instances,
//! handling configuration reloads, test mode activation, and process management.

use anyhow::{Context, Result};
use signal_hook::{
    consts::signal::{SIGHUP, SIGINT, SIGTERM, SIGUSR1, SIGUSR2},
    iterator::Signals,
};
use std::{
    sync::Arc,
    sync::atomic::{AtomicBool, Ordering},
    thread,
};

/// Test mode parameters passed via signal
#[derive(Debug, Clone)]
pub struct TestModeParams {
    pub temperature: u32,
    pub gamma: f32,
}

/// Unified signal message type for all signal-based communication
#[derive(Debug, Clone)]
pub enum SignalMessage {
    /// Configuration reload signal (SIGUSR2)
    Reload,
    /// Test mode signal with parameters (SIGUSR1)
    TestMode(TestModeParams),
    /// Shutdown signal (SIGTERM, SIGINT, SIGHUP)
    Shutdown,
    /// Time change detected - always reload regardless of config/state
    TimeChange,
    /// Sleep event detected (going to sleep or resuming)
    Sleep { resuming: bool },
}

/// Signal handling state shared between threads
pub struct SignalState {
    /// Atomic flag indicating if the application should keep running
    pub running: Arc<AtomicBool>,
    /// Channel receiver for unified signal messages
    pub signal_receiver: std::sync::mpsc::Receiver<SignalMessage>,
    /// Channel sender for unified signal messages (for D-Bus integration)
    pub signal_sender: std::sync::mpsc::Sender<SignalMessage>,
    /// Flag indicating state needs to be reloaded after config change
    pub needs_reload: Arc<AtomicBool>,
    /// Flag indicating if we're currently in test mode
    pub in_test_mode: Arc<AtomicBool>,
    /// Current active preset name (if any)
    pub current_preset: Arc<std::sync::Mutex<Option<String>>>,
}

/// Handle a signal message received in the main loop
pub fn handle_signal_message(
    signal_msg: SignalMessage,
    backend: &mut Box<dyn crate::backend::ColorTemperatureBackend>,
    config: &mut crate::config::Config,
    signal_state: &SignalState,
    current_state: &mut crate::time_state::TimeState,
    debug_enabled: bool,
) -> Result<()> {
    match signal_msg {
        SignalMessage::TestMode(test_params) => {
            // Check if we're already in test mode
            if signal_state.in_test_mode.load(Ordering::Relaxed) {
                log_pipe!();
                log_warning!("Already in test mode - ignoring new test request");
                log_indented!("Exit the current test mode first (press Escape)");
                log_end!();
                return Ok(());
            }

            #[cfg(debug_assertions)]
            {
                eprintln!(
                    "DEBUG: Main loop received test signal: {}K @ {}%",
                    test_params.temperature, test_params.gamma
                );
                let log_msg = format!(
                    "Main loop received test signal: {}K @ {}%\n",
                    test_params.temperature, test_params.gamma
                );
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                    .and_then(|mut f| {
                        use std::io::Write;
                        f.write_all(log_msg.as_bytes())
                    });
            }

            // Set test mode flag
            signal_state.in_test_mode.store(true, Ordering::Relaxed);

            // Enter test mode loop (blocks until test mode exits)
            let result = crate::commands::test::run_test_mode_loop(
                test_params,
                backend,
                signal_state,
                config,
                debug_enabled,
            );

            // Clear test mode flag when exiting
            signal_state.in_test_mode.store(false, Ordering::Relaxed);

            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Returned from test mode loop, resuming main loop");

            result?;
        }
        SignalMessage::Shutdown => {
            #[cfg(debug_assertions)]
            {
                eprintln!("DEBUG: Main loop received shutdown signal");
                let log_msg = "Main loop received shutdown signal\n".to_string();
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                    .and_then(|mut f| {
                        use std::io::Write;
                        f.write_all(log_msg.as_bytes())
                    });
            }

            // Set running to false to trigger main loop exit
            signal_state.running.store(false, Ordering::SeqCst);
        }
        SignalMessage::Reload => {
            #[cfg(debug_assertions)]
            {
                eprintln!(
                    "DEBUG: Main loop processing reload message, PID: {}",
                    std::process::id()
                );
                let log_msg = format!(
                    "Main loop processing reload message, PID: {}\n",
                    std::process::id()
                );
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                    .and_then(|mut f| {
                        use std::io::Write;
                        f.write_all(log_msg.as_bytes())
                    });
            }

            // Get the old preset from our stored state
            let old_preset = signal_state.current_preset.lock().unwrap().clone();

            // Reload configuration
            match crate::config::Config::load() {
                Ok(new_config) => {
                    // Clone the old config before replacing it
                    let old_config = config.clone();

                    #[cfg(debug_assertions)]
                    {
                        eprintln!(
                            "DEBUG: Config reload - old coords: lat={:?}, lon={:?}, new coords: lat={:?}, lon={:?}",
                            old_config.latitude,
                            old_config.longitude,
                            new_config.latitude,
                            new_config.longitude
                        );
                        let log_msg = format!(
                            "Config reload - old coords: lat={:?}, lon={:?}, new coords: lat={:?}, lon={:?}\n",
                            old_config.latitude,
                            old_config.longitude,
                            new_config.latitude,
                            new_config.longitude
                        );
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                            .and_then(|mut f| {
                                use std::io::Write;
                                f.write_all(log_msg.as_bytes())
                            });
                    }

                    // Check if the entire config changed (not just state)
                    let config_changed = old_config != new_config;

                    // Replace config with new loaded config
                    *config = new_config;

                    // For geo mode, we can't calculate the correct state here without GeoTransitionTimes
                    // So we'll let the main loop handle it properly
                    let is_geo_mode = config.transition_mode.as_deref() == Some("geo");

                    // Only calculate new state for non-geo modes
                    let new_state = if !is_geo_mode {
                        crate::time_state::get_transition_state(config, None)
                    } else {
                        // For geo mode, keep the current state - main loop will recalculate with geo_times
                        *current_state
                    };

                    #[cfg(debug_assertions)]
                    {
                        let old_state = *current_state;
                        eprintln!(
                            "DEBUG: Config changed: {}, State transition - old: {old_state:?}, new: {new_state:?}, geo_mode: {is_geo_mode}",
                            config_changed
                        );
                        let log_msg = format!(
                            "Config changed: {}, State transition - old: {old_state:?}, new: {new_state:?}, geo_mode: {is_geo_mode}\n",
                            config_changed
                        );
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                            .and_then(|mut f| {
                                use std::io::Write;
                                f.write_all(log_msg.as_bytes())
                            });
                    }

                    // Apply state if either the config changed OR the state changed
                    // For geo mode, always reload if config changed (even if we can't calculate state here)
                    if config_changed || (!is_geo_mode && *current_state != new_state) {
                        if config_changed {
                            log_indented!("Configuration changed, applying changes...");

                            // Check if there's an active preset and announce it
                            let new_preset =
                                crate::config::loading::get_active_preset().ok().flatten();
                            match (&old_preset, &new_preset) {
                                (Some(old), None) => {
                                    // Preset was deactivated
                                    log_pipe!();
                                    log_info!(
                                        "Deactivated preset '{}', restored default configuration",
                                        old
                                    );
                                }
                                (_, Some(new)) => {
                                    // Preset was activated or changed
                                    log_pipe!();
                                    log_info!("Active preset: {}", new);
                                }
                                (None, None) => {
                                    // No preset before or after - just a regular config change
                                }
                            }

                            // Update the stored preset
                            *signal_state.current_preset.lock().unwrap() = new_preset;
                        } else {
                            log_indented!("State changed after config reload, applying changes...");
                        }

                        // Set flag to trigger state reapplication in main loop
                        // This allows the main loop to handle startup transitions properly
                        signal_state.needs_reload.store(true, Ordering::SeqCst);

                        #[cfg(debug_assertions)]
                        {
                            eprintln!("DEBUG: Set needs_reload flag after config/state change");
                            let log_msg = "Set needs_reload flag after config/state change\n";
                            let _ = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                                .and_then(|mut f| {
                                    use std::io::Write;
                                    f.write_all(log_msg.as_bytes())
                                });
                        }

                        // IMPORTANT: Do NOT update current_state here when needs_reload is set!
                        // The main loop needs to compare the OLD state (before config change)
                        // with the NEW state (after config change) to determine if startup
                        // transitions should be applied. Updating current_state here would
                        // make them appear identical, preventing startup transitions.
                    } else {
                        log_indented!("Configuration and state unchanged");
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "DEBUG: State unchanged after config reload - old: {current_state:?}, new: {new_state:?}"
                        );

                        // Safe to update current_state here since nothing changed
                        // This keeps our state tracking accurate for non-geo modes
                        if !is_geo_mode {
                            *current_state = new_state;
                        }
                    }
                }
                Err(e) => {
                    log_pipe!();
                    log_error!("Failed to reload config: {e}");
                }
            }
        }
        SignalMessage::TimeChange => {
            // Time change detected - always reload regardless of config/state
            #[cfg(debug_assertions)]
            {
                eprintln!("DEBUG: Main loop processing time change message");
            }

            // Simply set the reload flag - the main loop will handle recalculating
            // everything with the new system time
            signal_state.needs_reload.store(true, Ordering::SeqCst);

            // Note: We don't need to reload config or check for changes here
            // The time change was already logged by the detector
        }
        SignalMessage::Sleep { resuming } => {
            // Sleep event detected
            #[cfg(debug_assertions)]
            {
                eprintln!(
                    "DEBUG: Main loop processing sleep message, resuming: {}",
                    resuming
                );
            }

            if resuming {
                // System is resuming from sleep - trigger reload
                signal_state.needs_reload.store(true, Ordering::SeqCst);
            }
            // If going to sleep, we don't need to do anything
        }
    }

    Ok(())
}

/// Set up signal handling for the application.
///
/// Returns a SignalState containing the running flag and signal receiver channel.
/// Spawns a background thread that monitors for signals and sends appropriate
/// messages via the channel.
pub fn setup_signal_handler(debug_enabled: bool) -> Result<SignalState> {
    let running = Arc::new(AtomicBool::new(true));
    let in_test_mode = Arc::new(AtomicBool::new(false));
    let (signal_sender, signal_receiver) = std::sync::mpsc::channel::<SignalMessage>();

    let mut signals = Signals::new([SIGINT, SIGTERM, SIGHUP, SIGUSR1, SIGUSR2])
        .context("failed to register signal handlers")?;

    let running_clone = running.clone();
    let signal_sender_clone = signal_sender.clone();

    thread::spawn(move || {
        #[cfg(debug_assertions)]
        {
            eprintln!(
                "DEBUG: Signal handler setup complete for PID: {}",
                std::process::id()
            );
            let log_msg = format!(
                "Signal handler setup complete for PID: {}\n",
                std::process::id()
            );
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                .and_then(|mut f| {
                    use std::io::Write;
                    f.write_all(log_msg.as_bytes())
                });
        }

        #[cfg(debug_assertions)]
        {
            eprintln!(
                "DEBUG: Signal handler thread starting for PID: {}",
                std::process::id()
            );
            let log_msg = format!(
                "Signal handler thread starting for PID: {}\n",
                std::process::id()
            );
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                .and_then(|mut f| {
                    use std::io::Write;
                    f.write_all(log_msg.as_bytes())
                });
        }

        #[cfg(debug_assertions)]
        let mut signal_count = 0;
        #[cfg(debug_assertions)]
        let mut sigusr2_count = 0;

        for sig in signals.forever() {
            #[cfg(debug_assertions)]
            {
                signal_count += 1;
            }

            #[cfg(debug_assertions)]
            {
                let log_msg = format!("Signal handler processing signal #{signal_count}: {sig}\n");
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                    .and_then(|mut f| {
                        use std::io::Write;
                        f.write_all(log_msg.as_bytes())
                    });
            }

            match sig {
                SIGUSR1 => {
                    // SIGUSR1 is used for test mode
                    // Read test parameters from temp file
                    let test_file_path = format!("/tmp/sunsetr-test-{}.tmp", std::process::id());
                    match std::fs::read_to_string(&test_file_path) {
                        Ok(content) => {
                            let lines: Vec<&str> = content.trim().lines().collect();
                            if lines.len() == 2
                                && let (Ok(temp), Ok(gamma)) =
                                    (lines[0].parse::<u32>(), lines[1].parse::<f32>())
                            {
                                // Log different messages based on whether this is enter or exit
                                log_pipe!();
                                if temp == 0 {
                                    log_info!("Received test mode exit signal");
                                } else {
                                    log_info!("Received test mode signal");
                                }

                                let test_params = TestModeParams {
                                    temperature: temp,
                                    gamma,
                                };

                                match signal_sender_clone.send(SignalMessage::TestMode(test_params))
                                {
                                    Ok(()) => {
                                        #[cfg(debug_assertions)]
                                        {
                                            eprintln!(
                                                "DEBUG: Test mode parameters sent: {temp}K @ {gamma}%"
                                            );
                                        }
                                    }
                                    Err(_) => {
                                        #[cfg(debug_assertions)]
                                        {
                                            eprintln!(
                                                "DEBUG: Failed to send test parameters - channel disconnected"
                                            );
                                        }
                                        break;
                                    }
                                }
                            }
                            // Clean up temp file
                            let _ = std::fs::remove_file(&test_file_path);
                        }
                        Err(_) => {
                            #[cfg(debug_assertions)]
                            {
                                eprintln!(
                                    "DEBUG: Failed to read test parameters from {test_file_path}"
                                );
                            }
                        }
                    }
                }
                SIGUSR2 => {
                    #[cfg(debug_assertions)]
                    {
                        sigusr2_count += 1;
                    }

                    // SIGUSR2 is used for config reload
                    #[cfg(debug_assertions)]
                    {
                        eprintln!(
                            "DEBUG: SIGUSR2 #{} received by PID: {}, sending reload message",
                            sigusr2_count,
                            std::process::id()
                        );
                        let log_msg = format!(
                            "SIGUSR2 #{} received by PID: {}, sending reload message\n",
                            sigusr2_count,
                            std::process::id()
                        );
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                            .and_then(|mut f| {
                                use std::io::Write;
                                f.write_all(log_msg.as_bytes())
                            });
                    }

                    log_pipe!();
                    log_info!("Received configuration reload signal");

                    // Send reload message via channel (non-blocking)
                    match signal_sender_clone.send(SignalMessage::Reload) {
                        Ok(()) => {
                            #[cfg(debug_assertions)]
                            {
                                eprintln!(
                                    "DEBUG: Reload message #{sigusr2_count} sent successfully"
                                );
                                let log_msg =
                                    format!("Reload message #{sigusr2_count} sent successfully\n");
                                let _ = std::fs::OpenOptions::new()
                                    .create(true)
                                    .append(true)
                                    .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                                    .and_then(|mut f| {
                                        use std::io::Write;
                                        f.write_all(log_msg.as_bytes())
                                    });
                            }
                        }
                        Err(_e) => {
                            // Channel receiver was dropped - main thread probably exiting
                            #[cfg(debug_assertions)]
                            {
                                eprintln!(
                                    "DEBUG: Failed to send reload message #{sigusr2_count}: {_e:?} - channel disconnected"
                                );
                                let log_msg = format!(
                                    "Failed to send reload message #{sigusr2_count}: {_e:?} - channel disconnected\n"
                                );
                                let _ = std::fs::OpenOptions::new()
                                    .create(true)
                                    .append(true)
                                    .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                                    .and_then(|mut f| {
                                        use std::io::Write;
                                        f.write_all(log_msg.as_bytes())
                                    });
                            }

                            // Channel is disconnected, break out of signal loop
                            #[cfg(debug_assertions)]
                            {
                                let log_msg = format!(
                                    "Signal handler thread exiting due to channel disconnection after {signal_count} signals ({sigusr2_count} SIGUSR2)\n"
                                );
                                let _ = std::fs::OpenOptions::new()
                                    .create(true)
                                    .append(true)
                                    .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                                    .and_then(|mut f| {
                                        use std::io::Write;
                                        f.write_all(log_msg.as_bytes())
                                    });
                            }
                            break;
                        }
                    }
                }
                _ => {
                    #[cfg(debug_assertions)]
                    {
                        let signal_name = match sig {
                            SIGINT => "SIGINT (Ctrl+C)",
                            SIGTERM => "SIGTERM (termination request)",
                            SIGHUP => "SIGHUP (session logout)",
                            _ => "unknown signal",
                        };
                        eprintln!(
                            "DEBUG: Received {signal_name} (signal #{signal_count}), setting running=false"
                        );
                        let log_msg = format!(
                            "Received {signal_name} (signal #{signal_count}), setting running=false\n"
                        );
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                            .and_then(|mut f| {
                                use std::io::Write;
                                f.write_all(log_msg.as_bytes())
                            });
                    }

                    // Always log shutdown signals for user clarity
                    let user_message = match sig {
                        SIGINT => {
                            if debug_enabled {
                                "Received SIGINT (Ctrl+C), initiating graceful shutdown..."
                            } else {
                                "Received interrupt signal, initiating graceful shutdown..."
                            }
                        }
                        SIGTERM => "Received termination request, initiating graceful shutdown...",
                        SIGHUP => "Received hangup signal, initiating graceful shutdown...",
                        _ => "Received shutdown signal, initiating graceful shutdown...",
                    };

                    log_pipe!();
                    log_info!("{}", user_message);

                    // Send shutdown message to main loop first
                    if let Err(e) = signal_sender_clone.send(SignalMessage::Shutdown) {
                        log_pipe!();
                        log_warning!("Failed to send shutdown message: {e}");
                        log_indented!("Cleanup will rely on fallback mechanisms");
                    }

                    // For shutdown signals, set the flag to stop
                    running_clone.store(false, Ordering::SeqCst);

                    // Note: We don't do emergency cleanup here anymore because it interferes
                    // with the normal cleanup path trying to reset gamma to 6500K.
                    // The Drop trait and normal cleanup should handle most cases.

                    #[cfg(debug_assertions)]
                    {
                        let log_msg = format!(
                            "Signal handler set running=false after {signal_count} signals ({sigusr2_count} SIGUSR2), continuing signal processing\n"
                        );
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                            .and_then(|mut f| {
                                use std::io::Write;
                                f.write_all(log_msg.as_bytes())
                            });
                    }

                    // Continue processing signals until process exits
                    // Don't break - keep signal thread alive
                }
            }
        }
    });

    // Get the initial preset if any
    let initial_preset = crate::config::loading::get_active_preset().ok().flatten();

    Ok(SignalState {
        running,
        signal_receiver,
        signal_sender,
        needs_reload: Arc::new(AtomicBool::new(false)),
        in_test_mode,
        current_preset: Arc::new(std::sync::Mutex::new(initial_preset)),
    })
}
