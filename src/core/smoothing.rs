//! Smooth startup transition system for gradual state application.
//!
//! This module provides animated transitions when sunsetr starts, smoothly moving
//! from default day values to the current target state over a configured duration.
//! It handles both static targets (stable day/night) and dynamic targets (during
//! ongoing sunrise/sunset transitions).
//!
//! # When This System Is Used
//!
//! This system is only active when `startup_transition = true` in the configuration.
//! When `startup_transition = false`, sunsetr starts hyprsunset directly at the
//! correct interpolated state for immediate accuracy without any animation.
//!
//! # Timing Consistency
//!
//! The system captures the target state at startup and applies that exact state
//! after the transition completes, regardless of how much time has passed. This
//! prevents timing-related bugs where starting near transition boundaries could
//! cause the program to jump to an unexpected state.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::backend::ColorTemperatureBackend;
use crate::common::constants::*;
use crate::common::logger::Log;
use crate::common::utils::{ProgressBar, interpolate_f32, interpolate_u32};
use crate::config::Config;
use crate::state::period::{TimeState, get_transition_state};

/// Type of smooth transition being performed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransitionType {
    /// Startup transition from day values to current target state
    Startup,
    /// Shutdown transition from current values to day values
    Shutdown,
}

/// High-precision sleep that combines OS sleep with busy-waiting for accuracy.
///
/// For durations > 2ms, sleeps for duration - 1ms using OS sleep, then
/// busy-waits for the remaining time to achieve sub-millisecond precision.
fn high_precision_sleep(duration: Duration) {
    let start = Instant::now();

    // For very short durations, just busy-wait
    if duration <= Duration::from_millis(2) {
        while start.elapsed() < duration {
            std::hint::spin_loop();
        }
        return;
    }

    // Sleep for most of the duration (leave 1ms for busy-wait)
    let sleep_duration = duration.saturating_sub(Duration::from_millis(1));
    thread::sleep(sleep_duration);

    // Busy-wait for the remaining time for precision
    while start.elapsed() < duration {
        std::hint::spin_loop();
    }
}

/// Manages smooth animated transitions during application startup and shutdown.
///
/// The transition system provides a gentle visual transition from one state to another,
/// preventing jarring changes. For startup, it transitions from default day settings
/// to the appropriate current state. For shutdown, it transitions from current values
/// to day values. It supports both static targets (stable day/night periods) and
/// dynamic targets (during sunrise/sunset).
///
/// # Features
/// - Animated progress bar with live temperature/gamma display
/// - Dynamic target tracking for ongoing transitions
/// - Graceful fallback on communication errors
/// - Configurable duration and update frequency
pub struct SmoothTransition {
    /// Starting temperature
    start_temp: u32,
    /// Starting gamma value
    start_gamma: f32,
    /// Target temperature
    target_temp: u32,
    /// Target gamma value
    target_gamma: f32,
    /// Type of transition (startup or shutdown)
    transition_type: TransitionType,
    /// Time when the transition started
    start_time: Instant,
    /// Total duration of the transition in seconds
    duration: Duration,
    /// Whether we're transitioning to a dynamic target (during sunrise/sunset)
    is_dynamic_target: bool,
    /// The starting state that was captured for the transition (used for startup transitions)
    initial_state: Option<TimeState>,
    /// Progress bar instance for displaying transition progress
    progress_bar: ProgressBar,
    /// Whether to show the animated progress bar during transitions.
    /// When false, transitions still occur but without visual feedback.
    /// This is useful for test mode or other scenarios where terminal output
    /// should be minimal.
    show_progress_bar: bool,
    /// Whether to suppress debug logs during the transition.
    /// This is independent of progress bar display - logs can be suppressed
    /// even when the progress bar is not shown (e.g., in simulation mode).
    suppress_logs: bool,
    /// Geo transition times for accurate dynamic target calculation in geo mode.
    /// Needed when transitioning during sunrise/sunset in geo mode.
    geo_times: Option<crate::geo::times::GeoTransitionTimes>,
    /// Whether to skip the final state announcement after transition.
    /// Used for test mode where we don't want to announce entering a specific state.
    no_announce: bool,
    /// Minimum interval between updates in milliseconds (user-configurable)
    base_ms: u64,
}

