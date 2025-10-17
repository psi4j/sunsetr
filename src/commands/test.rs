//! Implementation of the test command for interactive gamma/temperature testing.
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
use crate::io::signals::TestModeParams;
use anyhow::{Context, Result};

/// Validate temperature value using the same logic as config validation
fn validate_temperature(temp: u32) -> Result<()> {
    use crate::common::constants::{MAXIMUM_TEMP, MINIMUM_TEMP};

    if temp < MINIMUM_TEMP {
        log_error_exit!(
            "Temperature {} is too low (minimum: {}K)",
            temp,
            MINIMUM_TEMP
        );
        std::process::exit(1);
    }

    if temp > MAXIMUM_TEMP {
        log_error_exit!(
            "Temperature {} is too high (maximum: {}K)",
            temp,
            MAXIMUM_TEMP
        );
        std::process::exit(1);
    }

    Ok(())
}

/// Validate gamma value using the same logic as config validation
fn validate_gamma(gamma: f32) -> Result<()> {
    use crate::common::constants::{MAXIMUM_GAMMA, MINIMUM_GAMMA};

    if gamma < MINIMUM_GAMMA {
        log_error_exit!("Gamma {} is too low (minimum: {})", gamma, MINIMUM_GAMMA);
        std::process::exit(1);
    }

    if gamma > MAXIMUM_GAMMA {
        log_error_exit!("Gamma {} is too high (maximum: {})", gamma, MAXIMUM_GAMMA);
        std::process::exit(1);
    }

    Ok(())
}

