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
    Shutdown { instant: bool },
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
    /// Flag indicating if shutdown should skip smooth transitions
    pub instant_shutdown: Arc<AtomicBool>,
    /// Current active preset name (if any)
    pub current_preset: Arc<std::sync::Mutex<Option<String>>>,
    /// Pending config loaded by signal handler for core to process
    pub pending_config: Arc<std::sync::Mutex<Option<crate::config::Config>>>,
}

/// Handle a signal message received in the main loop
pub fn handle_signal_message(
    signal_msg: SignalMessage,
    backend: &mut Box<dyn crate::backend::ColorTemperatureBackend>,
    signal_state: &SignalState,
    current_runtime_state: &crate::core::runtime_state::RuntimeState,
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
                current_runtime_state,
                debug_enabled,
            );

            // Clear test mode flag when exiting
            signal_state.in_test_mode.store(false, Ordering::Relaxed);

            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Returned from test mode loop, resuming main loop");

            result?;
        }
        SignalMessage::Shutdown { instant } => {
            #[cfg(debug_assertions)]
            {
                eprintln!(
                    "DEBUG: Main loop received shutdown signal (instant={})",
                    instant
                );
                let log_msg = format!("Main loop received shutdown signal (instant={})\n", instant);
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                    .and_then(|mut f| {
                        use std::io::Write;
                        f.write_all(log_msg.as_bytes())
                    });
            }

            // Set instant shutdown flag if needed
            signal_state
                .instant_shutdown
                .store(instant, Ordering::SeqCst);

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

            // Handle config loading with proper error handling (signal handler responsibility)
            match crate::config::Config::load() {
                Ok(new_config) => {
                    // Store the valid config for core to process
                    *signal_state.pending_config.lock().unwrap() = Some(new_config);
                    signal_state.needs_reload.store(true, Ordering::SeqCst);

                    #[cfg(debug_assertions)]
                    eprintln!("DEBUG: Config loaded successfully, setting needs_reload flag");
                }
                Err(e) => {
                    // Graceful error handling - log and continue with existing config
                    log_pipe!();
                    log_error!("Failed to reload config: {e}");
                    log_indented!("Continuing with previous configuration");

                    #[cfg(debug_assertions)]
                    eprintln!("DEBUG: Config reload failed, not setting needs_reload flag");
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
    let instant_shutdown = Arc::new(AtomicBool::new(false));
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
                    let test_file_path = format!("/tmp/sunsetr-test-{}.tmp", std::process::id());

                    if let Ok(content) = std::fs::read_to_string(&test_file_path) {
                        // Test mode logic
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

                            match signal_sender_clone.send(SignalMessage::TestMode(test_params)) {
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
                }
                SIGUSR2 => {
                    #[cfg(debug_assertions)]
                    {
                        sigusr2_count += 1;
                        eprintln!(
                            "DEBUG: SIGUSR2 #{} received by PID: {}, sending reload message",
                            sigusr2_count,
                            std::process::id()
                        );
                    }

                    // Send reload signal
                    match signal_sender_clone.send(SignalMessage::Reload) {
                        Ok(()) => {
                            log_pipe!();
                            log_info!("Received configuration reload signal");
                        }
                        Err(_) => {
                            #[cfg(debug_assertions)]
                            {
                                eprintln!(
                                    "DEBUG: Reload signal send failed - channel disconnected"
                                );
                            }
                            break;
                        }
                    }
                }
                SIGTERM => {
                    // Check for instant shutdown flag first
                    let shutdown_file_path =
                        format!("/tmp/sunsetr-shutdown-{}.tmp", std::process::id());
                    let is_instant_shutdown = std::fs::read_to_string(&shutdown_file_path)
                        .map(|content| content.trim() == "instant")
                        .unwrap_or(false);

                    // Clean up temp file if it exists
                    let _ = std::fs::remove_file(&shutdown_file_path);

                    if is_instant_shutdown {
                        // Instant shutdown - skip smooth transitions
                        #[cfg(debug_assertions)]
                        {
                            eprintln!(
                                "DEBUG: Received SIGTERM with instant shutdown flag (signal #{signal_count}), setting running=false"
                            );
                        }

                        log_pipe!();
                        log_info!("Received instant shutdown request for restart");

                        // Set running flag to false immediately
                        running_clone.store(false, Ordering::SeqCst);

                        // Send shutdown message with instant flag
                        if let Err(e) =
                            signal_sender_clone.send(SignalMessage::Shutdown { instant: true })
                        {
                            log_warning!("Failed to send instant shutdown message: {e}");
                        }

                        // Exit signal thread
                        break;
                    } else {
                        // Normal shutdown handling - fall through to existing logic
                        #[cfg(debug_assertions)]
                        {
                            eprintln!(
                                "DEBUG: Received SIGTERM (termination request) (signal #{signal_count}), setting running=false"
                            );
                        }

                        log_pipe!();
                        log_info!("Received termination request, initiating graceful shutdown...");

                        // Send shutdown message to main loop first
                        if let Err(e) =
                            signal_sender_clone.send(SignalMessage::Shutdown { instant: false })
                        {
                            log_warning!("Failed to send shutdown message: {e}");
                        }

                        // Set running flag to false
                        running_clone.store(false, Ordering::SeqCst);

                        // Exit signal thread
                        break;
                    }
                }
                SIGINT => {
                    // SIGINT (Ctrl+C) - graceful shutdown
                    #[cfg(debug_assertions)]
                    {
                        eprintln!(
                            "DEBUG: Received SIGINT (Ctrl+C) (signal #{signal_count}), setting running=false"
                        );
                    }

                    log_pipe!();
                    if debug_enabled {
                        log_info!("Received SIGINT (Ctrl+C), initiating graceful shutdown...");
                    } else {
                        log_info!("Received interrupt signal, initiating graceful shutdown...");
                    }

                    // Send shutdown message to main loop first
                    if let Err(e) =
                        signal_sender_clone.send(SignalMessage::Shutdown { instant: false })
                    {
                        log_warning!("Failed to send shutdown message: {e}");
                    }

                    // Set running flag to false
                    running_clone.store(false, Ordering::SeqCst);

                    // Exit signal thread
                    break;
                }
                _ => {
                    // Handle SIGHUP and any other signals
                    if sig == SIGHUP {
                        #[cfg(debug_assertions)]
                        {
                            let log_msg = "Received SIGHUP - terminal disconnected, forcing exit\n";
                            let _ = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                                .and_then(|mut f| {
                                    use std::io::Write;
                                    f.write_all(log_msg.as_bytes())
                                });
                        }

                        // Set running flag to false
                        running_clone.store(false, Ordering::SeqCst);

                        // Exit immediately - no point in graceful shutdown without a terminal
                        std::process::exit(0);
                    }

                    #[cfg(debug_assertions)]
                    {
                        let signal_name = match sig {
                            SIGINT => "SIGINT (Ctrl+C)",
                            SIGTERM => "SIGTERM (termination request)",
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

                    // Now we know it's not SIGHUP, safe to log to terminal
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
                        _ => "Received shutdown signal, initiating graceful shutdown...",
                    };

                    log_pipe!();
                    log_info!("{}", user_message);

                    // Send shutdown message to main loop first
                    if let Err(e) =
                        signal_sender_clone.send(SignalMessage::Shutdown { instant: false })
                    {
                        // If we can't send the message, the main loop is likely already gone
                        // Exit the signal thread in this case
                        log_pipe!();
                        log_warning!("Failed to send shutdown message: {e}");
                        log_indented!("Main loop appears to have already exited");
                        log_end!();

                        // Set the flag anyway for any other threads that might be checking
                        running_clone.store(false, Ordering::SeqCst);

                        // Exit signal thread since main loop is gone
                        break;
                    }

                    // For shutdown signals, set the flag to stop
                    running_clone.store(false, Ordering::SeqCst);

                    // Note: We don't do emergency cleanup here anymore because it interferes
                    // with the normal cleanup path trying to reset gamma to 6500K.
                    // The Drop trait and normal cleanup should handle most cases.

                    #[cfg(debug_assertions)]
                    {
                        let log_msg = format!(
                            "Signal handler set running=false after {signal_count} signals ({sigusr2_count} SIGUSR2)\n"
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

                    // Continue processing signals to handle repeated termination requests
                    // The main loop will exit when running=false is detected
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
        instant_shutdown,
        current_preset: Arc::new(std::sync::Mutex::new(initial_preset)),
        pending_config: Arc::new(std::sync::Mutex::new(None)),
    })
}