/// Adaptive interval controller that adjusts update frequency based on system performance.
///
/// Uses a game engine-style SmoothDamp algorithm for smooth, stable convergence to the
/// optimal update rate without oscillation or overshoot.
struct AdaptiveInterval {
    /// Current interval in milliseconds (smoothly animated)
    current_ms: f64,
    /// Target interval based on system performance
    target_ms: f64,
    /// Rate of change (velocity) for smooth damping
    velocity: f64,
    /// Minimum interval in milliseconds (user-configurable floor)
    base_ms: u64,
    /// How quickly to reach target (in seconds)
    smooth_time: f64,
    /// Maximum rate of change (ms per second)
    max_speed: f64,
}

impl AdaptiveInterval {
    /// Creates a new adaptive interval controller for the given transition duration.
    fn new(transition_duration: Duration, base_ms: u64) -> Self {
        // Start at the user's configured minimum interval
        // The adaptive algorithm will adjust upward from here based on system performance
        let initial_ms = base_ms as f64;

        // Adjust smooth_time based on transition duration
        // Short transitions (< 2s): respond quickly (0.15s)
        // Long transitions (> 10s): respond slowly (0.5s)
        let duration_secs = transition_duration.as_secs_f32();
        let smooth_time = if duration_secs < 2.0 {
            0.15_f64
        } else if duration_secs < 10.0 {
            0.2 + ((duration_secs - 2.0) * 0.0375) as f64 // Linear interpolation
        } else {
            0.5_f64
        };

        Self {
            current_ms: initial_ms,
            target_ms: initial_ms,
            velocity: 0.0,
            base_ms,
            smooth_time,
            max_speed: 100.0, // Can change by up to 100ms per second
        }
    }

    /// Updates the interval based on measured system latency using SmoothDamp algorithm.
    /// Returns the next interval to use for sleeping between updates.
    fn update(&mut self, measured_latency: Duration) -> Duration {
        let latency_ms = measured_latency.as_secs_f64() * 1000.0;

        // Calculate target interval with headroom (1.5x latency + small buffer)
        // This ensures we're not running at 100% capacity
        self.target_ms = (latency_ms * 1.5 + 2.0).max(self.base_ms as f64).min(250.0); // Cap at 250ms for reasonable responsiveness

        // Time step for this update (approximate)
        let dt = self.current_ms / 1000.0; // Convert to seconds

        // SmoothDamp algorithm
        // This creates a critically damped spring that smoothly approaches the target
        let omega = 2.0 / self.smooth_time;
        let x = omega * dt;
        let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);

        let change = self.current_ms - self.target_ms;
        let original_to = self.target_ms;

        // Velocity update
        let temp = (self.velocity + omega * change) * dt;
        self.velocity = (self.velocity - omega * temp) * exp;

        // Clamp velocity to max speed
        let max_delta = self.max_speed * dt;
        self.velocity = self.velocity.clamp(-max_delta, max_delta);

        // Update position
        let output = self.target_ms + (change + temp) * exp;

        // Prevent overshooting in common scenarios
        if (original_to - self.current_ms).signum() == (output - original_to).signum() {
            self.current_ms = original_to;
            self.velocity = 0.0;
        } else {
            self.current_ms = output;
        }

        // Ensure we respect the minimum interval
        self.current_ms = self.current_ms.max(self.base_ms as f64);

        Duration::from_secs_f64(self.current_ms / 1000.0)
    }
}

impl SmoothTransition {
    /// Create a new startup transition with the given target state.
    ///
    /// The transition always starts from day values to provide a consistent
    /// baseline, regardless of the target state. This creates a natural feel
    /// where the display appears to "wake up" and adjust to the current time.
    ///
    /// # Arguments
    /// * `current_state` - Target state to transition towards
    /// * `config` - Configuration containing transition duration and color values
    /// * `geo_times` - Optional geo transition times for accurate dynamic target calculation
    ///
    /// # Returns
    /// New SmoothTransition ready for execution
    pub fn startup(
        current_state: TimeState,
        config: &Config,
        geo_times: Option<crate::geo::times::GeoTransitionTimes>,
    ) -> Self {
        // Always start from day values for consistent animation baseline
        let start_temp = config
            .day_temp
            .unwrap_or(crate::common::constants::DEFAULT_DAY_TEMP);
        let start_gamma = config
            .day_gamma
            .unwrap_or(crate::common::constants::DEFAULT_DAY_GAMMA);

        // Check if this is a dynamic target (we're starting during a transition)
        let is_dynamic_target = current_state.is_transitioning();

        // Get the configured startup transition duration
        let duration_secs = config.startup_duration.unwrap_or(DEFAULT_STARTUP_DURATION);

        // Get the configured minimum interval
        let base_ms = config
            .adaptive_interval
            .unwrap_or(DEFAULT_ADAPTIVE_INTERVAL);

        // For startup, targets will be calculated dynamically
        let (target_temp, target_gamma) = current_state.values(config);

        Self {
            start_temp,
            start_gamma,
            target_temp,
            target_gamma,
            transition_type: TransitionType::Startup,
            start_time: Instant::now(),
            duration: Duration::from_secs_f64(duration_secs),
            is_dynamic_target,
            initial_state: Some(current_state),
            progress_bar: ProgressBar::new(PROGRESS_BAR_WIDTH),
            show_progress_bar: true,
            suppress_logs: false,
            geo_times,
            no_announce: false,
            base_ms,
        }
    }

