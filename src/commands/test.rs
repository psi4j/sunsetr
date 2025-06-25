//! Implementation of the --test command.
//!
//! This command applies specific temperature and gamma values using the Wayland backend only,
//! and waits for user to press Escape or Ctrl+C to restore the previous state.

use anyhow::Result;
use crate::logger::Log;
use crate::config::Config;
use crate::signals::TestModeParams;
use crate::backend::ColorTemperatureBackend;

/// Validate temperature value using the same logic as config validation
fn validate_temperature(temp: u32) -> Result<()> {
    use crate::constants::{MINIMUM_TEMP, MAXIMUM_TEMP};
    
    if temp < MINIMUM_TEMP {
        anyhow::bail!("Temperature {} is too low (minimum: {}K)", temp, MINIMUM_TEMP);
    }
    
    if temp > MAXIMUM_TEMP {
        anyhow::bail!("Temperature {} is too high (maximum: {}K)", temp, MAXIMUM_TEMP);
    }
    
    Ok(())
}

/// Validate gamma value using the same logic as config validation
fn validate_gamma(gamma: f32) -> Result<()> {
    use crate::constants::{MINIMUM_GAMMA, MAXIMUM_GAMMA};
    
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
    
    Log::log_block_start(&format!("Testing display settings: {}K @ {}%", temperature, gamma));
    
    // Check for existing sunsetr process
    match crate::utils::get_running_sunsetr_pid() {
        Ok(pid) => {
            Log::log_decorated(&format!("Found existing sunsetr process (PID: {}), sending test signal...", pid));
            
            // Write test parameters to temp file
            let test_file_path = format!("/tmp/sunsetr-test-{}.tmp", pid);
            std::fs::write(&test_file_path, format!("{}\n{}", temperature, gamma))?;
            
            // Send SIGUSR1 signal to existing process
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Sending SIGUSR1 to PID {} with test params: {}K @ {}%", pid, temperature, gamma);
            
            match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), nix::sys::signal::Signal::SIGUSR1) {
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
                    let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), nix::sys::signal::Signal::SIGUSR1);
                    
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
            
            // Load config for backend initialization (but only use Wayland)
            let config = match Config::load() {
                Ok(cfg) => cfg,
                Err(e) => {
                    Log::log_warning(&format!("Failed to load config: {}, using defaults", e));
                    // For test command, we can create a minimal config
                    return Err(anyhow::anyhow!("Config required for backend initialization"));
                }
            };
            
            // Run direct test when no existing process
            run_direct_test(temperature, gamma, debug_enabled, &config)?;
        }
    }
    
    Log::log_end();
    Ok(())
}

/// Run direct test when no existing sunsetr process is running
fn run_direct_test(temperature: u32, gamma: f32, debug_enabled: bool, config: &Config) -> Result<()> {
    // Apply test values using Wayland backend only
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
                    
                    // Restore to default values
                    Log::log_block_start("Restoring display to defaults...");
                    backend.apply_temperature_gamma(6500, 100.0, &running)?;
                    Log::log_decorated("Display restored to defaults");
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

/// Run test mode in a temporary loop (blocking until test mode exits)
pub fn run_test_mode_loop(
    test_params: TestModeParams,
    backend: &mut Box<dyn ColorTemperatureBackend>,
    signal_state: &crate::signals::SignalState,
    config: &crate::config::Config,
) -> Result<()> {
    #[cfg(debug_assertions)]
    eprintln!("DEBUG: Entering test mode loop with {}K @ {}%", test_params.temperature, test_params.gamma);
    
    Log::log_decorated(&format!("Entering test mode: {}K @ {}%", test_params.temperature, test_params.gamma));
    
    // Apply test values
    match backend.apply_temperature_gamma(test_params.temperature, test_params.gamma, &signal_state.running) {
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
        if !signal_state.running.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        
        // Check for new test signals (including exit signal)
        match signal_state.signal_receiver.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(signal_msg) => {
                use crate::signals::SignalMessage;
                match signal_msg {
                    SignalMessage::TestMode(new_params) => {
                        #[cfg(debug_assertions)]
                        eprintln!("DEBUG: Test mode received new signal: {}K @ {}%", new_params.temperature, new_params.gamma);
                        
                        if new_params.temperature == 0 {
                            // Exit test mode signal received
                            Log::log_decorated("Exiting test mode, restoring normal operation...");
                            break;
                        } else {
                            // Apply new test values
                            Log::log_decorated(&format!("Updating test values: {}K @ {}%", new_params.temperature, new_params.gamma));
                            let _ = backend.apply_temperature_gamma(new_params.temperature, new_params.gamma, &signal_state.running);
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
    let (temperature, gamma) = crate::time_state::get_initial_values_for_state(current_state, config);
    
    match backend.apply_temperature_gamma(temperature, gamma, &signal_state.running) {
        Ok(_) => {
            Log::log_decorated(&format!("Normal operation restored: {}K @ {}%", temperature, gamma));
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Restored normal values: {}K @ {}%", temperature, gamma);
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
    
    Log::log_indented("(Press Escape or Ctrl+C to exit)");
    
    // Enable raw mode to capture keys
    enable_raw_mode()?;
    
    let result = loop {
        // Wait for keyboard input
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc => break Ok(()),
                KeyCode::Char('c') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => break Ok(()),
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