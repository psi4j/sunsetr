//! Geo mode transition times with full timezone context.
//!
//! This module provides the `GeoTransitionTimes` structure that maintains
//! transition times in the coordinate's timezone, preserving full date and
//! timezone information throughout the calculation pipeline. This solves
//! issues with midnight crossings and timezone differences.

use anyhow::Result;
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, TimeZone, Timelike};
use chrono_tz::Tz;
use std::time::Duration as StdDuration;

use crate::geo::solar::{SolarCalculationResult, calculate_solar_times_unified};
use crate::time_state::{TimeState, TransitionState};

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
pub struct GeoTransitionTimes {
    /// The timezone of the coordinates
    pub coordinate_tz: Tz,

    /// The date these transitions were calculated for
    /// Used internally for recalculation detection
    calculated_date: NaiveDate,

    /// Sunset transition boundaries in coordinate timezone
    /// These represent the actual astronomical events
    pub sunset_start: DateTime<Tz>,
    pub sunset_end: DateTime<Tz>,

    /// Sunrise transition boundaries in coordinate timezone
    /// These may be for today or tomorrow depending on when constructed
    pub sunrise_start: DateTime<Tz>,
    pub sunrise_end: DateTime<Tz>,

    /// Cached solar calculation result for recalculation
    cached_solar_result: Option<SolarCalculationResult>,
}

/// Helper to truncate a DateTime<Tz> to second precision
fn truncate_to_second(dt: DateTime<Tz>) -> DateTime<Tz> {
    dt.with_nanosecond(0).unwrap_or(dt)
}

impl GeoTransitionTimes {
    /// Create from fresh solar calculations.
    pub fn new(latitude: f64, longitude: f64) -> Result<Self> {
        let solar_result = calculate_solar_times_unified(latitude, longitude)?;
        let now = crate::time_source::now();
        // Use the date in the coordinate timezone, not local timezone
        // This is critical for correct date selection when local and coordinate timezones differ
        let now_in_tz = now.with_timezone(&solar_result.city_timezone);
        let today = now_in_tz.date_naive();

        Self::from_solar_result(&solar_result, today, now)
    }

