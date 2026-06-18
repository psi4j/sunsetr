//! Runtime state management with execution context.
//!
//! This module provides the RuntimeState struct which represents the primary
//! state of the application, combining a Period with the necessary context
//! (config, schedule, current_time) to perform all runtime calculations.

use anyhow::Context;
use chrono::{DateTime, Local};
use std::fmt;

use crate::common::constants::{
    DEFAULT_DAY_GAMMA, DEFAULT_DAY_TEMP, DEFAULT_NIGHT_GAMMA, DEFAULT_NIGHT_TEMP,
    DEFAULT_UPDATE_INTERVAL,
};
use crate::common::utils::{interpolate_f64, interpolate_inverse_u32};
use crate::config::Config;
use crate::core::period::Period;
use crate::core::schedule::Schedule;
use crate::geo::times::GeoTimes;

/// Core application runtime state with execution context.
///
/// This struct represents the primary state of the application, combining a Period
/// with the necessary context (config, schedule, current_time) to perform all
/// runtime calculations. This is the working state used throughout the application.
///
/// Uses owned data for simplicity and to avoid lifetime management complexity.
#[derive(Debug, Clone)]
pub struct RuntimeState {
    pub period: Period,
    pub config: Config,
    pub schedule: Option<Schedule>,
    pub current_time: DateTime<Local>,
}

impl RuntimeState {
    /// Create a new RuntimeState with execution context
    pub fn new(
        period: Period,
        config: &Config,
        schedule: Option<Schedule>,
        current_time: DateTime<Local>,
    ) -> Self {
        Self {
            period,
            config: config.clone(),
            schedule,
            current_time,
        }
    }

