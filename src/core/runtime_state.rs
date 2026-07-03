//! Runtime state management with execution context.

use anyhow::Context;
use chrono::{DateTime, Local};
use std::fmt;

use crate::common::constants::{DEFAULT_DAY_GAMMA, DEFAULT_DAY_TEMP, DEFAULT_UPDATE_INTERVAL_SEC};
use crate::common::utils::{interpolate_f64, interpolate_inverse_u32};
use crate::config::{Config, TransitionMode};
use crate::core::period::Period;
use crate::core::schedule::Schedule;
use crate::geo::times::GeoTimes;

/// The primary application state, pairing a Period with the context (config,
/// schedule, current_time) needed for all runtime calculations.
///
/// Uses owned data to avoid lifetime-management complexity.
#[derive(Debug, Clone)]
pub struct RuntimeState {
    period: Period,
    config: Config,
    schedule: Option<Schedule>,
    current_time: DateTime<Local>,
}

impl RuntimeState {
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

    pub fn temperature(&self) -> u32 {
        match self.period {
            Period::Day => self.config.day_temp,
            Period::Night => self.config.night_temp,
            Period::Static => self.config.static_temp.unwrap_or(DEFAULT_DAY_TEMP),
            Period::Sunset => {
                let progress = self.progress().unwrap_or(0.0);
                interpolate_inverse_u32(self.config.day_temp, self.config.night_temp, progress)
            }
            Period::Sunrise => {
                let progress = self.progress().unwrap_or(0.0);
                interpolate_inverse_u32(self.config.night_temp, self.config.day_temp, progress)
            }
        }
    }

    pub fn gamma(&self) -> f64 {
        match self.period {
            Period::Day => self.config.day_gamma,
            Period::Night => self.config.night_gamma,
            Period::Static => self.config.static_gamma.unwrap_or(DEFAULT_DAY_GAMMA),
            Period::Sunset => {
                let progress = self.progress().unwrap_or(0.0);
                interpolate_f64(self.config.day_gamma, self.config.night_gamma, progress)
            }
            Period::Sunrise => {
                let progress = self.progress().unwrap_or(0.0);
                interpolate_f64(self.config.night_gamma, self.config.day_gamma, progress)
            }
        }
    }

    pub fn values(&self) -> (u32, f64) {
        (self.temperature(), self.gamma())
    }

    /// Progress through the current transitioning period, None when stable or
    /// static.
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

    /// Advance to the next expected period without rechecking the wall clock.
    ///
    /// Used after sleeping to the end of a transitioning period. Forcing the
    /// next period here avoids a timing race that a fresh wall-clock read could
    /// hit at the boundary.
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

    /// Build a RuntimeState from a new config, recomputing geo times as needed.
    ///
    /// Invalid coordinates or a failed solar recalculation during a geo-mode
    /// reload return Err. These are treated as critical failures rather than
    /// silently falling back.
    pub fn with_config(&self, new_config: &Config) -> anyhow::Result<RuntimeState> {
        let updated_geo_times = if new_config.transition_mode == TransitionMode::Geo {
            if let (Some(lat), Some(lon)) = (new_config.latitude, new_config.longitude) {
                if let Some(current_times) = self.geo_times() {
                    let mut new_times = current_times.clone();
                    if new_times.handle_location_change(lat, lon).is_ok() {
                        Some(new_times)
                    } else {
                        Some(
                            crate::geo::times::GeoTimes::from_config(new_config)
                                .context(
                                    "Solar calculations failed after config reload (this is a bug)",
                                )?
                                .ok_or_else(|| {
                                    anyhow::anyhow!("Config validation failed: missing coordinates")
                                })?,
                        )
                    }
                } else {
                    Some(
                        crate::geo::times::GeoTimes::from_config(new_config)
                            .context(
                                "Solar calculations failed after config reload (this is a bug)",
                            )?
                            .ok_or_else(|| {
                                anyhow::anyhow!("Config validation failed: missing coordinates")
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

    pub fn has_same_effective_values(&self, other: &RuntimeState) -> bool {
        let (temp1, gamma1) = self.values();
        let (temp2, gamma2) = other.values();
        temp1 == temp2 && (gamma1 - gamma2).abs() < 0.01
    }

    /// Time until the next state change the main loop must wake for, or
    /// `Duration::MAX` in static mode.
    pub fn time_until_next_event(&self) -> std::time::Duration {
        self.schedule
            .as_ref()
            .map_or(std::time::Duration::MAX, |schedule| {
                schedule.time_until_next_event(&self.config, self.period, self.current_time)
            })
    }

    /// Time until the current transitioning period ends, or None when not
    /// transitioning.
    pub fn time_until_transition_end(&self) -> Option<std::time::Duration> {
        self.schedule
            .as_ref()
            .and_then(|schedule| schedule.time_until_transition_end(self.current_time))
    }

    /// Absolute start of the next period, or None in static mode.
    pub fn next_period_start(&self) -> Option<DateTime<Local>> {
        self.schedule
            .as_ref()
            .and_then(|schedule| schedule.next_period_start(self.period, self.current_time))
    }

    pub fn effective_update_interval_secs(&self) -> u64 {
        match &self.config.update_interval {
            crate::config::UpdateInterval::Fixed(secs) => *secs,
            crate::config::UpdateInterval::Adaptive => self
                .schedule
                .as_ref()
                .and_then(|schedule| {
                    schedule.adaptive_interval(&self.config, self.period, self.current_time)
                })
                .unwrap_or(DEFAULT_UPDATE_INTERVAL_SEC),
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The geo schedule's solar times, or None for any other schedule.
    pub fn geo_times(&self) -> Option<&GeoTimes> {
        match &self.schedule {
            Some(Schedule::Geo(times)) => Some(times),
            _ => None,
        }
    }

    pub fn period(&self) -> Period {
        self.period
    }

    pub fn is_geo_mode(&self) -> bool {
        matches!(self.schedule, Some(Schedule::Geo(_)))
    }
}

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
