//! Main application entry point and high-level flow coordination.
//!
//! This module orchestrates the overall application lifecycle after command-line
//! argument parsing is complete. It coordinates between different modules:
//!
//! - `args`: Command-line argument parsing and help/version display
//! - `config`: Configuration loading and validation
//! - `backend`: Color temperature backend detection and management
//! - `time_state`: Time-based state calculation and transition logic
//! - `utils`: Shared utilities including terminal management and cleanup
//! - `signals`: Signal handling and process management
//! - `logger`: Centralized logging functionality
//! - `startup_transition`: Smooth startup transition management
//!
//! The main application flow is managed through the `ApplicationRunner` builder pattern:
//! - Normal startup: `ApplicationRunner::new(debug_enabled).run()`
//! - Geo restart: `ApplicationRunner::new(true).without_lock().with_previous_state(state).run()`
//! - Geo fresh start: `ApplicationRunner::new(true).without_headers().run()`
//!
//! The builder pattern provides flexibility for different startup contexts while
//! maintaining a clean API. The main flow consists of:
//! 1. Argument parsing and early exit for help/version
//! 2. Terminal setup and lock file management (optional)
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
    time::{Duration, SystemTime},
};

// Import macros from logger module for use in all submodules
#[macro_use]
mod logger;

mod args;
mod backend;
mod commands;
mod config;
mod constants;
mod dbus;
mod geo;
mod signals;
mod simulate;
mod smooth_transitions;
mod time_source;
mod time_state;
mod utils;

use crate::signals::setup_signal_handler;
use crate::utils::TerminalGuard;
use args::{CliAction, ParsedArgs};
use backend::{create_backend, detect_backend, detect_compositor};
use config::Config;
use constants::*;
use smooth_transitions::SmoothTransition;
use time_state::{
    TimeState, get_transition_state, should_update_state, time_until_next_event,
    time_until_transition_end,
};

/// Builder for configuring and running the sunsetr application.
///
/// This builder provides a flexible way to start sunsetr with different
/// configurations depending on the context (normal startup, geo restart, etc.).
///
/// # Examples
///
/// ```
/// // Normal application startup
/// ApplicationRunner::new(debug_enabled).run()?;
///
/// // Restart after geo selection without creating a new lock
/// ApplicationRunner::new(true)
///     .without_lock()
///     .with_previous_state(previous_state)
///     .run()?;
/// ```
pub struct ApplicationRunner {
    debug_enabled: bool,
    create_lock: bool,
    previous_state: Option<TimeState>,
    show_headers: bool,
}

impl ApplicationRunner {
    /// Create a new runner with defaults matching normal run
    pub fn new(debug_enabled: bool) -> Self {
        Self {
            debug_enabled,
            create_lock: true,
            previous_state: None,
            show_headers: true,
        }
    }

    /// Skip lock file creation (for geo restart)
    pub fn without_lock(mut self) -> Self {
        self.create_lock = false;
        self.show_headers = false; // Geo restarts never show headers
        self
    }

    /// Set previous state for smooth transitions
    pub fn with_previous_state(mut self, state: Option<TimeState>) -> Self {
        self.previous_state = state;
        self
    }

    /// Skip header display (for geo operations)
    pub fn without_headers(mut self) -> Self {
        self.show_headers = false;
        self
    }

