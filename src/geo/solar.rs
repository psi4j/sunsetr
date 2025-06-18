//! Solar position calculations for sunrise/sunset times and civil twilight.
//!
//! This module provides sunrise and sunset calculations based on geographic coordinates
//! using civil twilight definitions (sun elevation between +6 degrees and -6 degrees). This provides
//! natural transition times that vary by location and season.

use anyhow::{Context, Result};
use chrono::{Datelike, Local, NaiveDate, NaiveTime};
use std::time::Duration;
use sunrise::{Coordinates, DawnType, SolarDay, SolarEvent};

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

/// Calculate civil twilight times for display purposes.
///
/// Returns the actual +6° to -6° transition times for both sunset and sunrise,
/// along with the transition durations.
///
/// # Arguments
/// * `latitude` - Geographic latitude in degrees
/// * `longitude` - Geographic longitude in degrees
/// * `date` - Date for calculations
///
/// # Returns
/// Tuple of (sunset_time, sunset_start, sunset_end, sunrise_time, sunrise_start, sunrise_end, sunset_duration, sunrise_duration)
pub fn calculate_civil_twilight_times_for_display(
    latitude: f64,
    longitude: f64,
    date: chrono::NaiveDate,
    debug_enabled: bool,
) -> Result<CivilTwilightDisplayData, anyhow::Error> {
    use sunrise::{Coordinates, DawnType, SolarDay, SolarEvent};

    // Determine the timezone for these coordinates
    let timezone = determine_timezone_from_coordinates(latitude, longitude);

    // Create coordinates
    let coord = Coordinates::new(latitude, longitude)
        .ok_or_else(|| anyhow::anyhow!("Invalid coordinates"))?;
    let solar_day = SolarDay::new(coord, date);

    // Try to calculate all the key solar events
    // In polar regions, some events may not occur (e.g., sun never reaches +6°)

    // Get basic sunrise/sunset first (0° elevation)
    let sunrise_utc = solar_day.event_time(SolarEvent::Sunrise);
    let sunrise_time = sunrise_utc.with_timezone(&timezone).time();

    let sunset_utc = solar_day.event_time(SolarEvent::Sunset);
    let sunset_time = sunset_utc.with_timezone(&timezone).time();

    // Debug: log the UTC vs local times
    if debug_enabled {
        use crate::logger::Log;
        Log::log_pipe();
        Log::log_debug("Solar calculation details");
        Log::log_indented(&format!(
            "Raw coordinates: {:.4}°, {:.4}°",
            latitude, longitude
        ));
        Log::log_indented(&format!(
            "Sunrise UTC: {}, Local: {}, TZ: {}",
            sunrise_utc.format("%H:%M"),
            sunrise_time.format("%H:%M"),
            timezone
        ));
        Log::log_indented(&format!(
            "Sunset UTC: {}, Local: {}, TZ: {}",
            sunset_utc.format("%H:%M"),
            sunset_time.format("%H:%M"),
            timezone
        ));
    }

    // Try to get civil twilight times (-6° elevation)
    let civil_dusk_utc = solar_day.event_time(SolarEvent::Dusk(DawnType::Civil));
    let civil_dusk = civil_dusk_utc.with_timezone(&timezone).time();

    // Try to get golden hour times (+6° elevation)
    // These may fail in polar regions where sun never reaches +6°
    let golden_hour_start = {
        let golden_hour_start_utc = solar_day.event_time(SolarEvent::Elevation {
            elevation: f64::to_radians(6.0),
            morning: false,
        });
        golden_hour_start_utc.with_timezone(&timezone).time()
    };

    // For tomorrow's sunrise
    let tomorrow = date + chrono::Duration::days(1);
    let tomorrow_solar_day = SolarDay::new(coord, tomorrow);

    let tomorrow_civil_dawn_utc = tomorrow_solar_day.event_time(SolarEvent::Dawn(DawnType::Civil));
    let tomorrow_civil_dawn = tomorrow_civil_dawn_utc.with_timezone(&timezone).time();

    let golden_hour_end = {
        let golden_hour_end_utc = tomorrow_solar_day.event_time(SolarEvent::Elevation {
            elevation: f64::to_radians(6.0),
            morning: true,
        });
        golden_hour_end_utc.with_timezone(&timezone).time()
    };

    // Check for polar edge cases and unreasonable calculations
    let abs_latitude = latitude.abs();
    let is_polar_region = abs_latitude > 60.0; // Rough threshold for problematic calculations

    // Check if civil twilight calculations make sense
    let civil_twilight_duration = if civil_dusk > sunset_time {
        civil_dusk.signed_duration_since(sunset_time).num_minutes()
    } else {
        0
    };

    // If we're in a polar region OR civil twilight is extremely long, use fallback
    let use_fallback =
        is_polar_region && (civil_twilight_duration > 180 || civil_twilight_duration <= 0);

    let (sunset_start, sunset_end, sunrise_start, sunrise_end) = if use_fallback {
        // Use reasonable defaults for polar regions
        let transition_minutes = 45; // 45-minute transitions
        let half_transition = chrono::Duration::minutes(transition_minutes / 2);

        (
            sunset_time - half_transition, // Sunset starts 22.5 min before horizon
            sunset_time + half_transition, // Sunset ends 22.5 min after horizon
            tomorrow_civil_dawn - half_transition, // Sunrise starts 22.5 min before civil dawn
            tomorrow_civil_dawn + half_transition, // Sunrise ends 22.5 min after civil dawn
        )
    } else {
        // Try to use accurate +6° calculations if they seem reasonable
        let reasonable_golden_hour = golden_hour_start != sunset_time
            && golden_hour_start < sunset_time
            && sunset_time
                .signed_duration_since(golden_hour_start)
                .num_minutes()
                < 120;

        let reasonable_golden_hour_end = golden_hour_end != tomorrow_civil_dawn
            && golden_hour_end > tomorrow_civil_dawn
            && golden_hour_end
                .signed_duration_since(tomorrow_civil_dawn)
                .num_minutes()
                < 120;

        let sunset_pair = if reasonable_golden_hour {
            (golden_hour_start, civil_dusk)
        } else {
            (sunset_time, civil_dusk)
        };

        let sunrise_pair = if reasonable_golden_hour_end {
            (tomorrow_civil_dawn, golden_hour_end)
        } else {
            let tomorrow_sunrise_utc = tomorrow_solar_day.event_time(SolarEvent::Sunrise);
            let tomorrow_sunrise = tomorrow_sunrise_utc.with_timezone(&timezone).time();
            (tomorrow_civil_dawn, tomorrow_sunrise)
        };

        (sunset_pair.0, sunset_pair.1, sunrise_pair.0, sunrise_pair.1)
    };

    // Calculate durations using the determined start/end times
    let sunset_duration = if sunset_end > sunset_start {
        sunset_end.signed_duration_since(sunset_start)
    } else {
        chrono::Duration::hours(1) // fallback
    };

    let sunrise_duration = if sunrise_end > sunrise_start {
        sunrise_end.signed_duration_since(sunrise_start)
    } else {
        chrono::Duration::hours(1) // fallback
    };

    // Get tomorrow's actual sunrise time for display
    let tomorrow_sunrise_utc = tomorrow_solar_day.event_time(SolarEvent::Sunrise);
    let tomorrow_sunrise_time = tomorrow_sunrise_utc.with_timezone(&timezone).time();

    Ok((
        sunset_time,           // Actual sunset time (0°)
        sunset_start,          // Transition start (+6° or fallback)
        sunset_end,            // Transition end (-6°)
        tomorrow_sunrise_time, // Actual sunrise time (0°)
        sunrise_start,         // Transition start (-6°)
        sunrise_end,           // Transition end (+6° or fallback)
        sunset_duration
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(3600)),
        sunrise_duration
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(3600)),
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

/// Calculate sunset/sunrise center times and transition durations for geo mode.
///
/// This function calculates the actual sunset/sunrise times (sun at 0°) and the 
/// duration of civil twilight transitions (from +6° to -6°) for use with center-mode logic.
///
/// # Arguments
/// * `latitude` - Geographic latitude in degrees
/// * `longitude` - Geographic longitude in degrees
///
/// # Returns
/// Tuple of (sunset_time, sunset_duration, sunrise_time, sunrise_duration) or error
pub fn calculate_geo_center_times_and_durations(
    latitude: f64,
    longitude: f64,
) -> Result<
    (
        chrono::NaiveTime,
        std::time::Duration,
        chrono::NaiveTime,
        std::time::Duration,
    ),
    anyhow::Error,
> {
    use chrono::Local;
    use sunrise::{Coordinates, DawnType, SolarDay, SolarEvent};
    let today = Local::now().date_naive();

    // Create coordinates
    let coord = Coordinates::new(latitude, longitude)
        .ok_or_else(|| anyhow::anyhow!("Invalid coordinates"))?;
    let solar_day = SolarDay::new(coord, today);

    // Calculate the actual sunset and sunrise times (sun at 0° elevation)
    let sunset_utc = solar_day.event_time(SolarEvent::Sunset);
    let sunset_time = sunset_utc.with_timezone(&Local).time();

    let sunrise_utc = solar_day.event_time(SolarEvent::Sunrise);
    let sunrise_time = sunrise_utc.with_timezone(&Local).time();

    // Calculate the civil twilight boundary times
    let civil_dusk_utc = solar_day.event_time(SolarEvent::Dusk(DawnType::Civil));
    let civil_dusk = civil_dusk_utc.with_timezone(&Local).time();

    let civil_dawn_utc = solar_day.event_time(SolarEvent::Dawn(DawnType::Civil));
    let civil_dawn = civil_dawn_utc.with_timezone(&Local).time();

    // Use the reliable civil dusk calculation (we already have this above)
    // civil_dusk is calculated from Dusk(Civil) and should be correct

    // Calculate golden hour times using symmetry approach
    // The full transition (+6° to -6°) should be symmetric around sunset/sunrise
    let sunset_to_civil_dusk_duration = if civil_dusk > sunset_time {
        civil_dusk.signed_duration_since(sunset_time)
    } else {
        chrono::Duration::zero()
    };
    
    // Golden hour start should be the same duration before sunset as civil dusk is after
    let golden_hour_start = sunset_time - sunset_to_civil_dusk_duration;
    
    // For sunrise, calculate the same way
    let civil_dawn_to_sunrise_duration = if sunrise_time > civil_dawn {
        sunrise_time.signed_duration_since(civil_dawn)
    } else {
        chrono::Duration::zero()
    };
    
    // Golden hour end should be the same duration after sunrise as civil dawn is before
    let golden_hour_end = sunrise_time + civil_dawn_to_sunrise_duration;

    // Debug logging removed - now handled in geo selection flow

    // Calculate the actual transition durations
    let sunset_duration = if civil_dusk > golden_hour_start {
        let duration_chrono = civil_dusk.signed_duration_since(golden_hour_start);
        let duration_std = std::time::Duration::from_secs(duration_chrono.num_seconds().max(0) as u64);
        
        // Duration logging removed - now handled in geo selection flow
        
        duration_std
    } else {
        // Warning logging removed - now handled in geo selection flow
        std::time::Duration::from_secs(30 * 60) // 30-minute fallback
    };

    let sunrise_duration = if golden_hour_end > civil_dawn {
        let duration_chrono = golden_hour_end.signed_duration_since(civil_dawn);
        let duration_std = std::time::Duration::from_secs(duration_chrono.num_seconds().max(0) as u64);
        
        // Duration logging removed - now handled in geo selection flow
        
        duration_std
    } else {
        // Warning logging removed - now handled in geo selection flow
        std::time::Duration::from_secs(30 * 60) // 30-minute fallback
    };

    Ok((sunset_time, sunset_duration, sunrise_time, sunrise_duration))
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
            println!("{}: tzf-rs returned '{}', parsed as {:?}", location, tz_name, result);

            // The important thing is that we get a valid timezone, not a specific one
            // (tzf-rs may return different but equivalent timezone names)
            assert_ne!(result, chrono_tz::Tz::UTC, 
                "Should not default to UTC for {}", location);
        }
    }
}
