//! Main loop context tracking.
//!
//! This module provides the `Context` struct which tracks iteration metadata
//! for the main application loop, including timing, display state, loop control,
//! and change detection.

use chrono::{DateTime, Local};
use std::time::Instant;

use crate::core::period::Period;

/// Centralized context tracking for the main loop.
///
/// This struct consolidates all iteration metadata including timing, display state,
/// loop control, and change detection to ensure consistent behavior across iterations.
#[derive(Debug)]
pub(super) struct Context {
    /// When we last applied state (for throttling updates)
    /// Uses DateTime to support both real and simulated time
    last_update_time: Option<DateTime<Local>>,

    /// When we last reloaded config (for debouncing duplicate reload signals)
    /// Uses monotonic time (Instant) for immunity to system time changes
    last_config_reload: Option<Instant>,

    /// Previous progress value (for display precision calculation)
    previous_progress: Option<f32>,

    /// Previous period (for detecting period changes)
    previous_period: Option<Period>,

    /// Whether we've logged the first transition message
    first_transition_logged: bool,

    /// Whether this is the first main loop iteration
    is_first_iteration: bool,

    /// Whether we're skipping due to config reload
    config_reload_pending: bool,

    /// Whether we just slept to a transition boundary and should force the transition
    /// This flag prevents timing race conditions at transition boundaries
    sleeping_to_boundary: bool,
}

impl Context {
    /// Create a new context for the start of the main loop.
    pub(super) fn new() -> Self {
        Self {
            last_update_time: None,
            last_config_reload: None, // Start with None for first reload
            previous_progress: None,
            previous_period: None,
            first_transition_logged: false,
            is_first_iteration: true,
            config_reload_pending: false,
            sleeping_to_boundary: false,
        }
    }

    /// Record that we just applied a state update.
    pub(super) fn record_state_update(&mut self) {
        self.last_update_time = Some(crate::time::source::now());
        #[cfg(debug_assertions)]
        eprintln!(
            "DEBUG [Context]: Recorded state update at {:?}",
            self.last_update_time.as_ref().unwrap()
        );
    }

    /// Record that we just reloaded config and applied state (for stable periods).
    /// This sets a flag to skip the next iteration to avoid duplicate events.
    pub(super) fn record_config_reload(&mut self) {
        // Set last_update_time to track when we updated
        self.last_update_time = Some(crate::time::source::now());
        self.config_reload_pending = true;
        // Reset progress tracking to prevent precision confusion
        self.previous_progress = None;
        self.first_transition_logged = false;
        #[cfg(debug_assertions)]
        eprintln!("DEBUG [Context]: Recorded config reload for stable period, pending skip = true");
    }

    /// Check if we should apply a state update during a transition.
    /// Uses simulation-aware time to support both real and simulated execution.
    pub(super) fn should_update_during_transition(&self, update_interval_secs: u64) -> bool {
        match self.last_update_time {
            None => true, // No updates yet - do the first one
            Some(last) => {
                let now = crate::time::source::now();
                let elapsed = now.signed_duration_since(last);
                elapsed.num_seconds() >= update_interval_secs as i64
            }
        }
    }

    /// Check if we should log progress for this iteration.
    pub(super) fn should_log_progress(&self, period: Period, state_was_just_applied: bool) -> bool {
        if period.is_transitioning() {
            // For transitions: only log when state was actually applied
            if state_was_just_applied {
                true
            } else if self.last_update_time.is_none() {
                // First time through after startup - log initial state
                true
            } else {
                false
            }
        } else {
            // For stable periods: always log
            true
        }
    }

    /// Update progress tracking for display precision.
    pub(super) fn update_progress(&mut self, progress: Option<f32>) {
        if let Some(p) = progress {
            self.previous_progress = Some(p);
        }
    }

    /// Reset tracking when entering a stable period.
    pub(super) fn reset_for_stable_period(&mut self) {
        self.previous_progress = None;
        self.first_transition_logged = false;
    }

    /// Handle the first iteration of the main loop.
    pub(super) fn handle_first_iteration(&mut self) -> bool {
        if self.is_first_iteration {
            self.is_first_iteration = false;
            // Skip state update on first iteration to prevent false detections
            true
        } else {
            false
        }
    }

    /// Handle config reload skip.
    pub(super) fn handle_config_reload_skip(&mut self) -> bool {
        if self.config_reload_pending {
            self.config_reload_pending = false;
            #[cfg(debug_assertions)]
            eprintln!("DEBUG [Context]: Handling config reload skip, clearing pending flag");
            true
        } else {
            false
        }
    }

    /// Check if this is a period change.
    pub(super) fn is_period_change(&self, current: Period) -> bool {
        self.previous_period.is_some_and(|prev| prev != current)
    }

    /// Check if we should debounce a config reload request.
    /// Returns true if the reload happened too recently and should be ignored.
    /// Uses monotonic time for immunity to system time changes.
    pub(super) fn should_debounce_reload(&self, threshold_ms: u64) -> bool {
        match self.last_config_reload {
            Some(last) => last.elapsed().as_millis() < threshold_ms as u128,
            None => false, // First reload, don't debounce
        }
    }

    /// Record that we just processed a config reload.
    pub(super) fn record_reload_processed(&mut self) {
        self.last_config_reload = Some(Instant::now());
    }

    /// Record the current period for the next iteration's change detection.
    /// This should be called at the end of each main loop iteration.
    pub(super) fn record_current_period(&mut self, period: Period) {
        self.previous_period = Some(period);
    }

    // Getters for read-only field access

    /// Check if this is the first iteration of the main loop.
    pub(super) fn is_first_iteration(&self) -> bool {
        self.is_first_iteration
    }

    /// Get the previous progress value for display formatting.
    pub(super) fn previous_progress(&self) -> Option<f32> {
        self.previous_progress
    }

    /// Get the previous period for logging or comparison.
    pub(super) fn previous_period(&self) -> Option<Period> {
        self.previous_period
    }

    /// Check if we have recorded any state updates yet.
    pub(super) fn has_recorded_updates(&self) -> bool {
        self.last_update_time.is_some()
    }

    /// Check if the first transition has been logged.
    pub(super) fn first_transition_logged(&self) -> bool {
        self.first_transition_logged
    }

    /// Set the first transition logged flag.
    /// This is used to control spacing of transition progress logs.
    pub(super) fn set_first_transition_logged(&mut self, value: bool) {
        self.first_transition_logged = value;
    }

    /// Check if we just slept to a transition boundary.
    /// If true, the next iteration should force advance to the next period.
    pub(super) fn slept_to_transition_boundary(&self) -> bool {
        self.sleeping_to_boundary
    }

    /// Set the sleeping_to_boundary flag.
    /// This should be set to true when we calculate that we're sleeping exactly
    /// to a transition boundary, and cleared after forcing the transition.
    pub(super) fn set_sleeping_to_boundary(&mut self, value: bool) {
        self.sleeping_to_boundary = value;
        #[cfg(debug_assertions)]
        eprintln!("DEBUG [Context]: Set sleeping_to_boundary = {}", value);
    }
}
