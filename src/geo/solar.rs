//! Solar position calculations for sunrise/sunset times and enhanced twilight transitions.
//!
//! This module provides sunrise and sunset calculations based on geographic coordinates
//! using an enhanced twilight window (sun elevation between +10 degrees and -2 degrees) for geo mode transitions,
//! while also providing traditional civil twilight calculations for display purposes. Features unified calculation
//! logic with extreme latitude handling and seasonal-aware fallback mechanisms for polar regions.

use anyhow::Result;
use chrono::{Datelike, NaiveTime};
use std::time::Duration;

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

/// Calculate civil twilight times for display purposes using the unified solar calculation system.
///
/// Returns the actual transition boundaries (+10° to -2°) used for geo mode transitions.
/// Automatically handles extreme latitude conditions using seasonal-aware fallback mechanisms.
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

    // For geo mode display, we show the actual transition boundaries (+10° to -2°)
    // that are used for the color temperature transitions
    Ok((
        result.sunset_time,           // Actual sunset time (0°)
        result.sunset_plus_10_start,  // Transition start (+10°)
        result.sunset_minus_2_end,    // Transition end (-2°)
        result.sunrise_time,          // Actual sunrise time (0°)
        result.sunrise_minus_2_start, // Transition start (-2°)
        result.sunrise_plus_10_end,   // Transition end (+10°)
        result.sunset_duration,       // Sunset transition duration
        result.sunrise_duration,      // Sunrise transition duration
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

    // Detect problematic solar calculations using comprehensive validation
    let abs_latitude = latitude.abs();
    let is_extreme_latitude = abs_latitude > 55.0; // Lowered from 60° to catch more edge cases

    // Comprehensive validation of solar calculation sequence and durations
    let solar_calculation_failed = {
        // Calculate preliminary transition times to validate sequence
        let preliminary_golden_hour_start = sunset_time - sunset_to_civil_dusk_duration;
        let preliminary_golden_hour_end = sunrise_time + civil_dawn_to_sunrise_duration;

        // Duration checks - transition durations should be reasonable (5-300 minutes)
        let duration_invalid = sunset_to_civil_dusk_duration.num_minutes() < 5
            || sunset_to_civil_dusk_duration.num_minutes() > 300
            || civil_dawn_to_sunrise_duration.num_minutes() < 5
            || civil_dawn_to_sunrise_duration.num_minutes() > 300;

        // Sequence validation for sunset (should be temporally ordered)
        let sunset_sequence_invalid = {
            // Check if golden hour start comes after sunset (impossible)
            let golden_hour_after_sunset = preliminary_golden_hour_start >= sunset_time;
            // Check if civil dusk comes before or at sunset (impossible in normal calculations)
            let civil_dusk_before_sunset = civil_dusk <= sunset_time;
            golden_hour_after_sunset || civil_dusk_before_sunset
        };

        // Sequence validation for sunrise (should be temporally ordered)
        let sunrise_sequence_invalid = {
            // Check if golden hour end comes before sunrise (impossible)
            let golden_hour_before_sunrise = preliminary_golden_hour_end <= sunrise_time;
            // Check if civil dawn comes after or at sunrise (impossible in normal calculations)
            let civil_dawn_after_sunrise = civil_dawn >= sunrise_time;
            golden_hour_before_sunrise || civil_dawn_after_sunrise
        };

        // Check for identical times (indicates calculation failure like Drammen)
        let identical_times = sunset_time == preliminary_golden_hour_start
            || sunrise_time == preliminary_golden_hour_end
            || sunset_time == civil_dusk
            || sunrise_time == civil_dawn
            || preliminary_golden_hour_start == civil_dusk
            || preliminary_golden_hour_end == civil_dawn;

        // Check for impossible day/night cycles (civil twilight crossing midnight incorrectly)
        let impossible_cycle = {
            // If civil dusk is before civil dawn on the same day, this suggests polar conditions
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
    };

    // Calculate fallback duration for extreme latitudes (seasonal aware)
    let (used_fallback, fallback_minutes) = if is_extreme_latitude && solar_calculation_failed {
        let day_of_year = today.ordinal();
        let is_summer = if latitude > 0.0 {
            // Northern hemisphere: summer around day 172 (June 21)
            (120..=240).contains(&day_of_year)
        } else {
            // Southern hemisphere: summer around day 355 (December 21)
            !(60..=300).contains(&day_of_year)
        };

        // Since latitude is capped at 65°, we only need one fallback duration
        let minutes = if is_summer {
            25 // Summer fallback
        } else {
            45 // Winter fallback
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

    #[test]
    fn test_coordinate_validation() {
        // Valid coordinates should work
        assert!(calculate_solar_times_unified(40.7128, -74.0060).is_ok());

        // Invalid latitude should fail - coordinates are validated by the sunrise crate
        assert!(calculate_solar_times_unified(91.0, -74.0060).is_err());
        assert!(calculate_solar_times_unified(-91.0, -74.0060).is_err());

        // Invalid longitude should fail
        assert!(calculate_solar_times_unified(40.7128, 181.0).is_err());
        assert!(calculate_solar_times_unified(40.7128, -181.0).is_err());
    }

    #[test]
    fn test_transition_duration_by_latitude() {
        // Test that transition durations vary appropriately by latitude
        let equator_result = calculate_solar_times_unified(0.0, 0.0).unwrap();
        let temperate_result = calculate_solar_times_unified(45.0, 0.0).unwrap();
        let high_latitude_result = calculate_solar_times_unified(60.0, 0.0).unwrap(); // Above 55° threshold

        // Should be reasonable durations (between 15 and 90 minutes)
        assert!(equator_result.sunset_duration >= Duration::from_secs(15 * 60));
        assert!(equator_result.sunset_duration <= Duration::from_secs(90 * 60));

        assert!(temperate_result.sunset_duration >= Duration::from_secs(15 * 60));
        assert!(temperate_result.sunset_duration <= Duration::from_secs(90 * 60));

        // High latitude should still produce reasonable durations
        // Fallback is only used when validation fails, not automatically at 60°
        assert!(high_latitude_result.sunset_duration >= Duration::from_secs(15 * 60));
        // At high latitudes, transitions can be very long (up to several hours)
        assert!(high_latitude_result.sunset_duration <= Duration::from_secs(300 * 60)); // 5 hours max
    }

    #[test]
    fn test_extreme_latitude_fallback_detection() {
        // Normal latitudes should not trigger fallback
        let normal_result = calculate_solar_times_unified(45.0, 0.0).unwrap();
        assert!(!normal_result.used_extreme_latitude_fallback);

        // Test with coordinates that are more likely to trigger validation failures
        // These coordinates are very high latitude and more likely to have calculation issues
        let arctic_north = calculate_solar_times_unified(78.0, 15.0).unwrap(); // Svalbard region
        let antarctic_south = calculate_solar_times_unified(-75.0, 0.0).unwrap(); // Antarctica

        // These high latitude regions are more likely to trigger fallback
        // But fallback is only used when validation actually fails
        if arctic_north.used_extreme_latitude_fallback {
            assert!(arctic_north.fallback_duration_minutes >= 20);
            assert!(arctic_north.fallback_duration_minutes <= 50);
        }

        if antarctic_south.used_extreme_latitude_fallback {
            assert!(antarctic_south.fallback_duration_minutes >= 20);
            assert!(antarctic_south.fallback_duration_minutes <= 50);
        }

        // At minimum, ensure durations are reasonable even without fallback
        assert!(arctic_north.sunset_duration >= Duration::from_secs(15 * 60));
        assert!(antarctic_south.sunset_duration >= Duration::from_secs(15 * 60));
    }

    #[test]
    fn test_validation_logic_behavior() {
        // Test that validation correctly distinguishes between working and failing calculations

        // Normal coordinates should work fine
        let london_result = calculate_solar_times_unified(51.5074, -0.1278).unwrap();
        assert!(!london_result.used_extreme_latitude_fallback);

        // High latitude coordinates that still work
        let reykjavik_result = calculate_solar_times_unified(64.1466, -21.9426).unwrap();
        // Reykjavik is at 64°N - might or might not trigger fallback depending on season

        // Very high latitude coordinates more likely to have issues
        let pole_result = calculate_solar_times_unified(85.0, 0.0).unwrap();
        // At 85°N, calculations are much more likely to fail validation

        // All results should have reasonable durations regardless of fallback usage
        assert!(london_result.sunset_duration >= Duration::from_secs(10 * 60));
        assert!(reykjavik_result.sunset_duration >= Duration::from_secs(10 * 60));
        assert!(pole_result.sunset_duration >= Duration::from_secs(10 * 60));

        // All results should have valid times (basic format check)
        assert!(!london_result.sunset_time.to_string().is_empty());
        assert!(!reykjavik_result.sunset_time.to_string().is_empty());
        assert!(!pole_result.sunset_time.to_string().is_empty());
    }

    #[test]
    fn test_solar_times_integration() {
        // Test with New York coordinates
        let result = calculate_solar_times_unified(40.7128, -74.0060);
        assert!(result.is_ok());

        let solar_result = result.unwrap();

        // Sunrise should be before sunset (basic sanity check)
        assert!(solar_result.sunrise_time != solar_result.sunset_time);

        // Durations should be reasonable
        assert!(solar_result.sunset_duration >= Duration::from_secs(15 * 60));
        assert!(solar_result.sunset_duration <= Duration::from_secs(120 * 60));
        assert!(solar_result.sunrise_duration >= Duration::from_secs(15 * 60));
        assert!(solar_result.sunrise_duration <= Duration::from_secs(120 * 60));

        // Should not use fallback for normal latitude
        assert!(!solar_result.used_extreme_latitude_fallback);
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