    /// Execute the application
    pub fn run(self) -> Result<()> {
        // Show headers if requested (mimics run_application behavior)
        if self.show_headers {
            log_version!();
        }

        // Now execute the core logic (previously in run_application_core_with_lock_and_state)
        #[cfg(debug_assertions)]
        {
            let log_msg = format!(
                "DEBUG: Process {} startup: debug_enabled={}, create_lock={}\n",
                std::process::id(),
                self.debug_enabled,
                self.create_lock
            );
            let _ = std::fs::write(
                format!("/tmp/sunsetr-debug-{}.log", std::process::id()),
                log_msg,
            );
        }

        // Try to set up terminal features (cursor hiding, echo suppression)
        // This will gracefully handle cases where no terminal is available (e.g., systemd service)
        let _term = TerminalGuard::new().context("failed to initialize terminal features")?;

        // Note: PR_SET_PDEATHSIG is used for hyprsunset process management in the Hyprland backend
        // to ensure cleanup when sunsetr is forcefully killed. See backend/hyprland/process.rs

        // Set up signal handling
        let signal_state = setup_signal_handler(self.debug_enabled)?;

        // Start D-Bus sleep/resume monitoring (optional - graceful degradation if D-Bus unavailable)
        if let Err(e) =
            dbus::start_sleep_resume_monitor(signal_state.signal_sender.clone(), self.debug_enabled)
        {
            log_pipe!();
            log_warning!("D-Bus sleep/resume monitoring unavailable: {}", e);
            log_indented!(
                "Sleep/resume detection will not work, but sunsetr will continue normally"
            );
            log_indented!("This is normal in environments without systemd or D-Bus");
        }

        // Load and validate configuration first
        let config = Config::load()?;

        // Detect and validate the backend early
        let backend_type = detect_backend(&config)?;

        if self.create_lock {
            // Create lock file path
            let runtime_dir =
                std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
            let lock_path = format!("{runtime_dir}/sunsetr.lock");

            // Open lock file without truncating to preserve existing content
            // This prevents a race condition where File::create() would truncate
            // the file before we check if the lock can be acquired.
            // See tests/lock_file_unit_tests.rs and tests/lock_logic_test.rs for details.
            let mut lock_file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false) // Don't truncate existing file
                .open(&lock_path)?;

            // Try to acquire exclusive lock
            match lock_file.try_lock_exclusive() {
                Ok(_) => {
                    // Lock acquired - now safe to truncate and write our info
                    use std::io::{Seek, SeekFrom, Write};

                    // Truncate the file and reset position
                    lock_file.set_len(0)?;
                    lock_file.seek(SeekFrom::Start(0))?;

                    // Write our PID, compositor, and config dir to the lock file for restart functionality
                    let pid = std::process::id();
                    let compositor = detect_compositor().to_string();
                    writeln!(&lock_file, "{pid}")?;
                    writeln!(&lock_file, "{compositor}")?;
                    // Write config directory (empty line if using default)
                    if let Some(ref dir) = config::get_custom_config_dir() {
                        writeln!(&lock_file, "{}", dir.display())?;
                    } else {
                        writeln!(&lock_file)?;
                    }
                    lock_file.flush()?;

                    log_block_start!("Lock acquired, starting sunsetr...");
                    run_sunsetr_main_logic(
                        config,
                        backend_type,
                        &signal_state,
                        self.debug_enabled,
                        Some((lock_file, lock_path)),
                        self.previous_state,
                    )?;
                }
                Err(_) => {
                    // Handle lock conflict with smart validation
                    match handle_lock_conflict(&lock_path) {
                        Ok(()) => {
                            // Stale lock removed or cross-compositor cleanup completed
                            // Retry lock acquisition without truncating
                            let mut retry_lock_file = std::fs::OpenOptions::new()
                                .write(true)
                                .create(true)
                                .truncate(false)
                                .open(&lock_path)?;
                            match retry_lock_file.try_lock_exclusive() {
                                Ok(_) => {
                                    // Lock acquired - now safe to truncate and write our info
                                    use std::io::{Seek, SeekFrom, Write};

                                    // Truncate the file and reset position
                                    retry_lock_file.set_len(0)?;
                                    retry_lock_file.seek(SeekFrom::Start(0))?;

                                    // Write our PID, compositor, and config dir to the lock file
                                    let pid = std::process::id();
                                    let compositor = detect_compositor().to_string();
                                    writeln!(&retry_lock_file, "{pid}")?;
                                    writeln!(&retry_lock_file, "{compositor}")?;
                                    // Write config directory (empty line if using default)
                                    if let Some(ref dir) = config::get_custom_config_dir() {
                                        writeln!(&retry_lock_file, "{}", dir.display())?;
                                    } else {
                                        writeln!(&retry_lock_file)?;
                                    }
                                    retry_lock_file.flush()?;

                                    log_block_start!(
                                        "Lock acquired after cleanup, starting sunsetr..."
                                    );
                                    run_sunsetr_main_logic(
                                        config,
                                        backend_type,
                                        &signal_state,
                                        self.debug_enabled,
                                        Some((retry_lock_file, lock_path)),
                                        self.previous_state,
                                    )?;
                                }
                                Err(_) => {
                                    // Error already logged by handle_lock_conflict
                                    std::process::exit(EXIT_FAILURE);
                                }
                            }
                        }
                        Err(_) => {
                            // Error already logged by handle_lock_conflict
                            std::process::exit(EXIT_FAILURE);
                        }
                    }
                }
            }
        } else {
            // Skip lock creation (geo selection restart case or simulation mode)
            // Only show "Restarting" message if not in simulation mode
            if !time_source::is_simulated() {
                log_block_start!("Restarting sunsetr...");
            }
            run_sunsetr_main_logic(
                config,
                backend_type,
                &signal_state,
                self.debug_enabled,
                None,
                self.previous_state,
            )?;
        }

        Ok(())
    }
}

fn main() -> Result<()> {
    // Parse command-line arguments
    let parsed_args = ParsedArgs::from_env();

    // Extract config_dir from the action and set it globally if provided
    let config_dir = match &parsed_args.action {
        CliAction::Run { config_dir, .. }
        | CliAction::ReloadCommand { config_dir, .. }
        | CliAction::TestCommand { config_dir, .. }
        | CliAction::GeoCommand { config_dir, .. }
        | CliAction::PresetCommand { config_dir, .. }
        | CliAction::RunGeoSelection { config_dir, .. }
        | CliAction::Reload { config_dir, .. }
        | CliAction::Test { config_dir, .. }
        | CliAction::Simulate { config_dir, .. } => config_dir.clone(),
        _ => None,
    };

    // Set the config directory once at startup if provided
    if let Some(dir) = config_dir {
        config::set_config_dir(Some(dir))?;
    }

    match parsed_args.action {
        CliAction::ShowVersion => {
            args::display_version_info();
            Ok(())
        }
        CliAction::ShowHelp | CliAction::ShowHelpDueToError => {
            args::display_help();
            Ok(())
        }
        CliAction::Run { debug_enabled, .. } => {
            // Continue with normal application flow using builder pattern
            ApplicationRunner::new(debug_enabled).run()
        }
        // Handle both deprecated flag and new subcommand syntax for reload
        CliAction::Reload { debug_enabled, .. }
        | CliAction::ReloadCommand { debug_enabled, .. } => {
            commands::reload::handle_reload_command(debug_enabled)
        }
        // Handle both deprecated flag and new subcommand syntax for test
        CliAction::Test {
            debug_enabled,
            temperature,
            gamma,
            ..
        }
        | CliAction::TestCommand {
            debug_enabled,
            temperature,
            gamma,
            ..
        } => commands::test::handle_test_command(temperature, gamma, debug_enabled),
        // Handle both deprecated flag and new subcommand syntax for geo
        CliAction::RunGeoSelection { debug_enabled, .. }
        | CliAction::GeoCommand { debug_enabled, .. } => {
            match commands::geo::handle_geo_command(debug_enabled)? {
                geo::GeoCommandResult::RestartInDebugMode { previous_state } => {
                    ApplicationRunner::new(true)
                        .without_lock()
                        .with_previous_state(previous_state)
                        .run()
                }
                geo::GeoCommandResult::StartNewInDebugMode => {
                    ApplicationRunner::new(true).without_headers().run()
                }
                geo::GeoCommandResult::Completed => Ok(()),
            }
        }
        CliAction::Simulate {
            debug_enabled,
            start_time,
            end_time,
            multiplier,
            log_to_file,
            ..
        } => {
            // Handle --simulate flag: set up simulated time source
            // Keep the guards alive for the duration of the simulation
            let mut simulation_guards = simulate::handle_simulate_command(
                start_time,
                end_time,
                multiplier,
                debug_enabled,
                log_to_file,
            )?;

            // Run the application with simulated time
            // The output will go to stdout/stderr as normal, which the user can redirect
            ApplicationRunner::new(debug_enabled)
                .without_lock() // Don't interfere with real instances
                .without_headers() // Headers already shown by simulate command
                .run()?;

            // Only complete the simulation if it ran to completion (not interrupted)
            if time_source::simulation_ended() {
                simulation_guards.complete_simulation();
            }
            // Otherwise, the Drop implementation will handle cleanup without the "complete" message

            Ok(())
        }
        // Preset is subcommand-only (no deprecated flag version)
        CliAction::PresetCommand {
            debug_enabled,
            preset_name,
            ..
        } => match commands::preset::handle_preset_command(&preset_name)? {
            commands::preset::PresetResult::Exit => Ok(()),
            commands::preset::PresetResult::ContinueExecution => {
                ApplicationRunner::new(debug_enabled)
                    .without_headers()
                    .run()
            }
        },
    }
}