    /// Create a reload transition from specific temperature and gamma values.
    ///
    /// This constructor is used when reloading configuration or when the config
    /// changes at runtime, allowing the transition to start from the current
    /// display values rather than always starting from day values.
    ///
    /// # Arguments
    /// * `start_temp` - Starting temperature value
    /// * `start_gamma` - Starting gamma value
    /// * `target_state` - Target state to transition towards
    /// * `config` - Configuration containing transition duration
    /// * `geo_times` - Optional geo transition times for accurate dynamic target calculation
    ///
    /// # Returns
    /// New SmoothTransition ready for execution
    pub fn reload(
        start_temp: u32,
        start_gamma: f32,
        target_state: TimeState,
        config: &Config,
        geo_times: Option<crate::geo::times::GeoTransitionTimes>,
    ) -> Self {
        // Check if this is a dynamic target (we're starting during a transition)
        let is_dynamic_target = target_state.is_transitioning();

        // Get the configured startup transition duration
        let duration_secs = config.startup_duration.unwrap_or(DEFAULT_STARTUP_DURATION);

        // Get the configured minimum interval
        let base_ms = config
            .adaptive_interval
            .unwrap_or(DEFAULT_ADAPTIVE_INTERVAL);

        let (target_temp, target_gamma) = target_state.values(config);

        Self {
            start_temp,
            start_gamma,
            target_temp,
            target_gamma,
            transition_type: TransitionType::Startup,
            start_time: Instant::now(),
            duration: Duration::from_secs_f64(duration_secs),
            is_dynamic_target,
            initial_state: Some(target_state),
            progress_bar: ProgressBar::new(PROGRESS_BAR_WIDTH),
            show_progress_bar: true,
            suppress_logs: false,
            geo_times,
            no_announce: false,
            base_ms,
        }
    }

    /// Configure the transition for silent operation (no progress bar, no logs).
    ///
    /// This is commonly used for simulation mode, reloads, and test operations.
    /// Combines hiding the progress bar with suppressing debug logs for a
    /// completely quiet transition. Works for both startup and shutdown transitions.
    ///
    /// # Example
    /// ```ignore
    /// let transition = SmoothTransition::startup(state, config, geo_times)
    ///     .silent();
    /// ```
    pub fn silent(mut self) -> Self {
        self.show_progress_bar = false;
        self.suppress_logs = true;
        self
    }

    /// Skip the final state announcement after the transition completes.
    ///
    /// This is useful for test mode where we don't want to announce
    /// entering a specific state like "day mode" or "night mode".
    pub fn no_announce(mut self) -> Self {
        self.no_announce = true;
        self
    }

