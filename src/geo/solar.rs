//! Solar position calculations for sunrise/sunset times and enhanced twilight transitions.
//!
//! This module provides comprehensive solar calculations for the sunsetr blue light filter.
//! It calculates precise sunrise and sunset times based on geographic coordinates, with special
//! handling for extreme latitudes where standard astronomical calculations may fail.
//!
//! ## Key Features
//!
//! - **Enhanced twilight transitions**: Uses custom elevation angles (+10° to -2°) for smooth
//!   color temperature transitions, providing longer and more natural transition periods than
//!   using traditional sunset to civil twilight (O° to -6°)
//! - **Extreme latitude handling**: Comprehensive validation detects when astronomical calculations
//!   fail at high latitudes (>55°) and provides seasonal-aware fallback durations
//! - **Timezone precision**: Automatically determines timezone from coordinates using precise
//!   boundary data, ensuring times are calculated in the correct local timezone
//! - **Robust validation**: Multi-layered validation catches edge cases like identical times,
//!   invalid sequences, and impossible day/night cycles
//!
//! ## Solar Elevation Angles
//!
//! The module calculates transitions using these key elevation angles:
//! - **+10°**: Enhanced transition start (sunset) / end (sunrise)
//! - **0°**: Actual sunrise/sunset (geometric horizon)
//! - **-2°**: Enhanced transition end (sunset) / start (sunrise)
//! - **-6°**: Civil twilight (traditional, used for baseline calculations)
//!
//! ## Extreme Latitude Behavior
//!
//! For latitudes above 55°, the module detects when solar calculations produce invalid results
//! (common in polar regions) and switches to seasonal-aware fallback durations:
//! - **Summer**: 25-minute transitions (midnight sun conditions)
//! - **Winter**: 45-minute transitions (polar night conditions)
//!
//! These fallbacks ensure the application continues to function smoothly even in extreme
//! geographic conditions where traditional solar calculations break down.

use anyhow::Result;
use chrono::{Datelike, NaiveDate, NaiveTime};
use std::time::Duration;

/// Complete solar calculation result containing all transition times and metadata.
///
/// This structure provides comprehensive solar timing information for a specific location,
/// including enhanced transition boundaries, traditional civil twilight times, and metadata
/// about whether fallback calculations were used for extreme latitudes.
///
/// All times are returned in the location's local timezone for immediate use.
#[derive(Debug, Clone)]
pub struct SolarCalculationResult {
    /// **Core solar events** (all times in location's timezone)

    /// Actual sunset time (sun at 0° elevation)
    pub sunset_time: NaiveTime,
    /// Actual sunrise time (sun at 0° elevation)  
    pub sunrise_time: NaiveTime,
    /// Total duration of sunset transition (+10° to -2°)
    pub sunset_duration: Duration,
    /// Total duration of sunrise transition (-2° to +10°)
    pub sunrise_duration: Duration,

    /// **Enhanced transition boundaries** for geo mode (location timezone)

    /// Sunset transition start (sun at +10° elevation)
    pub sunset_plus_10_start: NaiveTime,
    /// Sunset transition end (sun at -2° elevation)
    pub sunset_minus_2_end: NaiveTime,
    /// Sunrise transition start (sun at -2° elevation)
    pub sunrise_minus_2_start: NaiveTime,
    /// Sunrise transition end (sun at +10° elevation)
    pub sunrise_plus_10_end: NaiveTime,

    /// **Traditional civil twilight boundaries** (location timezone)

    /// Civil dawn (sun at -6° elevation, morning)
    pub civil_dawn: NaiveTime,
    /// Civil dusk (sun at -6° elevation, evening)
    pub civil_dusk: NaiveTime,

    /// **Golden hour boundaries** (location timezone)

    /// Golden hour start (sunset - civil_twilight_duration)
    pub golden_hour_start: NaiveTime,
    /// Golden hour end (sunrise + civil_twilight_duration)
    pub golden_hour_end: NaiveTime,

    /// **Location and calculation metadata**

