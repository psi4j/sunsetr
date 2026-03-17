//! Geo mode transition times with full timezone context.
//!
//! This module provides the `GeoTimes` structure that maintains
//! transition times in the coordinate's timezone, preserving full date and
//! timezone information throughout the calculation pipeline. This solves
//! issues with midnight crossings and timezone differences.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, TimeZone, Timelike};
use chrono_tz::Tz;
use std::time::Duration as StdDuration;

use crate::core::period::Period;
use crate::geo::solar::{SolarCalculationResult, calculate_solar_times_unified};

/// Holds transition times with full timezone context for geo mode.
///
/// This structure maintains the astronomical truth of when transitions occur
/// while providing convenient methods for display and calculation.
///
/// # Key Insight
/// By storing `DateTime<Tz>` instead of `NaiveTime`, we preserve full date
/// and timezone information. This means:
/// - Comparisons automatically handle day boundaries
/// - No confusion about "today" vs "tomorrow"
/// - Duration calculations are simple subtraction
/// - Timezone conversions preserve correctness
#[derive(Debug, Clone)]
pub struct GeoTimes {
    pub coordinate_tz: Tz,
    calculated_date: NaiveDate,
    pub sunset_start: DateTime<Tz>,
    pub sunset_end: DateTime<Tz>,
    pub sunrise_start: DateTime<Tz>,
    pub sunrise_end: DateTime<Tz>,
}

/// Helper to truncate a DateTime<Tz> to second precision
fn truncate_to_second(dt: DateTime<Tz>) -> DateTime<Tz> {
    dt.with_nanosecond(0).unwrap_or(dt)
}

impl GeoTimes {
    /// Create from fresh solar calculations.
    pub fn new(latitude: f64, longitude: f64) -> Result<Self> {
        let now = crate::time::source::now();
        let coordinate_tz =
            crate::geo::solar::determine_timezone_from_coordinates(latitude, longitude);
        let now_in_tz = now.with_timezone(&coordinate_tz);
        let today = now_in_tz.date_naive();

        let solar_result = calculate_solar_times_unified(latitude, longitude, today)?;
        Self::from_solar_result(&solar_result, today, now, latitude, longitude)
    }

    /// Create GeoTimes from config if in geo mode.
    ///
    /// Returns None if:
    /// - Not in geo mode
    /// - Latitude or longitude is missing  
    /// - GeoTimes initialization fails
    ///
    /// On failure, logs a warning about falling back to traditional geo calculation.
    pub fn from_config(config: &crate::config::Config) -> Result<Option<Self>> {
        if config.transition_mode.as_deref() != Some("geo") {
            return Ok(None);
        }

        match (config.latitude, config.longitude) {
            (Some(lat), Some(lon)) => Self::new(lat, lon).map(Some).with_context(|| {
                format!(
                    "Failed to calculate solar times for coordinates: lat={:.4}, lon={:.4}",
                    lat, lon
                )
            }),
            _ => Ok(None),
        }
    }

