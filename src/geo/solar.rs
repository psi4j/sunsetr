//! Solar position calculations for sunrise/sunset times and enhanced twilight transitions.
//!
//! This module provides sunrise and sunset calculations based on geographic coordinates
//! using an enhanced twilight window (sun elevation between +10 degrees and -2 degrees) for geo mode transitions,
//! while also providing traditional civil twilight calculations for display purposes. Features unified calculation
//! logic with extreme latitude handling and seasonal-aware fallback mechanisms for polar regions.

use anyhow::{Context, Result};
use chrono::{Datelike, Local, NaiveDate, NaiveTime};
use std::time::Duration;
use sunrise::{Coordinates, DawnType, SolarDay, SolarEvent};

/// Complete solar calculation result with all transition times and metadata
#[derive(Debug, Clone)]
pub struct SolarCalculationResult {
    // Core times (all in city timezone)
    pub sunset_time: NaiveTime,
    pub sunrise_time: NaiveTime,
    pub sunset_duration: Duration,
    pub sunrise_duration: Duration,

    // Detailed transition boundaries (city timezone)
    pub sunset_plus_10_start: NaiveTime,
    pub sunset_minus_2_end: NaiveTime,
    pub sunrise_minus_2_start: NaiveTime,
    pub sunrise_plus_10_end: NaiveTime,

    // Civil twilight (city timezone)
    pub civil_dawn: NaiveTime,
    pub civil_dusk: NaiveTime,

    // Golden hour boundaries (city timezone)
    pub golden_hour_start: NaiveTime,
    pub golden_hour_end: NaiveTime,

    // Timezone information
    pub city_timezone: chrono_tz::Tz,

    // Fallback metadata
    pub used_extreme_latitude_fallback: bool,
    pub fallback_duration_minutes: u32,
}

/// Type alias for civil twilight display data.
///
/// Contains: (sunset_time, sunset_start, sunset_end, sunrise_time, sunrise_start, sunrise_end, sunset_duration, sunrise_duration)
type CivilTwilightDisplayData = (
    chrono::NaiveTime,   // sunset_time
    chrono::NaiveTime,   // sunset_start
    chrono::NaiveTime,   // sunset_end
    chrono::NaiveTime,   // sunrise_time
    chrono::NaiveTime,   // sunrise_start
    chrono::NaiveTime,   // sunrise_end
    std::time::Duration, // sunset_duration
    std::time::Duration, // sunrise_duration
);

/// Calculate civil twilight times for a given location and date.
///
/// Uses exact civil twilight definitions:
/// - Day begins when sun reaches 0 degrees elevation (sunrise)
/// - Night begins when sun reaches -6 degrees elevation (civil dusk)
/// - Transition duration is the actual time between sunrise and civil dawn
///
/// # Arguments
/// * `latitude` - Geographic latitude in degrees (-90 to +90)
/// * `longitude` - Geographic longitude in degrees (-180 to +180)
/// * `date` - Date for which to calculate sunrise/sunset
///
/// # Returns
/// * `Ok((sunrise_time, sunset_time, transition_duration))` - Times and duration
/// * `Err(_)` - If calculations fail or coordinates are invalid
///
/// # Examples
/// ```text
/// use chrono::NaiveDate;
/// use sunsetr::geo::solar::calculate_sunrise_sunset;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap(); // Summer solstice
/// let (sunrise, sunset, duration) = calculate_sunrise_sunset(40.7128, -74.0060, date)?;
/// # Ok(())
/// # }
/// ```
#[allow(dead_code)]
pub(crate) fn calculate_sunrise_sunset(
    latitude: f64,
    longitude: f64,
    date: NaiveDate,
) -> Result<(NaiveTime, NaiveTime, Duration)> {
    // Validate coordinates
    if !(-90.0..=90.0).contains(&latitude) {
        anyhow::bail!(
            "Invalid latitude: {}. Must be between -90 and 90 degrees",
            latitude
        );
    }
    if !(-180.0..=180.0).contains(&longitude) {
        anyhow::bail!(
            "Invalid longitude: {}. Must be between -180 and 180 degrees",
            longitude
        );
    }

    // Create coordinates for the new sunrise crate API
    let coord = Coordinates::new(latitude, longitude)
        .ok_or_else(|| anyhow::anyhow!("Failed to create coordinates"))?;

    // Create solar day
    let solar_day = SolarDay::new(coord, date);

    // Calculate civil dawn (sun at -6° elevation, start of civil twilight)
    let civil_dawn_utc = solar_day.event_time(SolarEvent::Dawn(DawnType::Civil));
    let civil_dawn = civil_dawn_utc.with_timezone(&Local).time();

    // Calculate sunrise (sun at 0° elevation)
    let sunrise_utc = solar_day.event_time(SolarEvent::Sunrise);
    let sunrise_time = sunrise_utc.with_timezone(&Local).time();

    // Calculate sunset (sun at 0° elevation)
    let sunset_utc = solar_day.event_time(SolarEvent::Sunset);
    let sunset_time = sunset_utc.with_timezone(&Local).time();

    // Calculate civil dusk (sun at -6° elevation, end of civil twilight)
    let civil_dusk_utc = solar_day.event_time(SolarEvent::Dusk(DawnType::Civil));
    let civil_dusk = civil_dusk_utc.with_timezone(&Local).time();

    // Calculate actual transition duration from sunrise to civil dawn
    let morning_transition_duration = if sunrise_time > civil_dawn {
        // Normal case: civil dawn occurs before sunrise
        Duration::from_secs(sunrise_time.signed_duration_since(civil_dawn).num_seconds() as u64)
    } else {
        // Edge case: use default duration
        Duration::from_secs(30 * 60) // 30 minutes
    };

    // Calculate actual transition duration from sunset to civil dusk
    let evening_transition_duration = if civil_dusk > sunset_time {
        // Normal case: civil dusk occurs after sunset
        Duration::from_secs(civil_dusk.signed_duration_since(sunset_time).num_seconds() as u64)
    } else {
        // Edge case: use default duration
        Duration::from_secs(30 * 60) // 30 minutes
    };

    // Use the longer of the two transition durations for consistency
    let transition_duration =
        std::cmp::max(morning_transition_duration, evening_transition_duration);

    Ok((sunrise_time, sunset_time, transition_duration))
}