    /// Create a shutdown transition from current state to day values.
    /// Returns None if duration < 0.1 (instant transition).
    ///
    /// # Arguments
    /// * `config` - Configuration containing transition settings
    /// * `geo_times` - Optional geographic transition times for calculating current state
    ///
    /// # Returns
    /// Some(StartupTransition) if duration >= 0.1, None for instant transition
    pub fn shutdown(
        config: &Config,
        geo_times: Option<crate::geo::times::GeoTransitionTimes>,
    ) -> Option<Self> {
        // Get the shutdown duration (fall back to startup_duration if not set)
        let duration = config
            .shutdown_duration
            .or(config.startup_duration)
            .unwrap_or(DEFAULT_SHUTDOWN_DURATION);

        // If duration < 0.1, return None (instant transition)
        if duration < 0.1 {
            return None;
        }

        // Calculate current state to get starting values
        // For geo mode, ensure we have geo_times or create them
        let geo_times_for_state = if config.transition_mode.as_deref() == Some("geo") {
            // Use existing geo_times if available, otherwise try to create them
            match geo_times {
                Some(times) => Some(times),
                None => {
                    // Attempt to create geo_times from config, ignoring errors
                    // (will fall back to manual times if creation fails)
                    crate::geo::times::GeoTransitionTimes::from_config(config)
                        .ok()
                        .flatten()
                }
            }
        } else {
            None
        };

        let current_state = get_transition_state(config, geo_times_for_state.as_ref());
        let (start_temp, start_gamma) = current_state.values(config);

        // Get day values to transition to
        let target_temp = config
            .day_temp
            .unwrap_or(crate::common::constants::DEFAULT_DAY_TEMP);
        let target_gamma = config
            .day_gamma
            .unwrap_or(crate::common::constants::DEFAULT_DAY_GAMMA);

        // Get the configured minimum interval
        let base_ms = config
            .adaptive_interval
            .unwrap_or(DEFAULT_ADAPTIVE_INTERVAL);

        // Create transition from current values to day values
        Some(Self {
            start_temp,
            start_gamma,
            target_temp,
            target_gamma,
            transition_type: TransitionType::Shutdown,
            start_time: Instant::now(),
            duration: Duration::from_secs_f64(duration),
            is_dynamic_target: false, // Shutdown always goes to fixed day values
            initial_state: None,      // No initial state for shutdown transitions
            progress_bar: ProgressBar::new(PROGRESS_BAR_WIDTH),
            show_progress_bar: false, // No progress bar for shutdown
            suppress_logs: false,     // Normal logging
            geo_times: None,          // Not needed for shutdown
            no_announce: false,
            base_ms,
        })
    }

    /// Calculate current target values for animation purposes during the transition.
    ///
    /// For startup transitions, this method determines the target temperature and gamma
    /// values to animate towards. For static targets (stable day/night), it returns
    /// fixed values. For dynamic targets (ongoing sunrise/sunset), it tracks the current
    /// transition progress to create smooth animation.
    ///
    /// For shutdown transitions, this always returns the fixed day values.
    ///
    /// Note: This is used only for animation targeting during transitions.
    /// The final state applied after startup completion is always the originally captured
    /// state to prevent timing-related issues.
    ///
    /// # Arguments
    /// * `config` - Configuration containing temperature and gamma ranges
    ///
    /// # Returns
    /// Tuple of (target_temperature, target_gamma) for the current animation frame
    fn calculate_current_target(&self, config: &Config) -> (u32, f32) {
        match self.transition_type {
            TransitionType::Shutdown => {
                // Shutdown always targets day values
                (self.target_temp, self.target_gamma)
            }
            TransitionType::Startup => {
                // For startup, we need to handle initial_state
                let initial_state = match &self.initial_state {
                    Some(state) => state,
                    None => return (self.target_temp, self.target_gamma), // Fallback to stored targets
                };

                // If this is a simple stable state, just return its values
                if initial_state.is_stable() {
                    return initial_state.values(config);
                }

                // Complex case: target is a transition (Sunset or Sunrise)
                // If we're in a dynamic transition, recalculate where we should be now
                if self.is_dynamic_target {
                    // Get the current transition state to see if it's still progressing
                    // Use the stored geo_times for accurate calculation in geo mode
                    let current_state = get_transition_state(config, self.geo_times.as_ref());

                    // Check if we're still in the same type of transition
                    let same_transition = matches!(
                        (initial_state, current_state),
                        (TimeState::Sunset { .. }, TimeState::Sunset { .. })
                            | (TimeState::Sunrise { .. }, TimeState::Sunrise { .. })
                    );

                    if same_transition {
                        // We're still in the same transition, use current progress
                        // The current_state already has the latest progress value
                        return current_state.values(config);
                    }
                }

                // If we're not in a dynamic transition or the transition changed,
                // use the initial state's values (static target)
                initial_state.values(config)
            }
        }
    }

