//! Implementation of the --test command for interactive gamma/temperature testing.
//!
//! This command operates in two modes:
//! 1. **With existing sunsetr process**: Sends SIGUSR1 signal with test parameters via temp file.
//!    The existing process temporarily applies the test values using its configured backend.
//! 2. **Without existing process**: Uses the Wayland backend directly for testing.
//!    This avoids backend conflicts and provides universal testing capability.
//!
//! In both modes, the user can press Escape or Ctrl+C to restore the previous state.

use crate::backend::ColorTemperatureBackend;
use crate::config::Config;
use crate::logger::Log;
use crate::signals::TestModeParams;
use anyhow::Result;

/// Validate temperature value using the same logic as config validation
fn validate_temperature(temp: u32) -> Result<()> {
    use crate::constants::{MAXIMUM_TEMP, MINIMUM_TEMP};

    if temp < MINIMUM_TEMP {
        anyhow::bail!(
            "Temperature {} is too low (minimum: {}K)",
            temp,
            MINIMUM_TEMP
        );
    }

    if temp > MAXIMUM_TEMP {
        anyhow::bail!(
            "Temperature {} is too high (maximum: {}K)",
            temp,
            MAXIMUM_TEMP
        );
    }

    Ok(())
}

/// Validate gamma value using the same logic as config validation
fn validate_gamma(gamma: f32) -> Result<()> {
    use crate::constants::{MAXIMUM_GAMMA, MINIMUM_GAMMA};

    if gamma < MINIMUM_GAMMA {
        anyhow::bail!("Gamma {} is too low (minimum: {})", gamma, MINIMUM_GAMMA);
    }

    if gamma > MAXIMUM_GAMMA {
        anyhow::bail!("Gamma {} is too high (maximum: {})", gamma, MAXIMUM_GAMMA);
    }

    Ok(())
}

/// Handle the --test command to apply specific temperature and gamma values
pub fn handle_test_command(temperature: u32, gamma: f32, debug_enabled: bool) -> Result<()> {
    Log::log_version();

    // Validate arguments using same logic as config
    validate_temperature(temperature)?;
    validate_gamma(gamma)?;

    // Load and validate configuration first
    // This ensures we fail fast with a clear error message if config is invalid
    let config = Config::load()?;

    Log::log_block_start(&format!(
        "Testing display settings: {}K @ {}%",
        temperature, gamma
    ));

    // Check for existing sunsetr process
    match crate::utils::get_running_sunsetr_pid() {
        Ok(pid) => {
            Log::log_decorated(&format!(
                "Found existing sunsetr process (PID: {}), sending test signal...",
                pid
            ));

            // Write test parameters to temp file
            let test_file_path = format!("/tmp/sunsetr-test-{}.tmp", pid);
            std::fs::write(&test_file_path, format!("{}\n{}", temperature, gamma))?;

            // Send SIGUSR1 signal to existing process
            #[cfg(debug_assertions)]
            eprintln!(
                "DEBUG: Sending SIGUSR1 to PID {} with test params: {}K @ {}%",
                pid, temperature, gamma
            );

            match nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGUSR1,
            ) {
                Ok(_) => {
                    Log::log_decorated("Test signal sent successfully");

                    #[cfg(debug_assertions)]
                    eprintln!("DEBUG: Waiting 200ms for process to apply values...");

                    // Give the existing process a moment to apply the test values
                    std::thread::sleep(std::time::Duration::from_millis(200));

                    Log::log_decorated("Test values should now be applied");
                    Log::log_block_start("Press Escape or Ctrl+C to restore previous settings");

                    // Hide cursor during interactive wait
                    let _terminal_guard = crate::utils::TerminalGuard::new();

                    // Wait for user to exit test mode
                    wait_for_user_exit()?;

                    // Send SIGUSR1 with special params (temp=0) to exit test mode
                    Log::log_decorated("Restoring normal operation...");

                    // Write special "exit test mode" parameters
                    let test_file_path = format!("/tmp/sunsetr-test-{}.tmp", pid);
                    std::fs::write(&test_file_path, "0\n0")?;

                    // Send SIGUSR1 to signal exit from test mode
                    let _ = nix::sys::signal::kill(
                        nix::unistd::Pid::from_raw(pid as i32),
                        nix::sys::signal::Signal::SIGUSR1,
                    );

                    Log::log_decorated("Test complete");
                }
                Err(e) => {
                    // Clean up temp file on error
                    let _ = std::fs::remove_file(&test_file_path);
                    anyhow::bail!("Failed to send test signal to existing process: {}", e);
                }
            }
        }
        Err(_) => {
            Log::log_decorated("No existing sunsetr process found, running direct test...");

            // Run direct test when no existing process
            run_direct_test(temperature, gamma, debug_enabled, &config)?;
        }
    }

    Log::log_end();
    Ok(())
}