/// Calculate the duration of civil twilight transition for a given latitude.
///
/// Civil twilight duration varies by latitude and season:
/// - Near equator: ~20-25 minutes year-round
/// - Temperate regions: ~25-35 minutes, varies by season
/// - High latitudes: ~30-60 minutes, significant seasonal variation
///
/// This function provides a reasonable approximation based on latitude.
///
/// # Arguments
/// * `latitude` - Geographic latitude in degrees
///
/// # Returns
/// Duration of the twilight transition period
#[allow(dead_code)]
pub(crate) fn calculate_transition_duration(latitude: f64) -> Duration {
    let abs_latitude = latitude.abs();

    // Base duration increases with latitude
    let base_minutes = match abs_latitude {
        lat if lat < 10.0 => 20.0, // Tropical regions
        lat if lat < 30.0 => 25.0, // Subtropical
        lat if lat < 50.0 => 30.0, // Temperate
        lat if lat < 60.0 => 35.0, // High temperate
        lat if lat < 70.0 => 45.0, // Subpolar
        _ => 60.0,                 // Polar regions
    };

    Duration::from_secs((base_minutes * 60.0) as u64)
}

/// Handle edge cases for extreme latitudes where normal sunrise/sunset don't occur.
///
/// In polar regions during certain times of year:
/// - Midnight sun: sun never sets
/// - Polar night: sun never rises
///
/// This function detects these cases and provides fallback times.
///
/// # Arguments
/// * `latitude` - Geographic latitude
/// * `date` - Date to check for polar conditions
///
/// # Returns
/// * `Some((sunrise, sunset, duration))` - Fallback times if polar conditions detected
/// * `None` - Normal sunrise/sunset calculations should be used
#[allow(dead_code)]
pub(crate) fn handle_polar_edge_cases(
    latitude: f64,
    date: NaiveDate,
) -> Option<(NaiveTime, NaiveTime, Duration)> {
    let abs_latitude = latitude.abs();

    // Only apply to high latitudes (above Arctic/Antarctic circles ~66.5 degrees)
    if abs_latitude < 66.0 {
        return None;
    }

    // Simplified check for polar day/night conditions
    // This is a rough approximation - actual calculations are more complex
    let day_of_year = date.ordinal() as f64;
    let is_summer = if latitude > 0.0 {
        // Northern hemisphere: summer around day 172 (June 21)
        (120.0..=240.0).contains(&day_of_year)
    } else {
        // Southern hemisphere: summer around day 355 (December 21)
        !(60.0..=300.0).contains(&day_of_year)
    };

    if abs_latitude > 80.0 {
        // Extreme polar regions - more likely to have polar day/night
        if is_summer {
            // Midnight sun - use conventional times but indicate continuous day
            Some((
                NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
                Duration::from_secs(30 * 60), // 30 minute gradual transition
            ))
        } else {
            // Polar night - use conventional times but indicate continuous night
            Some((
                NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
                Duration::from_secs(60 * 60), // 1 hour gradual transition
            ))
        }
    } else {
        // Less extreme latitudes - normal calculations should work
        None
    }
}