    /// Execute the startup transition sequence
    ///
    /// Performs a smooth animated transition from day values (day temperature and gamma
    /// from the config) to the correct state for the current time. This creates a natural
    /// "wake up" effect where the display starts bright and adjusts to the appropriate
    /// settings over the configured startup transition duration.
    ///
    /// For dynamic targets (starting during ongoing sunrise/sunset transitions), the target
    /// values are dynamically calculated during animation to track the moving transition,
    /// creating smooth visual progression.
    ///
    /// The final applied state is always the originally captured state to prevent
    /// timing-related bugs where the startup transition duration could cause incorrect
    /// state transitions.
    ///
    /// # Animation Flow
    /// - **Start**: Always from day temperature/gamma (consistent baseline)
    /// - **Target**: Correct state for current time (day/night/transition progress)  
    /// - **Dynamic tracking**: Target moves for ongoing transitions during animation
    /// - **Final state**: Originally captured state applied after animation
    ///
    /// # Arguments
    /// * `backend` - ColorTemperatureBackend for applying state changes
    /// * `config` - Configuration with transition settings
    /// * `running` - Atomic flag to check if the program should continue
    ///
    /// # Returns
    /// Result indicating success or failure of the transition
    pub fn execute(
        &mut self,
        backend: &mut dyn ColorTemperatureBackend,
        config: &Config,
        running: &AtomicBool,
    ) -> anyhow::Result<()> {
        // Calculate initial target values to check if transition is needed
        let (initial_target_temp, initial_target_gamma) = self.calculate_current_target(config);

        // If target is same as start, no need for transition
        // This applies to both startup and shutdown transitions
        if self.start_temp == initial_target_temp
            && self.start_gamma == initial_target_gamma
            && !self.is_dynamic_target
        {
            match self.transition_type {
                TransitionType::Startup => {
                    // Apply the originally captured state to maintain timing consistency
                    // Even when no transition is needed, we should use the captured state
                    // rather than recalculating, to avoid potential timing-related state changes

                    // Suppress announcement if no_announce is set
                    let logging_was_enabled = if self.no_announce {
                        let was_enabled = Log::is_enabled();
                        Log::set_enabled(false);
                        was_enabled
                    } else {
                        true
                    };

                    if let Some(initial_state) = self.initial_state {
                        backend.apply_startup_state(initial_state, config, running)?;
                    }

                    // Restore logging if we disabled it
                    if self.no_announce && logging_was_enabled {
                        Log::set_enabled(true);
                    }
                }
                TransitionType::Shutdown => {
                    // For shutdown, just apply the final day values directly
                    backend.apply_temperature_gamma(
                        self.target_temp,
                        self.target_gamma,
                        running,
                    )?;
                }
            }

            return Ok(());
        }

        // Create appropriate transition description
        // Suppress logging during transition if either progress bar is shown
        // or logs are explicitly suppressed
        if self.show_progress_bar || self.suppress_logs {
            if self.show_progress_bar {
                // Print this with the normal logger before disabling it
                // Format duration with proper decimal handling
                let duration_str = if self.duration.as_secs_f64() >= 1.0 {
                    format!("{}s", self.duration.as_secs())
                } else {
                    format!("{:.1}s", self.duration.as_secs_f64())
                };

                // Show dynamic target tracking if applicable
                if self.is_dynamic_target {
                    log_block_start!(
                        "Starting smooth transition with dynamic target tracking ({})",
                        duration_str
                    );
                } else {
                    log_block_start!(
                        "Starting smooth transition to target values ({})",
                        duration_str
                    );
                }
            }

            // Disable logging during the transition
            Log::set_enabled(false);
        }

        // Initialize adaptive interval controller with user-configured minimum
        let mut adaptive_interval = AdaptiveInterval::new(self.duration, self.base_ms);

        // Add a blank line before the progress bar for spacing
        if self.show_progress_bar {
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "┃").ok();
            stdout.flush().ok();
        }

