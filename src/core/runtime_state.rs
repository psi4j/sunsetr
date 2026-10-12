//! Runtime state management with execution context.

use anyhow::Context;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::common::constants::{
    DEFAULT_DAY_GAMMA, DEFAULT_DAY_TEMP, DEFAULT_UPDATE_INTERVAL_SEC, MAXIMUM_GAMMA, MAXIMUM_TEMP,
    MINIMUM_GAMMA, MINIMUM_TEMP,
};
use crate::common::utils::{interpolate_f64, interpolate_inverse_u32};
use crate::config::{Config, TransitionMode};
use crate::core::period::Period;
use crate::core::schedule::Schedule;
use crate::geo::times::GeoTimes;

/// The transient adjustment of one value. Offset is added to the scheduled
/// value each tick, so the display follows the schedule. Hold replaces the
/// scheduled value until reset. Held by the running instance only, never
/// written to a config.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldAdjustment<T> {
    Offset(T),
    Hold(T),
}

impl FieldAdjustment<i32> {
    pub fn is_zero(&self) -> bool {
        *self == FieldAdjustment::Offset(0)
    }
}

impl FieldAdjustment<f64> {
    /// Zero within 0.05 percent, matching the one-decimal display precision,
    /// so float residue from accumulated deltas counts as zero and an offset
    /// too small to display is not reported as set.
    pub fn is_zero(&self) -> bool {
        matches!(self, FieldAdjustment::Offset(offset) if offset.abs() < 0.05)
    }
}

impl fmt::Display for FieldAdjustment<i32> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldAdjustment::Offset(offset) => write!(f, "{offset:+}K"),
            FieldAdjustment::Hold(value) => write!(f, "={value}K"),
        }
    }
}

impl fmt::Display for FieldAdjustment<f64> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldAdjustment::Offset(offset) => write!(f, "{offset:+.1}%"),
            FieldAdjustment::Hold(value) => write!(f, "={value:.1}%"),
        }
    }
}

/// The adjustment of the temperature (Kelvin) and gamma (percent).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Adjustment {
    pub temp: FieldAdjustment<i32>,
    pub gamma: FieldAdjustment<f64>,
}

impl Default for Adjustment {
    fn default() -> Self {
        Adjustment {
            temp: FieldAdjustment::Offset(0),
            gamma: FieldAdjustment::Offset(0.0),
        }
    }
}

impl Adjustment {
    pub fn is_zero(&self) -> bool {
        self.temp.is_zero() && self.gamma.is_zero()
    }

    /// The values that result from the adjustment of the scheduled values,
    /// clamped to the supported range.
    pub fn apply(&self, temp: u32, gamma: f64) -> (u32, f64) {
        let temp = match self.temp {
            FieldAdjustment::Offset(offset) => i64::from(temp) + i64::from(offset),
            FieldAdjustment::Hold(value) => i64::from(value),
        }
        .clamp(i64::from(MINIMUM_TEMP), i64::from(MAXIMUM_TEMP)) as u32;
        let gamma = match self.gamma {
            FieldAdjustment::Offset(offset) => gamma + offset,
            FieldAdjustment::Hold(value) => value,
        }
        .clamp(MINIMUM_GAMMA, MAXIMUM_GAMMA);
        (temp, gamma)
    }
}

impl fmt::Display for Adjustment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}, {}", self.temp, self.gamma)
    }
}

/// What one `adjust` field asks of the running instance. Assign carries the
/// value the field is held at and Delta the amount it moves by.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdjustOp<T> {
    Assign(T),
    Delta(T),
}

/// One `adjust` command as the instance receives it. `reset` discards the
/// adjustment held so far before the fields are applied. `is_static` is the
/// `--static` flag.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct AdjustRequest {
    pub reset: bool,
    pub is_static: bool,
    pub temp: Option<AdjustOp<i32>>,
    pub gamma: Option<AdjustOp<f64>>,
}

