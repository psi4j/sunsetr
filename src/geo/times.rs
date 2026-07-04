//! Geo mode transition times with full timezone context.
//!
//! Each window boundary is an absolute `DateTime<Tz>`, so windows that cross
//! midnight and coordinates in a foreign timezone stay unambiguous.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, TimeZone, Timelike};
use chrono_tz::Tz;
use std::time::Duration as StdDuration;

use crate::core::period::Period;
use crate::geo::solar::{SolarTimes, calculate_solar_times};

#[derive(Debug, Clone)]
pub struct GeoTimes {
    pub coordinate_tz: Tz,
    calculated_date: NaiveDate,
    pub sunset_start: DateTime<Tz>,
    pub sunset_end: DateTime<Tz>,
    pub sunrise_start: DateTime<Tz>,
    pub sunrise_end: DateTime<Tz>,
}

fn truncate_to_second(dt: DateTime<Tz>) -> DateTime<Tz> {
    dt.with_nanosecond(0).unwrap_or(dt)
}

/// Anchor a transition window to `start_date`, rolling the end past midnight
/// when it wraps so the stored window stays a forward interval.
fn anchor_window(
    tz: Tz,
    start_date: NaiveDate,
    start: NaiveTime,
    end: NaiveTime,
) -> Result<(DateTime<Tz>, DateTime<Tz>)> {
    let end_date = if start > end {
        start_date + Duration::days(1)
    } else {
        start_date
    };

    let start_dt = truncate_to_second(
        tz.from_local_datetime(&start_date.and_time(start))
            .single()
            .ok_or_else(|| anyhow::anyhow!("Ambiguous transition start time"))?,
    );
    let end_dt = truncate_to_second(
        tz.from_local_datetime(&end_date.and_time(end))
            .single()
            .ok_or_else(|| anyhow::anyhow!("Ambiguous transition end time"))?,
    );

    Ok((start_dt, end_dt))
}

fn sunrise_start_date(end_date: NaiveDate, start: NaiveTime, end: NaiveTime) -> NaiveDate {
    if start > end {
        end_date - Duration::days(1)
    } else {
        end_date
    }
}

impl GeoTimes {
    pub fn new(latitude: f64, longitude: f64) -> Result<Self> {
        let now = crate::time::source::now();
        let coordinate_tz = crate::geo::solar::determine_timezone(latitude, longitude);
        let now_in_tz = now.with_timezone(&coordinate_tz);
        let today = now_in_tz.date_naive();

        let solar_result = calculate_solar_times(latitude, longitude, today)?;
        Self::from_solar_result(&solar_result, today, now, latitude, longitude)
    }