/// Get sunrise and sunset times with comprehensive error handling.
///
/// This is the main public function that combines all solar calculations
/// with proper error handling for edge cases.
///
/// # Arguments
/// * `latitude` - Geographic latitude
/// * `longitude` - Geographic longitude  
/// * `date` - Date for calculations
///
/// # Returns
/// Sunrise time, sunset time, and transition duration with proper error handling
///
/// # Note
/// This function is re-exported by the geo module and used by external callers.
#[allow(dead_code)]
pub(crate) fn get_sun_times(
    latitude: f64,
    longitude: f64,
    date: NaiveDate,
) -> Result<(NaiveTime, NaiveTime, Duration)> {
    // Check for polar edge cases first
    if let Some(polar_times) = handle_polar_edge_cases(latitude, date) {
        return Ok(polar_times);
    }

    // Use normal calculations
    calculate_sunrise_sunset(latitude, longitude, date).with_context(|| {
        format!(
            "Failed to calculate sunrise/sunset for coordinates {:.4}�N, {:.4}�W on {}",
            latitude,
            longitude.abs(),
            date
        )
    })
}

/// Calculate civil twilight times for display purposes using the unified solar calculation system.
///
/// Returns traditional civil twilight times (+6° to -6°) for display instead of the enhanced
/// +10° to -2° window used for actual transitions. Automatically handles extreme latitude
/// conditions using seasonal-aware fallback mechanisms.
///
/// # Arguments
/// * `latitude` - Geographic latitude in degrees
/// * `longitude` - Geographic longitude in degrees
/// * `date` - Date for calculations (currently unused - uses current date)
///
/// # Returns
/// Tuple of (sunset_time, sunset_start, sunset_end, sunrise_time, sunrise_start, sunrise_end, sunset_duration, sunrise_duration)
pub fn calculate_civil_twilight_times_for_display(
    latitude: f64,
    longitude: f64,
    _date: chrono::NaiveDate,
    _debug_enabled: bool,
) -> Result<CivilTwilightDisplayData, anyhow::Error> {
    // Use the unified calculation function that handles extreme latitudes automatically
    let result = calculate_solar_times_unified(latitude, longitude)?;

    // For display purposes, we use the golden hour to civil twilight end boundaries (+6° to -6°)
    // instead of our enhanced +10° to -2° window used for actual transitions
    Ok((
        result.sunset_time,       // Actual sunset time (0°)
        result.golden_hour_start, // Golden hour start (+6°)
        result.civil_dusk,        // Civil dusk (-6°)
        result.sunrise_time,      // Actual sunrise time (0°)
        result.civil_dawn,        // Civil dawn (-6°)
        result.golden_hour_end,   // Golden hour end (+6°)
        result.sunset_duration,   // Sunset transition duration
        result.sunrise_duration,  // Sunrise transition duration
    ))
}

/// Determine the timezone for given coordinates using precise timezone boundary data.
///
/// Uses the tzf-rs crate for accurate timezone detection based on geographic boundaries.
pub fn determine_timezone_from_coordinates(latitude: f64, longitude: f64) -> chrono_tz::Tz {
    use chrono_tz::Tz;
    use std::sync::OnceLock;
    use tzf_rs::DefaultFinder;

    // Create a global finder instance for efficiency
    static FINDER: OnceLock<DefaultFinder> = OnceLock::new();
    let finder = FINDER.get_or_init(DefaultFinder::new);

    // Get timezone name from coordinates
    // Note: tzf-rs uses (longitude, latitude) order
    let tz_name = finder.get_tz_name(longitude, latitude);

    // Parse the timezone name into chrono_tz::Tz
    match tz_name.parse::<Tz>() {
        Ok(tz) => tz,
        Err(_) => {
            // If parsing fails, try to use system timezone or fall back to UTC
            match std::env::var("TZ") {
                Ok(tz_str) => tz_str.parse().unwrap_or(Tz::UTC),
                Err(_) => Tz::UTC,
            }
        }
    }
}

