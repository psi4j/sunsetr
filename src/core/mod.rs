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

mod context;
pub mod period;
pub mod runtime_state;
pub mod smoothing;

use anyhow::Result;
use std::{path::PathBuf, sync::atomic::Ordering, time::Duration};

use crate::{
    backend::ColorTemperatureBackend,
    common::{constants::*, utils},
    config::{self, Config},
    core::{
        context::Context,
        period::{Period, StateChange},
        runtime_state::RuntimeState,
        smoothing::SmoothTransition,
    },
    io::lock::LockFile,
    io::signals::SignalState,
    state::ipc::IpcNotifier,
};

/// Parameters for creating a Core instance.
///
/// This struct bundles all the dependencies needed to create a Core,
/// following the idiomatic Rust pattern to avoid functions with too many parameters.
pub(crate) struct CoreParams {
    pub backend: Box<dyn ColorTemperatureBackend>,
    pub runtime_state: RuntimeState,
    pub signal_state: SignalState,
    pub debug_enabled: bool,
    pub lock_info: Option<(LockFile, PathBuf)>,
    pub initial_previous_runtime_state: Option<RuntimeState>,
    pub bypass_smoothing: bool,
    pub ipc_notifier: Option<IpcNotifier>,
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
    // Application infrastructure (unchanged)
    backend: Box<dyn ColorTemperatureBackend>,
    signal_state: SignalState,
    debug_enabled: bool,
    lock_info: Option<(LockFile, PathBuf)>,
    bypass_smoothing: bool,
    ipc_notifier: Option<IpcNotifier>,

    // SINGLE source of truth with clean state history
    runtime_state: RuntimeState,
    previous_runtime_state: Option<RuntimeState>, // Complete previous state for transitions
}

impl Core {
    /// Create a new Core instance from parameters.
    pub fn new(params: CoreParams) -> Self {
        Self {
            backend: params.backend,
            signal_state: params.signal_state,
            debug_enabled: params.debug_enabled,
            lock_info: params.lock_info,
            bypass_smoothing: params.bypass_smoothing,
            ipc_notifier: params.ipc_notifier,
            runtime_state: params.runtime_state,
            previous_runtime_state: params.initial_previous_runtime_state,
        }
    }

    /// Update runtime state (handles geo_times recalculation automatically)
    pub fn update_runtime_state(&mut self) -> StateChange {
        let (new_runtime_state, change) = self.runtime_state.with_current_period();

        if !matches!(change, StateChange::None) {
            // Clean state transition: current becomes previous, new becomes current
            self.previous_runtime_state = Some(self.runtime_state.clone());
            self.runtime_state = new_runtime_state;
        } else {
            // Even when no state change occurs, we need to update the current_time
            // in RuntimeState so that transitioning periods can calculate updated progress
            self.runtime_state = new_runtime_state;
        }

        change
    }

