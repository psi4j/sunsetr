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
use crate::config::Config;
use crate::constants::*;
use crate::logger::Log;
use crate::time_state::{get_transition_state, TimeState, TransitionState};
use crate::utils::{interpolate_f32, interpolate_u32};

/// Manages smooth animated transitions during application startup.
///
/// The startup transition system provides a gentle visual transition from
/// default day settings to the appropriate current state, preventing jarring
/// changes when the application starts. It supports both static targets
/// (stable day/night periods) and dynamic targets (during sunrise/sunset).
///
/// # Features
/// - Animated progress bar with live temperature/gamma display
/// - Dynamic target tracking for ongoing transitions
/// - Graceful fallback on communication errors
/// - Configurable duration and update frequency
pub struct StartupTransition {
    /// Starting temperature (typically the day temp for smooth animation)
    start_temp: u32,
    /// Starting gamma value
    start_gamma: f32,
    /// Time when the transition started
    start_time: Instant,
    /// Total duration of the transition in seconds
    duration: Duration,
    /// Whether we're transitioning to a dynamic target (during sunrise/sunset)
    is_dynamic_target: bool,
    /// The starting state that was captured for the transition
    initial_state: TransitionState,
    /// Last drawn progress bar percentage (for avoiding redundant redraws)
    last_progress_pct: Option<usize>,
}

impl StartupTransition {
    /// Create a new startup transition with the given target state.
    ///
    /// The transition always starts from day values to provide a consistent
    /// baseline, regardless of the target state. This creates a natural feel
    /// where the display appears to "wake up" and adjust to the current time.
    ///
    /// # Arguments
    /// * `current_state` - Target state to transition towards
    /// * `config` - Configuration containing transition duration and color values
    ///
    /// # Returns
    /// New StartupTransition ready for execution
    pub fn new(current_state: TransitionState, config: &Config) -> Self {
        // Always start from day values for consistent animation baseline
        let start_temp = config
            .day_temp
            .unwrap_or(crate::constants::DEFAULT_DAY_TEMP);
        let start_gamma = config
            .day_gamma
            .unwrap_or(crate::constants::DEFAULT_DAY_GAMMA);

        // Check if this is a dynamic target (we're starting during a transition)
        let is_dynamic_target = matches!(current_state, TransitionState::Transitioning { .. });

        // Get the configured startup transition duration
        let duration_secs = config
            .startup_transition_duration
            .unwrap_or(DEFAULT_STARTUP_TRANSITION_DURATION);

        Self {
            start_temp,
            start_gamma,
            start_time: Instant::now(),
            duration: Duration::from_secs(duration_secs),
            is_dynamic_target,
            initial_state: current_state,
            last_progress_pct: None,
        }
    }