    /// Create from a solar calculation result with intelligent date selection.
    ///
    /// When tomorrow's sunrise is needed, this function recalculates solar times
    /// for tomorrow's date to ensure DST correctness and astronomical accuracy.
    pub(crate) fn from_solar_result(
        result: &SolarCalculationResult,
        base_date: NaiveDate,
        current_time: DateTime<Local>,
        latitude: f64,
        longitude: f64,
    ) -> Result<Self> {
        let tz = result.city_timezone;
        let now_in_tz = current_time.with_timezone(&tz);

        let today_sunset_start = truncate_to_second(
            tz.from_local_datetime(&base_date.and_time(result.sunset_plus_10_start))
                .single()
                .ok_or_else(|| anyhow::anyhow!("Ambiguous sunset start time"))?,
        );

        let today_sunset_end = truncate_to_second(
            tz.from_local_datetime(&base_date.and_time(result.sunset_minus_2_end))
                .single()
                .ok_or_else(|| anyhow::anyhow!("Ambiguous sunset end time"))?,
        );

        let today_sunrise_end = truncate_to_second(
            tz.from_local_datetime(&base_date.and_time(result.sunrise_plus_10_end))
                .single()
                .ok_or_else(|| anyhow::anyhow!("Ambiguous sunrise end time"))?,
        );

        let (sunrise_start, sunrise_end) = if now_in_tz >= today_sunrise_end {
            let tomorrow = base_date + Duration::days(1);
            let tomorrow_solar = calculate_solar_times_unified(latitude, longitude, tomorrow)?;

            (
                truncate_to_second(
                    tz.from_local_datetime(
                        &tomorrow.and_time(tomorrow_solar.sunrise_minus_2_start),
                    )
                    .single()
                    .ok_or_else(|| anyhow::anyhow!("Ambiguous tomorrow sunrise start time"))?,
                ),
                truncate_to_second(
                    tz.from_local_datetime(&tomorrow.and_time(tomorrow_solar.sunrise_plus_10_end))
                        .single()
                        .ok_or_else(|| anyhow::anyhow!("Ambiguous tomorrow sunrise end time"))?,
                ),
            )
        } else {
            (
                truncate_to_second(
                    tz.from_local_datetime(&base_date.and_time(result.sunrise_minus_2_start))
                        .single()
                        .ok_or_else(|| anyhow::anyhow!("Ambiguous today sunrise start time"))?,
                ),
                today_sunrise_end,
            )
        };

        Ok(Self {
            coordinate_tz: tz,
            calculated_date: base_date,
            sunset_start: today_sunset_start,
            sunset_end: today_sunset_end,
            sunrise_start,
            sunrise_end,
        })
    }

    /// Check if recalculation is needed.
    ///
    /// Returns true if we've passed both sunset and sunrise ends,
    /// or if the date has changed significantly (e.g., after system suspend).
    pub fn needs_recalculation(&self, now: DateTime<Local>) -> bool {
        let now_in_tz = now.with_timezone(&self.coordinate_tz);
        let current_date = now_in_tz.date_naive();

        let date_jump = (current_date
            .signed_duration_since(self.calculated_date)
            .num_days())
        .abs()
            > 1;
        let passed_transitions = now_in_tz >= self.sunset_end && now_in_tz >= self.sunrise_end;

        date_jump || passed_transitions
    }

    /// Recalculate for the next period.
    ///
    /// Uses the current date in coordinate timezone as the base for new calculations.
    /// This handles multi-day gaps (e.g., computer suspension) and ensures times
    /// are calculated for the correct date.
    pub fn recalculate_for_next_period(&mut self, latitude: f64, longitude: f64) -> Result<()> {
        let now = crate::time::source::now();
        let now_in_tz = now.with_timezone(&self.coordinate_tz);
        let current_date = now_in_tz.date_naive();

        let solar_result = calculate_solar_times_unified(latitude, longitude, current_date)?;

        *self = Self::from_solar_result(&solar_result, current_date, now, latitude, longitude)?;
        Ok(())
    }

    /// Get current period.
    ///
    /// The stored DateTime values include full date information, so comparisons
    /// automatically handle day boundaries correctly.
    pub fn get_current_period(&self, now: DateTime<Local>) -> Period {
        let now_in_tz = now.with_timezone(&self.coordinate_tz);

        if now_in_tz >= self.sunset_start && now_in_tz < self.sunset_end {
            return Period::Sunset;
        }

        if now_in_tz >= self.sunrise_start && now_in_tz < self.sunrise_end {
            return Period::Sunrise;
        }

        let in_day_period = if self.sunrise_end.date_naive() > self.sunset_start.date_naive() {
            now_in_tz < self.sunset_start
        } else {
            now_in_tz >= self.sunrise_end && now_in_tz < self.sunset_start
        };

        if in_day_period {
            Period::Day
        } else {
            Period::Night
        }
    }

    /// Calculate progress as 0.0 to 1.0.
    fn calculate_progress(&self, now: DateTime<Tz>, start: DateTime<Tz>, end: DateTime<Tz>) -> f32 {
        let total_ms = end.timestamp_millis() - start.timestamp_millis();
        let elapsed_ms = now.timestamp_millis() - start.timestamp_millis();
        let linear_progress = (elapsed_ms as f32 / total_ms as f32).clamp(0.0, 1.0);
        crate::common::utils::smoothstep(linear_progress)
    }

