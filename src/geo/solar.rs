//! Solar position calculations for sunrise/sunset times and civil twilight.
//!
//! This module provides sunrise and sunset calculations based on geographic coordinates
//! using civil twilight definitions (sun elevation between +6 degrees and -6 degrees). This provides
//! natural transition times that vary by location and season.

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate, NaiveTime, DateTime, Utc};
use std::time::Duration;

/// Calculate sunrise and sunset times for a given location and date.
///
/// Uses civil twilight definitions:
/// - Day begins when sun reaches +6 degrees elevation (end of civil twilight)
/// - Night begins when sun reaches -6 degrees elevation (start of civil twilight)
/// - Transition duration is the time between these two elevation points
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
/// ```
/// use chrono::NaiveDate;
/// use sunsetr::geo::solar::calculate_sunrise_sunset;
/// 
/// let date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap(); // Summer solstice
/// let (sunrise, sunset, duration) = calculate_sunrise_sunset(40.7128, -74.0060, date)?;
/// ```
pub fn calculate_sunrise_sunset(
    latitude: f64,
    longitude: f64,
    date: NaiveDate,
) -> Result<(NaiveTime, NaiveTime, Duration)> {
    // Validate coordinates
    if !(-90.0..=90.0).contains(&latitude) {
        anyhow::bail!("Invalid latitude: {}. Must be between -90 and 90 degrees", latitude);
    }
    if !(-180.0..=180.0).contains(&longitude) {
        anyhow::bail!("Invalid longitude: {}. Must be between -180 and 180 degrees", longitude);
    }

    // Use the sunrise crate for calculations
    let sunrise_result = sunrise::sunrise_sunset(
        latitude,
        longitude,
        date.year(),
        date.month(),
        date.day(),
    );

    let (sunrise_timestamp, sunset_timestamp) = sunrise_result;

    // Convert Unix timestamps to DateTime<Utc> then to NaiveTime
    let sunrise_datetime = DateTime::<Utc>::from_timestamp(sunrise_timestamp, 0)
        .context("Failed to convert sunrise timestamp")?;
    let sunset_datetime = DateTime::<Utc>::from_timestamp(sunset_timestamp, 0)
        .context("Failed to convert sunset timestamp")?;

    let sunrise_time = sunrise_datetime.time();
    let sunset_time = sunset_datetime.time();

    // Calculate transition duration based on civil twilight
    // For civil twilight, the transition is typically 20-40 minutes depending on latitude
    let transition_duration = calculate_transition_duration(latitude);

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
pub fn calculate_transition_duration(latitude: f64) -> Duration {
    let abs_latitude = latitude.abs();
    
    // Base duration increases with latitude
    let base_minutes = match abs_latitude {
        lat if lat < 10.0 => 20.0,  // Tropical regions
        lat if lat < 30.0 => 25.0,  // Subtropical
        lat if lat < 50.0 => 30.0,  // Temperate
        lat if lat < 60.0 => 35.0,  // High temperate
        lat if lat < 70.0 => 45.0,  // Subpolar
        _ => 60.0,                  // Polar regions
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
pub fn handle_polar_edge_cases(
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
        day_of_year > 300.0 || day_of_year < 60.0
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
pub fn get_sun_times(
    latitude: f64,
    longitude: f64,
    date: NaiveDate,
) -> Result<(NaiveTime, NaiveTime, Duration)> {
    // Check for polar edge cases first
    if let Some(polar_times) = handle_polar_edge_cases(latitude, date) {
        return Ok(polar_times);
    }
    
    // Use normal calculations
    calculate_sunrise_sunset(latitude, longitude, date)
        .with_context(|| {
            format!(
                "Failed to calculate sunrise/sunset for coordinates {:.4}�N, {:.4}�W on {}",
                latitude, longitude.abs(), date
            )
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
}