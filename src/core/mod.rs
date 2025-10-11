//! Core application logic and state management.
//!
//! This module encapsulates the main logic of sunsetr, managing the
//! continuous color temperature adjustment loop. It handles:
//!
//! - Time-based state transitions (day/night/transitioning)
//! - Signal processing (SIGUSR1 for test mode, SIGUSR2 for reload)
//! - Configuration hot-reloading
//! - Geo mode daily recalculation
//! - Smooth transitions between states
//! - Backend communication for applying color changes
//!
//! The `Core` struct maintains all runtime state, providing encapsulation
//! and making the code easier to test and reason about.

pub mod period;
pub mod smoothing;

use anyhow::{Context, Result};
use std::{path::PathBuf, sync::atomic::Ordering, time::Duration};

use crate::{
    backend::ColorTemperatureBackend,
    common::{constants::*, utils},
    config::{self, Config},
    core::{
        period::{
            Period, get_current_period, should_update_state, time_until_next_event,
            time_until_transition_end,
        },
        smoothing::SmoothTransition,
    },
    geo::times::GeoTimes,
    io::lock::LockFile,
    io::signals::SignalState,
};

/// Parameters for creating a Core instance.
///
/// This struct bundles all the dependencies needed to create a Core,
/// following the idiomatic Rust pattern to avoid functions with too many parameters.
pub(crate) struct CoreParams {
    pub backend: Box<dyn ColorTemperatureBackend>,
    pub config: Config,
    pub signal_state: SignalState,
    pub debug_enabled: bool,
    pub geo_times: Option<GeoTimes>,
    pub lock_info: Option<(LockFile, PathBuf)>,
    pub initial_previous_state: Option<Period>,
    pub from_reload: bool,
    pub bypass_smoothing: bool,
}

/// Core state machine managing the main application loop.
///
/// This struct encapsulates all the runtime state
/// It provides methods for:
/// - Executing the main application flow
/// - Applying initial and immediate states
/// - Running the continuous update loop
/// - Handling configuration reloads and signal processing
pub(crate) struct Core {
    backend: Box<dyn ColorTemperatureBackend>,
    config: Config,
    signal_state: SignalState,
    debug_enabled: bool,
    geo_times: Option<GeoTimes>,
    lock_info: Option<(LockFile, PathBuf)>,
    from_reload: bool,
    bypass_smoothing: bool,
    // Main loop persistent state
    current_transition_state: Period,
    previous_state: Option<Period>,
}

impl Core {
    /// Create a new Core instance from parameters.
    pub fn new(params: CoreParams) -> Self {
        // Calculate initial state
        let current_transition_state =
            get_current_period(&params.config, params.geo_times.as_ref());

        Self {
            backend: params.backend,
            config: params.config,
            signal_state: params.signal_state,
            debug_enabled: params.debug_enabled,
            geo_times: params.geo_times,
            lock_info: params.lock_info,
            from_reload: params.from_reload,
            bypass_smoothing: params.bypass_smoothing,
            current_transition_state,
            previous_state: params.initial_previous_state,
        }
    }