    /// Calculate temperature for this period with context
    pub fn temperature(&self) -> u32 {
        match self.period {
            Period::Day => self.config.day_temp.unwrap_or(DEFAULT_DAY_TEMP),
            Period::Night => self.config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP),
            Period::Static => self.config.static_temp.unwrap_or(DEFAULT_DAY_TEMP),
            Period::Sunset => {
                let progress = self.progress().unwrap_or(0.0);
                let day_temp = self.config.day_temp.unwrap_or(DEFAULT_DAY_TEMP);
                let night_temp = self.config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP);
                interpolate_inverse_u32(day_temp, night_temp, progress)
            }
            Period::Sunrise => {
                let progress = self.progress().unwrap_or(0.0);
                let day_temp = self.config.day_temp.unwrap_or(DEFAULT_DAY_TEMP);
                let night_temp = self.config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP);
                interpolate_inverse_u32(night_temp, day_temp, progress)
            }
        }
    }

    /// Calculate gamma for this period with context
    pub fn gamma(&self) -> f64 {
        match self.period {
            Period::Day => self.config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA),
            Period::Night => self.config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA),
            Period::Static => self.config.static_gamma.unwrap_or(DEFAULT_DAY_GAMMA),
            Period::Sunset => {
                let progress = self.progress().unwrap_or(0.0);
                let day_gamma = self.config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA);
                let night_gamma = self.config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA);
                interpolate_f64(day_gamma, night_gamma, progress)
            }
            Period::Sunrise => {
                let progress = self.progress().unwrap_or(0.0);
                let day_gamma = self.config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA);
                let night_gamma = self.config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA);
                interpolate_f64(night_gamma, day_gamma, progress)
            }
        }
    }

    /// Get both temperature and gamma values
    pub fn values(&self) -> (u32, f64) {
        (self.temperature(), self.gamma())
    }

    /// Transition progress for the current period, None when stable or static.
    pub fn progress(&self) -> Option<f32> {
        self.schedule
            .as_ref()
            .and_then(|schedule| schedule.progress(self.period, self.current_time))
    }

    /// Updated RuntimeState for the current instant, recalculating a geo
    /// schedule across a day boundary when due.
    pub fn with_current_period(&self) -> (RuntimeState, crate::core::period::StateChange) {
        let now = crate::time::source::now();

        let updated_schedule = match &self.schedule {
            Some(Schedule::Geo(times)) if times.needs_recalculation(now) => {
                if let (Some(lat), Some(lon)) = (self.config.latitude, self.config.longitude) {
                    let mut new_times = times.clone();
                    if new_times.recalculate_for_next_period(lat, lon).is_ok() {
                        Some(Schedule::Geo(new_times))
                    } else {
                        self.schedule.clone()
                    }
                } else {
                    self.schedule.clone()
                }
            }
            _ => self.schedule.clone(),
        };

        let new_period = updated_schedule
            .as_ref()
            .map_or(Period::Static, |schedule| schedule.current_period(now));
        let change = crate::core::period::should_update_state(&self.period, &new_period);

        let new_state = RuntimeState::new(new_period, &self.config, updated_schedule, now);

        (new_state, change)
    }

    /// Create updated RuntimeState by advancing to the next expected period.
    ///
    /// This method is used when we've slept to a transition boundary and need to
    /// force advance to the next period WITHOUT rechecking wall clock time.
    /// This prevents timing race conditions at transition boundaries.
    ///
    /// The period progression follows the natural cycle:
    /// - Day -> Sunset -> Night -> Sunrise -> Day
    /// - Static -> Static (never changes)
    ///
    /// # Returns
    /// Tuple of (new RuntimeState with next period, StateChange indicating what happened)
    pub fn with_next_period(&self) -> (RuntimeState, crate::core::period::StateChange) {
        let next_period = match self.period {
            Period::Day => Period::Sunset,
            Period::Sunset => Period::Night,
            Period::Night => Period::Sunrise,
            Period::Sunrise => Period::Day,
            Period::Static => Period::Static,
        };

        let change = crate::core::period::should_update_state(&self.period, &next_period);

        let new_state = RuntimeState::new(
            next_period,
            &self.config,
            self.schedule.clone(),
            crate::time::source::now(),
        );

        #[cfg(debug_assertions)]
        eprintln!(
            "DEBUG [RuntimeState]: Forced transition from {:?} to {:?}, change: {:?}",
            self.period, next_period, change
        );

        (new_state, change)
    }

    /// Create RuntimeState with new config (handles geo_times updates automatically)
    ///
    /// Returns Result to preserve current error handling behavior where invalid
    /// coordinates during config reload are treated as critical failures.
    pub fn with_config(&self, new_config: &Config) -> anyhow::Result<RuntimeState> {
        let updated_geo_times = if new_config.transition_mode.as_deref() == Some("geo") {
            if let (Some(lat), Some(lon)) = (new_config.latitude, new_config.longitude) {
                if let Some(current_times) = self.geo_times() {
                    let mut new_times = current_times.clone();
                    if new_times.handle_location_change(lat, lon).is_ok() {
                        Some(new_times)
                    } else {
                        Some(
                            crate::geo::times::GeoTimes::from_config(new_config)
                                .context(
                                    "Solar calculations failed after config reload - this is a bug",
                                )?
                                .ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "Config validation failed - missing coordinates"
                                    )
                                })?,
                        )
                    }
                } else {
                    Some(
                        crate::geo::times::GeoTimes::from_config(new_config)
                            .context(
                                "Solar calculations failed after config reload - this is a bug",
                            )?
                            .ok_or_else(|| {
                                anyhow::anyhow!("Config validation failed - missing coordinates")
                            })?,
                    )
                }
            } else {
                None
            }
        } else {
            None
        };

        let schedule = Schedule::from_config(new_config, updated_geo_times);
        let now = crate::time::source::now();
        let new_period = schedule
            .as_ref()
            .map_or(Period::Static, |schedule| schedule.current_period(now));

        Ok(RuntimeState::new(new_period, new_config, schedule, now))
    }

    /// Check if two RuntimeStates have same effective values (no transition needed)
    pub fn has_same_effective_values(&self, other: &RuntimeState) -> bool {
        let (temp1, gamma1) = self.values();
        let (temp2, gamma2) = other.values();
        temp1 == temp2 && (gamma1 - gamma2).abs() < 0.01
    }

    /// Time until next period change (replaces time_until_next_event)
    pub fn time_until_next_event(&self) -> std::time::Duration {
        crate::core::period::time_until_next_event(&self.config, self.geo_times())
    }

    /// Time until current transition ends (replaces time_until_transition_end)
    pub fn time_until_transition_end(&self) -> Option<std::time::Duration> {
        crate::core::period::time_until_transition_end(&self.config, self.geo_times())
    }

    /// Get the effective update interval in seconds for the current state.
    ///
    /// Dispatches on the `UpdateInterval` config variant:
    /// - `Fixed(secs)` returns the fixed value
    /// - `Adaptive` calculates the optimal interval based on the smoothstep derivative
    ///   and mired range at the current position in the transition
    /// - `None` defaults to Adaptive
    pub fn effective_update_interval_secs(&self) -> u64 {
        match &self.config.update_interval {
            Some(crate::config::UpdateInterval::Fixed(secs)) => *secs,
            Some(crate::config::UpdateInterval::Adaptive) | None => self
                .schedule
                .as_ref()
                .and_then(|schedule| {
                    schedule.adaptive_interval(&self.config, self.period, self.current_time)
                })
                .unwrap_or(DEFAULT_UPDATE_INTERVAL),
        }
    }

    // ACCESSOR METHODS FOR COMPATIBILITY AND INTEGRATION

    /// Access config for application lifecycle needs
    ///
    /// This provides read-only access to the config owned by RuntimeState.
    /// Idiomatic pattern: borrowing rather than cloning for efficiency.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Transition times when this is a geo schedule, else None.
    pub fn geo_times(&self) -> Option<&GeoTimes> {
        match &self.schedule {
            Some(Schedule::Geo(times)) => Some(times),
            _ => None,
        }
    }

    /// Access current period for compatibility with existing APIs
    ///
    /// Provides direct read access to the period field.
    pub fn period(&self) -> Period {
        self.period
    }

    /// Check if RuntimeState is in geo mode
    ///
    /// Convenience method for common conditional logic.
    pub fn is_geo_mode(&self) -> bool {
        self.config.transition_mode.as_deref() == Some("geo")
    }
}

/// Display implementation for RuntimeState (with progress information)
impl fmt::Display for RuntimeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.period {
            Period::Day => write!(f, "Day"),
            Period::Night => write!(f, "Night"),
            Period::Static => write!(f, "Static"),
            Period::Sunset => {
                if let Some(progress) = self.progress() {
                    write!(f, "Sunset ({:.1}%)", progress * 100.0)
                } else {
                    write!(f, "Sunset")
                }
            }
            Period::Sunrise => {
                if let Some(progress) = self.progress() {
                    write!(f, "Sunrise ({:.1}%)", progress * 100.0)
                } else {
                    write!(f, "Sunrise")
                }
            }
        }
    }
}