    /// Handle config reload with clean state transition pattern.
    /// Returns Ok((sent_state_applied, entering_transition)) where:
    /// - sent_state_applied: true if a StateApplied event was sent
    /// - entering_transition: true if we're entering a transition period from stable
    pub fn handle_config_reload(&mut self, new_config: Config) -> Result<(bool, bool)> {
        #[cfg(debug_assertions)]
        eprintln!("DEBUG: Detected needs_reload flag, applying state with startup transition");

        // Clear the signal flag first
        self.signal_state
            .needs_reload
            .store(false, Ordering::SeqCst);

        // Capture current preset from SignalState BEFORE any changes
        // This happens at the same logical point as the debug logging for state changes
        let previous_preset = { self.signal_state.current_preset.lock().unwrap().clone() };

        let target_state = self.runtime_state.with_config(&new_config)?;

        // Debug logging for config reload state change detection
        if self.debug_enabled {
            let current_values = self.runtime_state.values();
            let target_values = target_state.values();
            let new_preset = crate::state::preset::get_active_preset().ok().flatten();

            log_pipe!();
            log_debug!("Reload state change detection:");
            log_indented!(
                "State: {:?} → {:?}",
                self.runtime_state.period(),
                target_state.period()
            );
            log_indented!("Temperature: {}K → {}K", current_values.0, target_values.0);
            log_indented!("Gamma: {}% → {}%", current_values.1, target_values.1);

            // Log preset change if applicable
            if previous_preset != new_preset {
                log_indented!("Preset: {:?} → {:?}", previous_preset, new_preset);
            }

            let smoothing_enabled = target_state.config().smoothing.unwrap_or(DEFAULT_SMOOTHING);
            if smoothing_enabled {
                log_indented!("Smooth transition: enabled");
            } else {
                log_indented!("Smooth transition: disabled");
            }
        }

        if !self.runtime_state.has_same_effective_values(&target_state) {
            let smoothing_enabled = target_state.config().smoothing.unwrap_or(DEFAULT_SMOOTHING);
            let is_wayland_backend = self.backend.backend_name() == "Wayland";

            if smoothing_enabled && is_wayland_backend {
                // Clean state transition: current becomes previous, target becomes current
                self.previous_runtime_state = Some(self.runtime_state.clone());
                self.runtime_state = target_state;

                // Check for preset change and emit event immediately with target values
                if let Some(ref ipc_notifier) = self.ipc_notifier {
                    let current_preset = crate::state::preset::get_active_preset().ok().flatten();
                    if previous_preset != current_preset {
                        let (target_temp, target_gamma) = self.runtime_state.values();
                        let target_period = self.runtime_state.period();
                        ipc_notifier.send_preset_changed(
                            previous_preset.clone(),
                            current_preset.clone(),
                            target_period,
                            target_temp,
                            target_gamma,
                        );
                        // Update SignalState tracking after event emission
                        *self.signal_state.current_preset.lock().unwrap() = current_preset;
                    }
                }

                // Create transition using new RuntimeState-based signature
                let prev_runtime_state = self.previous_runtime_state.as_ref().unwrap();
                let mut transition =
                    SmoothTransition::reload(prev_runtime_state, &self.runtime_state);
                transition = transition.silent();

                match transition.execute(
                    self.backend.as_mut(),
                    &self.runtime_state,
                    &self.signal_state.running,
                ) {
                    Ok(_) => {
                        // Get periods for later use
                        let prev_period = prev_runtime_state.period();
                        let current_period = self.runtime_state.period();

                        // Determine if we should send StateApplied based on the period change
                        let sent_state_applied = if let Some(ref ipc_notifier) = self.ipc_notifier {
                            // Check if period changed
                            if prev_period != current_period {
                                #[cfg(debug_assertions)]
                                eprintln!(
                                    "DEBUG: Sending PeriodChanged event from config reload: {:?} -> {:?}",
                                    prev_period, current_period
                                );
                                ipc_notifier.send_period_changed(prev_period, current_period);
                            }

                            // Send StateApplied if:
                            // 1. We're in a stable period, OR
                            // 2. We're transitioning FROM a stable/static period TO a transition period
                            let should_send = !current_period.is_transitioning()
                                || (!prev_period.is_transitioning()
                                    && current_period.is_transitioning());

                            if should_send {
                                #[cfg(debug_assertions)]
                                eprintln!(
                                    "DEBUG: Sending StateApplied event from config reload ({})",
                                    if !current_period.is_transitioning() {
                                        "stable period"
                                    } else {
                                        "entering transition from stable"
                                    }
                                );
                                ipc_notifier.send_state_applied(&self.runtime_state);
                                true
                            } else {
                                #[cfg(debug_assertions)]
                                eprintln!(
                                    "DEBUG: Skipping StateApplied from config reload (continuing transition - main loop will handle)"
                                );
                                false
                            }
                        } else {
                            false
                        };

                        // Determine if we're entering a transition
                        let entering_transition =
                            !prev_period.is_transitioning() && current_period.is_transitioning();

                        log_pipe!();
                        log_info!("Configuration reloaded and state applied successfully");
                        Ok((sent_state_applied, entering_transition)) // Return both flags
                    }
                    Err(e) => {
                        log_warning!("Failed to apply transition after config reload: {e}");
                        // Reset to previous state on failure
                        if let Some(prev_state) = self.previous_runtime_state.take() {
                            self.runtime_state = prev_state;
                        }
                        Err(e)
                    }
                }
            } else {
                // No smooth transition - direct state update
                self.previous_runtime_state = Some(self.runtime_state.clone());
                self.runtime_state = target_state;

                match self
                    .backend
                    .apply_startup_state(&self.runtime_state, &self.signal_state.running)
                {
                    Ok(_) => {
                        // Get periods for later use
                        let prev_period = self
                            .previous_runtime_state
                            .as_ref()
                            .map(|s| s.period())
                            .unwrap_or(Period::Day); // Default to stable if no previous
                        let current_period = self.runtime_state.period();

                        // Emit events based on period type
                        let sent_state_applied = if let Some(ref ipc_notifier) = self.ipc_notifier {
                            let current_preset =
                                crate::state::preset::get_active_preset().ok().flatten();
                            if previous_preset != current_preset {
                                let (target_temp, target_gamma) = self.runtime_state.values();
                                let target_period = self.runtime_state.period();
                                #[cfg(debug_assertions)]
                                eprintln!(
                                    "DEBUG: Sending PresetChanged event from config reload (non-smooth)"
                                );
                                ipc_notifier.send_preset_changed(
                                    previous_preset.clone(),
                                    current_preset.clone(),
                                    target_period,
                                    target_temp,
                                    target_gamma,
                                );
                                // Update SignalState tracking after event emission
                                *self.signal_state.current_preset.lock().unwrap() = current_preset;
                            }

                            // Check if period changed
                            if prev_period != current_period {
                                #[cfg(debug_assertions)]
                                eprintln!(
                                    "DEBUG: Sending PeriodChanged event from config reload (non-smooth): {:?} -> {:?}",
                                    prev_period, current_period
                                );
                                ipc_notifier.send_period_changed(prev_period, current_period);
                            }

                            // Send StateApplied if:
                            // 1. We're in a stable period, OR
                            // 2. We're transitioning FROM a stable/static period TO a transition period
                            let should_send = !current_period.is_transitioning()
                                || (!prev_period.is_transitioning()
                                    && current_period.is_transitioning());

                            if should_send {
                                #[cfg(debug_assertions)]
                                eprintln!(
                                    "DEBUG: Sending StateApplied event from config reload (non-smooth, {})",
                                    if !current_period.is_transitioning() {
                                        "stable period"
                                    } else {
                                        "entering transition from stable"
                                    }
                                );
                                ipc_notifier.send_state_applied(&self.runtime_state);
                                true
                            } else {
                                #[cfg(debug_assertions)]
                                eprintln!(
                                    "DEBUG: Skipping StateApplied from config reload (continuing transition - main loop will handle)"
                                );
                                false
                            }
                        } else {
                            false
                        };

                        // Determine if we're entering a transition
                        let entering_transition =
                            !prev_period.is_transitioning() && current_period.is_transitioning();

                        log_pipe!();
                        log_info!("Configuration reloaded and state applied successfully");
                        Ok((sent_state_applied, entering_transition)) // Return both flags
                    }
                    Err(e) => {
                        log_warning!("Failed to apply new state after config reload: {e}");
                        // Reset to previous state on failure
                        if let Some(prev_state) = self.previous_runtime_state.take() {
                            self.runtime_state = prev_state;
                        }
                        Err(e)
                    }
                }
            }
        } else {
            // No transition needed - just update to new config version
            // Store previous period before updating state
            let prev_period = self.runtime_state.period();
            self.runtime_state = target_state;
            let current_period = self.runtime_state.period();

            // Log period change announcement if the period switched
            if prev_period != current_period {
                period::log_state_announcement(current_period);
            }

            // Emit events based on whether preset changed
            let sent_state_applied = if let Some(ref ipc_notifier) = self.ipc_notifier {
                let current_preset = crate::state::preset::get_active_preset().ok().flatten();
                if previous_preset != current_preset {
                    let (target_temp, target_gamma) = self.runtime_state.values();
                    let target_period = self.runtime_state.period();
                    ipc_notifier.send_preset_changed(
                        previous_preset.clone(),
                        current_preset.clone(),
                        target_period,
                        target_temp,
                        target_gamma,
                    );
                    // Update SignalState tracking after event emission
                    *self.signal_state.current_preset.lock().unwrap() = current_preset;
                }

                // Check if period changed
                if prev_period != current_period {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "DEBUG: Sending PeriodChanged event from config reload (no value change): {:?} -> {:?}",
                        prev_period, current_period
                    );
                    ipc_notifier.send_period_changed(prev_period, current_period);
                }

                // Send StateApplied if:
                // 1. We're in a stable period, OR
                // 2. We're transitioning FROM a stable/static period TO a transition period
                let should_send = !current_period.is_transitioning()
                    || (!prev_period.is_transitioning() && current_period.is_transitioning());

                if should_send {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "DEBUG: Sending StateApplied event from config reload (no value change, {})",
                        if !current_period.is_transitioning() {
                            "stable period"
                        } else {
                            "entering transition from stable"
                        }
                    );
                    ipc_notifier.send_state_applied(&self.runtime_state);
                    true
                } else {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "DEBUG: Skipping StateApplied from config reload (continuing transition, no value change)"
                    );
                    false
                }
            } else {
                false
            };

            // Determine if we're entering a transition
            let entering_transition =
                !prev_period.is_transitioning() && current_period.is_transitioning();

            log_pipe!();
            log_info!("Configuration reloaded (no state change needed)");
            Ok((sent_state_applied, entering_transition)) // Return both flags
        }
        // Note: No code after this point - all branches above return early
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

        // Apply initial settings
        self.apply_initial_state()?;

        // Log solar debug info on startup for geo mode (after initial state is applied)
        if self.debug_enabled
            && self.runtime_state.is_geo_mode()
            && let (Some(lat), Some(lon)) = (
                self.runtime_state.config().latitude,
                self.runtime_state.config().longitude,
            )
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
        let smooth_shutdown_performed = if self
            .runtime_state
            .config()
            .smoothing
            .unwrap_or(DEFAULT_SMOOTHING)
            && is_wayland_backend
            && !is_instant_shutdown
        {
            if let Some(mut transition) = SmoothTransition::shutdown(&self.runtime_state) {
                // Use silent mode for shutdown to suppress progress bar and logs
                transition = transition.silent();
                transition
                    .execute(
                        &mut *self.backend,
                        &self.runtime_state,
                        &self.signal_state.running,
                    )
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
        let smoothing = self
            .runtime_state
            .config()
            .smoothing
            .unwrap_or(DEFAULT_SMOOTHING);
        let startup_duration = self
            .runtime_state
            .config()
            .startup_duration
            .unwrap_or(DEFAULT_STARTUP_DURATION);

        // Apply smooth transitions based on config (skip if bypass_smoothing is set for --instant flag)
        let should_transition = smoothing && is_wayland_backend && !self.bypass_smoothing;

        // Treat durations < 0.1 as instant (no transition)
        if should_transition && startup_duration >= 0.1 {
            // Create transition based on whether we have a previous runtime state
            let mut transition = if let Some(ref prev_runtime_state) = self.previous_runtime_state {
                // Config reload: transition from previous state values to current state
                SmoothTransition::reload(prev_runtime_state, &self.runtime_state)
            } else {
                // Initial startup: use default transition (from day values)
                SmoothTransition::startup(&self.runtime_state)
            };

            // Disable progress bar and logs in simulation mode (runs silently)
            if crate::time::source::is_simulated() {
                transition = transition.silent();
            }

            match transition.execute(
                self.backend.as_mut(),
                &self.runtime_state,
                &self.signal_state.running,
            ) {
                Ok(_) => {}
                Err(e) => {
                    log_warning!("Failed to apply smooth startup transition: {e}");
                    log_decorated!("Falling back to immediate transition...");

                    // Fallback to immediate application
                    self.apply_immediate_state(self.runtime_state.period())?;
                }
            }
        } else {
            // Use immediate transition to current interpolated values
            self.apply_immediate_state(self.runtime_state.period())?;
        }

        // Broadcast initial state via IPC (after successful state application)
        if let Some(ref ipc_notifier) = self.ipc_notifier {
            ipc_notifier.send_state_applied(&self.runtime_state);
        }

        Ok(())
    }

    /// Apply state immediately without smooth transition.
    ///
    /// This is used as a fallback when smooth transitions are disabled,
    /// not supported by the backend, or when a smooth transition fails.
    fn apply_immediate_state(&mut self, _new_period: Period) -> Result<()> {
        match self
            .backend
            .apply_startup_state(&self.runtime_state, &self.signal_state.running)
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
        // Note: State tracking is now handled by runtime_state, not separate fields
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
        // Centralized context tracking for the main loop
        let mut tracker = Context::new();

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
            let current_state = self.runtime_state.period();
            if tracker.is_first_iteration() && self.process_initial_signals(&current_state)? {
                continue 'main_loop; // Skip to next iteration if system going to sleep
            }

            // CRITICAL: Check if we just slept to a transition boundary
            // This MUST happen before any time-based re-evaluation to prevent race conditions
            if tracker.slept_to_transition_boundary() {
                #[cfg(debug_assertions)]
                eprintln!("DEBUG [main_loop]: Processing forced transition from boundary sleep");

                // Clear the flag immediately
                tracker.set_sleeping_to_boundary(false);

                // Force advance to next period WITHOUT rechecking wall clock time
                let (new_state, change) = self.runtime_state.with_next_period();

                #[cfg(debug_assertions)]
                eprintln!(
                    "DEBUG [main_loop]: Forced transition: {:?} -> {:?}, change: {:?}",
                    self.runtime_state.period(),
                    new_state.period(),
                    change
                );

                // Update runtime state
                self.runtime_state = new_state;

                // Apply the new state to the backend
                // This ensures the transition happens exactly at the boundary
                if change != crate::core::period::StateChange::None {
                    self.backend
                        .apply_transition_state(&self.runtime_state, &self.signal_state.running)?;
                    tracker.record_state_update();

                    // Send IPC events if notifier is available
                    if let Some(ref ipc_notifier) = self.ipc_notifier {
                        let current_period = self.runtime_state.period();

                        // Check for period changes (from forced transition)
                        if tracker.is_period_change(current_period) {
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "DEBUG [forced_transition]: Sending PeriodChanged event: {:?} -> {:?}",
                                tracker.previous_period().unwrap_or(current_period),
                                current_period
                            );
                            ipc_notifier.send_period_changed(
                                tracker.previous_period().unwrap_or(current_period),
                                current_period,
                            );
                        }

                        // Send state applied event
                        #[cfg(debug_assertions)]
                        eprintln!("DEBUG [forced_transition]: Sending StateApplied event");
                        ipc_notifier.send_state_applied(&self.runtime_state);
                    }
                }

                // Update context tracking for next iteration
                tracker.record_current_period(self.runtime_state.period());
                tracker.update_progress(self.runtime_state.progress());

                // Reset transition tracking when entering stable period
                if !self.runtime_state.period().is_transitioning() {
                    tracker.reset_for_stable_period();
                }

                // Continue to next iteration - skip all time-based re-evaluation
                continue 'main_loop;
            }

            // Check if we need to reload state after config change
            if self.signal_state.needs_reload.load(Ordering::SeqCst) {
                // Debounce: ignore reload requests that come too quickly
                const RELOAD_DEBOUNCE_MS: u64 = 100; // Short debounce to catch duplicate signals

                if tracker.should_debounce_reload(RELOAD_DEBOUNCE_MS) {
                    #[cfg(debug_assertions)]
                    eprintln!("DEBUG: Ignoring duplicate config reload (too recent)");

                    // Clear the reload flag and skip this reload
                    self.signal_state
                        .needs_reload
                        .store(false, Ordering::SeqCst);

                    // Also clear any pending config
                    let _ = self.signal_state.pending_config.lock().unwrap().take();
                } else {
                    // Get the pre-validated config from signal handler (if available)
                    let new_config = { self.signal_state.pending_config.lock().unwrap().take() };

                    // If no pending config (e.g., from TimeChange or Sleep signals), load it now
                    let (new_config, is_time_change_reload) = match new_config {
                        Some(config) => (Some(config), false),
                        None => {
                            // No pending config - this happens for TimeChange/Sleep signals
                            // Load the current config to ensure state consistency
                            match crate::config::Config::load() {
                                Ok(config) => {
                                    #[cfg(debug_assertions)]
                                    eprintln!(
                                        "DEBUG: Loaded config for reload (no pending config)"
                                    );
                                    (Some(config), true)
                                }
                                Err(e) => {
                                    log_pipe!();
                                    log_error!("Failed to load config: {}", e);
                                    log_indented!("Continuing with previous configuration");

                                    #[cfg(debug_assertions)]
                                    eprintln!("DEBUG: Config load failed, skipping reload");
                                    (None, false)
                                }
                            }
                        }
                    };

                    if let Some(new_config) = new_config {
                        // For normal config reloads (SIGUSR2), update runtime_state first
                        // to ensure fresh progress calculations in IPC events.
                        // For time change reloads, skip this to allow handle_config_reload
                        // to detect the period change properly.
                        if !is_time_change_reload {
                            let _ = self.update_runtime_state();
                        }

                        // Process the config reload
                        match self.handle_config_reload(new_config) {
                            Ok((sent_state_applied, entering_transition)) => {
                                // Update last reload time for debouncing
                                tracker.record_reload_processed();

                                // Record config reload in tracker
                                if sent_state_applied {
                                    if entering_transition {
                                        // When entering a transition, record that we applied state
                                        // This sets last_update_time without setting config_reload_pending
                                        tracker.record_state_update();

                                        // Log the initial transition progress to CLI
                                        if let Some(progress) = self.runtime_state.progress() {
                                            let percentage_str = utils::format_progress_percentage(
                                                progress,
                                                tracker.previous_progress(),
                                            );
                                            let update_interval = self
                                                .runtime_state
                                                .config()
                                                .update_interval
                                                .unwrap_or(DEFAULT_UPDATE_INTERVAL);
                                            log_block_start!(
                                                "Transition {} complete. Next update in {} seconds",
                                                percentage_str,
                                                update_interval
                                            );
                                            tracker.update_progress(Some(progress));
                                            tracker.set_first_transition_logged(true);
                                        }
                                    } else {
                                        // For stable periods, record config reload to skip next iteration
                                        tracker.record_config_reload();
                                    }
                                }

                                #[cfg(debug_assertions)]
                                eprintln!(
                                    "DEBUG: Config reload complete, sent_state_applied={}, entering_transition={}",
                                    sent_state_applied, entering_transition
                                );
                            }
                            Err(e) => {
                                // This would be a RuntimeState transition error, not config parsing
                                log_pipe!();
                                log_error!("Failed to apply config changes: {e}");
                                log_indented!("Continuing with previous configuration");
                            }
                        }
                    }
                    // Always clear the reload flag
                    self.signal_state
                        .needs_reload
                        .store(false, Ordering::SeqCst);
                }
            }

            // Note: geo_times recalculation is now handled automatically in update_runtime_state()

            // Determine if we should update state this iteration
            let should_update = if tracker.handle_first_iteration() {
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: First iteration, skipping state update check");
                false
            } else if tracker.handle_config_reload_skip() {
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: Config reload handled, skipping redundant state update");
                false
            } else {
                // Check if it's time for an update during transitions
                if self.runtime_state.period().is_transitioning() {
                    let update_interval = self
                        .runtime_state
                        .config()
                        .update_interval
                        .unwrap_or(DEFAULT_UPDATE_INTERVAL);

                    if !tracker.should_update_during_transition(update_interval) {
                        #[cfg(debug_assertions)]
                        eprintln!("DEBUG: Skipping update - not time yet");
                        false
                    } else {
                        let state_change = self.update_runtime_state();
                        let update_needed = !matches!(state_change, StateChange::None);

                        #[cfg(debug_assertions)]
                        eprintln!(
                            "DEBUG: update_runtime_state result: {state_change:?} (update_needed: {update_needed})"
                        );

                        update_needed
                    }
                } else {
                    // For stable periods, always check for updates
                    let state_change = self.update_runtime_state();
                    let update_needed = !matches!(state_change, StateChange::None);

                    #[cfg(debug_assertions)]
                    eprintln!(
                        "DEBUG: update_runtime_state result: {state_change:?} (update_needed: {update_needed})"
                    );

                    update_needed
                }
            };

            if should_update && self.signal_state.running.load(Ordering::SeqCst) {
                #[cfg(debug_assertions)]
                eprintln!(
                    "DEBUG: Applying state update - state: {:?}",
                    self.runtime_state.period()
                );

                match self
                    .backend
                    .apply_transition_state(&self.runtime_state, &self.signal_state.running)
                {
                    Ok(_) => {
                        #[cfg(debug_assertions)]
                        eprintln!("DEBUG: State application successful");

                        // Record that we applied state
                        tracker.record_state_update();

                        // Emit time-based events from main loop
                        if let Some(ref ipc_notifier) = self.ipc_notifier {
                            let current_period = self.runtime_state.period();

                            // Check for period changes
                            if tracker.is_period_change(current_period) {
                                #[cfg(debug_assertions)]
                                eprintln!(
                                    "DEBUG: Sending PeriodChanged event: {:?} -> {:?}",
                                    tracker.previous_period().unwrap_or(current_period),
                                    current_period
                                );
                                ipc_notifier.send_period_changed(
                                    tracker.previous_period().unwrap_or(current_period),
                                    current_period,
                                );
                            }

                            // Always send StateApplied when we actually applied state
                            // (timing check already happened to decide whether to apply)
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "DEBUG: Sending StateApplied event from main loop (state was applied)"
                            );
                            ipc_notifier.send_state_applied(&self.runtime_state);
                        }
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
                        // Don't update current_period - try again next cycle
                    }
                }
            }

            // Determine if we should log progress (only when state was actually applied)
            let should_log_progress =
                tracker.should_log_progress(self.runtime_state.period(), should_update);

            // If this is the first time through and we haven't recorded initial state yet
            if !tracker.has_recorded_updates() && self.runtime_state.period().is_transitioning() {
                tracker.record_state_update();
            }

            let calculated_sleep_duration = Self::determine_sleep_duration(
                &self.runtime_state,
                &mut tracker,
                self.debug_enabled,
                should_log_progress,
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

                    // Signal received - process it through the signals module
                    crate::io::signals::handle_signal_message(
                        signal_msg,
                        &mut self.backend,
                        &self.signal_state,
                        &self.runtime_state,
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

    /// Process any pending signals on the first iteration.
    ///
    /// This ensures signals sent before the main loop starts are handled properly.
    /// Returns true if we should skip to the next iteration (e.g., system going to sleep).
    fn process_initial_signals(&mut self, _current_state: &Period) -> Result<bool> {
        while let Ok(signal_msg) = self.signal_state.signal_receiver.try_recv() {
            // Check if this is a system sleep signal (not resume)
            let going_to_sleep = matches!(
                signal_msg,
                crate::io::signals::SignalMessage::Sleep { resuming: false }
            );

            // Critical signals are handled via atomic flags (signal-safe pattern)
            // Config reload: handled via needs_reload flag
            // Shutdown: handled via running/instant_shutdown flags
            // Sleep: detected here, skips main loop iteration as intended
            // TestMode: currently not implemented (would require config mutation)

            // If system is going to sleep, signal to skip the rest of the loop
            if going_to_sleep {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Determine sleep duration for the main loop.
    ///
    /// This function determines how long to sleep based on the current state
    /// (transitioning vs stable) and logs appropriate progress information.
    /// During transitions, it shows percentage complete with adaptive precision.
    /// During stable states, it shows time until next transition.
    ///
    /// # Arguments
    /// * `tracker` - The centralized context tracker
    /// * `should_log` - Whether to actually log progress (only when state was applied)
    ///
    /// Returns the duration to sleep before the next check.
    fn determine_sleep_duration(
        runtime_state: &RuntimeState,
        tracker: &mut Context,
        debug_enabled: bool,
        should_log: bool,
    ) -> Result<Duration> {
        // Determine sleep duration based on state
        let sleep_duration = if runtime_state.period().is_transitioning() {
            let update_interval = Duration::from_secs(
                runtime_state
                    .config()
                    .update_interval
                    .unwrap_or(DEFAULT_UPDATE_INTERVAL),
            );

            // Check if we're near the end of the transition
            if let Some(time_remaining) = runtime_state.time_until_transition_end() {
                if time_remaining < update_interval {
                    // We're sleeping exactly to the transition boundary
                    // Set flag to force transition on next iteration without time rechecking
                    tracker.set_sleeping_to_boundary(true);

                    #[cfg(debug_assertions)]
                    eprintln!(
                        "DEBUG [determine_sleep_duration]: Sleeping to boundary, time_remaining={:.3}s",
                        time_remaining.as_secs_f64()
                    );

                    // Sleep only until the transition ends
                    time_remaining
                } else {
                    // Normal update interval - not at boundary yet
                    update_interval
                }
            } else {
                // Fallback to normal interval (shouldn't happen)
                update_interval
            }
        } else {
            runtime_state.time_until_next_event()
        };

        // Handle transition progress
        if let Some(progress) = runtime_state.progress() {
            #[cfg(debug_assertions)]
            {
                let current_percentage = progress * 100.0;
                let percentage_change = if let Some(prev) = tracker.previous_progress() {
                    (current_percentage - prev * 100.0).abs()
                } else {
                    0.0
                };
                eprintln!(
                    "DEBUG: progress={progress:.6}, \
                         current_percentage={current_percentage:.4}, \
                         percentage_change={percentage_change:.4}, \
                         should_log={should_log}"
                );
            }

            // Only log if we actually applied state (prevents duplicate logs after config reload)
            if should_log {
                // Use the common formatting function for consistent display
                let percentage_str =
                    utils::format_progress_percentage(progress, tracker.previous_progress());

                // Use centralized duration formatting with ceiling rounding
                let display_secs = utils::format_duration_seconds_ceil(sleep_duration);

                let log_message = format!(
                    "Transition {} complete. Next update in {} seconds",
                    percentage_str, display_secs
                );

                if debug_enabled {
                    // In debug mode, always use log_block_start for better visibility
                    log_block_start!("{}", log_message);
                } else if !tracker.first_transition_logged() {
                    // space out first log
                    log_block_start!("{}", log_message);
                    tracker.set_first_transition_logged(true);
                } else {
                    // group the rest of the logs together
                    log_decorated!("{}", log_message);
                }
            }

            // Update progress tracking through the centralized tracker
            tracker.update_progress(Some(progress));
        } else {
            // Stable state - reset tracking through the centralized tracker
            tracker.reset_for_stable_period();

            // Debug logging to show exact transition time (skip for static mode)
            if debug_enabled && runtime_state.period() != Period::Static {
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
                let next = runtime_state.period().next_period();
                let transition_info = format!(
                    "{} {} → {} {}",
                    runtime_state.period().display_name(),
                    runtime_state.period().symbol(),
                    next.display_name(),
                    next.symbol()
                );

                // For geo mode, show time in both city timezone and local timezone
                if runtime_state.is_geo_mode()
                    && let (Some(lat), Some(lon)) = (
                        runtime_state.config().latitude,
                        runtime_state.config().longitude,
                    )
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

            // Detect if we just entered a stable state using the tracker
            let just_entered_stable = tracker
                .previous_period()
                .map(|prev| prev.is_transitioning())
                .unwrap_or(true); // First iteration counts as entering stable

            // Only log the countdown when entering stable state and there's meaningful time remaining
            // Skip this for static mode since it never transitions
            if just_entered_stable
                && sleep_duration >= Duration::from_secs(1)
                && runtime_state.period() != Period::Static
            {
                // Use centralized duration formatting with ceiling rounding
                let total_seconds = utils::format_duration_seconds_ceil(sleep_duration);
                let hours = total_seconds / 3600;
                let minutes = (total_seconds % 3600) / 60;
                let seconds = total_seconds % 60;

                // Format based on duration length for better readability
                if hours > 0 {
                    // Show hours and minutes for durations over 1 hour
                    if minutes > 0 {
                        log_block_start!("Next transition in {} hours {} minutes", hours, minutes);
                    } else {
                        log_block_start!("Next transition in {} hours", hours);
                    }
                } else {
                    // Show minutes and seconds for durations under 1 hour
                    if minutes > 0 {
                        log_block_start!(
                            "Next transition in {} minutes {} seconds",
                            minutes,
                            seconds
                        );
                    } else {
                        log_block_start!("Next transition in {} seconds", seconds);
                    }
                }
            }
        }

        // Update previous period in the tracker for next iteration
        tracker.record_current_period(runtime_state.period());

        Ok(sleep_duration)
    }
}
