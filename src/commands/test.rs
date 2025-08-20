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
    log_version!();

    // Validate arguments using same logic as config
    validate_temperature(temperature)?;
    validate_gamma(gamma)?;

    // Load and validate configuration first
    // This ensures we fail fast with a clear error message if config is invalid
    let config = Config::load()?;

    log_block_start!("Testing display settings: {}K @ {}%", temperature, gamma);

    // Check for existing sunsetr process
    match crate::utils::get_running_sunsetr_pid() {
        Ok(pid) => {
            log_decorated!("Found existing sunsetr process (PID: {pid}), sending test signal...");

            // Write test parameters to temp file
            let test_file_path = format!("/tmp/sunsetr-test-{pid}.tmp");
            std::fs::write(&test_file_path, format!("{temperature}\n{gamma}"))?;

            // Send SIGUSR1 signal to existing process
            #[cfg(debug_assertions)]
            eprintln!(
                "DEBUG: Sending SIGUSR1 to PID {pid} with test params: {temperature}K @ {gamma}%"
            );

            match nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGUSR1,
            ) {
                Ok(_) => {
                    log_decorated!("Test signal sent successfully");

                    #[cfg(debug_assertions)]
                    eprintln!("DEBUG: Waiting 200ms for process to apply values...");

                    // Give the existing process a moment to apply the test values
                    std::thread::sleep(std::time::Duration::from_millis(200));

                    log_decorated!("Test values should now be applied");
                    log_block_start!("Press Escape or Ctrl+C to restore previous settings");

                    // Hide cursor during interactive wait
                    let _terminal_guard = crate::utils::TerminalGuard::new();

                    // Wait for user to exit test mode
                    wait_for_user_exit()?;

                    // Send SIGUSR1 with special params (temp=0) to exit test mode
                    log_decorated!("Restoring normal operation...");

                    // Write special "exit test mode" parameters
                    let test_file_path = format!("/tmp/sunsetr-test-{pid}.tmp");
                    std::fs::write(&test_file_path, "0\n0")?;

                    // Send SIGUSR1 to signal exit from test mode
                    let _ = nix::sys::signal::kill(
                        nix::unistd::Pid::from_raw(pid as i32),
                        nix::sys::signal::Signal::SIGUSR1,
                    );

                    log_decorated!("Test complete");
                }
                Err(e) => {
                    // Clean up temp file on error
                    let _ = std::fs::remove_file(&test_file_path);
                    anyhow::bail!("Failed to send test signal to existing process: {}", e);
                }
            }
        }
        Err(_) => {
            log_decorated!("No existing sunsetr process found, running direct test...");

            // Run direct test when no existing process
            run_direct_test(temperature, gamma, debug_enabled, &config)?;
        }
    }

    log_end!();
    Ok(())
}

