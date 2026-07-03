//! Per-iteration state tracking for the main loop.

use chrono::{DateTime, Local};

use crate::core::period::Period;

#[derive(Debug)]
pub(super) struct Context {
    last_update_time: Option<DateTime<Local>>,
    previous_progress: Option<f32>,
    previous_period: Option<Period>,
    first_transition_logged: bool,
    is_first_iteration: bool,
    config_reload_pending: bool,
    sleeping_to_boundary: bool,
}

impl Context {
    pub(super) fn new() -> Self {
        Self {
            last_update_time: None,
            previous_progress: None,
            previous_period: None,
            first_transition_logged: false,
            is_first_iteration: true,
            config_reload_pending: false,
            sleeping_to_boundary: false,
        }
    }

    pub(super) fn record_state_update(&mut self) {
        self.last_update_time = Some(crate::time::source::now());
        #[cfg(debug_assertions)]
        eprintln!(
            "DEBUG [Context]: Recorded state update at {:?}",
            self.last_update_time.as_ref().unwrap()
        );
    }

    /// Flags the next iteration to be skipped so a stable-period config reload
    /// does not emit a duplicate event.
    pub(super) fn record_config_reload(&mut self) {
        self.last_update_time = Some(crate::time::source::now());
        self.config_reload_pending = true;
        self.previous_progress = None;
        self.first_transition_logged = false;
        #[cfg(debug_assertions)]
        eprintln!("DEBUG [Context]: Recorded config reload for stable period, pending skip = true");
    }

    pub(super) fn should_update_during_transition(&self, update_interval_secs: u64) -> bool {
        match self.last_update_time {
            None => true,
            Some(last) => {
                let now = crate::time::source::now();
                let elapsed = now.signed_duration_since(last);
                elapsed.num_seconds() >= update_interval_secs as i64
            }
        }
    }

    pub(super) fn should_log_progress(&self, period: Period, state_was_just_applied: bool) -> bool {
        period.is_transitioning() && (state_was_just_applied || self.last_update_time.is_none())
    }

    pub(super) fn update_progress(&mut self, progress: Option<f32>) {
        if let Some(p) = progress {
            self.previous_progress = Some(p);
        }
    }

    pub(super) fn reset_for_stable_period(&mut self) {
        self.previous_progress = None;
        self.first_transition_logged = false;
    }

    pub(super) fn handle_first_iteration(&mut self) -> bool {
        if self.is_first_iteration {
            self.is_first_iteration = false;
            true
        } else {
            false
        }
    }

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

    pub(super) fn is_period_change(&self, current: Period) -> bool {
        self.previous_period.is_some_and(|prev| prev != current)
    }

    pub(super) fn record_current_period(&mut self, period: Period) {
        self.previous_period = Some(period);
    }

    pub(super) fn previous_progress(&self) -> Option<f32> {
        self.previous_progress
    }

    pub(super) fn previous_period(&self) -> Option<Period> {
        self.previous_period
    }

    pub(super) fn has_recorded_updates(&self) -> bool {
        self.last_update_time.is_some()
    }

    pub(super) fn first_transition_logged(&self) -> bool {
        self.first_transition_logged
    }

    pub(super) fn set_first_transition_logged(&mut self, value: bool) {
        self.first_transition_logged = value;
    }

    /// Whether the previous sleep landed exactly on a transition boundary. When
    /// true, the next iteration force advances to the next period.
    pub(super) fn slept_to_transition_boundary(&self) -> bool {
        self.sleeping_to_boundary
    }

    /// Set to true when a sleep will land exactly on a transition boundary, and
    /// back to false after the forced transition.
    pub(super) fn set_sleeping_to_boundary(&mut self, value: bool) {
        self.sleeping_to_boundary = value;
        #[cfg(debug_assertions)]
        eprintln!("DEBUG [Context]: Set sleeping_to_boundary = {}", value);
    }
}