    /// Create from a solar calculation result with intelligent date selection.
    fn from_solar_result(
        result: &SolarCalculationResult,
        base_date: NaiveDate,
        current_time: DateTime<Local>,
    ) -> Result<Self> {
        let tz = result.city_timezone;
        let now_in_tz = current_time.with_timezone(&tz);

        // Create today's transitions
        // Truncate to second precision to avoid sub-second comparison issues
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

        // Determine which day's sunrise to use
        // If we're past today's sunrise end, use tomorrow's
        let today_sunrise_end = truncate_to_second(
            tz.from_local_datetime(&base_date.and_time(result.sunrise_plus_10_end))
                .single()
                .ok_or_else(|| anyhow::anyhow!("Ambiguous sunrise end time"))?,
        );

        let (sunrise_start, sunrise_end) = if now_in_tz >= today_sunrise_end {
            // Use tomorrow's sunrise
            let tomorrow = base_date + Duration::days(1);
            (
                truncate_to_second(
                    tz.from_local_datetime(&tomorrow.and_time(result.sunrise_minus_2_start))
                        .single()
                        .ok_or_else(|| anyhow::anyhow!("Ambiguous tomorrow sunrise start time"))?,
                ),
                truncate_to_second(
                    tz.from_local_datetime(&tomorrow.and_time(result.sunrise_plus_10_end))
                        .single()
                        .ok_or_else(|| anyhow::anyhow!("Ambiguous tomorrow sunrise end time"))?,
                ),
            )
        } else {
            // Use today's sunrise
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
            cached_solar_result: Some(result.clone()),
        })
    }

    /// Check if recalculation is needed.
    ///
    /// Returns true if we've passed both sunset and sunrise ends,
    /// or if the date has changed significantly (e.g., after system suspend).
    pub fn needs_recalculation(&self, now: DateTime<Local>) -> bool {
        let now_in_tz = now.with_timezone(&self.coordinate_tz);
        let current_date = now_in_tz.date_naive();

        // Recalculate if:
        // 1. The date has changed by more than 1 day (system suspend/resume)
        // 2. We've passed both sunset and sunrise ends
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
    /// This handles multi-day gaps (e.g., computer suspension).
    pub fn recalculate_for_next_period(&mut self, latitude: f64, longitude: f64) -> Result<()> {
        // Either use cached result or recalculate
        let solar_result = if let Some(ref cached) = self.cached_solar_result {
            cached.clone()
        } else {
            calculate_solar_times_unified(latitude, longitude)?
        };

        let now = crate::time_source::now();
        let now_in_tz = now.with_timezone(&self.coordinate_tz);
        let current_date = now_in_tz.date_naive();

        // Use the current date as base for recalculation
        *self = Self::from_solar_result(&solar_result, current_date, now)?;
        Ok(())
    }

    /// Get current transition state.
    ///
    /// The stored DateTime values include full date information, so comparisons
    /// automatically handle day boundaries correctly.
    pub fn get_current_state(&self, now: DateTime<Local>) -> TransitionState {
        let now_in_tz = now.with_timezone(&self.coordinate_tz);

        // Check sunset transition
        if now_in_tz >= self.sunset_start && now_in_tz < self.sunset_end {
            let progress = self.calculate_progress(now_in_tz, self.sunset_start, self.sunset_end);
            return TransitionState::Transitioning {
                from: TimeState::Day,
                to: TimeState::Night,
                progress,
            };
        }

        // Check sunrise transition
        if now_in_tz >= self.sunrise_start && now_in_tz < self.sunrise_end {
            let progress = self.calculate_progress(now_in_tz, self.sunrise_start, self.sunrise_end);
            return TransitionState::Transitioning {
                from: TimeState::Night,
                to: TimeState::Day,
                progress,
            };
        }

        // Determine stable state
        // The cycle is: Night → [Sunrise] → Day → [Sunset] → Night
        //
        // We're in Day if:
        //   - We're past sunrise_end (morning has finished) AND
        //   - We're before sunset_start (evening hasn't started)
        //
        // Otherwise we're in Night
        // This works because sunrise might be tomorrow, so if we're at 4:55am
        // and sunrise_end is tomorrow at 6:41am, we're NOT past sunrise_end yet
        let in_day_period = now_in_tz >= self.sunrise_end && now_in_tz < self.sunset_start;

        TransitionState::Stable(if in_day_period {
            TimeState::Day
        } else {
            TimeState::Night
        })
    }

    /// Calculate progress as 0.0 to 1.0.
    fn calculate_progress(&self, now: DateTime<Tz>, start: DateTime<Tz>, end: DateTime<Tz>) -> f32 {
        let total = end.timestamp() - start.timestamp();
        let elapsed = now.timestamp() - start.timestamp();
        let linear_progress = (elapsed as f32 / total as f32).clamp(0.0, 1.0);

        // Apply Bezier curve for smooth S-curve
        crate::utils::bezier_curve(
            linear_progress,
            crate::constants::BEZIER_P1X,
            crate::constants::BEZIER_P1Y,
            crate::constants::BEZIER_P2X,
            crate::constants::BEZIER_P2Y,
        )
    }

    /// Calculate duration until next transition starts.
    ///
    /// Since transitions are stored as DateTime<Tz> with full date information,
    /// this correctly handles cases where the next transition is tomorrow.
    pub fn duration_until_next_transition(&self, now: DateTime<Local>) -> StdDuration {
        let now_in_tz = now.with_timezone(&self.coordinate_tz);

        // Check stored transitions first (they already have correct dates)
        let mut candidates = vec![];

        // Add stored transitions if they're in the future
        if now_in_tz < self.sunset_start {
            candidates.push(self.sunset_start);
        }
        if now_in_tz < self.sunrise_start {
            candidates.push(self.sunrise_start);
        }

        // If we've passed both stored transitions, we need tomorrow's
        // (This shouldn't normally happen as we recalculate when both are passed)
        if candidates.is_empty() {
            // Add tomorrow's transitions (same time, next day)
            candidates.push(self.sunset_start + Duration::days(1));
            candidates.push(self.sunrise_start + Duration::days(1));
        }

        // Find the earliest future transition
        let next = candidates
            .into_iter()
            .min()
            .expect("Should always have at least one future transition");

        // Calculate duration between now and next transition
        // Both times are truncated to second precision, so this should work cleanly
        let seconds = (next.timestamp() - now_in_tz.timestamp()).max(0) as u64;
        StdDuration::from_secs(seconds)
    }

    /// Get remaining time in current transition.
    pub fn duration_until_transition_end(&self, now: DateTime<Local>) -> Option<StdDuration> {
        let now_in_tz = now.with_timezone(&self.coordinate_tz);

        // Check if in sunset transition
        if now_in_tz >= self.sunset_start && now_in_tz < self.sunset_end {
            // Both times are truncated to second precision, so this should work cleanly
            let seconds = (self.sunset_end.timestamp() - now_in_tz.timestamp()).max(0) as u64;
            return Some(StdDuration::from_secs(seconds));
        }

        // Check if in sunrise transition
        if now_in_tz >= self.sunrise_start && now_in_tz < self.sunrise_end {
            // Both times are truncated to second precision, so this should work cleanly
            let seconds = (self.sunrise_end.timestamp() - now_in_tz.timestamp()).max(0) as u64;
            return Some(StdDuration::from_secs(seconds));
        }

        None // Not in transition
    }

    /// Format a time for display with optional local timezone.
    #[allow(dead_code)]
    pub fn format_time_for_display(&self, time: DateTime<Tz>) -> String {
        use chrono::Offset;

        let local_time = time.with_timezone(&Local);
        let coord_offset = time.offset().fix();
        let local_offset = local_time.offset().fix();

        if coord_offset == local_offset {
            // Same timezone, just show the time
            format!("{}", time.format("%H:%M:%S"))
        } else {
            // Different timezones, show both
            format!(
                "{} [{}]",
                time.format("%H:%M:%S"),
                local_time.format("%H:%M:%S")
            )
        }
    }

    /// Get display information for debug logging.
    #[allow(dead_code)]
    pub fn get_debug_info(&self) -> String {
        format!(
            "Sunset: {} to {}\nSunrise: {} to {}",
            self.format_time_for_display(self.sunset_start),
            self.format_time_for_display(self.sunset_end),
            self.format_time_for_display(self.sunrise_start),
            self.format_time_for_display(self.sunrise_end)
        )
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

    /// Handle time anomaly (e.g., system suspend/resume) by forcing recalculation.
    pub fn handle_time_anomaly(&mut self, latitude: f64, longitude: f64) -> Result<()> {
        self.recalculate_for_next_period(latitude, longitude)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_geo_transition_times_creation() {
        // Test with London coordinates
        let result = GeoTransitionTimes::new(51.5074, -0.1278);
        assert!(result.is_ok());

        let times = result.unwrap();
        assert_eq!(times.coordinate_tz.to_string(), "Europe/London");
    }

    #[test]
    fn test_timezone_preservation() {
        // Create a mock solar result for testing
        let solar_result = SolarCalculationResult {
            sunset_time: NaiveTime::from_hms_opt(19, 30, 0).unwrap(),
            sunrise_time: NaiveTime::from_hms_opt(5, 30, 0).unwrap(),
            sunset_duration: StdDuration::from_secs(3600),
            sunrise_duration: StdDuration::from_secs(3600),
            sunset_plus_10_start: NaiveTime::from_hms_opt(19, 0, 0).unwrap(),
            sunset_minus_2_end: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
            sunrise_minus_2_start: NaiveTime::from_hms_opt(5, 0, 0).unwrap(),
            sunrise_plus_10_end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
            civil_dawn: NaiveTime::from_hms_opt(4, 45, 0).unwrap(),
            civil_dusk: NaiveTime::from_hms_opt(20, 15, 0).unwrap(),
            golden_hour_start: NaiveTime::from_hms_opt(18, 30, 0).unwrap(),
            golden_hour_end: NaiveTime::from_hms_opt(6, 30, 0).unwrap(),
            city_timezone: chrono_tz::Europe::London,
            used_extreme_latitude_fallback: false,
            fallback_duration_minutes: 0,
        };

        let now = Local.with_ymd_and_hms(2024, 6, 21, 12, 0, 0).unwrap();
        let base_date = now.date_naive();

        let result = GeoTransitionTimes::from_solar_result(&solar_result, base_date, now);
        assert!(result.is_ok());

        let times = result.unwrap();
        // Verify that times are stored with timezone information
        assert_eq!(times.sunset_start.timezone(), chrono_tz::Europe::London);
        assert_eq!(times.sunrise_end.timezone(), chrono_tz::Europe::London);
    }
}