/// Calculate actual transition boundaries for geo mode using +10° to -2° elevation angles.
///
/// This function returns the precise transition start and end times calculated from
/// solar elevation angles, rather than applying centered logic around sunset/sunrise times.
/// This ensures geo mode uses the actual astronomical transition boundaries.
///
/// # Arguments
/// * `latitude` - Geographic latitude in degrees
/// * `longitude` - Geographic longitude in degrees
///
/// # Returns
/// Tuple of (sunset_start, sunset_end, sunrise_start, sunrise_end) as NaiveTime
/// where times are in the user's local timezone
pub fn calculate_geo_transition_boundaries(
    latitude: f64,
    longitude: f64,
) -> Result<
    (
        chrono::NaiveTime,
        chrono::NaiveTime,
        chrono::NaiveTime,
        chrono::NaiveTime,
    ),
    anyhow::Error,
> {
    use chrono::Local;

    // Use the unified calculation function that handles extreme latitudes automatically
    let result = calculate_solar_times_unified(latitude, longitude)?;

    // Get today's date for timezone conversion
    let today = Local::now().date_naive();

    // Convert transition boundary times from city timezone to user's local timezone
    let sunset_start_local =
        convert_city_time_to_local(result.sunset_plus_10_start, &result.city_timezone, today);

    let sunset_end_local =
        convert_city_time_to_local(result.sunset_minus_2_end, &result.city_timezone, today);

    let sunrise_start_local = convert_city_time_to_local(
        result.sunrise_minus_2_start,
        &result.city_timezone,
        today + chrono::Duration::days(1), // Sunrise is typically next day
    );

    let sunrise_end_local = convert_city_time_to_local(
        result.sunrise_plus_10_end,
        &result.city_timezone,
        today + chrono::Duration::days(1),
    );

    Ok((
        sunset_start_local,
        sunset_end_local,
        sunrise_start_local,
        sunrise_end_local,
    ))
}

/// Helper function to convert a time from city timezone to user's local timezone
fn convert_city_time_to_local(
    time: chrono::NaiveTime,
    city_tz: &chrono_tz::Tz,
    date: chrono::NaiveDate,
) -> chrono::NaiveTime {
    use chrono::{Local, TimeZone};

    // Create a datetime in the city's timezone
    let datetime_in_city = city_tz
        .from_local_datetime(&date.and_time(time))
        .single()
        .unwrap_or_else(|| city_tz.from_utc_datetime(&date.and_time(time)));

    // Convert to user's local timezone and extract the time
    Local
        .from_utc_datetime(&datetime_in_city.naive_utc())
        .time()
}