    /// Calculate current target values for animation purposes during the startup transition.
    ///
    /// This method determines the target temperature and gamma values to animate towards
    /// during the startup transition. For static targets (stable day/night), it returns
    /// fixed values. For dynamic targets (ongoing sunrise/sunset), it tracks the current
    /// transition progress to create smooth animation.
    ///
    /// Note: This is used only for animation targeting during the startup transition.
    /// The final state applied after startup completion is always the originally captured
    /// state to prevent timing-related issues.
    ///
    /// # Arguments
    /// * `config` - Configuration containing temperature and gamma ranges
    ///
    /// # Returns
    /// Tuple of (target_temperature, target_gamma) for the current animation frame
    fn calculate_current_target(&self, config: &Config) -> (u32, f32) {
        match self.initial_state {
            TransitionState::Stable(TimeState::Day) => {
                // Target is day values, simple case
                (
                    config
                        .day_temp
                        .unwrap_or(crate::constants::DEFAULT_DAY_TEMP),
                    config
                        .day_gamma
                        .unwrap_or(crate::constants::DEFAULT_DAY_GAMMA),
                )
            }
            TransitionState::Stable(TimeState::Night) => {
                // Target is night values, simple case
                (
                    config
                        .night_temp
                        .unwrap_or(crate::constants::DEFAULT_NIGHT_TEMP),
                    config
                        .night_gamma
                        .unwrap_or(crate::constants::DEFAULT_NIGHT_GAMMA),
                )
            }
            TransitionState::Transitioning {
                from,
                to,
                progress: initial_progress,
            } => {
                // Complex case: target is itself changing

                // If we're in a dynamic transition, recalculate where we should be now
                if self.is_dynamic_target {
                    // Get the current transition state to see if it's still progressing
                    let current_state = get_transition_state(config);

                    // If we're still in a transition of the same type, use its current progress
                    if let TransitionState::Transitioning {
                        from: current_from,
                        to: current_to,
                        progress: current_progress,
                    } = current_state
                    {
                        if current_from == from && current_to == to {
                            // We're still in the same transition, use current progress
                            let day_temp = config
                                .day_temp
                                .unwrap_or(crate::constants::DEFAULT_DAY_TEMP);
                            let night_temp = config
                                .night_temp
                                .unwrap_or(crate::constants::DEFAULT_NIGHT_TEMP);
                            let day_gamma = config
                                .day_gamma
                                .unwrap_or(crate::constants::DEFAULT_DAY_GAMMA);
                            let night_gamma = config
                                .night_gamma
                                .unwrap_or(crate::constants::DEFAULT_NIGHT_GAMMA);

                            match (from, to) {
                                (TimeState::Day, TimeState::Night) => {
                                    // Transitioning from day to night (sunset)
                                    let temp =
                                        interpolate_u32(day_temp, night_temp, current_progress);
                                    let gamma =
                                        interpolate_f32(day_gamma, night_gamma, current_progress);
                                    return (temp, gamma);
                                }
                                (TimeState::Night, TimeState::Day) => {
                                    // Transitioning from night to day (sunrise)
                                    let temp =
                                        interpolate_u32(night_temp, day_temp, current_progress);
                                    let gamma =
                                        interpolate_f32(night_gamma, day_gamma, current_progress);
                                    return (temp, gamma);
                                }
                                _ => (), // Fall through to static calculation
                            }
                        }
                    }
                }

                // If we're not in a dynamic transition or the transition changed,
                // calculate based on the initial progress (static target)
                let day_temp = config
                    .day_temp
                    .unwrap_or(crate::constants::DEFAULT_DAY_TEMP);
                let night_temp = config
                    .night_temp
                    .unwrap_or(crate::constants::DEFAULT_NIGHT_TEMP);
                let day_gamma = config
                    .day_gamma
                    .unwrap_or(crate::constants::DEFAULT_DAY_GAMMA);
                let night_gamma = config
                    .night_gamma
                    .unwrap_or(crate::constants::DEFAULT_NIGHT_GAMMA);

                match (from, to) {
                    (TimeState::Day, TimeState::Night) => {
                        // Transitioning from day to night (sunset)
                        let temp = interpolate_u32(day_temp, night_temp, initial_progress);
                        let gamma = interpolate_f32(day_gamma, night_gamma, initial_progress);
                        (temp, gamma)
                    }
                    (TimeState::Night, TimeState::Day) => {
                        // Transitioning from night to day (sunrise)
                        let temp = interpolate_u32(night_temp, day_temp, initial_progress);
                        let gamma = interpolate_f32(night_gamma, day_gamma, initial_progress);
                        (temp, gamma)
                    }
                    _ => (self.start_temp, self.start_gamma), // Fallback for edge cases
                }
            }
        }
    }