    /// Build from config when in geo mode with coordinates set.
    ///
    /// Returns `Ok(None)` outside geo mode or when coordinates are missing, and
    /// `Err` when the solar calculation fails.
    pub fn from_config(config: &crate::config::Config) -> Result<Option<Self>> {
        if config.transition_mode != crate::config::TransitionMode::Geo {
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

    /// Build from a precomputed solar result. Once `current_time` is past today's
    /// sunrise, the sunrise window is computed from tomorrow's solar times, so it
    /// carries tomorrow's sunrise and DST offset.
    pub(crate) fn from_solar_result(
        result: &SolarTimes,
        base_date: NaiveDate,
        current_time: DateTime<Local>,
        latitude: f64,
        longitude: f64,
    ) -> Result<Self> {
        let tz = result.city_timezone;
        let now_in_tz = current_time.with_timezone(&tz);

        let (today_sunset_start, today_sunset_end) = anchor_window(
            tz,
            base_date,
            result.sunset_plus_10_start,
            result.sunset_minus_2_end,
        )?;

        let today_sunrise_end = truncate_to_second(
            tz.from_local_datetime(&base_date.and_time(result.sunrise_plus_10_end))
                .single()
                .ok_or_else(|| anyhow::anyhow!("Ambiguous sunrise end time"))?,
        );

        let (sunrise_start, sunrise_end) = if now_in_tz >= today_sunrise_end {
            let tomorrow = base_date + Duration::days(1);
            let tomorrow_solar = calculate_solar_times(latitude, longitude, tomorrow)?;

            let start_date = sunrise_start_date(
                tomorrow,
                tomorrow_solar.sunrise_minus_2_start,
                tomorrow_solar.sunrise_plus_10_end,
            );
            anchor_window(
                tz,
                start_date,
                tomorrow_solar.sunrise_minus_2_start,
                tomorrow_solar.sunrise_plus_10_end,
            )?
        } else {
            let start_date = sunrise_start_date(
                base_date,
                result.sunrise_minus_2_start,
                result.sunrise_plus_10_end,
            );
            anchor_window(
                tz,
                start_date,
                result.sunrise_minus_2_start,
                result.sunrise_plus_10_end,
            )?
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

    /// True when the stored windows no longer describe the current day: past both
    /// sunset and sunrise ends, a date jump of more than a day (as after a system
    /// suspend), or a backward clock jump.
    pub fn needs_recalculation(&self, now: DateTime<Local>) -> bool {
        let now_in_tz = now.with_timezone(&self.coordinate_tz);
        let current_date = now_in_tz.date_naive();

        let date_jump = (current_date
            .signed_duration_since(self.calculated_date)
            .num_days())
        .abs()
            > 1;
        let passed_transitions = now_in_tz >= self.sunset_end && now_in_tz >= self.sunrise_end;
        let stale_after_backward_jump =
            now_in_tz < self.sunset_start && self.sunrise_start.date_naive() > current_date;

        date_jump || passed_transitions || stale_after_backward_jump
    }

    /// Recompute the windows for the current date in the coordinate timezone,
    /// covering multi-day gaps.
    pub fn recalculate_for_next_period(&mut self, latitude: f64, longitude: f64) -> Result<()> {
        let now = crate::time::source::now();
        let now_in_tz = now.with_timezone(&self.coordinate_tz);
        let current_date = now_in_tz.date_naive();

        let solar_result = calculate_solar_times(latitude, longitude, current_date)?;

        *self = Self::from_solar_result(&solar_result, current_date, now, latitude, longitude)?;
        Ok(())
    }

    pub fn current_period(&self, now: DateTime<Local>) -> Period {
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

    fn calculate_progress(&self, now: DateTime<Tz>, start: DateTime<Tz>, end: DateTime<Tz>) -> f32 {
        let total_ms = end.timestamp_millis() - start.timestamp_millis();
        let elapsed_ms = now.timestamp_millis() - start.timestamp_millis();
        let linear_progress = (elapsed_ms as f32 / total_ms as f32).clamp(0.0, 1.0);
        crate::common::utils::smoothstep(linear_progress)
    }

    pub fn sunset_progress(&self, now: DateTime<Local>) -> Option<f32> {
        let now_in_tz = now.with_timezone(&self.coordinate_tz);
        if now_in_tz >= self.sunset_start && now_in_tz < self.sunset_end {
            Some(self.calculate_progress(now_in_tz, self.sunset_start, self.sunset_end))
        } else {
            None
        }
    }

    pub fn sunrise_progress(&self, now: DateTime<Local>) -> Option<f32> {
        let now_in_tz = now.with_timezone(&self.coordinate_tz);
        if now_in_tz >= self.sunrise_start && now_in_tz < self.sunrise_end {
            Some(self.calculate_progress(now_in_tz, self.sunrise_start, self.sunrise_end))
        } else {
            None
        }
    }

    pub fn time_until_next_transition(&self, now: DateTime<Local>) -> StdDuration {
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

    pub fn time_until_transition_end(&self, now: DateTime<Local>) -> Option<StdDuration> {
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

    pub fn handle_location_change(&mut self, latitude: f64, longitude: f64) -> Result<()> {
        *self = Self::new(latitude, longitude)?;
        Ok(())
    }
}
