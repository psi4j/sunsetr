//! Runtime state management with execution context.
//!
//! This module provides the RuntimeState struct which represents the primary
//! state of the application, combining a Period with the necessary context
//! (config, geo_times, current_time) to perform all runtime calculations.

use chrono::NaiveTime;
use std::fmt;

use crate::common::constants::{
    DEFAULT_DAY_GAMMA, DEFAULT_DAY_TEMP, DEFAULT_NIGHT_GAMMA, DEFAULT_NIGHT_TEMP,
};
use crate::common::utils::{interpolate_f32, interpolate_u32};
use crate::config::Config;
use crate::core::period::{
    Period, calculate_sunrise_progress_for_period, calculate_sunset_progress_for_period,
};
use crate::geo::times::GeoTimes;

/// Core application runtime state with execution context.
///
/// This struct represents the primary state of the application, combining a Period
/// with the necessary context (config, geo_times, current_time) to perform all
/// runtime calculations. This is the working state used throughout the application.
///
/// Uses owned data for simplicity and to avoid lifetime management complexity.
#[derive(Debug, Clone)]
pub struct RuntimeState {
    pub period: Period,
    pub config: Config,              // ← Owned data (cloned)
    pub geo_times: Option<GeoTimes>, // ← Owned data (cloned)
    pub current_time: NaiveTime,     // ← Copy type
}

impl RuntimeState {
    /// Create a new RuntimeState with execution context
    pub fn new(
        period: Period,
        config: &Config, // ← Borrow to clone from
        geo_times: Option<&GeoTimes>,
        current_time: NaiveTime,
    ) -> Self {
        Self {
            period,
            config: config.clone(),
            geo_times: geo_times.cloned(),
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
                interpolate_u32(day_temp, night_temp, progress)
            }
            Period::Sunrise => {
                let progress = self.progress().unwrap_or(0.0);
                let day_temp = self.config.day_temp.unwrap_or(DEFAULT_DAY_TEMP);
                let night_temp = self.config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP);
                interpolate_u32(night_temp, day_temp, progress)
            }
        }
    }

    /// Calculate gamma for this period with context
    pub fn gamma(&self) -> f32 {
        match self.period {
            Period::Day => self.config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA),
            Period::Night => self.config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA),
            Period::Static => self.config.static_gamma.unwrap_or(DEFAULT_DAY_GAMMA),
            Period::Sunset => {
                let progress = self.progress().unwrap_or(0.0);
                let day_gamma = self.config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA);
                let night_gamma = self.config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA);
                interpolate_f32(day_gamma, night_gamma, progress)
            }
            Period::Sunrise => {
                let progress = self.progress().unwrap_or(0.0);
                let day_gamma = self.config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA);
                let night_gamma = self.config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA);
                interpolate_f32(night_gamma, day_gamma, progress)
            }
        }
    }

    /// Get both temperature and gamma values
    pub fn values(&self) -> (u32, f32) {
        (self.temperature(), self.gamma())
    }

    /// Get progress for transitioning periods
    /// Returns the same progress that would have been stored in the original enum
    pub fn progress(&self) -> Option<f32> {
        match self.period {
            Period::Sunset => calculate_sunset_progress_for_period(
                self.current_time,
                &self.config,
                self.geo_times.as_ref(),
            ),
            Period::Sunrise => calculate_sunrise_progress_for_period(
                self.current_time,
                &self.config,
                self.geo_times.as_ref(),
            ),
            _ => None,
        }
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