/// Unified solar calculation function that handles all scenarios including extreme latitudes.
///
/// This is the single source of truth for all solar calculations. It returns complete
/// information about sunset/sunrise times, transition boundaries, and civil twilight
/// times, all in the city's timezone. Other functions should use this for consistency.
///
/// # Arguments
/// * `latitude` - Geographic latitude in degrees
/// * `longitude` - Geographic longitude in degrees
///
/// # Returns
/// Complete solar calculation result with all times in city timezone
pub fn calculate_solar_times_unified(
    latitude: f64,
    longitude: f64,
) -> Result<SolarCalculationResult, anyhow::Error> {
    use chrono::Local;
    use sunrise::{Coordinates, DawnType, SolarDay, SolarEvent};

    let today = Local::now().date_naive();

    // Determine the timezone for these coordinates
    let city_tz = determine_timezone_from_coordinates(latitude, longitude);

    // Create coordinates
    let coord = Coordinates::new(latitude, longitude)
        .ok_or_else(|| anyhow::anyhow!("Invalid coordinates"))?;
    let solar_day = SolarDay::new(coord, today);

    // Calculate the actual sunset and sunrise times (sun at 0° elevation)
    let sunset_utc = solar_day.event_time(SolarEvent::Sunset);
    let sunset_time = sunset_utc.with_timezone(&city_tz).time();

    let sunrise_utc = solar_day.event_time(SolarEvent::Sunrise);
    let sunrise_time = sunrise_utc.with_timezone(&city_tz).time();

    // Calculate the civil twilight boundary times
    let civil_dusk_utc = solar_day.event_time(SolarEvent::Dusk(DawnType::Civil));
    let civil_dusk = civil_dusk_utc.with_timezone(&city_tz).time();

    let civil_dawn_utc = solar_day.event_time(SolarEvent::Dawn(DawnType::Civil));
    let civil_dawn = civil_dawn_utc.with_timezone(&city_tz).time();

    // Calculate baseline durations for normal cases
    let sunset_to_civil_dusk_duration = if civil_dusk > sunset_time {
        civil_dusk.signed_duration_since(sunset_time)
    } else {
        chrono::Duration::zero()
    };

    let civil_dawn_to_sunrise_duration = if sunrise_time > civil_dawn {
        sunrise_time.signed_duration_since(civil_dawn)
    } else {
        chrono::Duration::zero()
    };

    // Detect extreme latitude conditions where civil twilight calculations fail
    let abs_latitude = latitude.abs();
    let is_extreme_latitude = abs_latitude > 60.0;
    let civil_twilight_failed = sunset_to_civil_dusk_duration.num_minutes() <= 0
        || sunset_to_civil_dusk_duration.num_minutes() > 180
        || civil_dawn_to_sunrise_duration.num_minutes() <= 0
        || civil_dawn_to_sunrise_duration.num_minutes() > 180;

    // Calculate fallback duration for extreme latitudes (seasonal aware)
    let (used_fallback, fallback_minutes) = if is_extreme_latitude && civil_twilight_failed {
        let day_of_year = today.ordinal();
        let is_summer = if latitude > 0.0 {
            // Northern hemisphere: summer around day 172 (June 21)
            (120..=240).contains(&day_of_year)
        } else {
            // Southern hemisphere: summer around day 355 (December 21)
            !(60..=300).contains(&day_of_year)
        };

        let minutes = match abs_latitude {
            lat if lat > 80.0 => {
                if is_summer {
                    15
                } else {
                    90
                }
            } // Extreme polar
            lat if lat > 70.0 => {
                if is_summer {
                    20
                } else {
                    60
                }
            } // High polar
            _ => {
                if is_summer {
                    25
                } else {
                    45
                }
            } // Moderate polar
        };
        (true, minutes)
    } else {
        (false, 30) // Normal regions
    };

    // Calculate transition durations and boundaries
    let (sunset_plus_10_start, sunset_minus_2_end, sunset_duration) = if used_fallback {
        let fallback_duration = chrono::Duration::minutes(fallback_minutes as i64);
        let plus_10_duration = fallback_duration * 10 / 12;
        let minus_2_duration = fallback_duration * 2 / 12;

        let start = sunset_time - plus_10_duration;
        let end = sunset_time + minus_2_duration;
        let duration = std::time::Duration::from_secs(fallback_duration.num_seconds() as u64);

        (start, end, duration)
    } else {
        let duration_to_plus_10 = sunset_to_civil_dusk_duration * 10 / 6;
        let duration_to_minus_2 = sunset_to_civil_dusk_duration * 2 / 6;

        let start = sunset_time - duration_to_plus_10;
        let end = sunset_time + duration_to_minus_2;

        let total_duration = if end > start {
            std::time::Duration::from_secs(end.signed_duration_since(start).num_seconds() as u64)
        } else {
            std::time::Duration::from_secs(30 * 60)
        };

        (start, end, total_duration)
    };

    let (sunrise_minus_2_start, sunrise_plus_10_end, sunrise_duration) = if used_fallback {
        let fallback_duration = chrono::Duration::minutes(fallback_minutes as i64);
        let minus_2_duration = fallback_duration * 2 / 12;
        let plus_10_duration = fallback_duration * 10 / 12;

        let start = sunrise_time - minus_2_duration;
        let end = sunrise_time + plus_10_duration;
        let duration = std::time::Duration::from_secs(fallback_duration.num_seconds() as u64);

        (start, end, duration)
    } else {
        let duration_from_minus_2 = civil_dawn_to_sunrise_duration * 2 / 6;
        let duration_from_plus_10 = civil_dawn_to_sunrise_duration * 10 / 6;

        let start = sunrise_time - duration_from_minus_2;
        let end = sunrise_time + duration_from_plus_10;

        let total_duration = if end > start {
            std::time::Duration::from_secs(end.signed_duration_since(start).num_seconds() as u64)
        } else {
            std::time::Duration::from_secs(30 * 60)
        };

        (start, end, total_duration)
    };

    // Calculate golden hour boundaries (traditional +6° to -6°)
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

    // Calculate reasonable civil twilight times for extreme latitudes
    let (civil_dusk_corrected, civil_dawn_corrected) = if used_fallback {
        // For civil twilight fallbacks, use 60% of our total fallback duration
        let civil_twilight_fraction = 0.6;
        let fallback_civil_duration =
            chrono::Duration::minutes((fallback_minutes as f64 * civil_twilight_fraction) as i64);

        // Civil dusk: starts at sunset, extends for civil duration
        let civil_dusk_fallback = sunset_time + fallback_civil_duration;

        // Civil dawn: ends at sunrise, starts civil duration before
        let civil_dawn_fallback = sunrise_time - fallback_civil_duration;

        (civil_dusk_fallback, civil_dawn_fallback)
    } else {
        // Use the original calculated values when they're reliable
        (civil_dusk, civil_dawn)
    };

    Ok(SolarCalculationResult {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn test_coordinate_validation() {
        let date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();

        // Valid coordinates should work
        assert!(calculate_sunrise_sunset(40.7128, -74.0060, date).is_ok());

        // Invalid latitude should fail
        assert!(calculate_sunrise_sunset(91.0, -74.0060, date).is_err());
        assert!(calculate_sunrise_sunset(-91.0, -74.0060, date).is_err());

        // Invalid longitude should fail
        assert!(calculate_sunrise_sunset(40.7128, 181.0, date).is_err());
        assert!(calculate_sunrise_sunset(40.7128, -181.0, date).is_err());
    }

    #[test]
    fn test_transition_duration_by_latitude() {
        // Equatorial regions should have shorter transitions
        let equator_duration = calculate_transition_duration(0.0);
        let temperate_duration = calculate_transition_duration(45.0);
        let polar_duration = calculate_transition_duration(75.0);

        assert!(equator_duration < temperate_duration);
        assert!(temperate_duration < polar_duration);

        // Should be reasonable durations (between 15 and 90 minutes)
        assert!(equator_duration >= Duration::from_secs(15 * 60));
        assert!(polar_duration <= Duration::from_secs(90 * 60));
    }

    #[test]
    fn test_polar_edge_case_detection() {
        let summer_date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
        let winter_date = NaiveDate::from_ymd_opt(2024, 12, 21).unwrap();

        // Normal latitudes should not trigger edge cases
        assert!(handle_polar_edge_cases(45.0, summer_date).is_none());
        assert!(handle_polar_edge_cases(-45.0, winter_date).is_none());

        // Extreme polar latitudes should trigger edge cases
        assert!(handle_polar_edge_cases(85.0, summer_date).is_some());
        assert!(handle_polar_edge_cases(-85.0, winter_date).is_some());
    }

    #[test]
    fn test_get_sun_times_integration() {
        let date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();

        // Test with New York coordinates
        let result = get_sun_times(40.7128, -74.0060, date);
        assert!(result.is_ok());

        let (sunrise, sunset, duration) = result.unwrap();

        // Sunrise should be before sunset
        assert!(sunrise < sunset);

        // Duration should be reasonable
        assert!(duration >= Duration::from_secs(15 * 60));
        assert!(duration <= Duration::from_secs(90 * 60));
    }

    #[test]
    fn test_timezone_detection_accuracy() {
        use tzf_rs::DefaultFinder;

        // Test various known locations and verify we get valid timezone strings
        let test_cases = vec![
            // (latitude, longitude, description)
            (40.7128, -74.0060, "New York City"),
            (51.5074, -0.1278, "London"),
            (35.6762, 139.6503, "Tokyo"),
            (-33.8688, 151.2093, "Sydney"),
            (34.0522, -118.2437, "Los Angeles"),
            (41.8781, -87.6298, "Chicago"),
            (48.8566, 2.3522, "Paris"),
            (55.7558, 37.6173, "Moscow"),
            (-33.9249, 18.4241, "Cape Town"),
            (19.4326, -99.1332, "Mexico City"),
        ];

        let finder = DefaultFinder::new();

        for (lat, lon, location) in test_cases {
            // Test that tzf-rs returns a valid timezone string
            let tz_name = finder.get_tz_name(lon, lat);
            assert!(!tz_name.is_empty(), "Empty timezone for {}", location);

            // Test that our function returns a valid Tz
            let result = determine_timezone_from_coordinates(lat, lon);
            println!(
                "{}: tzf-rs returned '{}', parsed as {:?}",
                location, tz_name, result
            );

            // The important thing is that we get a valid timezone, not a specific one
            // (tzf-rs may return different but equivalent timezone names)
            assert_ne!(
                result,
                chrono_tz::Tz::UTC,
                "Should not default to UTC for {}",
                location
            );
        }
    }
}