impl AdjustRequest {
    /// The adjustment that results from this request on top of `current`,
    /// where `applied` holds the values `current` produces now.
    ///
    /// A hold is sticky. A Delta on a held value moves the held value whether
    /// or not the request is static, and only `reset` returns the field to an
    /// offset. A static Delta on an offset holds the applied value moved by
    /// the delta, so the display moves by exactly the amount given. An Assign
    /// always holds. A static request with no fields holds both values as
    /// applied.
    pub fn resolve(&self, current: Adjustment, applied: (u32, f64)) -> Adjustment {
        let base = if self.reset {
            Adjustment::default()
        } else {
            current
        };
        let (temp_op, gamma_op) = if self.is_static && self.temp.is_none() && self.gamma.is_none() {
            (Some(AdjustOp::Delta(0)), Some(AdjustOp::Delta(0.0)))
        } else {
            (self.temp, self.gamma)
        };
        let temp = temp_op.map_or(base.temp, |op| {
            resolve_field(base.temp, op, self.is_static, applied.0 as i32, |a, b| {
                a.saturating_add(b)
            })
        });
        let gamma = gamma_op.map_or(base.gamma, |op| {
            resolve_field(base.gamma, op, self.is_static, applied.1, |a, b| a + b)
        });
        Adjustment { temp, gamma }
    }
}

fn resolve_field<T: Copy>(
    current: FieldAdjustment<T>,
    op: AdjustOp<T>,
    is_static: bool,
    applied: T,
    add: impl Fn(T, T) -> T,
) -> FieldAdjustment<T> {
    match (op, current) {
        (AdjustOp::Assign(value), _) => FieldAdjustment::Hold(value),
        (AdjustOp::Delta(delta), FieldAdjustment::Hold(held)) => {
            FieldAdjustment::Hold(add(held, delta))
        }
        (AdjustOp::Delta(delta), FieldAdjustment::Offset(_)) if is_static => {
            FieldAdjustment::Hold(add(applied, delta))
        }
        (AdjustOp::Delta(delta), FieldAdjustment::Offset(offset)) => {
            FieldAdjustment::Offset(add(offset, delta))
        }
    }
}

/// The primary application state, pairing a Period with the context (config,
/// schedule, current_time) needed for all runtime calculations.
///
/// Uses owned data to avoid lifetime-management complexity. The adjustment
/// survives `with_current_period`, `with_next_period` and `with_config`, and
/// is replaced only through `with_adjustment`.
#[derive(Debug, Clone)]
pub struct RuntimeState {
    period: Period,
    config: Config,
    schedule: Option<Schedule>,
    current_time: DateTime<Local>,
    adjustment: Adjustment,
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
            adjustment: Adjustment::default(),
        }
    }

    pub fn with_adjustment(&self, adjustment: Adjustment) -> RuntimeState {
        RuntimeState {
            adjustment,
            ..self.clone()
        }
    }

    pub fn adjustment(&self) -> Adjustment {
        self.adjustment
    }

    /// The scheduled temperature plus the adjustment, clamped to the supported
    /// range.
    pub fn temperature(&self) -> u32 {
        self.adjustment
            .apply(self.base_temperature(), self.base_gamma())
            .0
    }

    /// The scheduled gamma plus the adjustment, clamped to the supported range.
    pub fn gamma(&self) -> f64 {
        self.adjustment
            .apply(self.base_temperature(), self.base_gamma())
            .1
    }

    fn base_temperature(&self) -> u32 {
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

    fn base_gamma(&self) -> f64 {
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

        let new_state = RuntimeState {
            adjustment: self.adjustment,
            ..RuntimeState::new(new_period, &self.config, updated_schedule, now)
        };

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

        let new_state = RuntimeState {
            adjustment: self.adjustment,
            ..RuntimeState::new(
                next_period,
                &self.config,
                self.schedule.clone(),
                crate::time::source::now(),
            )
        };

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

        Ok(RuntimeState {
            adjustment: self.adjustment,
            ..RuntimeState::new(new_period, new_config, schedule, now)
        })
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

    pub fn transition_end_reached(&self) -> bool {
        self.schedule
            .as_ref()
            .and_then(|schedule| schedule.time_until_transition_end(crate::time::source::now()))
            .is_none()
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