    /// Timezone for the coordinates (determined automatically)
    pub city_timezone: chrono_tz::Tz,
    /// Whether fallback durations were used due to extreme latitude
    pub used_extreme_latitude_fallback: bool,
    /// Fallback duration in minutes (25 for summer, 45 for winter)
    pub fallback_duration_minutes: u32,
}

/// Type alias for civil twilight display data returned to the UI layer.
///
/// This tuple contains all the timing information needed to display sunset/sunrise
/// information to users, including the enhanced transition boundaries used by geo mode.
///
/// # Tuple Contents
/// 0. `sunset_time` - Actual sunset (0° elevation)
/// 1. `sunset_start` - Enhanced transition start (+10° elevation)
/// 2. `sunset_end` - Enhanced transition end (-2° elevation)
/// 3. `sunrise_time` - Actual sunrise (0° elevation)
/// 4. `sunrise_start` - Enhanced transition start (-2° elevation)
/// 5. `sunrise_end` - Enhanced transition end (+10° elevation)
/// 6. `sunset_duration` - Total sunset transition duration
/// 7. `sunrise_duration` - Total sunrise transition duration
///
/// All times are in the location's local timezone.
type CivilTwilightDisplayData = (
    chrono::NaiveTime,   // sunset_time (0°)
    chrono::NaiveTime,   // sunset_start (+10°)
    chrono::NaiveTime,   // sunset_end (-2°)
    chrono::NaiveTime,   // sunrise_time (0°)
    chrono::NaiveTime,   // sunrise_start (-2°)
    chrono::NaiveTime,   // sunrise_end (+10°)
    std::time::Duration, // sunset_duration
    std::time::Duration, // sunrise_duration
);