/// Handle the test command to apply specific temperature and gamma values
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
    match crate::io::instance::get_running_instance_pid() {
        Ok(pid) => {
            // Try to acquire test lock using the new RAII abstraction
            match crate::io::instance::acquire_test_lock() {
                Ok(_lock_guard) => {
                    // Test lock acquired - it will be automatically cleaned up when dropped
                    log_decorated!(
                        "Found existing sunsetr process (PID: {pid}), sending test signal..."
                    );

                    // Use the new send_test_signal abstraction
                    if debug_enabled {
                        log_debug!(
                            "Sending SIGUSR1 to PID {pid} with test params: {temperature}K @ {gamma}%"
                        );
                    }

                    match crate::io::instance::send_test_signal(pid, temperature, gamma) {
                        Ok(_) => {
                            log_decorated!("Test signal sent successfully");

                            if debug_enabled {
                                log_debug!("Waiting 200ms for process to apply values...");
                            }

                            // Give the existing process a moment to apply the test values
                            std::thread::sleep(std::time::Duration::from_millis(200));

                            log_decorated!("Test values should now be applied");
                            log_block_start!("Press Escape or Ctrl+C to restore previous settings");

                            // Hide cursor during interactive wait
                            let _terminal_guard = crate::common::utils::TerminalGuard::new();

                            // Wait for user to exit test mode
                            wait_for_user_exit()?;

                            // Send exit signal (temp=0) to exit test mode
                            log_decorated!("Restoring normal operation...");
                            let _ = crate::io::instance::send_test_signal(pid, 0, 0.0);

                            log_decorated!("Test complete");
                        }
                        Err(e) => {
                            log_error_exit!(
                                "Failed to send test signal to existing process: {}",
                                e
                            );
                            std::process::exit(1);
                        }
                    }
                }
                Err(_) => {
                    // Test lock file exists - another test is running
                    log_pipe!();
                    log_warning!("Test mode is already active in another terminal");
                    log_indented!("Exit the current test mode first (press Escape)");
                    log_end!();
                    return Ok(());
                }
            }
        }
        Err(_) => {
            // Try to acquire test lock using the new RAII abstraction
            match crate::io::instance::acquire_test_lock() {
                Ok(_lock_guard) => {
                    // Test lock acquired - it will be automatically cleaned up when dropped

                    log_decorated!("No existing sunsetr process found, running direct test...");

                    // Run direct test when no existing process
                    run_direct_test(temperature, gamma, debug_enabled, &config)?;
                }
                Err(_) => {
                    // Test lock file exists - another test is running
                    log_pipe!();
                    log_warning!("Test mode is already active in another terminal");
                    log_indented!("Exit the current test mode first (press Escape)");
                    log_end!();
                    return Ok(());
                }
            }
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

    // Create backend normally - each backend handles test mode appropriately
    let backend_result = match backend_type {
        crate::backend::BackendType::Hyprsunset => {
            // Hyprsunset backend in test mode starts hyprsunset with test values directly
            crate::backend::hyprsunset::HyprsunsetBackend::new_with_initial_values(
                debug_enabled,
                temperature,
                gamma,
            )
            .map(|backend| Box::new(backend) as Box<dyn crate::backend::ColorTemperatureBackend>)
        }
        _ => {
            // Other backends use normal creation path
            crate::backend::create_backend(backend_type, config, debug_enabled, None)
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
            // Only Wayland backend supports smooth transitions
            let is_wayland = backend.backend_name() == "Wayland";
            let smoothing_enabled = is_wayland
                && config
                    .smoothing
                    .unwrap_or(crate::common::constants::DEFAULT_SMOOTHING);

            // Apply test values with optional smooth transition
            let startup_duration = config
                .startup_duration
                .unwrap_or(crate::common::constants::DEFAULT_STARTUP_DURATION);

            if smoothing_enabled && startup_duration >= 0.1 {
                // Create a cloned config with test values as night values
                // We use night values to transition FROM day values (6500K, 100%)
                let mut test_config = config.clone();
                test_config.night_temp = Some(temperature);
                test_config.night_gamma = Some(gamma);

                // Create transition from day to night (test values)
                let mut transition = crate::core::smoothing::SmoothTransition::startup(
                    crate::core::period::Period::Night,
                    &test_config,
                    None, // No geo_times needed for test mode
                );

                // Configure for silent test operation
                transition = transition.silent();

                // Execute the transition
                match transition.execute(backend.as_mut(), &test_config, &running) {
                    Ok(_) => {
                        log_pipe!();
                        log_info!("Test values applied with smooth transition");
                    }
                    Err(e) => {
                        log_pipe!();
                        log_error!("Failed to apply test values with transition: {e}");

                        // Fall back to immediate application
                        match backend.apply_temperature_gamma(temperature, gamma, &running) {
                            Ok(_) => {
                                log_pipe!();
                                log_info!("Test values applied immediately (fallback)");
                            }
                            Err(e) => {
                                log_error_exit!("Failed to apply test values: {}", e);
                                std::process::exit(1);
                            }
                        }
                    }
                }
            } else {
                // Apply test values immediately
                // For Hyprsunset backend, we already started with test values, so skip redundant application
                if backend.backend_name() != "Hyprsunset" {
                    match backend.apply_temperature_gamma(temperature, gamma, &running) {
                        Ok(_) => {
                            log_block_start!("Test values applied successfully");
                        }
                        Err(e) => {
                            log_error_exit!("Failed to apply test values: {}", e);
                            std::process::exit(1);
                        }
                    }
                } else {
                    log_decorated!("Test values applied successfully");
                }
            }

            log_block_start!("Press Escape or Ctrl+C to restore previous settings");

            // Hide cursor during interactive wait
            let _terminal_guard = crate::common::utils::TerminalGuard::new();

            // Wait for user input
            wait_for_user_exit()?;

            // Only Wayland backend needs manual restoration
            // Hyprland-based backends automatically restore via CTM animation
            if is_wayland {
                log_block_start!("Restoring display to day values...");

                let shutdown_duration = config
                    .shutdown_duration
                    .unwrap_or(crate::common::constants::DEFAULT_SHUTDOWN_DURATION);

                if smoothing_enabled && shutdown_duration >= 0.1 {
                    // Create transition from test values back to day values
                    let mut transition = crate::core::smoothing::SmoothTransition::reload(
                        temperature,
                        gamma,
                        crate::core::period::Period::Day,
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
                            log_pipe!();
                            log_error!("Failed to restore with transition: {e}");

                            // Fall back to immediate restoration
                            match backend.apply_temperature_gamma(6500, 100.0, &running) {
                                Ok(_) => {
                                    log_pipe!();
                                    log_info!("Display restored to day values (6500K, 100%)");
                                }
                                Err(e) => {
                                    log_error_exit!("Failed to restore display: {}", e);
                                    std::process::exit(1);
                                }
                            }
                        }
                    }
                } else {
                    // Restore values immediately
                    backend.apply_temperature_gamma(6500, 100.0, &running)?;
                    log_pipe!();
                    log_info!("Display restored to day values (6500K, 100%)");
                }
            }
        }
        Err(e) => {
            log_error_exit!("Failed to initialize Wayland backend: {}", e);
            std::process::exit(1);
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
    signal_state: &crate::io::signals::SignalState,
    config: &crate::config::Config,
    debug_enabled: bool,
) -> Result<()> {
    if debug_enabled {
        log_debug!(
            "Entering test mode loop with {}K @ {}%",
            test_params.temperature,
            test_params.gamma
        );
    }

    log_indented!(
        "Entering test mode: {}K @ {}%",
        test_params.temperature,
        test_params.gamma
    );

    // Check if smooth transitions are enabled
    // Only Wayland backend supports smooth transitions
    let is_wayland = backend.backend_name() == "Wayland";
    let smoothing_enabled = is_wayland
        && config
            .smoothing
            .unwrap_or(crate::common::constants::DEFAULT_SMOOTHING);

    // Initialize geo_times if needed for current state calculation
    let geo_times = if config.transition_mode.as_deref() == Some("geo") {
        crate::geo::times::GeoTimes::from_config(config)
            .context("Failed to initialize geo transition times for test mode")?
    } else {
        None
    };

    // Get current values before applying test values
    let current_state = crate::core::period::get_current_period(config, geo_times.as_ref());
    let (original_temp, original_gamma) = current_state.values(config);

    // Apply test values with optional smooth transition
    let startup_duration = config
        .startup_duration
        .unwrap_or(crate::common::constants::DEFAULT_STARTUP_DURATION);

    if smoothing_enabled && startup_duration >= 0.1 {
        // Create a cloned config with test values as day values for the transition
        let mut test_config = config.clone();
        test_config.day_temp = Some(test_params.temperature);
        test_config.day_gamma = Some(test_params.gamma);

        // Create transition from current values to test values
        let mut transition = crate::core::smoothing::SmoothTransition::reload(
            original_temp,
            original_gamma,
            crate::core::period::Period::Day,
            &test_config,
            None, // No geo_times needed for test mode
        );

        // Configure for silent test operation without state announcement
        transition = transition.silent().no_announce();

        // Execute the transition
        match transition.execute(backend.as_mut(), &test_config, &signal_state.running) {
            Ok(_) => {
                log_pipe!();
                log_info!("Test values applied with smooth transition");
                if debug_enabled {
                    log_debug!("Backend successfully applied test values with transition");
                }
            }
            Err(e) => {
                log_warning!("Failed to apply test values with transition: {e}");
                if debug_enabled {
                    log_debug!("Backend failed to apply test values with transition: {e}");
                }

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
                if debug_enabled {
                    log_debug!("Backend successfully applied test values");
                }
            }
            Err(e) => {
                log_warning!("Failed to apply test values: {e}");
                if debug_enabled {
                    log_debug!("Backend failed to apply test values: {e}");
                }
                return Ok(()); // Exit test mode if we can't apply values
            }
        }
    }

    // Run temporary loop waiting for exit signal
    if debug_enabled {
        log_debug!("Test mode loop waiting for exit signal");
    }

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
                use crate::io::signals::SignalMessage;
                match signal_msg {
                    SignalMessage::TestMode(new_params) => {
                        // We only care about exit signals (temperature = 0)
                        // Any other test mode signal is ignored since we're already in test mode
                        if new_params.temperature == 0 {
                            // Exit test mode signal received
                            log_indented!("Exiting test mode, restoring normal operation...");
                            break;
                        }
                        // Silently ignore non-exit test signals while in test mode
                    }
                    SignalMessage::Reload => {
                        // Reload signal received during test mode - exit and let main loop handle it
                        log_decorated!("Reload signal received, exiting test mode...");
                        break;
                    }
                    SignalMessage::TimeChange => {
                        // Time change detected during test mode - exit and let main loop handle it
                        log_decorated!("Time change detected, exiting test mode...");
                        break;
                    }
                    SignalMessage::Shutdown { .. } => {
                        // Shutdown signal received during test mode - exit immediately
                        log_decorated!("Shutdown signal received, exiting test mode...");
                        break;
                    }
                    SignalMessage::Sleep { resuming } => {
                        // Sleep/resume detected during test mode - exit and let main loop handle it
                        if resuming {
                            log_decorated!("System resuming from sleep, exiting test mode...");
                        } else {
                            log_decorated!("System entering sleep, exiting test mode...");
                        }
                        break;
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Normal timeout, continue waiting
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // Channel disconnected, exit test mode
                if debug_enabled {
                    log_debug!("Test channel disconnected, exiting test mode");
                }
                break;
            }
        }
    }

    // Restore normal values before returning to main loop
    let restore_state = crate::core::period::get_current_period(config, geo_times.as_ref());
    let (restore_temp, restore_gamma) = restore_state.values(config);

    let shutdown_duration = config
        .shutdown_duration
        .unwrap_or(crate::common::constants::DEFAULT_SHUTDOWN_DURATION);

    if smoothing_enabled && shutdown_duration >= 0.1 {
        // Create a cloned config with restore values as day values for the transition
        let mut restore_config = config.clone();
        restore_config.day_temp = Some(restore_temp);
        restore_config.day_gamma = Some(restore_gamma);

        // Create transition from test values back to normal values
        let mut transition = crate::core::smoothing::SmoothTransition::reload(
            test_params.temperature,
            test_params.gamma,
            crate::core::period::Period::Day,
            &restore_config,
            None, // No geo_times needed for test mode
        );

        // Configure for silent test operation without state announcement
        transition = transition.silent().no_announce();

        // Execute the restoration transition
        match transition.execute(backend.as_mut(), &restore_config, &signal_state.running) {
            Ok(_) => {
                log_pipe!();
                log_info!(
                    "Normal operation restored with smooth transition: {restore_temp}K @ {restore_gamma}%"
                );
                if debug_enabled {
                    log_debug!(
                        "Restored normal values with transition: {restore_temp}K @ {restore_gamma}%"
                    );
                }
            }
            Err(e) => {
                log_pipe!();
                log_error!("Failed to restore with transition: {e}");

                // Fall back to immediate restoration
                match backend.apply_temperature_gamma(
                    restore_temp,
                    restore_gamma,
                    &signal_state.running,
                ) {
                    Ok(_) => {
                        log_pipe!();
                        log_info!(
                            "Normal operation restored immediately: {restore_temp}K @ {restore_gamma}%"
                        );
                    }
                    Err(e) => {
                        log_pipe!();
                        log_error!("Failed to restore normal operation: {e}");
                    }
                }
            }
        }
    } else {
        // Restore values immediately
        match backend.apply_temperature_gamma(restore_temp, restore_gamma, &signal_state.running) {
            Ok(_) => {
                log_pipe!();
                log_info!("Normal operation restored: {restore_temp}K @ {restore_gamma}%");
            }
            Err(e) => {
                log_error!("Failed to restore normal operation: {e}");
            }
        }
    }

    if debug_enabled {
        log_debug!("Exiting test mode loop");
    }

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

/// Display usage help for the test command (--help flag)
pub fn show_usage() {
    log_version!();
    log_block_start!("Usage: sunsetr test <temperature> <gamma>");
    log_block_start!("Arguments:");
    log_indented!("<temperature>  Color temperature in Kelvin (1000-10000)");
    log_indented!("<gamma>        Gamma percentage (10-200)");
    log_block_start!("Description:");
    log_indented!("Temporarily apply color temperature and gamma values for testing");
    log_pipe!();
    log_info!("For detailed help with examples, try: sunsetr help test");
    log_end!();
}

/// Display detailed help for the test command (help subcommand)
pub fn display_help() {
    log_version!();
    log_block_start!("test - Test specific temperature and gamma values");
    log_block_start!("Usage: sunsetr test <temperature> <gamma>");
    log_block_start!("Arguments:");
    log_indented!("<temperature>  Color temperature in Kelvin (1000-10000)");
    log_indented!("<gamma>        Gamma percentage (10-200)");
    log_block_start!("Description:");
    log_indented!("Temporarily applies the specified color temperature and gamma");
    log_indented!("values to test how they look on your display. Press Escape or");
    log_indented!("Ctrl+C to restore the previous settings.");
    log_block_start!("Behavior:");
    log_indented!("- If sunsetr is running: Signals test mode via SIGUSR1");
    log_indented!("- If not running: Applies values directly via backend");
    log_indented!("- Smooth transitions applied if configured");
    log_indented!("- Automatically restores on exit");
    log_block_start!("Examples:");
    log_indented!("# Test warm evening values");
    log_indented!("sunsetr test 3500 85");
    log_pipe!();
    log_indented!("# Test very warm night values");
    log_indented!("sunsetr test 2800 75");
    log_pipe!();
    log_indented!("# Test neutral daylight");
    log_indented!("sunsetr test 6500 100");
    log_end!();
}