/// Run direct test when no existing sunsetr process is running.
///
/// Uses the configured backend (Hyprland, Wayland, or auto-detected) to apply
/// test values directly. This ensures consistency with the user's normal
/// sunsetr configuration.
fn run_direct_test(
    temperature: u32,
    gamma: f32,
    debug_enabled: bool,
    config: &Config,
) -> Result<()> {
    // Create backend based on configuration
    let backend_type = crate::backend::detect_backend(config)?;

    // For Hyprland backend in test mode, start hyprsunset with test values directly
    let backend_result = match backend_type {
        crate::backend::BackendType::Hyprland => {
            crate::backend::hyprland::HyprlandBackend::new_with_initial_values(
                config,
                debug_enabled,
                temperature,
                gamma,
            )
            .map(|backend| Box::new(backend) as Box<dyn crate::backend::ColorTemperatureBackend>)
        }
        crate::backend::BackendType::Wayland => {
            crate::backend::create_backend(backend_type, config, debug_enabled)
        }
    };

    match backend_result {
        Ok(mut backend) => {
            log_decorated!(
                "Applying test values via {} backend...",
                backend.backend_name()
            );
            use std::sync::Arc;
            use std::sync::atomic::AtomicBool;

            let running = Arc::new(AtomicBool::new(true));

            // Check if startup transition is enabled
            // Skip transitions for Hyprland backend as hyprsunset handles its own transitions
            let is_hyprland = backend.backend_name() == "Hyprland";
            let startup_transition_enabled = !is_hyprland
                && config
                    .startup_transition
                    .unwrap_or(crate::constants::DEFAULT_STARTUP_TRANSITION);

            // Apply test values with optional smooth transition
            if startup_transition_enabled
                && config
                    .startup_transition_duration
                    .unwrap_or(crate::constants::DEFAULT_STARTUP_TRANSITION_DURATION)
                    > 0
            {
                // Create a cloned config with test values as night values
                // We use night values to transition FROM day values (6500K, 100%)
                let mut test_config = config.clone();
                test_config.night_temp = Some(temperature);
                test_config.night_gamma = Some(gamma);

                // Create transition from day to night (test values)
                let mut transition = crate::startup_transition::StartupTransition::new(
                    crate::time_state::TimeState::Night,
                    &test_config,
                    None, // No geo_times needed for test mode
                );

                // Configure for silent test operation
                transition = transition.silent();

                // Execute the transition
                match transition.execute(backend.as_mut(), &test_config, &running) {
                    Ok(_) => {
                        log_decorated!("Test values applied with smooth transition");
                    }
                    Err(e) => {
                        log_warning!("Failed to apply test values with transition: {e}");

                        // Fall back to immediate application
                        match backend.apply_temperature_gamma(temperature, gamma, &running) {
                            Ok(_) => {
                                log_decorated!("Test values applied immediately (fallback)");
                            }
                            Err(e) => {
                                anyhow::bail!("Failed to apply test values: {}", e);
                            }
                        }
                    }
                }
            } else {
                // Apply test values immediately
                // For Hyprland backend, we already started with test values, so skip redundant application
                if backend.backend_name() != "Hyprland" {
                    match backend.apply_temperature_gamma(temperature, gamma, &running) {
                        Ok(_) => {
                            log_decorated!("Test values applied successfully");
                        }
                        Err(e) => {
                            anyhow::bail!("Failed to apply test values: {}", e);
                        }
                    }
                } else {
                    log_decorated!("Test values applied successfully");
                }
            }

            log_block_start!("Press Escape or Ctrl+C to restore previous settings");

            // Hide cursor during interactive wait
            let _terminal_guard = crate::utils::TerminalGuard::new();

            // Wait for user input
            wait_for_user_exit()?;

            // For Hyprland backend, hyprsunset automatically restores on shutdown
            // For Wayland backend, we need to manually restore
            if !is_hyprland {
                log_block_start!("Restoring display to day values...");

                if startup_transition_enabled
                    && config
                        .startup_transition_duration
                        .unwrap_or(crate::constants::DEFAULT_STARTUP_TRANSITION_DURATION)
                        > 0
                {
                    // Create transition from test values back to day values
                    let mut transition =
                        crate::startup_transition::StartupTransition::new_from_values(
                            temperature,
                            gamma,
                            crate::time_state::TimeState::Day,
                            config,
                            None, // No geo_times needed for test mode
                        );

                    // Configure for silent restoration
                    transition = transition.silent();

                    // Execute the restoration transition
                    match transition.execute(backend.as_mut(), config, &running) {
                        Ok(_) => {
                            log_decorated!(
                                "Display restored to day values with smooth transition (6500K, 100%)"
                            );
                        }
                        Err(e) => {
                            log_warning!("Failed to restore with transition: {e}");

                            // Fall back to immediate restoration
                            match backend.apply_temperature_gamma(6500, 100.0, &running) {
                                Ok(_) => {
                                    log_decorated!("Display restored to day values (6500K, 100%)");
                                }
                                Err(e) => {
                                    anyhow::bail!("Failed to restore display: {}", e);
                                }
                            }
                        }
                    }
                } else {
                    // Restore values immediately
                    backend.apply_temperature_gamma(6500, 100.0, &running)?;
                    log_decorated!("Display restored to day values (6500K, 100%)");
                }
            }
        }
        Err(e) => {
            anyhow::bail!("Failed to initialize Wayland backend: {}", e);
        }
    }

    log_block_start!("Test complete");
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

    log_decorated!(
        "Entering test mode: {}K @ {}%",
        test_params.temperature,
        test_params.gamma
    );

    // Check if startup transition is enabled
    // Skip transitions for Hyprland backend as hyprsunset handles its own transitions
    let is_hyprland = backend.backend_name() == "Hyprland";
    let startup_transition_enabled = !is_hyprland
        && config
            .startup_transition
            .unwrap_or(crate::constants::DEFAULT_STARTUP_TRANSITION);

    // Get current values before applying test values
    let current_state = crate::time_state::get_transition_state(config, None);
    let (original_temp, original_gamma) = current_state.values(config);

    // Apply test values with optional smooth transition
    if startup_transition_enabled
        && config
            .startup_transition_duration
            .unwrap_or(crate::constants::DEFAULT_STARTUP_TRANSITION_DURATION)
            > 0
    {
        // Create a cloned config with test values as day values for the transition
        let mut test_config = config.clone();
        test_config.day_temp = Some(test_params.temperature);
        test_config.day_gamma = Some(test_params.gamma);

        // Create transition from current values to test values
        let mut transition = crate::startup_transition::StartupTransition::new_from_values(
            original_temp,
            original_gamma,
            crate::time_state::TimeState::Day,
            &test_config,
            None, // No geo_times needed for test mode
        );

        // Configure for silent test operation
        transition = transition.silent();

        // Execute the transition
        match transition.execute(backend.as_mut(), &test_config, &signal_state.running) {
            Ok(_) => {
                log_decorated!("Test values applied with smooth transition");
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: Backend successfully applied test values with transition");
            }
            Err(e) => {
                log_warning!("Failed to apply test values with transition: {e}");
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: Backend failed to apply test values with transition: {e}");

                // Fall back to immediate application
                match backend.apply_temperature_gamma(
                    test_params.temperature,
                    test_params.gamma,
                    &signal_state.running,
                ) {
                    Ok(_) => {
                        log_decorated!("Test values applied immediately (fallback)");
                    }
                    Err(e) => {
                        log_warning!("Failed to apply test values: {e}");
                        return Ok(()); // Exit test mode if we can't apply values
                    }
                }
            }
        }
    } else {
        // Apply test values immediately
        match backend.apply_temperature_gamma(
            test_params.temperature,
            test_params.gamma,
            &signal_state.running,
        ) {
            Ok(_) => {
                log_decorated!("Test values applied successfully");
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: Backend successfully applied test values");
            }
            Err(e) => {
                log_warning!("Failed to apply test values: {e}");
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: Backend failed to apply test values: {e}");
                return Ok(()); // Exit test mode if we can't apply values
            }
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
                            log_decorated!("Exiting test mode, restoring normal operation...");
                            break;
                        } else {
                            // Apply new test values
                            log_decorated!(
                                "Updating test values: {}K @ {}%",
                                new_params.temperature,
                                new_params.gamma
                            );
                            let _ = backend.apply_temperature_gamma(
                                new_params.temperature,
                                new_params.gamma,
                                &signal_state.running,
                            );
                        }
                    }
                    SignalMessage::Reload => {
                        // Reload signal received during test mode - exit and let main loop handle it
                        log_decorated!("Reload signal received, exiting test mode...");
                        break;
                    }
                    SignalMessage::Shutdown => {
                        // Shutdown signal received during test mode - exit immediately
                        log_decorated!("Shutdown signal received, exiting test mode...");
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
    let restore_state = crate::time_state::get_transition_state(config, None);
    let (restore_temp, restore_gamma) = restore_state.values(config);

    if startup_transition_enabled
        && config
            .startup_transition_duration
            .unwrap_or(crate::constants::DEFAULT_STARTUP_TRANSITION_DURATION)
            > 0
    {
        // Create a cloned config with restore values as day values for the transition
        let mut restore_config = config.clone();
        restore_config.day_temp = Some(restore_temp);
        restore_config.day_gamma = Some(restore_gamma);

        // Create transition from test values back to normal values
        let mut transition = crate::startup_transition::StartupTransition::new_from_values(
            test_params.temperature,
            test_params.gamma,
            crate::time_state::TimeState::Day,
            &restore_config,
            None, // No geo_times needed for test mode
        );

        // Configure for silent test operation
        transition = transition.silent();

        // Execute the restoration transition
        match transition.execute(backend.as_mut(), &restore_config, &signal_state.running) {
            Ok(_) => {
                log_decorated!(
                    "Normal operation restored with smooth transition: {restore_temp}K @ {restore_gamma}%"
                );
                #[cfg(debug_assertions)]
                eprintln!(
                    "DEBUG: Restored normal values with transition: {restore_temp}K @ {restore_gamma}%"
                );
            }
            Err(e) => {
                log_warning!("Failed to restore with transition: {e}");

                // Fall back to immediate restoration
                match backend.apply_temperature_gamma(
                    restore_temp,
                    restore_gamma,
                    &signal_state.running,
                ) {
                    Ok(_) => {
                        log_decorated!(
                            "Normal operation restored immediately: {restore_temp}K @ {restore_gamma}%"
                        );
                    }
                    Err(e) => {
                        log_warning!("Failed to restore normal operation: {e}");
                    }
                }
            }
        }
    } else {
        // Restore values immediately
        match backend.apply_temperature_gamma(restore_temp, restore_gamma, &signal_state.running) {
            Ok(_) => {
                log_decorated!("Normal operation restored: {restore_temp}K @ {restore_gamma}%");
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: Restored normal values: {restore_temp}K @ {restore_gamma}%");
            }
            Err(e) => {
                log_warning!("Failed to restore normal operation: {e}");
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: Failed to restore normal values: {e}");
            }
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
