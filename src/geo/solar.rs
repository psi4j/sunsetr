//! Solar position calculations for sunrise/sunset times and twilight transitions.
//!
//! Calculates sunrise and sunset times from geographic coordinates, with
//! special handling for extreme latitudes where standard astronomical
//! calculations break down.
//!
//! ## Solar elevation angles
//!
//! Transitions are computed from these elevation angles:
//! - +10 degrees: transition start (sunset) / end (sunrise)
//! - 0 degrees: actual sunrise/sunset (geometric horizon)
//! - -2 degrees: transition end (sunset) / start (sunrise)
//! - -6 degrees: civil twilight, used for baseline calculations
//!
//! The +10 to -2 degree band gives longer, more natural sunset/sunrise
//! transition windows than the traditional 0 to -6 degree civil twilight band.
//!
//! ## Extreme latitudes
//!
//! Above 55 degrees, solar calculations can produce invalid results. When that
//! is detected, the module falls back to seasonal durations: 25 minutes in
//! summer for midnight sun, 45 minutes in winter for polar night.

use anyhow::Result;
use chrono::{Datelike, NaiveDate, NaiveTime};
use std::time::Duration;

/// Solar timing for one location and date, in the location's timezone.
///
/// Includes the transition windows used by geo mode, civil twilight and
/// golden hour times, and whether an extreme-latitude fallback was used.
#[derive(Debug, Clone)]
pub struct SolarTimes {
    pub sunset_time: NaiveTime,
    pub sunrise_time: NaiveTime,
    pub sunset_duration: Duration,
    pub sunrise_duration: Duration,
    pub sunset_plus_10_start: NaiveTime,
    pub sunset_minus_2_end: NaiveTime,
    pub sunrise_minus_2_start: NaiveTime,
    pub sunrise_plus_10_end: NaiveTime,
    pub civil_dawn: NaiveTime,
    pub civil_dusk: NaiveTime,
    pub golden_hour_start: NaiveTime,
    pub golden_hour_end: NaiveTime,
    pub city_timezone: chrono_tz::Tz,
    pub used_extreme_latitude_fallback: bool,
    pub fallback_duration_minutes: u32,
}

/// Timezone for the given coordinates, from tzf-rs boundary data.
///
/// Falls back to the `TZ` environment variable, then UTC, if detection fails.
pub fn determine_timezone(latitude: f64, longitude: f64) -> chrono_tz::Tz {
    use chrono_tz::Tz;
    use std::sync::OnceLock;
    use tzf_rs::DefaultFinder;

    static FINDER: OnceLock<DefaultFinder> = OnceLock::new();
    let finder = FINDER.get_or_init(DefaultFinder::new);
    let tz_name = finder.get_tz_name(longitude, latitude);

    match tz_name.parse::<Tz>() {
        Ok(tz) => tz,
        Err(_) => match std::env::var("TZ") {
            Ok(tz_str) => tz_str.parse().unwrap_or(Tz::UTC),
            Err(_) => Tz::UTC,
        },
    }
}