        // Loop until transition completes or program stops
        // For shutdown transitions, we continue even if running is false since we're shutting down
        let mut last_update = Instant::now();
        while self.transition_type == TransitionType::Shutdown || running.load(Ordering::SeqCst) {
            let loop_start = Instant::now();
            let elapsed = loop_start.duration_since(self.start_time);

            // Calculate progress (0.0 to 1.0) using millisecond precision
            let elapsed_ms = elapsed.as_millis() as f32;
            let duration_ms = self.duration.as_millis() as f32;
            let linear_progress = (elapsed_ms / duration_ms).min(1.0);

            // Apply Bézier curve for smooth acceleration/deceleration
            // This creates a gentle S-curve that starts slow, speeds up in the middle,
            // and slows down at the end, matching the natural transition curves used
            // for sunrise/sunset transitions and avoiding jarring linear movements
            let progress = crate::common::utils::bezier_curve(
                linear_progress,
                crate::common::constants::BEZIER_P1X,
                crate::common::constants::BEZIER_P1Y,
                crate::common::constants::BEZIER_P2X,
                crate::common::constants::BEZIER_P2Y,
            );

            // Calculate current target (this may change if we're in a dynamic transition)
            let (target_temp, target_gamma) = self.calculate_current_target(config);

            // Calculate current interpolated values
            let current_temp = interpolate_u32(self.start_temp, target_temp, progress);
            let current_gamma = interpolate_f32(self.start_gamma, target_gamma, progress);

            // Draw the progress bar if enabled
            if self.show_progress_bar {
                let suffix = format!("(temp: {current_temp}K, gamma: {current_gamma:.1}%)");
                self.progress_bar.update(progress, Some(&suffix));
            }

            // Apply interpolated values
            if backend
                .apply_temperature_gamma(current_temp, current_gamma, running)
                .is_err()
            {
                log_warning!(
                    "Failed to apply temperature/gamma during startup transition. \
                    Will attempt to set final state after transition."
                );
                // Don't abort the whole transition, just log and continue
                // The final state will be attempted later
            }

            // Measure how long the actual work took
            let work_latency = loop_start.elapsed();

            // Let the adaptive controller decide next interval based on system performance
            let update_interval = adaptive_interval.update(work_latency);

            // Debug logging only in debug builds
            #[cfg(debug_assertions)]
            {
                static mut UPDATE_COUNT: u32 = 0;
                unsafe {
                    UPDATE_COUNT += 1;
                    if UPDATE_COUNT.is_multiple_of(50) {
                        eprintln!(
                            "Adaptive: interval={:?}, latency={:?}, current={:.2}ms, target={:.2}ms",
                            update_interval,
                            work_latency,
                            adaptive_interval.current_ms,
                            adaptive_interval.target_ms
                        );
                    }
                }
            }

            // Break if we've reached 100%
            if progress >= 1.0 {
                break;
            }

            // Sleep until next update, respecting the adaptive interval
            let time_since_last_update = loop_start.duration_since(last_update);
            if time_since_last_update < update_interval {
                let sleep_duration = update_interval - time_since_last_update;

                // In simulation mode, use a much shorter real sleep
                if crate::time::source::is_simulated() {
                    // Sleep for 1ms real time for smooth animation
                    thread::sleep(Duration::from_millis(1));
                } else {
                    high_precision_sleep(sleep_duration);
                }
            }
            last_update = Instant::now();
        }

        // Re-enable logging after transition
        if self.show_progress_bar {
            self.progress_bar.finish();

            // Add spacing line after progress bar
            println!("┃");
            io::stdout().flush().ok();

            // Re-enable logging
            Log::set_enabled(true);

            // Log the completion message using the logger
            let transition_name = match self.transition_type {
                TransitionType::Startup => "Startup",
                TransitionType::Shutdown => "Shutdown",
            };
            log_decorated!("{} transition complete", transition_name);
        } else if self.suppress_logs {
            // Re-enable logging after suppressed transition
            Log::set_enabled(true);
        }

        match self.transition_type {
            TransitionType::Startup => {
                // Temporarily disable logging if we're not showing progress to suppress
                // the "Entering X mode" announcement from apply_startup_state
                // Skip this if logs are already suppressed OR if no_announce is set
                let logging_was_enabled =
                    if (!self.show_progress_bar && !self.suppress_logs) || self.no_announce {
                        let was_enabled = Log::is_enabled();
                        Log::set_enabled(false);
                        was_enabled
                    } else {
                        true
                    };

                // Apply the originally captured state instead of recalculating it
                //
                // IMPORTANT: We must use the state that was captured when the program started,
                // not recalculate it after the startup transition completes. This prevents a
                // timing bug where starting near transition boundaries could cause the program
                // to jump to the wrong state (e.g., starting during a sunset transition but
                // ending up in night mode because 10 seconds passed during startup).
                if let Some(initial_state) = self.initial_state {
                    backend.apply_startup_state(initial_state, config, running)?;
                }

                // Restore logging state if we changed it
                if ((!self.show_progress_bar && !self.suppress_logs) || self.no_announce)
                    && logging_was_enabled
                {
                    Log::set_enabled(true);
                }
            }
            TransitionType::Shutdown => {
                // Apply final state AFTER re-enabling logging (this is intentional)
                // We suppress the flood of intermediate updates during the transition to avoid
                // terminal spam, but we deliberately show the final state application for
                // debugging context. This gives us the best of both worlds: a clean transition
                // without hundreds of log entries, plus visibility of the final state.
                backend.apply_temperature_gamma(self.target_temp, self.target_gamma, running)?;
            }
        }

        Ok(())
    }
}
