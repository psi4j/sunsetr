//! Runtime state management with execution context.
//!
//! This module provides the RuntimeState struct which represents the primary
//! state of the application, combining a Period with the necessary context
//! (config, geo_times, current_time) to perform all runtime calculations.

use anyhow::Context;
use chrono::NaiveTime;
use std::fmt;

use crate::common::constants::{
    DEFAULT_DAY_GAMMA, DEFAULT_DAY_TEMP, DEFAULT_NIGHT_GAMMA, DEFAULT_NIGHT_TEMP,
};
use crate::common::utils::{interpolate_f32, interpolate_inverse_u32};
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

    // NEW: INTERNALIZED PERIOD MANAGEMENT WITH GEO_TIMES LIFECYCLE

    /// Create updated RuntimeState with current period/time (immutable)
    /// Handles geo_times recalculation automatically and replaces external functions
    pub fn with_current_period(&self) -> (RuntimeState, crate::core::period::StateChange) {
        // Handle geo_times recalculation if needed (this was done in Core.check_geo_times_update)
        let updated_geo_times = if let Some(ref times) = self.geo_times {
            if times.needs_recalculation(crate::time::source::now()) {
                // Recreate geo_times (matches current Core logic)
                if let (Some(lat), Some(lon)) = (self.config.latitude, self.config.longitude) {
                    let mut new_times = times.clone();
                    if new_times.recalculate_for_next_period(lat, lon).is_ok() {
                        Some(new_times)
                    } else {
                        self.geo_times.clone() // Keep old on error
                    }
                } else {
                    self.geo_times.clone() // No coordinates available
                }
            } else {
                self.geo_times.clone() // No recalculation needed
            }
        } else {
            None
        };

        let new_period =
            crate::core::period::get_current_period(&self.config, updated_geo_times.as_ref());
        let change = crate::core::period::should_update_state(&self.period, &new_period);

        // For geo mode, we need to use coordinates timezone time for accurate progress calculations
        let current_time = if self.is_geo_mode() {
            if let Some(ref times) = updated_geo_times {
                // Get current time in the coordinates timezone - this matches what geo progress expects
                crate::time::source::now()
                    .with_timezone(&times.coordinate_tz)
                    .time()
            } else {
                // Fallback to local time if no geo_times available
                crate::time::source::now().time()
            }
        } else {
            // Use local time for non-geo modes
            crate::time::source::now().time()
        };

        let new_state = RuntimeState::new(
            new_period,
            &self.config,
            updated_geo_times.as_ref(),
            current_time,
        );

        (new_state, change)
    }

    /// Create updated RuntimeState by advancing to the next expected period.
    ///
    /// This method is used when we've slept to a transition boundary and need to
    /// force advance to the next period WITHOUT rechecking wall clock time.
    /// This prevents timing race conditions at transition boundaries.
    ///
    /// The period progression follows the natural cycle:
    /// - Day → Sunset → Night → Sunrise → Day
    /// - Static → Static (never changes)
    ///
    /// # Returns
    /// Tuple of (new RuntimeState with next period, StateChange indicating what happened)
    pub fn with_next_period(&self) -> (RuntimeState, crate::core::period::StateChange) {
        // Determine the next period in the cycle
        let next_period = match self.period {
            Period::Day => Period::Sunset,
            Period::Sunset => Period::Night,
            Period::Night => Period::Sunrise,
            Period::Sunrise => Period::Day,
            Period::Static => Period::Static, // Static mode never changes
        };

        // Determine what type of state change this represents and log it
        // Use should_update_state() for both detection and logging (matches normal flow)
        let change = crate::core::period::should_update_state(&self.period, &next_period);

        // Get the current time (in appropriate timezone)
        let current_time = if self.is_geo_mode() {
            if let Some(ref times) = self.geo_times {
                crate::time::source::now()
                    .with_timezone(&times.coordinate_tz)
                    .time()
            } else {
                crate::time::source::now().time()
            }
        } else {
            crate::time::source::now().time()
        };

        // Create new state with the next period
        // We keep the same geo_times since we're not recalculating based on time
        let new_state = RuntimeState::new(
            next_period,
            &self.config,
            self.geo_times.as_ref(),
            current_time,
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
        // Handle geo_times based on new config (matches current Core.handle_config_reload logic)
        let updated_geo_times = if new_config.transition_mode.as_deref() == Some("geo") {
            if let (Some(lat), Some(lon)) = (new_config.latitude, new_config.longitude) {
                // Check if location changed and update existing geo_times
                if let Some(ref current_times) = self.geo_times {
                    let mut new_times = current_times.clone();
                    if new_times.handle_location_change(lat, lon).is_ok() {
                        Some(new_times)
                    } else {
                        // Fall back to creating fresh geo_times - preserve critical error behavior
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
                    // Create new geo_times - preserve critical error behavior
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
                None // No coordinates in config
            }
        } else {
            None // Not geo mode, clear geo_times
        };

        let new_period =
            crate::core::period::get_current_period(new_config, updated_geo_times.as_ref());

        // For geo mode, ensure we use coordinates time for accurate calculations
        let current_time = if new_config.transition_mode.as_deref() == Some("geo") {
            if let Some(ref times) = updated_geo_times {
                // Get current time in the coordinates timezone
                crate::time::source::now()
                    .with_timezone(&times.coordinate_tz)
                    .time()
            } else {
                // Fallback to local time if no geo_times available
                crate::time::source::now().time()
            }
        } else {
            // Use local time for non-geo modes
            crate::time::source::now().time()
        };

        Ok(RuntimeState::new(
            new_period,
            new_config,
            updated_geo_times.as_ref(),
            current_time,
        ))
    }

    /// Check if two RuntimeStates have same effective values (no transition needed)
    pub fn has_same_effective_values(&self, other: &RuntimeState) -> bool {
        let (temp1, gamma1) = self.values();
        let (temp2, gamma2) = other.values();
        temp1 == temp2 && (gamma1 - gamma2).abs() < 0.01
    }

    // NEW: INTERNALIZED TIMING FUNCTIONS

    /// Time until next period change (replaces time_until_next_event)
    pub fn time_until_next_event(&self) -> std::time::Duration {
        crate::core::period::time_until_next_event(&self.config, self.geo_times.as_ref())
    }

    /// Time until current transition ends (replaces time_until_transition_end)
    pub fn time_until_transition_end(&self) -> Option<std::time::Duration> {
        crate::core::period::time_until_transition_end(&self.config, self.geo_times.as_ref())
    }

    // ACCESSOR METHODS FOR COMPATIBILITY AND INTEGRATION

    /// Access config for application lifecycle needs
    ///
    /// This provides read-only access to the config owned by RuntimeState.
    /// Idiomatic pattern: borrowing rather than cloning for efficiency.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Access geo_times for integration with other systems
    ///
    /// Returns Option<&GeoTimes> matching the owned geo_times field.
    /// Idiomatic pattern: Option<&T> preserves the optional nature while borrowing.
    pub fn geo_times(&self) -> Option<&GeoTimes> {
        self.geo_times.as_ref()
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