    /// Get sunset progress if currently in sunset transition
    pub fn get_sunset_progress_if_active(&self, current_time: chrono::NaiveTime) -> Option<f32> {
        let today = crate::time::source::now()
            .with_timezone(&self.coordinate_tz)
            .date_naive();

        let current_datetime = match today
            .and_time(current_time)
            .and_local_timezone(self.coordinate_tz)
        {
            chrono::LocalResult::Single(dt) => dt,
            _ => return None,
        };

        if current_datetime >= self.sunset_start && current_datetime < self.sunset_end {
            Some(self.calculate_progress(current_datetime, self.sunset_start, self.sunset_end))
        } else {
            None
        }
    }

    /// Get sunrise progress if currently in sunrise transition
    pub fn get_sunrise_progress_if_active(&self, current_time: chrono::NaiveTime) -> Option<f32> {
        let today = crate::time::source::now()
            .with_timezone(&self.coordinate_tz)
            .date_naive();

        let current_datetime = match today
            .and_time(current_time)
            .and_local_timezone(self.coordinate_tz)
        {
            chrono::LocalResult::Single(dt) => dt,
            _ => return None,
        };

        if current_datetime >= self.sunrise_start && current_datetime < self.sunrise_end {
            Some(self.calculate_progress(current_datetime, self.sunrise_start, self.sunrise_end))
        } else {
            None
        }
    }

    /// Calculate duration until next transition starts.
    ///
    /// Since transitions are stored as DateTime<Tz> with full date information,
    /// this correctly handles cases where the next transition is tomorrow.
    pub fn duration_until_next_transition(&self, now: DateTime<Local>) -> StdDuration {
        let now_in_tz = now.with_timezone(&self.coordinate_tz);

        let mut candidates = vec![];

        if now_in_tz < self.sunset_start {
            candidates.push(self.sunset_start);
        }
        if now_in_tz < self.sunrise_start {
            candidates.push(self.sunrise_start);
        }

        if candidates.is_empty() {
            candidates.push(self.sunset_start + Duration::days(1));
            candidates.push(self.sunrise_start + Duration::days(1));
        }

        let next = candidates
            .into_iter()
            .min()
            .expect("Should always have at least one future transition");

        let millis = (next.timestamp_millis() - now_in_tz.timestamp_millis()).max(0) as u64;
        StdDuration::from_millis(millis)
    }

    /// Get remaining time in current transition.
    pub fn duration_until_transition_end(&self, now: DateTime<Local>) -> Option<StdDuration> {
        let now_in_tz = now.with_timezone(&self.coordinate_tz);

        if now_in_tz >= self.sunset_start && now_in_tz < self.sunset_end {
            let millis =
                (self.sunset_end.timestamp_millis() - now_in_tz.timestamp_millis()).max(0) as u64;
            return Some(StdDuration::from_millis(millis));
        }

        if now_in_tz >= self.sunrise_start && now_in_tz < self.sunrise_end {
            let millis =
                (self.sunrise_end.timestamp_millis() - now_in_tz.timestamp_millis()).max(0) as u64;
            return Some(StdDuration::from_millis(millis));
        }

        None
    }

    /// Get times as NaiveTime in local timezone for backward compatibility.
    ///
    /// This converts the stored coordinate timezone times to local timezone
    /// and returns them as NaiveTime values, matching the old API.
    pub fn as_naive_times_local(&self) -> (NaiveTime, NaiveTime, NaiveTime, NaiveTime) {
        (
            self.sunset_start.with_timezone(&Local).time(),
            self.sunset_end.with_timezone(&Local).time(),
            self.sunrise_start.with_timezone(&Local).time(),
            self.sunrise_end.with_timezone(&Local).time(),
        )
    }

    /// Handle location change by completely recalculating.
    pub fn handle_location_change(&mut self, latitude: f64, longitude: f64) -> Result<()> {
        *self = Self::new(latitude, longitude)?;
        Ok(())
    }
}