    /// Execute the core application logic.
    ///
    /// This method orchestrates the main sunsetr loop using the resources
    /// and configuration provided during construction.
    ///
    /// # Returns
    /// Result indicating success or failure of the application run
    pub fn execute(mut self) -> Result<()> {
        // Log base directory if using custom config path
        if let Some(custom_dir) = config::get_custom_config_dir() {
            // Use privacy function for path display
            let display_path = utils::private_path(&custom_dir);
            log_block_start!("Base directory: {}", display_path);
        }

        // The backend is already created and passed to Core, no need to create it again
        // Just log that we're using it
        log_block_start!(
            "Successfully connected to {} backend",
            self.backend.backend_name()
        );

        // If we're using Hyprland native CTM or Hyprsunset backend under Hyprland compositor,
        // reset Wayland gamma to clean up any leftover gamma from previous Wayland backend sessions.
        // This ensures a clean slate when switching from another compositor
        if (self.backend.backend_name() == "Hyprland"
            || self.backend.backend_name() == "Hyprsunset")
            && std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()
        {
            // Create a temporary Wayland backend to reset Wayland gamma
            match crate::backend::wayland::WaylandBackend::new(&self.config, self.debug_enabled) {
                Ok(mut wayland_backend) => {
                    use crate::backend::ColorTemperatureBackend;
                    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
                    if let Err(e) = wayland_backend.apply_temperature_gamma(6500, 100.0, &running) {
                        if self.debug_enabled {
                            log_warning!("Failed to reset Wayland gamma: {e}");
                            log_indented!(
                                "This is normal if no Wayland gamma control is available"
                            );
                        }
                    } else if self.debug_enabled {
                        log_decorated!("Successfully reset Wayland gamma");
                    }
                }
                Err(e) => {
                    if self.debug_enabled {
                        log_error!("Could not create Wayland backend for reset: {e}");
                        log_indented!("This is normal if Wayland gamma control is not available");
                    }
                }
            }
        }

        // Apply initial settings
        self.apply_initial_state()?;

        // Log solar debug info on startup for geo mode (after initial state is applied)
        if self.debug_enabled
            && self.config.transition_mode.as_deref() == Some("geo")
            && let (Some(lat), Some(lon)) = (self.config.latitude, self.config.longitude)
        {
            let _ = crate::geo::log_solar_debug_info(lat, lon);
        }

        // Main application loop
        self.main_loop()?;

        // Ensure proper cleanup on shutdown
        log_block_start!("Shutting down sunsetr...");

        // Smooth shutdown transition (Wayland backend only)
        let is_wayland_backend = self.backend.backend_name() == "Wayland";
        let is_instant_shutdown = self.signal_state.instant_shutdown.load(Ordering::SeqCst);
        let smooth_shutdown_performed = if self.config.smoothing.unwrap_or(DEFAULT_SMOOTHING)
            && is_wayland_backend
            && !is_instant_shutdown
        {
            // Create fresh geo_times from current config if in geo mode
            // This ensures we use the correct location after any config reloads
            let fresh_geo_times = if self.config.transition_mode.as_deref() == Some("geo") {
                crate::geo::times::GeoTimes::from_config(&self.config)
                    .ok()
                    .flatten()
            } else {
                None
            };

            if let Some(mut transition) = SmoothTransition::shutdown(&self.config, fresh_geo_times)
            {
                // Use silent mode for shutdown to suppress progress bar and logs
                transition = transition.silent();
                transition
                    .execute(&mut *self.backend, &self.config, &self.signal_state.running)
                    .is_ok()
            } else {
                false
            }
        } else {
            false
        };

        // Manual gamma reset for Wayland backend only
        if !smooth_shutdown_performed && self.backend.backend_name() == "Wayland" {
            if self.debug_enabled {
                log_decorated!("Resetting color temperature and gamma...");
            }
            let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
            if let Err(e) = self.backend.apply_temperature_gamma(6500, 100.0, &running) {
                log_warning!("Failed to reset color temperature: {e}");
            } else if self.debug_enabled {
                log_decorated!("Gamma reset completed successfully");
            }
        }

        // Clean up resources (backend, lock file)
        if let Some((lock_file, lock_path)) = self.lock_info {
            utils::cleanup_application(self.backend, lock_file, &lock_path, self.debug_enabled);
        } else {
            // No lock file to clean (geo restart or simulation mode)
            self.backend.cleanup(self.debug_enabled);
        }
        log_end!();

        Ok(())
    }