/// Calculate enhanced twilight transition times for display in the user interface.
///
/// This function returns the precise transition boundaries used by geo mode, which differ
/// from traditional civil twilight by using custom elevation angles (+10° to -2°) that
/// provide longer, more natural color temperature transitions.
///
/// The function automatically handles extreme latitudes by detecting when astronomical
/// calculations fail and switching to seasonal-aware fallback durations. All times are
/// returned in the location's local timezone for immediate display.
///
/// # Arguments
/// * `latitude` - Geographic latitude in degrees (-90.0 to +90.0)
/// * `longitude` - Geographic longitude in degrees (-180.0 to +180.0)
/// * `date` - Date for calculations (currently unused - uses current system date)
/// * `_debug_enabled` - Debug flag (currently unused)
///
/// # Returns
/// Tuple containing all timing information for UI display:
/// - Sunset/sunrise times and enhanced transition boundaries
/// - Transition durations for progress indication
/// - All times in location's local timezone
///
/// # Errors
/// Returns an error if:
/// - Coordinates are invalid (outside valid ranges)
/// - Timezone detection fails
/// - Solar calculation library encounters an error
///
/// # Example
/// ```rust
/// # use sunsetr::geo::solar::calculate_civil_twilight_times_for_display;
/// # use chrono::NaiveDate;
/// let today = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
/// let result = calculate_civil_twilight_times_for_display(40.7128, -74.0060, today, false)?;
/// let (sunset_time, sunset_start, sunset_end, _, _, _, sunset_duration, _) = result;
/// println!("Sunset transition: {} to {} (duration: {} minutes)",
///          sunset_start.format("%H:%M"),
///          sunset_end.format("%H:%M"),
///          sunset_duration.as_secs() / 60);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn calculate_civil_twilight_times_for_display(
    latitude: f64,
    longitude: f64,
    date: chrono::NaiveDate,
    _debug_enabled: bool,
) -> Result<CivilTwilightDisplayData, anyhow::Error> {
    // Use the unified calculation function that handles extreme latitudes automatically
    let result = calculate_solar_times_unified(latitude, longitude, date)?;

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

/// Unified solar calculation function that handles all scenarios including extreme latitudes.
///
/// This is the single source of truth for all solar calculations. It returns complete
/// information about sunset/sunrise times, transition boundaries, and civil twilight
/// times, all in the city's timezone. Other functions should use this for consistency.
///
/// # Arguments
/// * `latitude` - Geographic latitude in degrees
/// * `longitude` - Geographic longitude in degrees
/// * `date` - The date for which to calculate solar times (NaiveDate)
///
/// # Returns
/// Complete solar calculation result with all times in city timezone
pub fn calculate_solar_times_unified(
    latitude: f64,
    longitude: f64,
    date: NaiveDate,
) -> Result<SolarCalculationResult, anyhow::Error> {
    use sunrise::{Coordinates, DawnType, SolarDay, SolarEvent};

    // Step 1: Determine the precise timezone for these coordinates
    // This is critical for ensuring all calculations are in the correct local time
    let city_tz = determine_timezone_from_coordinates(latitude, longitude);

    // Step 2: Create coordinate object and validate input
    // The sunrise crate will reject coordinates outside valid ranges
    let coord = Coordinates::new(latitude, longitude).ok_or_else(|| {
        anyhow::anyhow!("Invalid coordinates: lat={}, lon={}", latitude, longitude)
    })?;
    let solar_day = SolarDay::new(coord, date);

    // Step 3: Calculate core solar events using astronomical algorithms
    // All calculations start in UTC and are converted to city timezone

    // Sunset and sunrise (sun at geometric horizon, 0° elevation)
    let sunset_utc = solar_day.event_time(SolarEvent::Sunset);
    let sunset_time = sunset_utc.with_timezone(&city_tz).time();

    let sunrise_utc = solar_day.event_time(SolarEvent::Sunrise);
    let sunrise_time = sunrise_utc.with_timezone(&city_tz).time();

    // Civil twilight boundaries (sun at -6° elevation)
    // These are used as baseline for calculating enhanced transition durations
    let civil_dusk_utc = solar_day.event_time(SolarEvent::Dusk(DawnType::Civil));
    let civil_dusk = civil_dusk_utc.with_timezone(&city_tz).time();

    let civil_dawn_utc = solar_day.event_time(SolarEvent::Dawn(DawnType::Civil));
    let civil_dawn = civil_dawn_utc.with_timezone(&city_tz).time();

    // Step 4: Calculate baseline civil twilight durations
    // These durations are used to derive the enhanced transition timings
    let sunset_to_civil_dusk_duration = if civil_dusk > sunset_time {
        civil_dusk.signed_duration_since(sunset_time)
    } else {
        // Handle edge case where civil dusk precedes sunset (can happen at extreme latitudes)
        chrono::Duration::zero()
    };

    let civil_dawn_to_sunrise_duration = if sunrise_time > civil_dawn {
        sunrise_time.signed_duration_since(civil_dawn)
    } else {
        // Handle edge case where sunrise precedes civil dawn (can happen at extreme latitudes)
        chrono::Duration::zero()
    };

    // Step 5: Comprehensive validation to detect calculation failures
    // At extreme latitudes, astronomical calculations often produce invalid results
    let abs_latitude = latitude.abs();
    let is_extreme_latitude = abs_latitude > 55.0; // Threshold lowered from 60° to catch more edge cases

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

    // Step 6: Determine if fallback calculations are needed
    // Only extreme latitudes (>55°) with failed validation require fallback
    let (used_fallback, fallback_minutes) = if is_extreme_latitude && solar_calculation_failed {
        let day_of_year = date.ordinal();

        // **Seasonal awareness**: Polar regions have different lighting conditions by season
        let is_summer = if latitude > 0.0 {
            // Northern hemisphere: summer solstice around day 172 (June 21)
            // Extended range accounts for long polar day period
            (120..=240).contains(&day_of_year)
        } else {
            // Southern hemisphere: summer solstice around day 355 (December 21)
            // Inverted logic: summer when northern hemisphere is in winter
            !(60..=300).contains(&day_of_year)
        };

        // **Fallback durations** are based on empirical observations of polar lighting
        // Since user coordinates are capped at ±65°, we use moderate fallback values
        let minutes = if is_summer {
            25 // Summer: shorter transitions during polar day conditions
        } else {
            45 // Winter: longer transitions during polar night conditions
        };
        (true, minutes)
    } else {
        // Normal latitudes or successful calculations don't need fallback
        (false, 30) // Default fallback (unused in practice)
    };

    // Step 7: Calculate final transition boundaries and durations
    // Use either calculated values or fallback values depending on validation results
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
