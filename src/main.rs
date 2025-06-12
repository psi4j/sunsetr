//! Main application entry point and high-level flow coordination.
//!
//! This module orchestrates the overall application lifecycle after command-line
//! argument parsing is complete. It coordinates between different modules:
//!
//! - `args`: Command-line argument parsing and help/version display
//! - `config`: Configuration loading and validation
//! - `backend`: Color temperature backend detection and management
//! - `time_state`: Time-based state calculation and transition logic
//! - `utils`: Shared utilities including terminal management, signal handling, and cleanup
//! - `logger`: Centralized logging functionality
//! - `startup_transition`: Smooth startup transition management
//!
//! The main application flow consists of:
//! 1. Argument parsing and early exit for help/version (handled by `args` module)
//! 2. Terminal setup and lock file management
//! 3. Configuration loading and backend detection
//! 4. Initial state application (with optional smooth startup transition)
//! 5. Main monitoring loop with periodic state updates
//! 6. Graceful cleanup on shutdown
//!
//! This structure keeps the main function focused on high-level flow while delegating
//! specific responsibilities to appropriate modules.

use anyhow::{Context, Result};
use fs2::FileExt;
use std::{
    fs::File,
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};

mod args;
mod backend;
mod config;
mod constants;
mod geo;
mod logger;
mod startup_transition;
mod time_state;
mod utils;

use crate::utils::{TerminalGuard, cleanup_application, setup_signal_handler};
use args::{CliAction, ParsedArgs};
use backend::{create_backend, detect_backend};
use config::Config;
use constants::*;
use logger::Log;
use startup_transition::StartupTransition;
use time_state::{
    TransitionState, get_transition_state, should_update_state, time_until_next_event,
};

// Constants
const CHECK_INTERVAL: Duration = Duration::from_secs(CHECK_INTERVAL_SECS);

fn main() -> Result<()> {
    // Parse command-line arguments
    let parsed_args = ParsedArgs::from_env();

    match parsed_args.action {
        CliAction::ShowVersion => {
            args::display_version_info();
            Ok(())
        }
        CliAction::ShowHelp | CliAction::ShowHelpDueToError => {
            args::display_help();
            Ok(())
        }
        CliAction::Run { debug_enabled } => {
            // Continue with normal application flow
            run_application(debug_enabled)
        }
        CliAction::RunGeoSelection { debug_enabled } => {
            // Handle --geo flag: delegate to geo module for complete workflow
            geo::handle_geo_selection(debug_enabled)
        }
    }
}