    /// Apply the initial state when starting the application.
    ///
    /// Handles both smooth startup transitions and immediate state application
    /// based on configuration settings and backend capabilities. Smooth transitions
    /// are only supported on the Wayland backend, while Hyprland-based backends use
    /// CTM animation for their own smooth effects.
    fn apply_initial_state(&mut self) -> Result<()> {
        if !self.signal_state.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        // Note: No reset needed here - backends should start with correct interpolated values
        // Cross-backend reset (if needed) is handled separately before this function

        // Smooth transitions only work with Wayland backend
        let is_wayland_backend = self.backend.backend_name() == "Wayland";
        let smoothing = self.config.smoothing.unwrap_or(DEFAULT_SMOOTHING);
        let startup_duration = self
            .config
            .startup_duration
            .unwrap_or(DEFAULT_STARTUP_DURATION);

        // Force smooth transition after reload command (gamma was reset to neutral)
        // Skip transitions entirely if bypass_smoothing is set (for --instant flag)
        let should_transition =
            (self.from_reload || smoothing) && is_wayland_backend && !self.bypass_smoothing;

        // Treat durations < 0.1 as instant (no transition)
        if should_transition && startup_duration >= 0.1 {
            // Create transition based on whether we have a previous state
            let mut transition = if let Some(prev_state) = self.previous_state {
                // Config reload: transition from previous state values to new state
                let (start_temp, start_gamma) = prev_state.values(&self.config);
                // Clone geo_times to pass to the transition
                let geo_times_clone = self.geo_times.clone();
                SmoothTransition::reload(
                    start_temp,
                    start_gamma,
                    self.current_transition_state,
                    &self.config,
                    geo_times_clone,
                )
            } else {
                // Initial startup: use default transition (from day values)
                // Clone geo_times to pass to the transition
                let geo_times_clone = self.geo_times.clone();
                SmoothTransition::startup(
                    self.current_transition_state,
                    &self.config,
                    geo_times_clone,
                )
            };

            // Disable progress bar and logs in simulation mode (runs silently)
            if crate::time::source::is_simulated() {
                transition = transition.silent();
            }

            match transition.execute(
                self.backend.as_mut(),
                &self.config,
                &self.signal_state.running,
            ) {
                Ok(_) => {}
                Err(e) => {
                    log_warning!("Failed to apply smooth startup transition: {e}");
                    log_decorated!("Falling back to immediate transition...");

                    // Fallback to immediate application
                    self.apply_immediate_state(self.current_transition_state)?;
                }
            }
        } else {
            // Use immediate transition to current interpolated values
            self.apply_immediate_state(self.current_transition_state)?;
        }

        Ok(())
    }

    /// Apply state immediately without smooth transition.
    ///
    /// This is used as a fallback when smooth transitions are disabled,
    /// not supported by the backend, or when a smooth transition fails.
    fn apply_immediate_state(&mut self, new_period: Period) -> Result<()> {
        match self
            .backend
            .apply_startup_state(new_period, &self.config, &self.signal_state.running)
        {
            Ok(_) => {
                if self.debug_enabled {
                    log_pipe!();
                    log_debug!("Initial state applied successfully");
                }
            }
            Err(e) => {
                log_warning!("Failed to apply initial state: {e}");
                log_decorated!("Continuing anyway - will retry during operation...");
            }
        }
        // Update our tracked state
        self.current_transition_state = new_period;
        Ok(())
    }