/// Solar times for the given coordinates and date, in the location's timezone.
///
/// Handles extreme latitudes by falling back to seasonal durations when the
/// astronomical calculation produces an invalid result.
pub fn calculate_solar_times(
    latitude: f64,
    longitude: f64,
    date: NaiveDate,
) -> Result<SolarTimes, anyhow::Error> {
    use sunrise::{Coordinates, DawnType, SolarDay, SolarEvent};
    let city_tz = determine_timezone(latitude, longitude);

    let coord = Coordinates::new(latitude, longitude).ok_or_else(|| {
        anyhow::anyhow!("Invalid coordinates: lat={}, lon={}", latitude, longitude)
    })?;

    let solar_day = SolarDay::new(coord, date);
    let sunset_utc = solar_day.event_time(SolarEvent::Sunset);
    let sunrise_utc = solar_day.event_time(SolarEvent::Sunrise);
    let civil_dusk_utc = solar_day.event_time(SolarEvent::Dusk(DawnType::Civil));
    let civil_dawn_utc = solar_day.event_time(SolarEvent::Dawn(DawnType::Civil));

    let solar_event_missing = sunset_utc.is_none()
        || sunrise_utc.is_none()
        || civil_dusk_utc.is_none()
        || civil_dawn_utc.is_none();

    let local_time = |utc: chrono::DateTime<chrono::Utc>| utc.with_timezone(&city_tz).time();

    let sunset_time = sunset_utc
        .map(local_time)
        .unwrap_or_else(|| NaiveTime::from_hms_opt(19, 0, 0).unwrap());
    let sunrise_time = sunrise_utc
        .map(local_time)
        .unwrap_or_else(|| NaiveTime::from_hms_opt(6, 0, 0).unwrap());
    let civil_dusk = civil_dusk_utc
        .map(local_time)
        .unwrap_or_else(|| NaiveTime::from_hms_opt(19, 30, 0).unwrap());
    let civil_dawn = civil_dawn_utc
        .map(local_time)
        .unwrap_or_else(|| NaiveTime::from_hms_opt(5, 30, 0).unwrap());

    let sunset_to_civil_dusk_duration = if civil_dusk > sunset_time {
        civil_dusk.signed_duration_since(sunset_time)
    } else {
        // Civil dusk can precede sunset at extreme latitudes.
        chrono::Duration::zero()
    };

    let civil_dawn_to_sunrise_duration = if sunrise_time > civil_dawn {
        sunrise_time.signed_duration_since(civil_dawn)
    } else {
        // Sunrise can precede civil dawn at extreme latitudes.
        chrono::Duration::zero()
    };

    let abs_latitude = latitude.abs();
    let is_extreme_latitude = abs_latitude > 55.0;

    let solar_calculation_failed = {
        let preliminary_golden_hour_start = sunset_time - sunset_to_civil_dusk_duration;
        let preliminary_golden_hour_end = sunrise_time + civil_dawn_to_sunrise_duration;

        let duration_invalid = sunset_to_civil_dusk_duration.num_minutes() < 5
            || sunset_to_civil_dusk_duration.num_minutes() > 300
            || civil_dawn_to_sunrise_duration.num_minutes() < 5
            || civil_dawn_to_sunrise_duration.num_minutes() > 300;

        let sunset_sequence_invalid = {
            let golden_hour_after_sunset = preliminary_golden_hour_start >= sunset_time;
            let civil_dusk_before_sunset = civil_dusk <= sunset_time;
            golden_hour_after_sunset || civil_dusk_before_sunset
        };

        let sunrise_sequence_invalid = {
            let golden_hour_before_sunrise = preliminary_golden_hour_end <= sunrise_time;
            let civil_dawn_after_sunrise = civil_dawn >= sunrise_time;
            golden_hour_before_sunrise || civil_dawn_after_sunrise
        };

        // Identical times indicate a calculation failure
        let identical_times = sunset_time == preliminary_golden_hour_start
            || sunrise_time == preliminary_golden_hour_end
            || sunset_time == civil_dusk
            || sunrise_time == civil_dawn
            || preliminary_golden_hour_start == civil_dusk
            || preliminary_golden_hour_end == civil_dawn;

        // Dusk before dawn on the same day suggests polar conditions.
        let impossible_cycle = {
            civil_dusk < civil_dawn
                && (civil_dusk
                    .signed_duration_since(civil_dawn)
                    .num_hours()
                    .abs()
                    < 12)
        };

        duration_invalid
            || sunset_sequence_invalid
            || sunrise_sequence_invalid
            || identical_times
            || impossible_cycle
            || solar_event_missing
    };

    let (used_fallback, fallback_minutes) = if is_extreme_latitude && solar_calculation_failed {
        let day_of_year = date.ordinal();

        let is_summer = if latitude > 0.0 {
            // Northern hemisphere: summer solstice around day 172 (June 21)
            // Extended range accounts for long polar day period
            (120..=240).contains(&day_of_year)
        } else {
            // Southern hemisphere: summer solstice around day 355 (December 21)
            // Inverted logic: summer when northern hemisphere is in winter
            !(60..=300).contains(&day_of_year)
        };

        let minutes = if is_summer { 25 } else { 45 };
        (true, minutes)
    } else {
        (false, 30)
    };

    let (sunset_plus_10_start, sunset_minus_2_end, sunset_duration) = if used_fallback {
        let fallback_duration = chrono::Duration::minutes(fallback_minutes as i64);
        let plus_10_duration = fallback_duration * 10 / 12;
        let minus_2_duration = fallback_duration * 2 / 12;

        let start = sunset_time - plus_10_duration;
        let end = sunset_time + minus_2_duration;
        let millis = fallback_duration.num_milliseconds().max(0) as u64;
        let duration = std::time::Duration::from_millis(millis);

        (start, end, duration)
    } else {
        let duration_to_plus_10 = sunset_to_civil_dusk_duration * 10 / 6;
        let duration_to_minus_2 = sunset_to_civil_dusk_duration * 2 / 6;

        let start = sunset_time - duration_to_plus_10;
        let end = sunset_time + duration_to_minus_2;

        let total_duration = if end > start {
            let millis = end.signed_duration_since(start).num_milliseconds().max(0) as u64;
            std::time::Duration::from_millis(millis)
        } else {
            std::time::Duration::from_millis(30 * 60 * 1000)
        };

        (start, end, total_duration)
    };

    let (sunrise_minus_2_start, sunrise_plus_10_end, sunrise_duration) = if used_fallback {
        let fallback_duration = chrono::Duration::minutes(fallback_minutes as i64);
        let minus_2_duration = fallback_duration * 2 / 12;
        let plus_10_duration = fallback_duration * 10 / 12;

        let start = sunrise_time - minus_2_duration;
        let end = sunrise_time + plus_10_duration;
        let millis = fallback_duration.num_milliseconds().max(0) as u64;
        let duration = std::time::Duration::from_millis(millis);

        (start, end, duration)
    } else {
        let duration_from_minus_2 = civil_dawn_to_sunrise_duration * 2 / 6;
        let duration_from_plus_10 = civil_dawn_to_sunrise_duration * 10 / 6;

        let start = sunrise_time - duration_from_minus_2;
        let end = sunrise_time + duration_from_plus_10;

        let total_duration = if end > start {
            let millis = end.signed_duration_since(start).num_milliseconds().max(0) as u64;
            std::time::Duration::from_millis(millis)
        } else {
            std::time::Duration::from_millis(30 * 60 * 1000)
        };

        (start, end, total_duration)
    };

    let golden_hour_start = if used_fallback {
        sunset_time - chrono::Duration::minutes(fallback_minutes as i64 / 2)
    } else {
        sunset_time - sunset_to_civil_dusk_duration
    };

    let golden_hour_end = if used_fallback {
        sunrise_time + chrono::Duration::minutes(fallback_minutes as i64 / 2)
    } else {
        sunrise_time + civil_dawn_to_sunrise_duration
    };

    let (civil_dusk_corrected, civil_dawn_corrected) = if used_fallback {
        let civil_twilight_fraction = 0.6;
        let fallback_civil_duration =
            chrono::Duration::minutes((fallback_minutes as f64 * civil_twilight_fraction) as i64);

        let civil_dusk_fallback = sunset_time + fallback_civil_duration;
        let civil_dawn_fallback = sunrise_time - fallback_civil_duration;

        (civil_dusk_fallback, civil_dawn_fallback)
    } else {
        (civil_dusk, civil_dawn)
    };

    Ok(SolarTimes {
        sunset_time,
        sunrise_time,
        sunset_duration,
        sunrise_duration,
        sunset_plus_10_start,
        sunset_minus_2_end,
        sunrise_minus_2_start,
        sunrise_plus_10_end,
        civil_dawn: civil_dawn_corrected,
        civil_dusk: civil_dusk_corrected,
        golden_hour_start,
        golden_hour_end,
        city_timezone: city_tz,
        used_extreme_latitude_fallback: used_fallback,
        fallback_duration_minutes: fallback_minutes,
    })
}