/// Core application logic that coordinates the main sunsetr loop.
///
/// This function is called after lock acquisition and handles the main
/// application flow including backend setup, initial state application,
/// and the main monitoring loop.
///
/// # Arguments
/// * `config` - Application configuration
/// * `backend_type` - Detected backend type
/// * `signal_state` - Signal handling state
/// * `debug_enabled` - Whether debug logging is enabled
/// * `lock_info` - Optional lock file and path for cleanup
/// * `initial_previous_state` - Optional previous state for smooth transitions
///
/// # Returns
/// Result indicating success or failure of the application run
fn run_sunsetr_main_logic(
    mut config: Config,
    backend_type: backend::BackendType,
    signal_state: &crate::signals::SignalState,
    debug_enabled: bool,
    lock_info: Option<(File, String)>,
    initial_previous_state: Option<time_state::TimeState>,
) -> Result<()> {
    // Log configuration with resolved backend type
    config.log_config(Some(backend_type));

    // Initialize GeoTransitionTimes before backend creation if in geo mode
    // The Hyprland backend needs this to calculate correct initial values
    let geo_times = crate::geo::GeoTransitionTimes::from_config(&config)
        .context("Failed to initialize geo transition times")?;

    log_block_start!("Detected backend: {}", backend_type.name());

    let mut backend = create_backend(backend_type, &config, debug_enabled, geo_times.as_ref())?;

    // Backend creation already includes connection verification and logging
    log_block_start!(
        "Successfully connected to {} backend",
        backend.backend_name()
    );

    // If we're using Hyprland backend under Hyprland compositor, reset Wayland gamma
    // to clean up any leftover gamma from previous Wayland backend sessions.
    // This ensures a clean slate when switching between backends
    if backend.backend_name() == "hyprland" && std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()
    {
        if debug_enabled {
            log_pipe!();
            log_debug!("Detected Hyprland backend under Hyprland compositor");
            log_decorated!("Resetting any leftover Wayland gamma from previous sessions...");
        }

        // Create a temporary Wayland backend to reset Wayland gamma
        match crate::backend::wayland::WaylandBackend::new(&config, debug_enabled) {
            Ok(mut wayland_backend) => {
                use crate::backend::ColorTemperatureBackend;
                let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
                if let Err(e) = wayland_backend.apply_temperature_gamma(6500, 100.0, &running) {
                    if debug_enabled {
                        log_warning!("Failed to reset Wayland gamma: {e}");
                        log_indented!("This is normal if no Wayland gamma control is available");
                    }
                } else if debug_enabled {
                    log_decorated!("Successfully reset Wayland gamma");
                }
            }
            Err(e) => {
                if debug_enabled {
                    log_error!("Could not create Wayland backend for reset: {e}");
                    log_indented!("This is normal if Wayland gamma control is not available");
                }
            }
        }
    }

    let mut current_transition_state = get_transition_state(&config, geo_times.as_ref());
    let mut last_check_time = crate::time_source::system_now();

    // Apply initial settings
    apply_initial_state(
        &mut backend,
        current_transition_state,
        initial_previous_state,
        &config,
        &signal_state.running,
        debug_enabled,
        &geo_times,
    )?;

    // Log solar debug info on startup for geo mode (after initial state is applied)
    if debug_enabled
        && config.transition_mode.as_deref() == Some("geo")
        && let (Some(lat), Some(lon)) = (config.latitude, config.longitude)
    {
        let _ = crate::geo::log_solar_debug_info(lat, lon);
    }

    // Main application loop
    run_main_loop(
        &mut backend,
        &mut current_transition_state,
        &mut last_check_time,
        &mut config,
        signal_state,
        debug_enabled,
        geo_times.clone(),
    )?;

    // Ensure proper cleanup on shutdown
    log_block_start!("Shutting down sunsetr...");

    // Perform smooth shutdown transition if enabled (skip for Hyprland)
    let smooth_shutdown_performed =
        if config.smoothing.unwrap_or(DEFAULT_SMOOTHING) && backend.backend_name() != "Hyprland" {
            // Create fresh geo_times from current config if in geo mode
            // This ensures we use the correct location after any config reloads
            let fresh_geo_times = if config.transition_mode.as_deref() == Some("geo") {
                crate::geo::GeoTransitionTimes::from_config(&config)
                    .ok()
                    .flatten()
            } else {
                None
            };

            if let Some(mut transition) = SmoothTransition::shutdown(&config, fresh_geo_times) {
                // Use silent mode for shutdown to suppress progress bar and logs
                transition = transition.silent();
                transition
                    .execute(&mut *backend, &config, &signal_state.running)
                    .is_ok()
            } else {
                false
            }
        } else {
            false
        };

    // Reset gamma if needed (skip if smooth transition handled it or if using Hyprland)
    // Hyprland backend (hyprsunset v0.3.1+) resets gamma automatically on exit
    if !smooth_shutdown_performed && backend.backend_name() != "Hyprland" {
        if debug_enabled {
            log_decorated!("Resetting color temperature and gamma...");
        }
        let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        if let Err(e) = backend.apply_temperature_gamma(6500, 100.0, &running) {
            log_warning!("Failed to reset color temperature: {e}");
        } else if debug_enabled {
            log_decorated!("Gamma reset completed successfully");
        }
    }

    // Clean up resources (backend, lock file)
    if let Some((lock_file, lock_path)) = lock_info {
        utils::cleanup_application(backend, lock_file, &lock_path, debug_enabled);
    } else {
        // No lock file to clean up (geo selection restart case)
        backend.cleanup(debug_enabled);
    }
    log_end!();

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
/// * `previous_state` - Optional previous state (for config reloads)
/// * `config` - Application configuration
/// * `running` - Shared running state for shutdown detection
/// * `debug_enabled` - Whether debug logging is enabled
fn apply_initial_state(
    backend: &mut Box<dyn crate::backend::ColorTemperatureBackend>,
    current_state: TimeState,
    previous_state: Option<TimeState>,
    config: &Config,
    running: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    debug_enabled: bool,
    geo_times: &Option<crate::geo::GeoTransitionTimes>,
) -> Result<()> {
    if !running.load(Ordering::SeqCst) {
        return Ok(());
    }

    // Note: No reset needed here - backends should start with correct interpolated values
    // Cross-backend reset (if needed) is handled separately before this function

    // Check if startup transition is enabled and we're not using Hyprland backend
    // Hyprland (hyprsunset) has its own forced startup transition, so we skip ours
    let is_hyprland = backend.backend_name().to_lowercase() == "hyprland";
    let smoothing = config.smoothing.unwrap_or(DEFAULT_SMOOTHING);
    let startup_duration = config.startup_duration.unwrap_or(DEFAULT_STARTUP_DURATION);

    // Treat durations < 0.1 as instant (no transition)
    if smoothing && startup_duration >= 0.1 && !is_hyprland {
        // Create transition based on whether we have a previous state
        let mut transition = if let Some(prev_state) = previous_state {
            // Config reload: transition from previous state values to new state
            let (start_temp, start_gamma) = prev_state.values(config);
            // Clone geo_times to pass to the transition (geo_times are now passed from run_application)
            let geo_times_clone = geo_times.clone();
            SmoothTransition::reload(
                start_temp,
                start_gamma,
                current_state,
                config,
                geo_times_clone,
            )
        } else {
            // Initial startup: use default transition (from day values)
            // Clone geo_times to pass to the transition
            let geo_times_clone = geo_times.clone();
            SmoothTransition::startup(current_state, config, geo_times_clone)
        };

        // Disable progress bar and logs in simulation mode (runs silently)
        if crate::time_source::is_simulated() {
            transition = transition.silent();
        }

        match transition.execute(backend.as_mut(), config, running) {
            Ok(_) => {}
            Err(e) => {
                log_warning!("Failed to apply smooth startup transition: {e}");
                log_decorated!("Falling back to immediate transition...");

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
    current_state: TimeState,
    config: &Config,
    running: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    debug_enabled: bool,
) -> Result<()> {
    match backend.apply_startup_state(current_state, config, running) {
        Ok(_) => {
            if debug_enabled {
                log_pipe!();
                log_debug!("Initial state applied successfully");
            }
        }
        Err(e) => {
            log_warning!("Failed to apply initial state: {e}");
            log_decorated!("Continuing anyway - will retry during operation...");
        }
    }
    Ok(())
}

/// Run the main application loop that monitors and applies state changes.
///
/// This loop continuously monitors the time-based state and applies changes
/// to the backend when necessary. It handles transition detection, comprehensive
/// time anomaly detection (suspend/resume, clock changes, DST), and graceful shutdown.
///
/// ## Time Anomaly Detection
///
/// The loop uses wall clock time (`SystemTime`) to detect various scenarios:
/// - System suspend/resume (handles overnight laptop sleep scenarios)
/// - Clock adjustments and DST transitions
/// - Time jumps that may require state recalculation
///   The `should_update_state` function handles these cases by checking elapsed time
fn run_main_loop(
    backend: &mut Box<dyn crate::backend::ColorTemperatureBackend>,
    current_transition_state: &mut TimeState,
    last_check_time: &mut SystemTime,
    config: &mut Config,
    signal_state: &crate::signals::SignalState,
    debug_enabled: bool,
    mut geo_times: Option<crate::geo::GeoTransitionTimes>,
) -> Result<()> {
    // Skip first iteration to prevent false state change detection due to startup timing
    let mut first_iteration = true;
    // Tracks if the initial transition progress log has been made using `log_block_start`.
    // Subsequent transition progress logs will use `log_decorated` when debug is disabled.
    let mut first_transition_log_done = false;
    // Track previous progress for decimal display logic
    let mut previous_progress: Option<f32> = None;
    // Track the previous state to detect transitions
    let mut previous_state: Option<TimeState> = None;

    // Note: geo_times is now passed as a parameter, initialized before the main loop starts
    // to ensure correct initial state calculation

    #[cfg(debug_assertions)]
    {
        let log_msg = format!("Entering main loop, PID: {}\n", std::process::id());
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
    let mut debug_loop_count: u64 = 0;

    // Initialize current state tracking
    let mut current_state = get_transition_state(config, geo_times.as_ref());

    // Track the last applied temperature and gamma values
    // Initialize with the values for the current state
    let (initial_temp, initial_gamma) = current_state.values(config);
    let mut last_applied_temp = initial_temp;
    let mut last_applied_gamma = initial_gamma;

    while signal_state.running.load(Ordering::SeqCst) && !crate::time_source::simulation_ended() {
        #[cfg(debug_assertions)]
        {
            debug_loop_count += 1;
            eprintln!("DEBUG: Main loop iteration {debug_loop_count} starting");
        }

        // Process any pending signals immediately (non-blocking check)
        // This ensures signals sent before the loop starts are handled
        if first_iteration {
            while let Ok(signal_msg) = signal_state.signal_receiver.try_recv() {
                crate::signals::handle_signal_message(
                    signal_msg,
                    backend,
                    config,
                    signal_state,
                    &mut current_state,
                    debug_enabled,
                )?;
                // Reset last_check_time after handling signal to avoid false time jump warnings
                // when processing takes time (especially during config reload)
                *last_check_time = crate::time_source::system_now();
            }
        }

        // Check if we need to reload state after config change
        if signal_state.needs_reload.load(Ordering::SeqCst) {
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Detected needs_reload flag, applying state with startup transition");

            // Clear the flag first
            signal_state.needs_reload.store(false, Ordering::SeqCst);

            // Handle geo times based on current mode
            if config.transition_mode.as_deref() == Some("geo") {
                // In geo mode - create or update geo times
                if let (Some(lat), Some(lon)) = (config.latitude, config.longitude) {
                    // Check if we already have geo_times and just need to update for location change
                    if let Some(ref mut times) = geo_times {
                        // Use handle_location_change for existing geo_times
                        if let Err(e) = times.handle_location_change(lat, lon) {
                            log_pipe!();
                            log_critical!("Failed to update geo times after config reload: {e}");
                            // Fall back to creating new times if update failed
                            geo_times = crate::geo::GeoTransitionTimes::from_config(config)
                                .context(
                                    "Solar calculations failed after config reload - this is a bug",
                                )?;
                        }
                    } else {
                        // Create new geo_times if none exists
                        geo_times = crate::geo::GeoTransitionTimes::from_config(config).context(
                            "Solar calculations failed after config reload - this is a bug",
                        )?;
                    }
                }
            } else {
                // Not in geo mode - clear geo_times to ensure we use manual times
                if geo_times.is_some() {
                    #[cfg(debug_assertions)]
                    eprintln!("DEBUG: Clearing geo_times after switching away from geo mode");
                    geo_times = None;
                }
            }

            // Get the new state and apply it with startup transition support
            let reload_state = get_transition_state(config, geo_times.as_ref());

            // Check if startup transitions are enabled
            let startup_transition_enabled = config
                .startup_transition
                .unwrap_or(DEFAULT_STARTUP_TRANSITION);

            // Debug logging for config reload state change detection
            if debug_enabled {
                log_pipe!();
                log_debug!("Config reload state change detection:");
                log_indented!("Current state: {:?}", current_state);
                log_indented!("Reload state: {:?}", reload_state);
                log_indented!(
                    "Current temp/gamma: {}K @ {}%",
                    last_applied_temp,
                    last_applied_gamma
                );
                log_indented!("Startup transition enabled: {}", startup_transition_enabled);
            }

            // ALWAYS use smooth transition during reload if enabled
            // The config or state has changed (that's why needs_reload was set)
            // We transition from current temp/gamma to whatever the new config requires
            if startup_transition_enabled {
                // Create a custom transition from actual current values to new state
                let mut transition = SmoothTransition::reload(
                    last_applied_temp,
                    last_applied_gamma,
                    reload_state,
                    config,
                    geo_times.clone(),
                );

                // Configure for silent reload operation
                transition = transition.silent();

                // Execute the transition
                match transition.execute(backend.as_mut(), config, &signal_state.running) {
                    Ok(_) => {
                        // Update our tracking variables
                        *current_transition_state = reload_state;
                        current_state = reload_state;

                        // Update last applied values
                        let (new_temp, new_gamma) = reload_state.values(config);
                        last_applied_temp = new_temp;
                        last_applied_gamma = new_gamma;

                        log_pipe!();
                        log_info!("Configuration reloaded and state applied successfully");
                    }
                    Err(e) => {
                        log_warning!("Failed to apply transition after config reload: {e}");
                        // Don't update tracking variables if application failed
                    }
                }
            } else {
                // Non-geo mode or transitions disabled: use normal apply_initial_state
                let previous_state = Some(current_state);

                match apply_initial_state(
                    backend,
                    reload_state,
                    previous_state,
                    config,
                    &signal_state.running,
                    debug_enabled,
                    &geo_times,
                ) {
                    Ok(_) => {
                        // Update our tracking variables
                        *current_transition_state = reload_state;
                        current_state = reload_state;

                        // Update last applied values
                        let (new_temp, new_gamma) = reload_state.values(config);
                        last_applied_temp = new_temp;
                        last_applied_gamma = new_gamma;

                        log_pipe!();
                        log_info!("Configuration reloaded and state applied successfully");
                    }
                    Err(e) => {
                        log_warning!("Failed to apply new state after config reload: {e}");
                        // Don't update tracking variables if application failed
                    }
                }

                // Reset last_check_time after config reload to avoid false time jump warnings
                // The reload process can take time and we don't want this to be seen as a time anomaly
                *last_check_time = crate::time_source::system_now();
            }
        }

        // Get current wall clock time for suspend detection
        let current_time = crate::time_source::system_now();

        // Check if geo_times needs recalculation (e.g., after midnight)
        if let Some(ref mut times) = geo_times
            && times.needs_recalculation(crate::time_source::now())
            && let (Some(lat), Some(lon)) = (config.latitude, config.longitude)
            && let Err(e) = times.recalculate_for_next_period(lat, lon)
        {
            log_warning!("Failed to recalculate geo times: {e}");
        }

        let new_state = get_transition_state(config, geo_times.as_ref());

        // Skip first iteration to prevent false state change detection caused by
        // timing differences between startup state application and main loop start
        let should_update = if first_iteration {
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: First iteration, skipping state update check");

            first_iteration = false;
            false
        } else {
            let update_needed = should_update_state(
                &current_state,
                &new_state,
                current_time,
                *last_check_time,
                config,
            );

            // If time anomaly was detected and we're in geo mode, handle it
            if update_needed && let Some(ref mut times) = geo_times {
                // Check if this was a time anomaly by looking at time difference
                let elapsed = current_time
                    .duration_since(*last_check_time)
                    .unwrap_or_else(|_| Duration::from_secs(0));

                // If elapsed time is unusual (suspend/resume or time jump)
                if (elapsed > Duration::from_secs(30) || current_time < *last_check_time)
                    && let (Some(lat), Some(lon)) = (config.latitude, config.longitude)
                    && let Err(e) = times.handle_time_anomaly(lat, lon)
                {
                    log_warning!("Failed to handle time anomaly in geo times: {e}");
                }
            }

            #[cfg(debug_assertions)]
            eprintln!(
                "DEBUG: should_update_state result: {update_needed}, current_state: {current_state:?}, new_state: {new_state:?}"
            );

            update_needed
        };

        // Update last check time after state evaluation
        *last_check_time = current_time;

        if should_update && signal_state.running.load(Ordering::SeqCst) {
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Applying state update - state: {new_state:?}");

            match backend.apply_transition_state(new_state, config, &signal_state.running) {
                Ok(_) => {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "DEBUG: State application successful, updating current_transition_state"
                    );

                    // Success - update our state
                    *current_transition_state = new_state;
                    current_state = new_state;

                    // Update last applied values
                    let (new_temp, new_gamma) = new_state.values(config);
                    last_applied_temp = new_temp;
                    last_applied_gamma = new_gamma;
                }
                Err(e) => {
                    #[cfg(debug_assertions)]
                    eprintln!("DEBUG: State application failed: {e}");

                    // Failure - check if it's a connection issue that couldn't be resolved
                    if e.to_string().contains("reconnection attempt") {
                        log_error!("Cannot communicate with {}: {}", backend.backend_name(), e);
                        log_decorated!(
                            "{} appears to be permanently unavailable. Exiting...",
                            backend.backend_name()
                        );
                        break; // Exit the main loop
                    } else {
                        // Other error - just log it and retry next cycle
                        log_pipe!();
                        log_error!("Failed to apply state: {e}");
                        log_decorated!("Will retry on next cycle...");
                    }
                    // Don't update current_transition_state - try again next cycle
                }
            }
        }

        // Calculate sleep duration and log progress
        // Use current_state which reflects any updates we just applied
        let calculated_sleep_duration = calculate_and_log_sleep(
            current_state,
            config,
            geo_times.as_ref(),
            &mut first_transition_log_done,
            debug_enabled,
            &mut previous_progress,
            &mut previous_state,
        )?;

        // Sleep with signal awareness using recv_timeout
        // This blocks until either a signal arrives or the timeout expires
        use std::sync::mpsc::RecvTimeoutError;

        // Helper: poll backend hotplug periodically during long sleeps
        let mut poll_interval = Duration::from_millis(250);
        if poll_interval > calculated_sleep_duration {
            poll_interval = calculated_sleep_duration;
        }

        // In simulation mode, time_source::sleep already handles the time scaling
        // We can't use recv_timeout with the full duration as it would sleep too long
        // So we need to handle simulation differently
        let recv_result = if crate::time_source::is_simulated() {
            // Sleep in a separate thread so we can still receive signals
            let sleep_handle = std::thread::spawn({
                let duration = calculated_sleep_duration;
                move || {
                    crate::time_source::sleep(duration);
                }
            });

            // Poll for signals while the sleep thread runs
            loop {
                // Periodically poll backend for hotplug events
                let _ = backend.poll_hotplug();

                match signal_state
                    .signal_receiver
                    .recv_timeout(Duration::from_millis(10))
                {
                    Ok(msg) => break Ok(msg),
                    Err(RecvTimeoutError::Timeout) => {
                        if sleep_handle.is_finished() {
                            break Err(RecvTimeoutError::Timeout);
                        }
                    }
                    Err(e) => break Err(e),
                }
            }
        } else {
            // Normal operation: block in small chunks to allow hotplug polling
            let start = std::time::Instant::now();
            let mut remaining = calculated_sleep_duration;

            loop {
                let chunk = if remaining > poll_interval {
                    poll_interval
                } else {
                    remaining
                };
                match signal_state.signal_receiver.recv_timeout(chunk) {
                    Ok(msg) => break Ok(msg),
                    Err(RecvTimeoutError::Timeout) => {
                        // Poll backend for hotplug and continue if time remains
                        let _ = backend.poll_hotplug();
                        if start.elapsed() >= calculated_sleep_duration {
                            break Err(RecvTimeoutError::Timeout);
                        }
                        remaining = calculated_sleep_duration.saturating_sub(start.elapsed());
                    }
                    Err(e) => break Err(e),
                }
            }
        };

        match recv_result {
            Ok(signal_msg) => {
                // Signal received - handle it immediately
                crate::signals::handle_signal_message(
                    signal_msg,
                    backend,
                    config,
                    signal_state,
                    &mut current_state,
                    debug_enabled,
                )?;
                // Reset last_check_time after handling signal to avoid false time jump warnings
                // when signal processing takes time (especially during config reload)
                *last_check_time = crate::time_source::system_now();
            }
            Err(RecvTimeoutError::Timeout) => {
                // Normal timeout - continue to next iteration
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: Sleep duration elapsed naturally");
            }
            Err(RecvTimeoutError::Disconnected) => {
                // Channel disconnected - check if it's expected shutdown
                if !signal_state.running.load(Ordering::SeqCst) {
                    // Expected shutdown - user pressed Ctrl+C or sent termination signal
                    #[cfg(debug_assertions)]
                    eprintln!("DEBUG: Channel disconnected during graceful shutdown");
                } else {
                    // Unexpected disconnection - signal handler thread died
                    log_pipe!();
                    log_error!("Signal handler disconnected unexpectedly");
                    log_indented!("Signals will no longer be processed");
                    log_indented!("Consider restarting sunsetr if signal handling is needed");
                    // Continue running without signal support
                }
            }
        }
    }

    #[cfg(debug_assertions)]
    {
        let log_msg = format!(
            "Main loop exiting normally for PID: {}\n",
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

    Ok(())
}

/// Calculate sleep duration and log progress for the main loop.
/// Returns the duration to sleep.
fn calculate_and_log_sleep(
    new_state: TimeState,
    config: &Config,
    geo_times: Option<&crate::geo::GeoTransitionTimes>,
    first_transition_log_done: &mut bool,
    debug_enabled: bool,
    previous_progress: &mut Option<f32>,
    previous_state: &mut Option<TimeState>,
) -> Result<Duration> {
    // Determine sleep duration based on state
    let sleep_duration = if new_state.is_transitioning() {
        let update_interval =
            Duration::from_secs(config.update_interval.unwrap_or(DEFAULT_UPDATE_INTERVAL));

        // Check if we're near the end of the transition
        if let Some(time_remaining) = time_until_transition_end(config, geo_times) {
            if time_remaining < update_interval {
                // Sleep only until the transition ends
                time_remaining
            } else {
                // Normal update interval
                update_interval
            }
        } else {
            // Fallback to normal interval (shouldn't happen)
            update_interval
        }
    } else {
        time_until_next_event(config, geo_times)
    };

    // Show next update timing with more context
    if let Some(progress) = new_state.progress() {
        // Calculate the percentage change from the previous update
        let current_percentage = progress * 100.0;
        let percentage_change = if let Some(prev) = *previous_progress {
            (current_percentage - prev * 100.0).abs()
        } else {
            // First update: determine initial precision based on where we are in the transition
            // Near start (< 1%): show decimals like 0.06%
            // Near end (> 99%): show decimals like 99.92%
            // In middle: can show as integer
            if !(1.0..=99.0).contains(&current_percentage) {
                0.05 // Small value to trigger decimal display at extremes
            } else {
                1.0 // Normal value for middle range
            }
        };

        #[cfg(debug_assertions)]
        {
            eprintln!(
                "DEBUG: progress={progress:.6}, \
                     current_percentage={current_percentage:.4}, \
                     percentage_change={percentage_change:.4}"
            );
        }

        // Format the percentage intelligently based on value and rate of change
        // The Bézier curve naturally creates varying speeds, so we adjust precision accordingly
        let percentage_str = {
            // Determine precision based on rate of change
            let (precision, min_value, max_value) = if percentage_change < 0.1 {
                // Very slow: 2 decimal places, never below 0.01 or above 99.99
                (2, 0.01, 99.99)
            } else if percentage_change < 1.0 {
                // Slow: 1 decimal place, never below 0.1 or above 99.9
                (1, 0.1, 99.9)
            } else {
                // Fast: integers, never show 0 or 100
                (0, 1.0, 99.0)
            };

            // Clamp and format with the appropriate precision
            let clamped = current_percentage.clamp(min_value, max_value);
            match precision {
                0 => format!("{}", clamped.round() as u8),
                1 => format!("{clamped:.1}"),
                2 => format!("{clamped:.2}"),
                _ => unreachable!(),
            }
        };

        let log_message = format!(
            "Transition {}% complete. Next update in {} seconds",
            percentage_str,
            sleep_duration.as_secs()
        );

        // Update the previous progress for next iteration
        *previous_progress = Some(progress);

        if debug_enabled {
            // In debug mode, always use log_block_start for better visibility
            log_block_start!("{}", log_message);
        } else if !*first_transition_log_done {
            // space out first log
            log_block_start!("{}", log_message);
            *first_transition_log_done = true;
        } else {
            // group the rest of the logs together
            log_decorated!("{}", log_message);
        }
    } else {
        // Stable state
        *first_transition_log_done = false; // Reset for the next transition period
        *previous_progress = None; // Reset progress tracking for next transition

        // Debug logging to show exact transition time (skip for static mode)
        if debug_enabled && new_state != TimeState::Static {
            let now = crate::time_source::now();
            let next_transition_time_raw =
                now + chrono::Duration::milliseconds(sleep_duration.as_millis() as i64);

            // Round up to the next whole second for display only if there are milliseconds
            // This ensures the displayed time matches when the transition actually occurs
            let millis = next_transition_time_raw.timestamp_millis();
            let remainder_millis = millis % 1000;
            let next_transition_time = if remainder_millis > 0 {
                // Has partial seconds, round up to next whole second
                let next_second_millis = ((millis / 1000) + 1) * 1000;
                chrono::DateTime::<chrono::Utc>::from_timestamp_millis(next_second_millis)
                    .map(|utc| utc.with_timezone(&chrono::Local))
                    .unwrap_or(next_transition_time_raw)
            } else {
                // Already at a whole second, use as-is
                next_transition_time_raw
            };

            // Determine transition direction based on current state
            let next = new_state.next_state();
            let transition_info = format!(
                "{} {} → {} {}",
                new_state.display_name(),
                new_state.symbol(),
                next.display_name(),
                next.symbol()
            );

            // For geo mode, show time in both city timezone and local timezone
            if config.transition_mode.as_deref() == Some("geo")
                && let (Some(lat), Some(lon)) = (config.latitude, config.longitude)
            {
                // Use tzf-rs to get the timezone for these exact coordinates
                let city_tz = crate::geo::solar::determine_timezone_from_coordinates(lat, lon);

                // Convert the next transition time to the city's timezone
                let next_transition_city_tz = next_transition_time.with_timezone(&city_tz);

                log_pipe!();
                // Check if city timezone matches local timezone by comparing offset
                use chrono::Offset;
                let city_offset = next_transition_city_tz.offset().fix();
                let local_offset = next_transition_time.offset().fix();
                let same_timezone = city_offset == local_offset;

                if same_timezone {
                    log_debug!(
                        "Next transition will begin at: {} {}",
                        next_transition_city_tz.format("%H:%M:%S"),
                        transition_info
                    );
                } else {
                    log_debug!(
                        "Next transition will begin at: {} [{}] {}",
                        next_transition_city_tz.format("%H:%M:%S"),
                        next_transition_time.format("%H:%M:%S"),
                        transition_info
                    );
                }
            } else {
                // Non-geo mode or geo mode without coordinates
                log_pipe!();
                log_debug!(
                    "Next transition will begin at: {} {}",
                    next_transition_time.format("%H:%M:%S"),
                    transition_info
                );
            }
        }

        // Detect if we just entered a stable state
        let just_entered_stable = match previous_state {
            Some(prev_state) if prev_state.is_transitioning() => true,
            None => true, // First iteration entering stable
            _ => false,
        };

        // Only log the countdown when entering stable state and there's meaningful time remaining
        // Skip this for static mode since it never transitions
        if just_entered_stable
            && sleep_duration >= Duration::from_secs(1)
            && new_state != TimeState::Static
        {
            log_block_start!(
                "Next transition in {} minutes {} seconds",
                sleep_duration.as_secs() / 60,
                sleep_duration.as_secs() % 60
            );
        }
    }

    // Update previous state for next iteration
    *previous_state = Some(new_state);

    Ok(sleep_duration)
}

/// Handle lock file conflicts with smart validation and cleanup
fn handle_lock_conflict(lock_path: &str) -> Result<()> {
    // Read the lock file to get PID and compositor info
    let lock_content = match std::fs::read_to_string(lock_path) {
        Ok(content) => content,
        Err(_) => {
            // Lock file doesn't exist or can't be read - assume it was cleaned up
            return Ok(());
        }
    };

    let lines: Vec<&str> = lock_content.trim().lines().collect();

    // Lock file format: PID (line 1), compositor (line 2), config_dir (line 3, optional)
    if lines.len() < 2 || lines.len() > 3 {
        // Invalid lock file format
        log_warning!("Lock file format invalid, removing");
        let _ = std::fs::remove_file(lock_path);
        return Ok(());
    }

    let pid = match lines[0].parse::<u32>() {
        Ok(pid) => pid,
        Err(_) => {
            log_warning!("Lock file contains invalid PID, removing stale lock");
            let _ = std::fs::remove_file(lock_path);
            return Ok(());
        }
    };

    let existing_compositor = lines[1].to_string();

    // Check if the process is actually running
    if !crate::utils::is_process_running(pid) {
        log_warning!("Removing stale lock file (process {pid} no longer running)");
        let _ = std::fs::remove_file(lock_path);
        return Ok(());
    }

    // Process is running - check if this is a cross-compositor switch scenario
    let current_compositor = detect_compositor().to_string();

    if existing_compositor != current_compositor {
        // Cross-compositor switch detected - force cleanup
        log_pipe!();
        log_warning!(
            "Cross-compositor switch detected: {existing_compositor} → {current_compositor}"
        );
        log_indented!("Terminating existing sunsetr process (PID: {pid})");

        if utils::kill_process(pid) {
            // Wait for process to fully exit
            std::thread::sleep(std::time::Duration::from_millis(500));

            // Clean up lock file
            let _ = std::fs::remove_file(lock_path);

            log_indented!("Cross-compositor cleanup completed");
            return Ok(());
        } else {
            log_pipe!();
            log_error!("Failed to terminate existing process");
            anyhow::bail!("Cannot force cleanup - existing process could not be terminated")
        }
    }

    // Same compositor - respect single instance enforcement
    log_pipe!();
    log_error!("sunsetr is already running (PID: {pid})");
    log_pipe!();
    log_decorated!("Did you mean to:");
    log_indented!("• Reload configuration: sunsetr --reload");
    log_indented!("• Test new values: sunsetr --test <temp> <gamma>");
    log_pipe!();
    anyhow::bail!("Cannot start - another sunsetr instance is running")
}