    /// Run the main application loop that monitors and applies state changes.
    ///
    /// This loop continuously monitors the time-based state and applies changes
    /// to the backend when necessary. It features:
    /// - Signal-driven operation with recv_timeout for efficiency
    /// - Automatic geo times recalculation after midnight
    /// - Smart sleep duration calculation based on transition progress
    /// - Progressive percentage display during transitions
    /// - Hotplug polling for display changes
    /// - Config reload support with smooth transitions
    fn main_loop(&mut self) -> Result<()> {
        // Skip first iteration to prevent false state change detection due to startup timing
        let mut first_iteration = true;
        // Tracks if the initial transition progress log has been made using `log_block_start`.
        // Subsequent transition progress logs will use `log_decorated` when debug is disabled.
        let mut first_transition_log_done = false;
        // Track previous progress for decimal display logic
        let mut previous_progress: Option<f32> = None;
        // Track the previous state to detect transitions
        let mut previous_state: Option<Period> = None;

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
        let mut current_state = get_current_period(&self.config, self.geo_times.as_ref());

        // Track the last applied temperature and gamma values
        // Initialize with the values for the current state
        let (initial_temp, initial_gamma) = current_state.values(&self.config);
        let mut last_applied_temp = initial_temp;
        let mut last_applied_gamma = initial_gamma;

        'main_loop: while self.signal_state.running.load(Ordering::SeqCst)
            && !crate::time::source::simulation_ended()
        {
            #[cfg(debug_assertions)]
            {
                debug_loop_count += 1;
                eprintln!("DEBUG: Main loop iteration {debug_loop_count} starting");
            }

            // Process any pending signals immediately (non-blocking check)
            // This ensures signals sent before the loop starts are handled
            if first_iteration && self.process_initial_signals(&mut current_state)? {
                continue 'main_loop; // Skip to next iteration if system going to sleep
            }

            // Check if we need to reload state after config change
            if self.signal_state.needs_reload.load(Ordering::SeqCst) {
                self.handle_config_reload(
                    &mut current_state,
                    &mut last_applied_temp,
                    &mut last_applied_gamma,
                )?;
            }

            // Check if geo_times needs recalculation (e.g., after midnight)
            self.check_geo_times_update()?;

            let new_period = get_current_period(&self.config, self.geo_times.as_ref());

            // Skip first iteration to prevent false state change detection caused by
            // timing differences between startup state application and main loop start
            let should_update = if first_iteration {
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: First iteration, skipping state update check");

                first_iteration = false;
                false
            } else {
                let update_needed = should_update_state(&current_state, &new_period);

                #[cfg(debug_assertions)]
                eprintln!(
                    "DEBUG: should_update_state result: {update_needed}, current_state: {current_state:?}, new_period: {new_period:?}"
                );

                update_needed
            };

            if should_update && self.signal_state.running.load(Ordering::SeqCst) {
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: Applying state update - state: {new_period:?}");

                match self.backend.apply_transition_state(
                    new_period,
                    &self.config,
                    &self.signal_state.running,
                ) {
                    Ok(_) => {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "DEBUG: State application successful, updating current_transition_state"
                        );

                        // Success - update our state
                        self.current_transition_state = new_period;
                        current_state = new_period;

                        // Update last applied values
                        let (new_temp, new_gamma) = new_period.values(&self.config);
                        last_applied_temp = new_temp;
                        last_applied_gamma = new_gamma;
                    }
                    Err(e) => {
                        #[cfg(debug_assertions)]
                        eprintln!("DEBUG: State application failed: {e}");

                        // Failure - check if it's a connection issue that couldn't be resolved
                        if e.to_string().contains("reconnection attempt") {
                            log_pipe!();
                            log_error!(
                                "Cannot communicate with {}: {}",
                                self.backend.backend_name(),
                                e
                            );
                            log_decorated!(
                                "{} appears to be permanently unavailable. Exiting...",
                                self.backend.backend_name()
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
            let calculated_sleep_duration = Self::determine_sleep_duration(
                current_state,
                &self.config,
                self.geo_times.as_ref(),
                &mut first_transition_log_done,
                self.debug_enabled,
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

            // In simulation mode, crate::time::source::sleep already handles the time scaling
            // We can't use recv_timeout with the full duration as it would sleep too long
            // So we need to handle simulation differently
            let recv_result = if crate::time::source::is_simulated() {
                // Sleep in a separate thread so we can still receive signals
                let sleep_handle = std::thread::spawn({
                    let duration = calculated_sleep_duration;
                    move || {
                        crate::time::source::sleep(duration);
                    }
                });

                // Poll for signals while the sleep thread runs
                loop {
                    // Periodically poll backend for hotplug events
                    let _ = self.backend.poll_hotplug();

                    match self
                        .signal_state
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
                    match self.signal_state.signal_receiver.recv_timeout(chunk) {
                        Ok(msg) => break Ok(msg),
                        Err(RecvTimeoutError::Timeout) => {
                            // Poll backend for hotplug and continue if time remains
                            let _ = self.backend.poll_hotplug();
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
                    // Check if this is a system sleep signal (not resume)
                    let going_to_sleep = matches!(
                        signal_msg,
                        crate::io::signals::SignalMessage::Sleep { resuming: false }
                    );

                    // Signal received - handle it immediately
                    crate::io::signals::handle_signal_message(
                        signal_msg,
                        &mut self.backend,
                        &mut self.config,
                        &self.signal_state,
                        &mut current_state,
                        self.debug_enabled,
                    )?;

                    // If system is going to sleep, skip the rest of the loop
                    // (no need to calculate sunsetr's sleep duration)
                    if going_to_sleep {
                        continue;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    // Normal timeout - continue to next iteration
                    #[cfg(debug_assertions)]
                    eprintln!("DEBUG: Sleep duration elapsed naturally");
                }
                Err(RecvTimeoutError::Disconnected) => {
                    // Channel disconnected - check if it's expected shutdown
                    if !self.signal_state.running.load(Ordering::SeqCst) {
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

    /// Handle configuration reload when signaled.
    ///
    /// This method handles the complete configuration reload process including:
    /// - Updating or clearing geo times based on the new mode
    /// - Applying the new state with optional smooth transitions
    /// - Updating tracking variables on success
    fn handle_config_reload(
        &mut self,
        current_state: &mut Period,
        last_applied_temp: &mut u32,
        last_applied_gamma: &mut f32,
    ) -> Result<()> {
        #[cfg(debug_assertions)]
        eprintln!("DEBUG: Detected needs_reload flag, applying state with startup transition");

        // Clear the flag first
        self.signal_state
            .needs_reload
            .store(false, Ordering::SeqCst);

        // Handle geo times based on current mode
        if self.config.transition_mode.as_deref() == Some("geo") {
            // In geo mode - create or update geo times
            if let (Some(lat), Some(lon)) = (self.config.latitude, self.config.longitude) {
                // Check if we already have geo_times and just need to update for location change
                if let Some(ref mut times) = self.geo_times {
                    // Use handle_location_change for existing geo_times
                    if let Err(e) = times.handle_location_change(lat, lon) {
                        log_pipe!();
                        log_critical!("Failed to update geo times after config reload: {e}");
                        // Fall back to creating new times if update failed
                        self.geo_times = crate::geo::times::GeoTimes::from_config(&self.config)
                            .context(
                                "Solar calculations failed after config reload - this is a bug",
                            )?;
                    }
                } else {
                    // Create new geo_times if none exists
                    self.geo_times = crate::geo::times::GeoTimes::from_config(&self.config)
                        .context("Solar calculations failed after config reload - this is a bug")?;
                }
            }
        } else {
            // Not in geo mode - clear geo_times to ensure we use manual times
            if self.geo_times.is_some() {
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: Clearing geo_times after switching away from geo mode");
                self.geo_times = None;
            }
        }

        // Get the new state and apply it with startup transition support
        let reload_state = get_current_period(&self.config, self.geo_times.as_ref());

        // Check if smooth transitions are enabled
        let is_wayland_backend = self.backend.backend_name() == "Wayland";
        let smoothing_enabled = self.config.smoothing.unwrap_or(DEFAULT_SMOOTHING);

        // Debug logging for config reload state change detection
        if self.debug_enabled {
            let (target_temp, target_gamma) = reload_state.values(&self.config);
            log_pipe!();
            log_debug!("Reload state change detection:");
            log_indented!("State: {:?} → {:?}", current_state, reload_state);
            log_indented!("Temperature: {}K → {}K", last_applied_temp, target_temp);
            log_indented!("Gamma: {}% → {}%", last_applied_gamma, target_gamma);
            if smoothing_enabled {
                log_indented!("Smooth transition: enabled");
            } else {
                log_indented!("Smooth transition: disabled");
            }
        }

        // Apply smooth transition from current to new values
        if smoothing_enabled && is_wayland_backend {
            // Create a custom transition from actual current values to new state
            let mut transition = SmoothTransition::reload(
                *last_applied_temp,
                *last_applied_gamma,
                reload_state,
                &self.config,
                self.geo_times.clone(),
            );

            // Configure for silent reload operation
            transition = transition.silent();

            // Execute the transition
            match transition.execute(
                self.backend.as_mut(),
                &self.config,
                &self.signal_state.running,
            ) {
                Ok(_) => {
                    // Update our tracking variables
                    self.current_transition_state = reload_state;
                    *current_state = reload_state;

                    // Update last applied values
                    let (new_temp, new_gamma) = reload_state.values(&self.config);
                    *last_applied_temp = new_temp;
                    *last_applied_gamma = new_gamma;

                    log_pipe!();
                    log_info!("Configuration reloaded and state applied successfully");
                }
                Err(e) => {
                    log_warning!("Failed to apply transition after config reload: {e}");
                    // Don't update tracking variables if application failed
                }
            }
        } else {
            // Non-Wayland backend or transitions disabled
            // This ensures apply_initial_state uses the correct target state
            self.previous_state = Some(*current_state);
            self.current_transition_state = reload_state;

            // Apply the initial state with the new configuration
            match self.apply_initial_state() {
                Ok(_) => {
                    // Update remaining tracking variables
                    *current_state = reload_state;

                    // Update last applied values
                    let (new_temp, new_gamma) = reload_state.values(&self.config);
                    *last_applied_temp = new_temp;
                    *last_applied_gamma = new_gamma;

                    log_pipe!();
                    log_info!("Configuration reloaded and state applied successfully");
                }
                Err(e) => {
                    log_warning!("Failed to apply new state after config reload: {e}");
                    // Reset current_transition_state since application failed
                    self.current_transition_state = *current_state;
                    // Don't update other tracking variables if application failed
                }
            }
        }

        Ok(())
    }

    /// Process any pending signals on the first iteration.
    ///
    /// This ensures signals sent before the main loop starts are handled properly.
    /// Returns true if we should skip to the next iteration (e.g., system going to sleep).
    fn process_initial_signals(&mut self, current_state: &mut Period) -> Result<bool> {
        while let Ok(signal_msg) = self.signal_state.signal_receiver.try_recv() {
            // Check if this is a system sleep signal (not resume)
            let going_to_sleep = matches!(
                signal_msg,
                crate::io::signals::SignalMessage::Sleep { resuming: false }
            );

            crate::io::signals::handle_signal_message(
                signal_msg,
                &mut self.backend,
                &mut self.config,
                &self.signal_state,
                current_state,
                self.debug_enabled,
            )?;

            // If system is going to sleep, signal to skip the rest of the loop
            if going_to_sleep {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Check if geo times need daily recalculation and update if necessary.
    ///
    /// This handles the automatic recalculation of sunrise/sunset times
    /// after midnight for geographic mode.
    fn check_geo_times_update(&mut self) -> Result<()> {
        if let Some(ref mut times) = self.geo_times
            && times.needs_recalculation(crate::time::source::now())
            && let (Some(lat), Some(lon)) = (self.config.latitude, self.config.longitude)
            && let Err(e) = times.recalculate_for_next_period(lat, lon)
        {
            log_warning!("Failed to recalculate geo times: {e}");
        }
        Ok(())
    }

    /// Determine sleep duration for the main loop.
    ///
    /// This function determines how long to sleep based on the current state
    /// (transitioning vs stable) and logs appropriate progress information.
    /// During transitions, it shows percentage complete with adaptive precision.
    /// During stable states, it shows time until next transition.
    ///
    /// Returns the duration to sleep before the next check.
    pub fn determine_sleep_duration(
        new_period: Period,
        config: &Config,
        geo_times: Option<&crate::geo::times::GeoTimes>,
        first_transition_log_done: &mut bool,
        debug_enabled: bool,
        previous_progress: &mut Option<f32>,
        previous_state: &mut Option<Period>,
    ) -> Result<Duration> {
        // Determine sleep duration based on state
        let sleep_duration = if new_period.is_transitioning() {
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
        if let Some(progress) = new_period.progress() {
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
            if debug_enabled && new_period != Period::Static {
                let now = crate::time::source::now();
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
                let next = new_period.next_period();
                let transition_info = format!(
                    "{} {} → {} {}",
                    new_period.display_name(),
                    new_period.symbol(),
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
                && new_period != Period::Static
            {
                log_block_start!(
                    "Next transition in {} minutes {} seconds",
                    sleep_duration.as_secs() / 60,
                    sleep_duration.as_secs() % 60
                );
            }
        }

        // Update previous state for next iteration
        *previous_state = Some(new_period);

        Ok(sleep_duration)
    }
}