    /// Draw an animated progress bar showing the current transition progress.
    ///
    /// Creates a visual progress indicator with live temperature and gamma values.
    /// Only redraws when the percentage changes to avoid flickering.
    ///
    /// # Arguments
    /// * `progress` - Current progress (0.0 to 1.0)
    /// * `current_temp` - Current temperature value being applied
    /// * `current_gamma` - Current gamma value being applied
    fn draw_progress_bar(&mut self, progress: f32, current_temp: u32, current_gamma: f32) {
        let percentage = (progress * 100.0) as usize;

        // Only redraw if percentage changed to prevent flickering
        if self.last_progress_pct == Some(percentage) && percentage < 100 {
            return;
        }

        let filled = (PROGRESS_BAR_WIDTH as f32 * progress) as usize;
        let empty = PROGRESS_BAR_WIDTH - filled;

        // Create progress bar string with proper styling
        let bar = if filled > 0 {
            format!(
                "{}>{}",
                "=".repeat(filled.saturating_sub(1)),
                " ".repeat(empty)
            )
        } else {
            " ".repeat(PROGRESS_BAR_WIDTH)
        };

        // Print progress line with live values
        print!(
            "\r\x1B[K┃[{}] {}% (temp: {}K, gamma: {:.1}%)",
            bar, percentage, current_temp, current_gamma
        );
        io::stdout().flush().ok();

        self.last_progress_pct = Some(percentage);
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
        if self.start_temp == initial_target_temp
            && self.start_gamma == initial_target_gamma
            && !self.is_dynamic_target
        {
            // Apply the originally captured state to maintain timing consistency
            // Even when no transition is needed, we should use the captured state
            // rather than recalculating, to avoid potential timing-related state changes
            backend.apply_startup_state(self.initial_state, config, running)?;

            return Ok(());
        }

        let transition_type = if self.is_dynamic_target {
            "with dynamic target tracking"
        } else {
            "to target values"
        };

        // Print this with the normal logger before disabling it
        Log::log_pipe();
        Log::log_decorated(&format!(
            "Starting smooth transition {} ({}s)",
            transition_type,
            self.duration.as_secs()
        ));

        // Disable logging during the transition to prevent interference with the progress bar
        Log::set_enabled(false);

        // Calculate the update interval - aim for smoother transitions
        let update_interval = Duration::from_millis(DEFAULT_STARTUP_UPDATE_INTERVAL_MS);

        // Add a blank line before the progress bar for spacing
        {
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "┃").ok();
            stdout.flush().ok();
        }

        // Loop until transition completes or program stops
        let mut last_update = Instant::now();
        while running.load(Ordering::SeqCst) {
            let now = Instant::now();
            let elapsed = now.duration_since(self.start_time);

            // Calculate progress (0.0 to 1.0)
            let progress = (elapsed.as_secs_f32() / self.duration.as_secs_f32()).min(1.0);

            // Calculate current target (this may change if we're in a dynamic transition)
            let (target_temp, target_gamma) = self.calculate_current_target(config);

            // Calculate current interpolated values
            let current_temp = interpolate_u32(self.start_temp, target_temp, progress);
            let current_gamma = interpolate_f32(self.start_gamma, target_gamma, progress);

            // Draw the progress bar instead of logging each step
            self.draw_progress_bar(progress, current_temp, current_gamma);

            // Apply interpolated values
            if backend
                .apply_temperature_gamma(current_temp, current_gamma, running)
                .is_err()
            {
                Log::log_warning(
                    "Failed to apply temperature/gamma during startup transition. \
                    Will attempt to set final state after transition.",
                );
                // Don't abort the whole transition, just log and continue
                // The final state will be attempted later
            }

            // Break if we've reached 100%
            if progress >= 1.0 {
                break;
            }

            // Sleep until next update, respecting the update interval
            let time_since_last_update = now.duration_since(last_update);
            if time_since_last_update < update_interval {
                thread::sleep(update_interval - time_since_last_update);
            }
            last_update = Instant::now();
        }

        // Add proper newline and spacing after progress bar completion
        {
            let mut stdout = io::stdout().lock();
            writeln!(stdout).ok();
            writeln!(stdout, "┃").ok();
            stdout.flush().ok();
        }

        // Re-enable logging
        Log::set_enabled(true);

        // Log the completion message using the logger
        Log::log_decorated("Startup transition complete");

        // Apply the originally captured state instead of recalculating it
        //
        // IMPORTANT: We must use the state that was captured when the program started,
        // not recalculate it after the startup transition completes. This prevents a
        // timing bug where starting near transition boundaries could cause the program
        // to jump to the wrong state (e.g., starting during a sunset transition but
        // ending up in night mode because 10 seconds passed during startup).
        backend.apply_startup_state(self.initial_state, config, running)?;

        Ok(())
    }
}
