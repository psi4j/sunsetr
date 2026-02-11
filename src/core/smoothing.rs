//! Smooth transition system for smooth interpolation between different temp and gamma values.
//!
//! This module provides animated transitions when sunsetr starts, during configuration changes, or exits,
//! smoothly moving from existing values to the current target state over a configured duration.
//! It handles static targets (stable and static periods) and dynamic targets (during ongoing
//! sunrise/sunset transitioning periods).
//!
//! # When This System Is Used
//!
//! This system is only active when `smoothing = true` and `backend = "wayland"` in the configuration,
//! and while the `startup_duration` and or `shutdown_duration` are greater than `0.1`. Providing a
//! value lower than `0.1` for either disables smoothing for startup or shutdown respectively.
//! Reloading is treated as if it were a startup transition.
//!
//! The Hyprland and Hyprsunset backends are not supported due to conflicting CTM animations.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::backend::ColorTemperatureBackend;
use crate::common::constants::*;
use crate::common::logger::Log;
use crate::common::utils::{ProgressBar, interpolate_f64, interpolate_inverse_u32};
use crate::core::period::Period;

/// Type of smooth transition being performed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransitionType {
    Startup,
    Shutdown,
}

/// Whether a SmoothTransition completed or was interrupted with final values.
///
/// Used to optionally pass current values to a new SmoothTransition in the event
/// that a SmoothTransition is interrupted and needs to recalculate target values.
/// This allows for better latency when performing consecutive reloads.
pub enum TransitionResult {
    Completed,
    Interrupted {
        current_temp: u32,
        current_gamma: f64,
    },
}

/// High-precision sleep that combines OS sleep with busy-waiting for accuracy.
///
/// For durations > 2ms, sleeps for duration - 1ms using OS sleep, then
/// busy-waits for the remaining time to achieve sub-millisecond precision.
fn high_precision_sleep(duration: Duration) {
    let start = Instant::now();

    if duration <= Duration::from_millis(2) {
        while start.elapsed() < duration {
            std::hint::spin_loop();
        }
        return;
    }

    let sleep_duration = duration.saturating_sub(Duration::from_millis(1));
    thread::sleep(sleep_duration);

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
    start_temp: u32,
    start_gamma: f64,
    target_temp: u32,
    target_gamma: f64,
    transition_type: TransitionType,
    start_time: Instant,
    duration: Duration,
    is_dynamic_target: bool,
    initial_state: Option<Period>,
    progress_bar: ProgressBar,
    show_progress_bar: bool,
    suppress_logs: bool,
    no_announce: bool,
    base_ms: f64,
}

/// Adaptive interval controller that adjusts update frequency based on system performance.
///
/// Uses a SmoothDamp algorithm for smooth, stable convergence to the
/// optimal update rate without oscillation or overshoot.
struct AdaptiveInterval {
    current_ms: f64,
    target_ms: f64,
    velocity: f64,
    base_ms: f64,
    smooth_time: f64,
    max_speed: f64,
}

impl AdaptiveInterval {
    /// Creates a new adaptive interval controller for the given transition duration.
    fn new(transition_duration: Duration, base_ms: f64) -> Self {
        let initial_ms = base_ms;

        let duration_secs = transition_duration.as_secs_f64();
        let smooth_time = if duration_secs < 2.0 {
            0.15_f64
        } else if duration_secs < 10.0 {
            0.2 + ((duration_secs - 2.0) * 0.0375)
        } else {
            0.5_f64
        };

        Self {
            current_ms: initial_ms,
            target_ms: initial_ms,
            velocity: 0.0,
            base_ms,
            smooth_time,
            max_speed: 100.0,
        }
    }