/// Main application logic after argument parsing is complete.
///
/// This function contains the core application flow: configuration loading,
/// backend setup, lock file management, and the main transition loop.
///
/// # Arguments
/// * `debug_enabled` - Whether debug logging should be enabled
///
/// # Returns
/// Result indicating success or failure of the application run
fn run_application(debug_enabled: bool) -> Result<()> {
    // Try to set up terminal features (cursor hiding, echo suppression)
    // This will gracefully handle cases where no terminal is available (e.g., systemd service)
    let _term = TerminalGuard::new().context("failed to initialize terminal features")?;

    Log::log_version();

    // Log debug mode status
    if debug_enabled {
        Log::log_pipe();
        Log::log_debug("Debug mode enabled - showing detailed backend operations");
    }

    // Set up signal handling
    let running = setup_signal_handler()?;

    // Create lock file path
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let lock_path = format!("{}/sunsetr.lock", runtime_dir);

    // Quick check if another instance is already running before doing expensive config validation
    if let Ok(existing_lock) = File::open(&lock_path) {
        if existing_lock.try_lock_exclusive().is_err() {
            Log::log_pipe();
            Log::log_error("Another instance of sunsetr is already running");
            std::process::exit(1);
        }
        // Lock succeeded, but we opened for reading - close it and create properly below
        drop(existing_lock);
    }

    // Load and validate configuration now that we know no other instance is running
    let config = Config::load()?;

    // Detect and validate the backend early
    let backend_type = detect_backend(&config)?;

    // Create and acquire lock file properly
    let lock_file = File::create(&lock_path)?;

    // Try to acquire exclusive lock (should succeed since we checked above, but race conditions possible)
    match lock_file.try_lock_exclusive() {
        Ok(_) => {
            Log::log_block_start("Lock acquired, starting sunsetr...");

            // Log configuration after acquiring lock
            config.log_config();

            Log::log_block_start(&format!("Detected backend: {}", backend_type.name()));

            let mut backend = create_backend(backend_type, &config, debug_enabled)?;

            // Backend creation already includes connection verification and logging
            Log::log_block_start(&format!(
                "Successfully connected to {} backend",
                backend.backend_name()
            ));

            let mut current_transition_state = get_transition_state(&config);
            let mut last_check_time = Instant::now();

            // Apply initial settings
            apply_initial_state(
                &mut backend,
                current_transition_state,
                &config,
                &running,
                debug_enabled,
            )?;

            // Main application loop
            run_main_loop(
                &mut backend,
                &mut current_transition_state,
                &mut last_check_time,
                &config,
                &running,
                debug_enabled,
            )?;

            // Ensure proper cleanup on shutdown
            Log::log_block_start("Shutting down sunsetr...");
            cleanup_application(backend, lock_file, &lock_path);
            Log::log_end();
        }
        Err(_) => {
            Log::log_pipe();
            Log::log_error("Another instance of sunsetr is already running");
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Apply the initial state when starting the application.
///
/// Handles both smooth startup transitions and immediate state application
/// based on configuration settings.
///
/// # Arguments
/// * `backend` - Backend to apply settings to
/// * `current_state` - Current transition state
/// * `config` - Application configuration
/// * `running` - Shared running state for shutdown detection
/// * `debug_enabled` - Whether debug logging is enabled
fn apply_initial_state(
    backend: &mut Box<dyn crate::backend::ColorTemperatureBackend>,
    current_state: TransitionState,
    config: &Config,
    running: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    debug_enabled: bool,
) -> Result<()> {
    if !running.load(Ordering::SeqCst) {
        return Ok(());
    }

    // Check if startup transition is enabled
    let startup_transition = config
        .startup_transition
        .unwrap_or(DEFAULT_STARTUP_TRANSITION);
    let startup_duration = config
        .startup_transition_duration
        .unwrap_or(DEFAULT_STARTUP_TRANSITION_DURATION);

    if startup_transition && startup_duration > 0 {
        // Use the smooth transition system (from day values to current state)
        let mut transition = StartupTransition::new(current_state, config);
        match transition.execute(backend.as_mut(), config, running) {
            Ok(_) => {}
            Err(e) => {
                Log::log_warning(&format!("Failed to apply smooth startup transition: {}", e));
                Log::log_decorated("Falling back to immediate transition...");

                // Fallback to immediate application
                apply_immediate_state(backend, current_state, config, running, debug_enabled)?;
            }
        }
    } else {
        // Use immediate transition to current interpolated values
        apply_immediate_state(backend, current_state, config, running, debug_enabled)?;
    }

    Ok(())
}

/// Apply state immediately without smooth transition.
fn apply_immediate_state(
    backend: &mut Box<dyn crate::backend::ColorTemperatureBackend>,
    current_state: TransitionState,
    config: &Config,
    running: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    debug_enabled: bool,
) -> Result<()> {
    match backend.apply_startup_state(current_state, config, running) {
        Ok(_) => {
            if debug_enabled {
                Log::log_pipe();
                Log::log_debug("Initial state applied successfully");
            }
        }
        Err(e) => {
            Log::log_warning(&format!("Failed to apply initial state: {}", e));
            Log::log_decorated("Continuing anyway - will retry during operation...");
        }
    }
    Ok(())
}

/// Run the main application loop that monitors and applies state changes.
///
/// This loop continuously monitors the time-based state and applies changes
/// to the backend when necessary. It handles transition detection, sleep/resume
/// detection, and graceful shutdown.
fn run_main_loop(
    backend: &mut Box<dyn crate::backend::ColorTemperatureBackend>,
    current_transition_state: &mut TransitionState,
    last_check_time: &mut Instant,
    config: &Config,
    running: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    _debug_enabled: bool,
) -> Result<()> {
    // Skip first iteration to prevent false state change detection due to startup timing
    let mut first_iteration = true;
    // Tracks if the initial transition progress log has been made using `log_block_start`.
    // Subsequent transition progress logs will use `log_decorated` when debug is disabled.
    let mut first_transition_log_done = false;

    while running.load(Ordering::SeqCst) {
        // Detect large time jumps (system sleep/resume scenarios)
        let current_time = Instant::now();
        let time_since_last_check = current_time.duration_since(*last_check_time);
        if time_since_last_check > Duration::from_secs(SLEEP_DETECTION_THRESHOLD_SECS) {
            Log::log_decorated(&format!(
                "Large time jump detected ({} minutes). System may have resumed from sleep.",
                time_since_last_check.as_secs() / 60
            ));
            Log::log_decorated("Forcing immediate state recalculation...");
        }
        *last_check_time = current_time;

        let new_state = get_transition_state(config);

        // Skip first iteration to prevent false state change detection caused by
        // timing differences between startup state application and main loop start
        let should_update = if first_iteration {
            first_iteration = false;
            false
        } else {
            should_update_state(current_transition_state, &new_state, time_since_last_check)
        };

        if should_update && running.load(Ordering::SeqCst) {
            match backend.apply_transition_state(new_state, config, running) {
                Ok(_) => {
                    // Success - update our state
                    *current_transition_state = new_state;
                }
                Err(e) => {
                    // Failure - check if it's a connection issue that couldn't be resolved
                    if e.to_string().contains("reconnection attempt") {
                        Log::log_error(&format!(
                            "Cannot communicate with {}: {}",
                            backend.backend_name(),
                            e
                        ));
                        Log::log_decorated(&format!(
                            "{} appears to be permanently unavailable. Exiting...",
                            backend.backend_name()
                        ));
                        break; // Exit the main loop
                    } else {
                        // Other error - just log it and retry next cycle
                        Log::log_warning(&format!("Failed to apply state: {}", e));
                        Log::log_decorated("Will retry on next cycle...");
                    }
                    // Don't update current_transition_state - try again next cycle
                }
            }
        }

        // Sleep and show progress
        handle_loop_sleep(new_state, config, running, &mut first_transition_log_done)?;
    }

    Ok(())
}

/// Handle the sleep duration and progress logging for the main loop.
fn handle_loop_sleep(
    new_state: TransitionState,
    config: &Config,
    running: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    first_transition_log_done: &mut bool,
) -> Result<()> {
    // Determine sleep duration based on state
    let sleep_duration = match new_state {
        TransitionState::Transitioning { .. } => {
            Duration::from_secs(config.update_interval.unwrap_or(DEFAULT_UPDATE_INTERVAL))
        }
        TransitionState::Stable(_) => time_until_next_event(config),
    };

    // Show next update timing with more context
    match new_state {
        TransitionState::Transitioning { progress, .. } => {
            let log_message = format!(
                "Transition {}% complete. Next update in {} seconds",
                (progress * 100.0) as u8,
                sleep_duration.as_secs()
            );

            if !*first_transition_log_done {
                Log::log_block_start(&log_message);
                *first_transition_log_done = true;
            } else {
                Log::log_decorated(&log_message);
            }
        }
        TransitionState::Stable(_) => {
            *first_transition_log_done = false; // Reset for the next transition period
            Log::log_block_start(&format!(
                "Next transition in {} minutes {} seconds",
                sleep_duration.as_secs() / 60,
                sleep_duration.as_secs() % 60
            ));
        }
    }

    // Sleep in smaller intervals to check running status
    let mut slept = Duration::from_secs(0);
    while slept < sleep_duration && running.load(Ordering::SeqCst) {
        let sleep_chunk = CHECK_INTERVAL.min(sleep_duration - slept);
        thread::sleep(sleep_chunk);
        slept += sleep_chunk;
    }

    Ok(())
}
