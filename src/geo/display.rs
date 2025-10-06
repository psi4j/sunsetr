//! Display and formatting utilities for geo module.
//!
//! This module handles all the visual output and formatting for geographic calculations,
//! including solar debug information, time formatting with timezone conversions, and
//! user-friendly display of transition times.

use anyhow::Result;
use chrono::{Local, NaiveDate, NaiveTime, Offset, TimeZone};
use chrono_tz::Tz;

/// Log detailed solar calculation debug information for given coordinates.
///
/// This function provides comprehensive solar timing diagnostics including:
/// - Raw UTC times and coordinate timezone conversion
/// - Enhanced transition boundaries (+10° to -2° elevation angles)
/// - Civil twilight times for reference (-6° elevation)
/// - Night and day duration calculations
/// - Timezone comparison when coordinate and local timezones differ
/// - Extreme latitude fallback warnings for polar regions
///
/// The output helps users understand exactly when transitions will occur
/// and how the geographic location affects color temperature scheduling.
pub fn log_solar_debug_info(latitude: f64, longitude: f64) -> Result<()> {
    let solar_result = crate::geo::solar::calculate_solar_times_unified(latitude, longitude)?;

    // Check if extreme latitude fallback was used and warn the user
    if solar_result.used_extreme_latitude_fallback {
        log_pipe!();
        log_warning!("⚠️ Using extreme latitude fallback values");
        log_indented!(
            "({})",
            if solar_result.fallback_duration_minutes <= 25 {
                "Summer polar approximation"
            } else {
                "Winter polar approximation"
            }
        );
    }

    let today = Local::now().date_naive();
    let city_tz = solar_result.city_timezone;

    // Calculate night duration (-2° evening to -2° morning)
    let night_duration = if solar_result.sunrise_minus_2_start > solar_result.sunset_minus_2_end {
        // Same day
        solar_result
            .sunrise_minus_2_start
            .signed_duration_since(solar_result.sunset_minus_2_end)
    } else {
        // Crosses midnight
        let time_to_midnight = NaiveTime::from_hms_opt(23, 59, 59)
            .unwrap()
            .signed_duration_since(solar_result.sunset_minus_2_end);
        let time_from_midnight = solar_result
            .sunrise_minus_2_start
            .signed_duration_since(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        time_to_midnight + time_from_midnight + chrono::Duration::seconds(1)
    };

    // Calculate day duration (+10° morning to +10° evening)
    let day_duration = if solar_result.sunset_plus_10_start > solar_result.sunrise_plus_10_end {
        // Same day
        solar_result
            .sunset_plus_10_start
            .signed_duration_since(solar_result.sunrise_plus_10_end)
    } else {
        // Crosses midnight
        let time_to_midnight = NaiveTime::from_hms_opt(23, 59, 59)
            .unwrap()
            .signed_duration_since(solar_result.sunrise_plus_10_end);
        let time_from_midnight = solar_result
            .sunset_plus_10_start
            .signed_duration_since(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        time_to_midnight + time_from_midnight + chrono::Duration::seconds(1)
    };

    log_pipe!();
    log_debug!("Solar calculation details:");
    log_indented!("        Raw coordinates: {latitude:.4}°, {longitude:.4}°");

    // Get sunrise/sunset UTC times
    use sunrise::{Coordinates, SolarDay, SolarEvent};
    let coord = Coordinates::new(latitude, longitude)
        .ok_or_else(|| anyhow::anyhow!("Invalid coordinates"))?;
    let solar_day = SolarDay::new(coord, today);
    let sunrise_utc = solar_day.event_time(SolarEvent::Sunrise);
    let sunset_utc = solar_day.event_time(SolarEvent::Sunset);

    log_indented!("            Sunrise UTC: {}", sunrise_utc.format("%H:%M"));
    log_indented!("             Sunset UTC: {}", sunset_utc.format("%H:%M"));

    // Format city timezone with both name and offset
    let city_offset_secs = {
        let test_datetime = today.and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap());
        city_tz
            .from_local_datetime(&test_datetime)
            .single()
            .map(|dt| dt.offset().fix().local_minus_utc())
            .unwrap_or_else(|| {
                city_tz
                    .from_utc_datetime(&test_datetime)
                    .offset()
                    .fix()
                    .local_minus_utc()
            })
    };
    let city_offset_hours = city_offset_secs / 3600;
    let city_offset_minutes = (city_offset_secs % 3600).abs() / 60;
    let city_offset_str = if city_offset_minutes == 0 {
        format!("{city_offset_hours:+03}:00")
    } else {
        format!("{city_offset_hours:+03}:{city_offset_minutes:02}")
    };

    log_indented!("    Coordinate Timezone: {city_tz} ({city_offset_str})");

    // Show timezone comparison info only if timezones differ
    if !is_city_timezone_same_as_local(&city_tz, today) {
        // Get current time in both timezones
        let now_utc = chrono::Utc::now();
        let now_city = now_utc.with_timezone(&city_tz);
        let now_local = now_utc.with_timezone(&Local);

        // Calculate time difference
        let city_offset_secs = now_city.offset().fix().local_minus_utc();
        let local_offset_secs = now_local.offset().fix().local_minus_utc();
        let offset_diff_secs = city_offset_secs - local_offset_secs;
        let offset_diff = chrono::Duration::seconds(offset_diff_secs as i64);
        let hours_diff = offset_diff.num_hours();
        let minutes_diff = offset_diff.num_minutes() % 60;

        // Get local timezone name using the existing system timezone detection
        let local_tz_name = match crate::geo::timezone::get_system_timezone() {
            Ok(tz) => tz.to_string(),
            Err(_) => {
                // Fallback to timezone abbreviation if system detection fails
                now_local.format("%Z").to_string()
            }
        };

        let local_offset_hours = local_offset_secs / 3600;
        let local_offset_minutes = (local_offset_secs % 3600).abs() / 60;
        let local_offset_str = if local_offset_minutes == 0 {
            format!("{local_offset_hours:+03}:00")
        } else {
            format!("{local_offset_hours:+03}:{local_offset_minutes:02}")
        };

        log_indented!("         Local timezone: {local_tz_name} ({local_offset_str})");
        log_indented!("  Current time (Coords): {}", now_city.format("%H:%M:%S"));
        log_indented!("   Current time (Local): {}", now_local.format("%H:%M:%S"));

        let diff_sign = if hours_diff >= 0 { "+" } else { "" };
        if minutes_diff == 0 {
            log_indented!("        Time difference: {diff_sign}{hours_diff} hours");
        } else {
            log_indented!(
                "        Time difference: {}{} hours {} minutes",
                diff_sign,
                hours_diff,
                minutes_diff.abs()
            );
        }
    }

    // Sunset sequence (descending elevation order)
    log_indented!("--- Sunset (descending) ---");

    log_indented!(
        "Transition start (+10°): {}",
        format_time_with_optional_local(
            solar_result.sunset_plus_10_start,
            &city_tz,
            today,
            "%H:%M:%S"
        )
    );
    log_indented!(
        "Golden hour start (+6°): {}",
        format_time_with_optional_local(
            solar_result.golden_hour_start,
            &city_tz,
            today,
            "%H:%M:%S"
        )
    );
    log_indented!(
        "            Sunset (0°): {}",
        format_time_with_optional_local(solar_result.sunset_time, &city_tz, today, "%H:%M:%S")
    );
    log_indented!(
        "   Transition end (-2°): {}",
        format_time_with_optional_local(
            solar_result.sunset_minus_2_end,
            &city_tz,
            today,
            "%H:%M:%S"
        )
    );
    log_indented!(
        "       Civil dusk (-6°): {}",
        format_time_with_optional_local(solar_result.civil_dusk, &city_tz, today, "%H:%M:%S")
    );
    log_indented!(
        "         Night duration: {} hours {} minutes",
        night_duration.num_hours(),
        night_duration.num_minutes() % 60
    );

    // Sunrise sequence (ascending elevation order)
    log_indented!("--- Sunrise (ascending) ---");

    let tomorrow = today + chrono::Duration::days(1);

    log_indented!(
        "       Civil dawn (-6°): {}",
        format_time_with_optional_local(solar_result.civil_dawn, &city_tz, tomorrow, "%H:%M:%S")
    );
    log_indented!(
        " Transition start (-2°): {}",
        format_time_with_optional_local(
            solar_result.sunrise_minus_2_start,
            &city_tz,
            tomorrow,
            "%H:%M:%S"
        )
    );
    log_indented!(
        "           Sunrise (0°): {}",
        format_time_with_optional_local(solar_result.sunrise_time, &city_tz, tomorrow, "%H:%M:%S")
    );
    log_indented!(
        "  Golden hour end (+6°): {}",
        format_time_with_optional_local(
            solar_result.golden_hour_end,
            &city_tz,
            tomorrow,
            "%H:%M:%S"
        )
    );
    log_indented!(
        "  Transition end (+10°): {}",
        format_time_with_optional_local(
            solar_result.sunrise_plus_10_end,
            &city_tz,
            tomorrow,
            "%H:%M:%S"
        )
    );
    log_indented!(
        "           Day duration: {} hours {} minutes",
        day_duration.num_hours(),
        day_duration.num_minutes() % 60
    );
    log_indented!(
        "        Sunset duration: {} minutes",
        solar_result.sunset_duration.as_secs() / 60
    );
    log_indented!(
        "       Sunrise duration: {} minutes",
        solar_result.sunrise_duration.as_secs() / 60
    );

    Ok(())
}

/// Format a time with optional timezone conversion and display.
///
/// This function intelligently formats times for display, showing both the
/// coordinate's local time and the user's local time when they differ. This
/// dual display is essential for geo mode where selected coordinates may be
/// in a different timezone, helping users understand when transitions occur
/// in both their local time and the coordinate's astronomical time.
///
/// # Display Format
/// - Same timezone: "HH:MM:SS"
/// - Different timezones: "HH:MM:SS [HH:MM:SS]" (coordinate time [user local time])
///
/// # Arguments
/// * `time` - The time to format (in coordinate's timezone)
/// * `city_tz` - The coordinate's timezone
/// * `date` - The date context for accurate timezone conversion
/// * `format_str` - The time format string (e.g., "%H:%M:%S")
///
/// # Returns
/// Formatted string with optional local time in brackets when timezones differ
pub fn format_time_with_optional_local(
    time: NaiveTime,
    city_tz: &Tz,
    date: NaiveDate,
    format_str: &str,
) -> String {
    if is_city_timezone_same_as_local(city_tz, date) {
        // Same timezone - show only the original time
        time.format(format_str).to_string()
    } else {
        // Different timezones - show both times
        let local_time = convert_time_to_local_tz(time, city_tz, date);
        format!(
            "{} [{}]",
            time.format(format_str),
            local_time.format(format_str)
        )
    }
}

/// Convert a NaiveTime from one timezone to another by reconstructing the full datetime.
///
/// Since NaiveTime lacks date and timezone information, we reconstruct a complete
/// DateTime with the proper date and timezone to ensure correct conversion. This
/// approach handles DST transitions and timezone ambiguities gracefully.
///
/// # Arguments
/// * `time` - The time to convert (naive, no timezone info)
/// * `from_tz` - The source timezone (typically the coordinate's timezone)
/// * `date` - The date context for proper DST handling
///
/// # Returns
/// The equivalent time in the user's local timezone
fn convert_time_to_local_tz(time: NaiveTime, from_tz: &Tz, date: NaiveDate) -> NaiveTime {
    // Create a datetime in the source timezone
    let datetime_in_tz = from_tz
        .from_local_datetime(&date.and_time(time))
        .single()
        .unwrap_or_else(|| from_tz.from_utc_datetime(&date.and_time(time)));

    // Convert to local timezone
    Local.from_utc_datetime(&datetime_in_tz.naive_utc()).time()
}

/// Check if the city timezone matches the user's local timezone.
///
/// This optimization prevents redundant timezone display in debug output when
/// the coordinate timezone matches the user's local timezone. The comparison
/// checks UTC offsets at a specific date/time to correctly handle DST boundaries.
///
/// # Arguments
/// * `city_tz` - The timezone of the selected coordinates
/// * `date` - The date for offset comparison (critical for DST accuracy)
///
/// # Returns
/// `true` if both timezones have identical UTC offsets at the given date
fn is_city_timezone_same_as_local(city_tz: &Tz, date: NaiveDate) -> bool {
    // Use a test time to compare timezone offsets
    let test_time = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
    let test_datetime = date.and_time(test_time);

    // Get the offset for both timezones at the given date
    let city_offset = city_tz
        .from_local_datetime(&test_datetime)
        .single()
        .map(|dt| dt.offset().fix())
        .unwrap_or_else(|| city_tz.from_utc_datetime(&test_datetime).offset().fix());

    let local_offset = Local
        .from_local_datetime(&test_datetime)
        .single()
        .map(|dt| dt.offset().fix())
        .unwrap_or_else(|| Local.from_utc_datetime(&test_datetime).offset().fix());

    city_offset == local_offset
}