/// Run direct test when no existing sunsetr process is running.
///
/// Uses the Wayland backend directly regardless of configuration to:
/// - Avoid spawning managed processes (like hyprsunset) just for testing
/// - Provide universal testing capability across all Wayland compositors
/// - Keep the test command simple and non-intrusive
fn run_direct_test(
    temperature: u32,
    gamma: f32,
    debug_enabled: bool,
    config: &Config,
) -> Result<()> {
    // Apply test values using Wayland backend for direct testing
    // This ensures universal compatibility without spawning managed processes
    Log::log_decorated("Applying test values via Wayland backend...");

    match crate::backend::wayland::WaylandBackend::new(config, debug_enabled) {
        Ok(mut backend) => {
            use crate::backend::ColorTemperatureBackend;
            use std::sync::Arc;
            use std::sync::atomic::AtomicBool;

            let running = Arc::new(AtomicBool::new(true));

            match backend.apply_temperature_gamma(temperature, gamma, &running) {
                Ok(_) => {
                    Log::log_decorated("Test values applied successfully");
                    Log::log_block_start("Press Escape or Ctrl+C to restore previous settings");

                    // Hide cursor during interactive wait
                    let _terminal_guard = crate::utils::TerminalGuard::new();

                    // Wait for user input
                    wait_for_user_exit()?;

                    // Restore to standard day values (6500K, 100%)
                    // Note: Since this is direct testing without config context,
                    // we restore to universally safe day values rather than
                    // attempting to calculate the "correct" state
                    Log::log_block_start("Restoring display to day values...");
                    backend.apply_temperature_gamma(6500, 100.0, &running)?;
                    Log::log_decorated("Display restored to day values (6500K, 100%)");
                }
                Err(e) => {
                    anyhow::bail!("Failed to apply test values: {}", e);
                }
            }
        }
        Err(e) => {
            anyhow::bail!("Failed to initialize Wayland backend: {}", e);
        }
    }

    Log::log_block_start("Test complete");
    Ok(())
}

/// Run test mode in a temporary loop (blocking until test mode exits).
///
/// This function is called by the main loop when it receives a SIGUSR1 test signal.
/// It temporarily takes control to:
/// 1. Apply the test temperature and gamma values
/// 2. Wait for an exit signal (another SIGUSR1 with temp=0, SIGUSR2, or shutdown)
/// 3. Restore the normal calculated values before returning to the main loop
///
/// This approach preserves all main loop state and timing while allowing temporary overrides.
pub fn run_test_mode_loop(
    test_params: TestModeParams,
    backend: &mut Box<dyn ColorTemperatureBackend>,
    signal_state: &crate::signals::SignalState,
    config: &crate::config::Config,
) -> Result<()> {
    #[cfg(debug_assertions)]
    eprintln!(
        "DEBUG: Entering test mode loop with {}K @ {}%",
        test_params.temperature, test_params.gamma
    );

    Log::log_decorated(&format!(
        "Entering test mode: {}K @ {}%",
        test_params.temperature, test_params.gamma
    ));

    // Apply test values
    match backend.apply_temperature_gamma(
        test_params.temperature,
        test_params.gamma,
        &signal_state.running,
    ) {
        Ok(_) => {
            Log::log_decorated("Test values applied successfully");
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Backend successfully applied test values");
        }
        Err(e) => {
            Log::log_warning(&format!("Failed to apply test values: {}", e));
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Backend failed to apply test values: {}", e);
            return Ok(()); // Exit test mode if we can't apply values
        }
    }

    // Run temporary loop waiting for exit signal
    #[cfg(debug_assertions)]
    eprintln!("DEBUG: Test mode loop waiting for exit signal");

    loop {
        // Check if process should exit
        if !signal_state
            .running
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            break;
        }

        // Check for new test signals (including exit signal)
        match signal_state
            .signal_receiver
            .recv_timeout(std::time::Duration::from_millis(100))
        {
            Ok(signal_msg) => {
                use crate::signals::SignalMessage;
                match signal_msg {
                    SignalMessage::TestMode(new_params) => {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "DEBUG: Test mode received new signal: {}K @ {}%",
                            new_params.temperature, new_params.gamma
                        );

                        if new_params.temperature == 0 {
                            // Exit test mode signal received
                            Log::log_decorated("Exiting test mode, restoring normal operation...");
                            break;
                        } else {
                            // Apply new test values
                            Log::log_decorated(&format!(
                                "Updating test values: {}K @ {}%",
                                new_params.temperature, new_params.gamma
                            ));
                            let _ = backend.apply_temperature_gamma(
                                new_params.temperature,
                                new_params.gamma,
                                &signal_state.running,
                            );
                        }
                    }
                    SignalMessage::Reload => {
                        // Reload signal received during test mode - exit and let main loop handle it
                        Log::log_decorated("Reload signal received, exiting test mode...");
                        break;
                    }
                    SignalMessage::Shutdown => {
                        // Shutdown signal received during test mode - exit immediately
                        Log::log_decorated("Shutdown signal received, exiting test mode...");
                        break;
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Normal timeout, continue waiting
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // Channel disconnected, exit test mode
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: Test channel disconnected, exiting test mode");
                break;
            }
        }
    }

    // Restore normal values before returning to main loop
    let current_state = crate::time_state::get_transition_state(config);
    let (temperature, gamma) =
        crate::time_state::get_initial_values_for_state(current_state, config);

    match backend.apply_temperature_gamma(temperature, gamma, &signal_state.running) {
        Ok(_) => {
            Log::log_decorated(&format!(
                "Normal operation restored: {}K @ {}%",
                temperature, gamma
            ));
            #[cfg(debug_assertions)]
            eprintln!(
                "DEBUG: Restored normal values: {}K @ {}%",
                temperature, gamma
            );
        }
        Err(e) => {
            Log::log_warning(&format!("Failed to restore normal operation: {}", e));
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Failed to restore normal values: {}", e);
        }
    }

    #[cfg(debug_assertions)]
    eprintln!("DEBUG: Exiting test mode loop");

    Ok(())
}

/// Wait for user to press Escape or Ctrl+C
fn wait_for_user_exit() -> Result<()> {
    use crossterm::{
        event::{self, Event, KeyCode},
        terminal::{disable_raw_mode, enable_raw_mode},
    };

    // Enable raw mode to capture keys
    enable_raw_mode()?;

    let result = loop {
        // Wait for keyboard input
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc => break Ok(()),
                KeyCode::Char('c')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    break Ok(());
                }
                _ => {
                    // Ignore other keys
                }
            }
        }
    };

    // Restore normal terminal mode
    disable_raw_mode()?;

    result
}