    /// Updates the interval based on measured system latency using SmoothDamp algorithm.
    /// Returns the next interval to use for sleeping between updates.
    fn update(&mut self, measured_latency: Duration) -> Duration {
        let latency_ms = measured_latency.as_secs_f64() * 1000.0;

        // Calculate target interval with headroom (1.5x latency + small buffer)
        // This ensures we're not running at 100% capacity
        self.target_ms = (latency_ms * 1.5 + 2.0).max(self.base_ms).min(250.0);

        let dt = self.current_ms / 1000.0;
        let omega = 2.0 / self.smooth_time;
        let x = omega * dt;
        let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
        let change = self.current_ms - self.target_ms;
        let original_to = self.target_ms;
        let temp = (self.velocity + omega * change) * dt;
        self.velocity = (self.velocity - omega * temp) * exp;
        let max_delta = self.max_speed * dt;
        self.velocity = self.velocity.clamp(-max_delta, max_delta);
        let output = self.target_ms + (change + temp) * exp;

        if (original_to - self.current_ms).signum() == (output - original_to).signum() {
            self.current_ms = original_to;
            self.velocity = 0.0;
        } else {
            self.current_ms = output;
        }

        self.current_ms = self.current_ms.max(self.base_ms);
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
    /// * `target_runtime_state` - Target RuntimeState to transition towards
    ///
    /// # Returns
    /// New SmoothTransition ready for execution
    pub fn startup(target_runtime_state: &crate::core::runtime_state::RuntimeState) -> Self {
        let start_temp = target_runtime_state
            .config()
            .day_temp
            .unwrap_or(DEFAULT_DAY_TEMP);
        let start_gamma = target_runtime_state
            .config()
            .day_gamma
            .unwrap_or(DEFAULT_DAY_GAMMA);

        let (target_temp, target_gamma) = target_runtime_state.values();
        let is_dynamic_target = target_runtime_state.period().is_transitioning();

        let duration_secs = target_runtime_state
            .config()
            .startup_duration
            .unwrap_or(DEFAULT_STARTUP_DURATION);

        let base_ms = target_runtime_state
            .config()
            .adaptive_interval
            .unwrap_or(DEFAULT_ADAPTIVE_INTERVAL) as f64;

        Self {
            start_temp,
            start_gamma,
            target_temp,
            target_gamma,
            transition_type: TransitionType::Startup,
            start_time: Instant::now(),
            duration: Duration::from_secs_f64(duration_secs),
            is_dynamic_target,
            initial_state: Some(target_runtime_state.period()),
            progress_bar: ProgressBar::new(PROGRESS_BAR_WIDTH),
            show_progress_bar: true,
            suppress_logs: false,
            no_announce: false,
            base_ms,
        }
    }

    /// Reload transition: from current RuntimeState values to new target RuntimeState values
    /// Used for config reloads where we transition from currently applied values to new config values
    ///
    /// # Arguments
    /// * `current_runtime_state` - Current RuntimeState (what's currently applied)
    /// * `target_runtime_state` - Target RuntimeState (new config values)
    ///
    /// # Returns
    /// New SmoothTransition ready for execution
    pub fn reload(
        current_runtime_state: &crate::core::runtime_state::RuntimeState,
        target_runtime_state: &crate::core::runtime_state::RuntimeState,
    ) -> Self {
        let (start_temp, start_gamma) = current_runtime_state.values();
        let (target_temp, target_gamma) = target_runtime_state.values();
        let is_dynamic_target = target_runtime_state.period().is_transitioning();

        let duration_secs = target_runtime_state
            .config()
            .startup_duration
            .unwrap_or(DEFAULT_STARTUP_DURATION);

        let base_ms = target_runtime_state
            .config()
            .adaptive_interval
            .unwrap_or(DEFAULT_ADAPTIVE_INTERVAL) as f64;

        Self {
            start_temp,
            start_gamma,
            target_temp,
            target_gamma,
            transition_type: TransitionType::Startup,
            start_time: Instant::now(),
            duration: Duration::from_secs_f64(duration_secs),
            is_dynamic_target,
            initial_state: Some(current_runtime_state.period()),
            progress_bar: ProgressBar::new(PROGRESS_BAR_WIDTH),
            show_progress_bar: true,
            suppress_logs: false,
            no_announce: false,
            base_ms,
        }
    }

    /// Override the start values for this transition.
    ///
    /// Used when resuming from an interrupted transition — the start values
    /// are set to the last-applied temp/gamma from the interrupted transition,
    /// preventing a visual jump on the display.
    pub fn with_start_values(mut self, temp: u32, gamma: f64) -> Self {
        self.start_temp = temp;
        self.start_gamma = gamma;
        self
    }

    /// Configure the transition for silent operation (no progress bar, no logs).
    ///
    /// This is commonly used for simulation mode, reloads, and test operations.
    /// Combines hiding the progress bar with suppressing debug logs for a
    /// completely quiet transition. Works for both startup and shutdown transitions.
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

    /// Test mode transition: from current RuntimeState values to specific test values.
    ///
    /// This is specifically designed for test mode where we want to transition from
    /// whatever the current state is to user-provided test values, then back.
    /// Uses startup_duration for timing consistency.
    pub fn test_mode(
        current_runtime_state: &crate::core::runtime_state::RuntimeState,
        test_temp: u32,
        test_gamma: f64,
    ) -> Self {
        let (start_temp, start_gamma) = current_runtime_state.values();
        let target_temp = test_temp;
        let target_gamma = test_gamma;
        let is_dynamic_target = false;

        let duration_secs = current_runtime_state
            .config()
            .startup_duration
            .unwrap_or(DEFAULT_STARTUP_DURATION);

        let base_ms = current_runtime_state
            .config()
            .adaptive_interval
            .unwrap_or(DEFAULT_ADAPTIVE_INTERVAL) as f64;

        Self {
            start_time: std::time::Instant::now(),
            duration: std::time::Duration::from_secs_f64(duration_secs),
            start_temp,
            start_gamma,
            target_temp,
            target_gamma,
            transition_type: TransitionType::Startup,
            is_dynamic_target,
            initial_state: None,
            progress_bar: ProgressBar::new(PROGRESS_BAR_WIDTH),
            show_progress_bar: false,
            suppress_logs: false,
            no_announce: true,
            base_ms,
        }
    }

    /// Test mode restoration: from test values back to RuntimeState values.
    ///
    /// This is the counterpart to test_mode() for restoring normal operation.
    /// Uses shutdown_duration for timing consistency.
    pub fn test_restore(
        target_runtime_state: &crate::core::runtime_state::RuntimeState,
        current_test_temp: u32,
        current_test_gamma: f64,
    ) -> Self {
        let start_temp = current_test_temp;
        let start_gamma = current_test_gamma;
        let (target_temp, target_gamma) = target_runtime_state.values();
        let is_dynamic_target = target_runtime_state.period().is_transitioning();

        let duration_secs = target_runtime_state
            .config()
            .shutdown_duration
            .unwrap_or(DEFAULT_SHUTDOWN_DURATION);

        let base_ms = target_runtime_state
            .config()
            .adaptive_interval
            .unwrap_or(DEFAULT_ADAPTIVE_INTERVAL) as f64;

        Self {
            start_time: std::time::Instant::now(),
            duration: std::time::Duration::from_secs_f64(duration_secs),
            start_temp,
            start_gamma,
            target_temp,
            target_gamma,
            transition_type: TransitionType::Shutdown,
            is_dynamic_target,
            initial_state: None,
            progress_bar: ProgressBar::new(PROGRESS_BAR_WIDTH),
            show_progress_bar: false,
            suppress_logs: false,
            no_announce: true,
            base_ms,
        }
    }

    /// Create a shutdown transition from current state to day values.
    /// Returns None if duration < 0.1 (instant transition).
    ///
    /// # Arguments
    /// * `config` - Configuration containing transition settings
    /// * `geo_times` - Optional geographic transition times for calculating current state
    ///
    /// # Returns
    /// Some(SmoothTransition) if duration >= 0.1, None for instant transition
    /// Shutdown transition: from current values to day values
    /// Uses RuntimeState to get both current state AND day target values
    pub fn shutdown(
        current_runtime_state: &crate::core::runtime_state::RuntimeState,
    ) -> Option<Self> {
        let duration_secs = current_runtime_state
            .config()
            .shutdown_duration
            .unwrap_or(DEFAULT_SHUTDOWN_DURATION);

        if duration_secs < 0.1 {
            return None;
        }

        let (start_temp, start_gamma) = current_runtime_state.values();

        let target_temp = current_runtime_state
            .config()
            .day_temp
            .unwrap_or(DEFAULT_DAY_TEMP);
        let target_gamma = current_runtime_state
            .config()
            .day_gamma
            .unwrap_or(DEFAULT_DAY_GAMMA);

        if start_temp == target_temp && (start_gamma - target_gamma).abs() < 0.01 {
            return None;
        }

        let base_ms = current_runtime_state
            .config()
            .adaptive_interval
            .unwrap_or(DEFAULT_ADAPTIVE_INTERVAL) as f64;

        Some(Self {
            start_temp,
            start_gamma,
            target_temp,
            target_gamma,
            transition_type: TransitionType::Shutdown,
            start_time: Instant::now(),
            duration: Duration::from_secs_f64(duration_secs),
            is_dynamic_target: false,
            initial_state: Some(current_runtime_state.period()),
            progress_bar: ProgressBar::new(PROGRESS_BAR_WIDTH),
            show_progress_bar: false,
            suppress_logs: false,
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
    /// * `current_runtime_state` - Current RuntimeState providing all context for calculations
    ///
    /// # Returns
    /// Tuple of (target_temperature, target_gamma) for the current animation frame
    fn calculate_current_target(
        &self,
        current_runtime_state: &crate::core::runtime_state::RuntimeState,
    ) -> (u32, f64) {
        match self.transition_type {
            TransitionType::Shutdown => (self.target_temp, self.target_gamma),
            TransitionType::Startup => {
                let initial_state = match &self.initial_state {
                    Some(state) => state,
                    None => return (self.target_temp, self.target_gamma),
                };

                if initial_state.is_stable() {
                    return (self.target_temp, self.target_gamma);
                }

                if self.is_dynamic_target {
                    let current_state = current_runtime_state.period();

                    let same_transition = matches!(
                        (initial_state, current_state),
                        (Period::Sunset, Period::Sunset) | (Period::Sunrise, Period::Sunrise)
                    );

                    if same_transition {
                        return current_runtime_state.values();
                    }
                }

                (self.target_temp, self.target_gamma)
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
    /// * `current_runtime_state` - Current RuntimeState providing all context
    /// * `running` - Atomic flag to check if the program should continue
    /// * `reload_signal` - Optional atomic flag checked each iteration; if set, the transition
    ///   is interrupted and returns `TransitionResult::Interrupted` with the last-applied values
    ///
    /// # Returns
    /// Result containing `TransitionResult::Completed` on normal finish, or
    /// `TransitionResult::Interrupted` with the current temp/gamma if a reload signal was detected
    pub fn execute(
        &mut self,
        backend: &mut dyn ColorTemperatureBackend,
        current_runtime_state: &crate::core::runtime_state::RuntimeState,
        running: &AtomicBool,
        reload_signal: Option<&AtomicBool>,
    ) -> anyhow::Result<TransitionResult> {
        let (initial_target_temp, initial_target_gamma) =
            self.calculate_current_target(current_runtime_state);

        if self.start_temp == initial_target_temp
            && self.start_gamma == initial_target_gamma
            && !self.is_dynamic_target
        {
            match self.transition_type {
                TransitionType::Startup => {
                    let logging_was_enabled = if self.no_announce {
                        let was_enabled = Log::is_enabled();
                        Log::set_enabled(false);
                        was_enabled
                    } else {
                        true
                    };

                    if self.initial_state.is_some() {
                        backend.apply_startup_state(current_runtime_state, running)?;
                    }

                    if self.no_announce && logging_was_enabled {
                        Log::set_enabled(true);
                    }
                }
                TransitionType::Shutdown => {
                    backend.apply_temperature_gamma(
                        self.target_temp,
                        self.target_gamma,
                        running,
                    )?;
                }
            }

            return Ok(TransitionResult::Completed);
        }

        if self.show_progress_bar || self.suppress_logs {
            if self.show_progress_bar {
                let duration_str = if self.duration.as_secs_f64() >= 1.0 {
                    format!("{}s", self.duration.as_secs())
                } else {
                    format!("{:.1}s", self.duration.as_secs_f64())
                };

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

            Log::set_enabled(false);
        }

        let mut adaptive_interval = AdaptiveInterval::new(self.duration, self.base_ms);

        if self.show_progress_bar {
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "┃").ok();
            stdout.flush().ok();
        }

        let mut last_update = Instant::now();
        while self.transition_type == TransitionType::Shutdown || running.load(Ordering::SeqCst) {
            let loop_start = Instant::now();
            let elapsed = loop_start.duration_since(self.start_time);

            let elapsed_ms = elapsed.as_millis() as f32;
            let duration_ms = self.duration.as_millis() as f32;
            let linear_progress = (elapsed_ms / duration_ms).min(1.0);

            let progress = crate::common::utils::bezier_curve(
                linear_progress,
                crate::common::constants::BEZIER_P1X,
                crate::common::constants::BEZIER_P1Y,
                crate::common::constants::BEZIER_P2X,
                crate::common::constants::BEZIER_P2Y,
            );

            let (target_temp, target_gamma) = self.calculate_current_target(current_runtime_state);
            let current_temp = interpolate_inverse_u32(self.start_temp, target_temp, progress);
            let current_gamma = interpolate_f64(self.start_gamma, target_gamma, progress);

            if self.show_progress_bar {
                let suffix = format!("(temp: {current_temp}K, gamma: {current_gamma:.1}%)");
                self.progress_bar.update(progress, Some(&suffix));
            }

            if backend
                .apply_temperature_gamma(current_temp, current_gamma, running)
                .is_err()
            {
                log_warning!(
                    "Failed to apply temperature/gamma during startup transition. \
                    Will attempt to set final state after transition."
                );
            }

            if let Some(signal) = reload_signal
                && signal.load(Ordering::SeqCst)
            {
                if self.show_progress_bar || self.suppress_logs {
                    Log::set_enabled(true);
                }

                #[cfg(debug_assertions)]
                eprintln!(
                    "DEBUG: Smooth transition interrupted by reload signal at {:.1}% (temp: {}K, gamma: {:.1}%)",
                    progress * 100.0,
                    current_temp,
                    current_gamma
                );

                return Ok(TransitionResult::Interrupted {
                    current_temp,
                    current_gamma,
                });
            }

            let work_latency = loop_start.elapsed();
            let update_interval = adaptive_interval.update(work_latency);

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

            if progress >= 1.0 {
                break;
            }

            let time_since_last_update = loop_start.duration_since(last_update);
            if time_since_last_update < update_interval {
                let sleep_duration = update_interval - time_since_last_update;

                if crate::time::source::is_simulated() {
                    thread::sleep(Duration::from_millis(1));
                } else {
                    high_precision_sleep(sleep_duration);
                }
            }
            last_update = Instant::now();
        }

        if self.show_progress_bar {
            self.progress_bar.finish();
            println!("┃");
            io::stdout().flush().ok();
            Log::set_enabled(true);

            let transition_name = match self.transition_type {
                TransitionType::Startup => "Startup",
                TransitionType::Shutdown => "Shutdown",
            };
            log_decorated!("{} transition complete", transition_name);
        } else if self.suppress_logs {
            Log::set_enabled(true);
        }

        match self.transition_type {
            TransitionType::Startup => {
                let logging_was_enabled =
                    if (!self.show_progress_bar && !self.suppress_logs) || self.no_announce {
                        let was_enabled = Log::is_enabled();
                        Log::set_enabled(false);
                        was_enabled
                    } else {
                        true
                    };

                if self.initial_state.is_some() {
                    backend.apply_startup_state(current_runtime_state, running)?;
                }

                if ((!self.show_progress_bar && !self.suppress_logs) || self.no_announce)
                    && logging_was_enabled
                {
                    Log::set_enabled(true);
                }
            }
            TransitionType::Shutdown => {
                backend.apply_temperature_gamma(self.target_temp, self.target_gamma, running)?;
            }
        }

        Ok(TransitionResult::Completed)
    }
}
