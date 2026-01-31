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
    backend: Box<dyn ColorTemperatureBackend>,
    signal_state: SignalState,
    debug_enabled: bool,
    lock_info: Option<(LockFile, PathBuf)>,
    bypass_smoothing: bool,
    ipc_notifier: Option<IpcNotifier>,
    runtime_state: RuntimeState,
    previous_runtime_state: Option<RuntimeState>,
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
            self.previous_runtime_state = Some(self.runtime_state.clone());
            self.runtime_state = new_runtime_state;
        } else {
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

        self.signal_state
            .needs_reload
            .store(false, Ordering::SeqCst);

        let previous_preset = { self.signal_state.current_preset.lock().unwrap().clone() };
        let target_state = self.runtime_state.with_config(&new_config)?;
        let values_changed = !self.runtime_state.has_same_effective_values(&target_state);
        let period_changed = self.runtime_state.period() != target_state.period();
        let current_preset = crate::state::preset::get_active_preset().ok().flatten();
        let preset_changed = previous_preset != current_preset;

        if !values_changed && !period_changed && !preset_changed {
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Config reload skipped - no changes detected");
            return Ok((false, false));
        }

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
                self.previous_runtime_state = Some(self.runtime_state.clone());
                self.runtime_state = target_state;

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
                        *self.signal_state.current_preset.lock().unwrap() = current_preset;
                    } else if values_changed {
                        let (target_temp, target_gamma) = self.runtime_state.values();
                        let target_period = self.runtime_state.period();
                        ipc_notifier.send_config_changed(target_period, target_temp, target_gamma);
                    }
                }

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
                        let prev_period = prev_runtime_state.period();
                        let current_period = self.runtime_state.period();

                        let sent_state_applied = if let Some(ref ipc_notifier) = self.ipc_notifier {
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

                        let entering_transition =
                            !prev_period.is_transitioning() && current_period.is_transitioning();

                        log_pipe!();
                        log_info!("Configuration reloaded and state applied successfully");
                        Ok((sent_state_applied, entering_transition))
                    }
                    Err(e) => {
                        log_warning!("Failed to apply transition after config reload: {e}");
                        if let Some(prev_state) = self.previous_runtime_state.take() {
                            self.runtime_state = prev_state;
                        }
                        Err(e)
                    }
                }
            } else {
                self.previous_runtime_state = Some(self.runtime_state.clone());
                self.runtime_state = target_state;

                match self
                    .backend
                    .apply_startup_state(&self.runtime_state, &self.signal_state.running)
                {
                    Ok(_) => {
                        let prev_period = self
                            .previous_runtime_state
                            .as_ref()
                            .map(|s| s.period())
                            .unwrap_or(Period::Day);
                        let current_period = self.runtime_state.period();

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
                                *self.signal_state.current_preset.lock().unwrap() = current_preset;
                            } else if values_changed {
                                let (target_temp, target_gamma) = self.runtime_state.values();
                                let target_period = self.runtime_state.period();
                                #[cfg(debug_assertions)]
                                eprintln!(
                                    "DEBUG: Sending ConfigChanged event from config reload (non-smooth)"
                                );
                                ipc_notifier.send_config_changed(
                                    target_period,
                                    target_temp,
                                    target_gamma,
                                );
                            }

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

                        let entering_transition =
                            !prev_period.is_transitioning() && current_period.is_transitioning();

                        log_pipe!();
                        log_info!("Configuration reloaded and state applied successfully");
                        Ok((sent_state_applied, entering_transition))
                    }
                    Err(e) => {
                        log_warning!("Failed to apply new state after config reload: {e}");
                        if let Some(prev_state) = self.previous_runtime_state.take() {
                            self.runtime_state = prev_state;
                        }
                        Err(e)
                    }
                }
            }
        } else {
            let prev_period = self.runtime_state.period();
            self.runtime_state = target_state;
            let current_period = self.runtime_state.period();

            if prev_period != current_period {
                period::log_state_announcement(current_period);
            }

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
                    *self.signal_state.current_preset.lock().unwrap() = current_preset;
                }

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

            let entering_transition =
                !prev_period.is_transitioning() && current_period.is_transitioning();

            log_pipe!();
            log_info!("Configuration reloaded (no state change needed)");
            Ok((sent_state_applied, entering_transition))
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
        if let Some(custom_dir) = config::get_custom_config_dir() {
            let display_path = utils::private_path(&custom_dir);
            log_block_start!("Base directory: {}", display_path);
        }

        log_block_start!(
            "Successfully connected to {} backend",
            self.backend.backend_name()
        );

        self.apply_initial_state()?;

        if self.debug_enabled
            && self.runtime_state.is_geo_mode()
            && let (Some(lat), Some(lon)) = (
                self.runtime_state.config().latitude,
                self.runtime_state.config().longitude,
            )
        {
            let _ = crate::geo::log_solar_debug_info(lat, lon);
        }

        self.main_loop()?;
        log_block_start!("Shutting down sunsetr...");
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

        if let Some((lock_file, lock_path)) = self.lock_info {
            utils::cleanup_application(self.backend, lock_file, &lock_path, self.debug_enabled);
        } else {
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

        let should_transition = smoothing && is_wayland_backend && !self.bypass_smoothing;

        if should_transition && startup_duration >= 0.1 {
            let mut transition = if let Some(ref prev_runtime_state) = self.previous_runtime_state {
                SmoothTransition::reload(prev_runtime_state, &self.runtime_state)
            } else {
                SmoothTransition::startup(&self.runtime_state)
            };

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
                    self.apply_immediate_state(self.runtime_state.period())?;
                }
            }
        } else {
            self.apply_immediate_state(self.runtime_state.period())?;
        }

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

            let current_state = self.runtime_state.period();
            if tracker.is_first_iteration() && self.process_initial_signals(&current_state)? {
                continue 'main_loop;
            }

            // CRITICAL: Check if we just slept to a transition boundary
            // This must happen before any time-based re-evaluation to prevent race conditions
            if tracker.slept_to_transition_boundary() {
                tracker.set_sleeping_to_boundary(false);
                let (new_state, change) = self.runtime_state.with_next_period();

                #[cfg(debug_assertions)]
                eprintln!(
                    "DEBUG [main_loop]: Forced transition: {:?} -> {:?}, change: {:?}",
                    self.runtime_state.period(),
                    new_state.period(),
                    change
                );

                self.runtime_state = new_state;
                if change != crate::core::period::StateChange::None {
                    self.backend
                        .apply_transition_state(&self.runtime_state, &self.signal_state.running)?;
                    tracker.record_state_update();

                    if let Some(ref ipc_notifier) = self.ipc_notifier {
                        let current_period = self.runtime_state.period();

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

                        #[cfg(debug_assertions)]
                        eprintln!("DEBUG [forced_transition]: Sending StateApplied event");
                        ipc_notifier.send_state_applied(&self.runtime_state);
                    }
                }

                tracker.record_current_period(self.runtime_state.period());
                tracker.update_progress(self.runtime_state.progress());
                if !self.runtime_state.period().is_transitioning() {
                    tracker.reset_for_stable_period();
                }
                continue 'main_loop;
            }

            if self.signal_state.needs_reload.load(Ordering::SeqCst) {
                let _ = self.signal_state.pending_config.lock().unwrap().take();
                let new_config = match crate::config::Config::load() {
                    Ok(config) => {
                        #[cfg(debug_assertions)]
                        eprintln!("DEBUG: Loaded config for reload (source of truth)");
                        Some(config)
                    }
                    Err(e) => {
                        log_pipe!();
                        log_error!("Failed to load config: {}", e);
                        log_indented!("Continuing with previous configuration");

                        #[cfg(debug_assertions)]
                        eprintln!("DEBUG: Config load failed, skipping reload");
                        None
                    }
                };

                if let Some(new_config) = new_config {
                    let _ = self.update_runtime_state();
                    match self.handle_config_reload(new_config) {
                        Ok((sent_state_applied, entering_transition)) => {
                            if sent_state_applied {
                                if entering_transition {
                                    tracker.record_state_update();
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
                            log_pipe!();
                            log_error!("Failed to apply config changes: {e}");
                            log_indented!("Continuing with previous configuration");
                        }
                    }
                }

                self.signal_state
                    .needs_reload
                    .store(false, Ordering::SeqCst);
            }

            let should_update = if tracker.handle_first_iteration() {
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: First iteration, skipping state update check");
                false
            } else if tracker.handle_config_reload_skip() {
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: Config reload handled, skipping redundant state update");
                false
            } else if self.runtime_state.period().is_transitioning() {
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
                let state_change = self.update_runtime_state();
                let update_needed = !matches!(state_change, StateChange::None);

                #[cfg(debug_assertions)]
                eprintln!(
                    "DEBUG: update_runtime_state result: {state_change:?} (update_needed: {update_needed})"
                );

                update_needed
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

                        tracker.record_state_update();

                        if let Some(ref ipc_notifier) = self.ipc_notifier {
                            let current_period = self.runtime_state.period();

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
                            break;
                        } else {
                            log_pipe!();
                            log_error!("Failed to apply state: {e}");
                            log_decorated!("Will retry on next cycle...");
                        }
                    }
                }
            }

            let should_log_progress =
                tracker.should_log_progress(self.runtime_state.period(), should_update);

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
            use std::sync::mpsc::RecvTimeoutError;

            // Helper: poll backend hotplug periodically during long sleeps
            let mut poll_interval = Duration::from_millis(10);
            if poll_interval > calculated_sleep_duration {
                poll_interval = calculated_sleep_duration;
            }

            // In simulation mode, crate::time::source::sleep already handles the time scaling
            // We can't use recv_timeout with the full duration as it would sleep too long
            // So we need to handle simulation differently
            let recv_result = if crate::time::source::is_simulated() {
                let sleep_handle = std::thread::spawn({
                    let duration = calculated_sleep_duration;
                    move || {
                        crate::time::source::sleep(duration);
                    }
                });

                loop {
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
                    let going_to_sleep = matches!(
                        signal_msg,
                        crate::io::signals::SignalMessage::Sleep { resuming: false }
                    );

                    crate::io::signals::handle_signal_message(
                        signal_msg,
                        &mut self.backend,
                        &self.signal_state,
                        &self.runtime_state,
                        self.debug_enabled,
                    )?;

                    if going_to_sleep {
                        continue;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    #[cfg(debug_assertions)]
                    eprintln!("DEBUG: Sleep duration elapsed naturally");
                }
                Err(RecvTimeoutError::Disconnected) => {
                    if !self.signal_state.running.load(Ordering::SeqCst) {
                        #[cfg(debug_assertions)]
                        eprintln!("DEBUG: Channel disconnected during graceful shutdown");
                    } else {
                        log_pipe!();
                        log_error!("Signal handler disconnected unexpectedly");
                        log_indented!("Signals will no longer be processed");
                        log_indented!("Consider restarting sunsetr if signal handling is needed");
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
            let going_to_sleep = matches!(
                signal_msg,
                crate::io::signals::SignalMessage::Sleep { resuming: false }
            );

            // Critical signals are handled via atomic flags (signal-safe pattern)
            // Config reload: handled via needs_reload flag
            // Shutdown: handled via running/instant_shutdown flags
            // Sleep: detected here, skips main loop iteration as intended

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
        let sleep_duration = if runtime_state.period().is_transitioning() {
            let update_interval = Duration::from_secs(
                runtime_state
                    .config()
                    .update_interval
                    .unwrap_or(DEFAULT_UPDATE_INTERVAL),
            );

            if let Some(time_remaining) = runtime_state.time_until_transition_end() {
                if time_remaining < update_interval {
                    tracker.set_sleeping_to_boundary(true);

                    #[cfg(debug_assertions)]
                    eprintln!(
                        "DEBUG [determine_sleep_duration]: Sleeping to boundary, time_remaining={:.3}s",
                        time_remaining.as_secs_f64()
                    );

                    time_remaining
                } else {
                    update_interval
                }
            } else {
                update_interval
            }
        } else {
            runtime_state.time_until_next_event()
        };

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

            if should_log {
                let percentage_str =
                    utils::format_progress_percentage(progress, tracker.previous_progress());

                let display_secs = utils::format_duration_seconds_ceil(sleep_duration);

                let log_message = format!(
                    "Transition {} complete. Next update in {} seconds",
                    percentage_str, display_secs
                );

                if debug_enabled {
                    log_block_start!("{}", log_message);
                } else if !tracker.first_transition_logged() {
                    log_block_start!("{}", log_message);
                    tracker.set_first_transition_logged(true);
                } else {
                    log_decorated!("{}", log_message);
                }
            }

            tracker.update_progress(Some(progress));
        } else {
            tracker.reset_for_stable_period();

            if debug_enabled && runtime_state.period() != Period::Static {
                let now = crate::time::source::now();
                let next_transition_time_raw =
                    now + chrono::Duration::milliseconds(sleep_duration.as_millis() as i64);

                let millis = next_transition_time_raw.timestamp_millis();
                let remainder_millis = millis % 1000;
                let next_transition_time = if remainder_millis > 0 {
                    let next_second_millis = ((millis / 1000) + 1) * 1000;
                    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(next_second_millis)
                        .map(|utc| utc.with_timezone(&chrono::Local))
                        .unwrap_or(next_transition_time_raw)
                } else {
                    next_transition_time_raw
                };

                let next = runtime_state.period().next_period();
                let transition_info = format!(
                    "{} {} → {} {}",
                    runtime_state.period().display_name(),
                    runtime_state.period().symbol(),
                    next.display_name(),
                    next.symbol()
                );

                if runtime_state.is_geo_mode()
                    && let (Some(lat), Some(lon)) = (
                        runtime_state.config().latitude,
                        runtime_state.config().longitude,
                    )
                {
                    let city_tz = crate::geo::solar::determine_timezone_from_coordinates(lat, lon);

                    let next_transition_city_tz = next_transition_time.with_timezone(&city_tz);

                    log_pipe!();
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
                    log_pipe!();
                    log_debug!(
                        "Next transition will begin at: {} {}",
                        next_transition_time.format("%H:%M:%S"),
                        transition_info
                    );
                }
            }

            let just_entered_stable = tracker
                .previous_period()
                .map(|prev| prev.is_transitioning())
                .unwrap_or(true);

            if just_entered_stable
                && sleep_duration >= Duration::from_secs(1)
                && runtime_state.period() != Period::Static
            {
                let total_seconds = utils::format_duration_seconds_ceil(sleep_duration);
                let hours = total_seconds / 3600;
                let minutes = (total_seconds % 3600) / 60;
                let seconds = total_seconds % 60;

                if hours > 0 {
                    if minutes > 0 {
                        log_block_start!("Next transition in {} hours {} minutes", hours, minutes);
                    } else {
                        log_block_start!("Next transition in {} hours", hours);
                    }
                } else if minutes > 0 {
                    log_block_start!("Next transition in {} minutes {} seconds", minutes, seconds);
                } else {
                    log_block_start!("Next transition in {} seconds", seconds);
                }
            }
        }

        tracker.record_current_period(runtime_state.period());

        Ok(sleep_duration)
    }
}
