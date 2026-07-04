//! Display and formatting utilities for the geo module.
//!
//! Solar debug output and time formatting with timezone conversions.

use anyhow::Result;
use chrono::{Local, NaiveDate, NaiveTime, Offset, TimeZone};
use chrono_tz::Tz;

/// Log a detailed solar calculation breakdown for the given coordinates.
pub fn log_solar_debug_info(latitude: f64, longitude: f64) -> Result<()> {
    let city_tz = crate::geo::solar::determine_timezone(latitude, longitude);
    let now = crate::time::source::now();
    let now_in_tz = now.with_timezone(&city_tz);
    let today = now_in_tz.date_naive();

    let solar_result = crate::geo::solar::calculate_solar_times(latitude, longitude, today)?;

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

    let night_duration = if solar_result.sunrise_minus_2_start > solar_result.sunset_minus_2_end {
        solar_result
            .sunrise_minus_2_start
            .signed_duration_since(solar_result.sunset_minus_2_end)
    } else {
        let time_to_midnight = NaiveTime::from_hms_opt(23, 59, 59)
            .unwrap()
            .signed_duration_since(solar_result.sunset_minus_2_end);
        let time_from_midnight = solar_result
            .sunrise_minus_2_start
            .signed_duration_since(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        time_to_midnight + time_from_midnight + chrono::Duration::seconds(1)
    };

    let day_duration = if solar_result.sunset_plus_10_start > solar_result.sunrise_plus_10_end {
        solar_result
            .sunset_plus_10_start
            .signed_duration_since(solar_result.sunrise_plus_10_end)
    } else {
        let time_to_midnight = NaiveTime::from_hms_opt(23, 59, 59)
            .unwrap()
            .signed_duration_since(solar_result.sunrise_plus_10_end);
        let time_from_midnight = solar_result
            .sunset_plus_10_start
            .signed_duration_since(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        time_to_midnight + time_from_midnight + chrono::Duration::seconds(1)
    };

    log_pipe!();
    log_debug!(
        "Solar calculation details for {}:",
        today.format("%Y-%m-%d")
    );
    log_indented!("        Raw coordinates: {latitude:.4}°, {longitude:.4}°");

    use sunrise::{Coordinates, SolarDay, SolarEvent};
    let coord = Coordinates::new(latitude, longitude)
        .ok_or_else(|| anyhow::anyhow!("Invalid coordinates"))?;
    let solar_day = SolarDay::new(coord, today);
    let sunrise_utc = solar_day.event_time(SolarEvent::Sunrise);
    let sunset_utc = solar_day.event_time(SolarEvent::Sunset);

    let fmt_utc = |t: Option<chrono::DateTime<chrono::Utc>>| {
        t.map_or_else(|| "N/A".to_string(), |t| t.format("%H:%M").to_string())
    };
    log_indented!("            Sunrise UTC: {}", fmt_utc(sunrise_utc));
    log_indented!("             Sunset UTC: {}", fmt_utc(sunset_utc));

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

    if !is_city_timezone_same_as_local(&city_tz, today) {
        let now_utc = chrono::Utc::now();
        let now_city = now_utc.with_timezone(&city_tz);
        let now_local = now_utc.with_timezone(&Local);

        let city_offset_secs = now_city.offset().fix().local_minus_utc();
        let local_offset_secs = now_local.offset().fix().local_minus_utc();
        let offset_diff_secs = city_offset_secs - local_offset_secs;
        let offset_diff = chrono::Duration::seconds(offset_diff_secs as i64);
        let hours_diff = offset_diff.num_hours();
        let minutes_diff = offset_diff.num_minutes() % 60;

        let local_tz_name = match crate::geo::timezone::get_system_timezone() {
            Ok(tz) => tz.to_string(),
            Err(_) => now_local.format("%Z").to_string(),
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

    log_indented!("--- Sunrise (ascending) ---");

    log_indented!(
        "       Civil dawn (-6°): {}",
        format_time_with_optional_local(solar_result.civil_dawn, &city_tz, today, "%H:%M:%S")
    );
    log_indented!(
        " Transition start (-2°): {}",
        format_time_with_optional_local(
            solar_result.sunrise_minus_2_start,
            &city_tz,
            today,
            "%H:%M:%S"
        )
    );
    log_indented!(
        "           Sunrise (0°): {}",
        format_time_with_optional_local(solar_result.sunrise_time, &city_tz, today, "%H:%M:%S")
    );
    log_indented!(
        "  Golden hour end (+6°): {}",
        format_time_with_optional_local(solar_result.golden_hour_end, &city_tz, today, "%H:%M:%S")
    );
    log_indented!(
        "  Transition end (+10°): {}",
        format_time_with_optional_local(
            solar_result.sunrise_plus_10_end,
            &city_tz,
            today,
            "%H:%M:%S"
        )
    );
    log_indented!(
        "       Sunrise duration: {} minutes",
        solar_result.sunrise_duration.as_secs() / 60
    );
    log_indented!(
        "           Day duration: {} hours {} minutes ({})",
        day_duration.num_hours(),
        day_duration.num_minutes() % 60,
        today.format("%m-%d")
    );

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

    let tomorrow = today + chrono::Duration::days(1);
    log_indented!(
        "        Sunset duration: {} minutes",
        solar_result.sunset_duration.as_secs() / 60
    );
    log_indented!(
        "         Night duration: {} hours {} minutes ({} → {})",
        night_duration.num_hours(),
        night_duration.num_minutes() % 60,
        today.format("%m-%d"),
        tomorrow.format("%m-%d")
    );

    Ok(())
}

/// Format a coordinate-timezone time, appending the user's local time in
/// brackets when the two timezones differ.
///
/// The dual display matters in geo mode, where the selected coordinates can sit
/// in a different timezone than the user.
pub fn format_time_with_optional_local(
    time: NaiveTime,
    city_tz: &Tz,
    date: NaiveDate,
    format_str: &str,
) -> String {
    if is_city_timezone_same_as_local(city_tz, date) {
        time.format(format_str).to_string()
    } else {
        let local_time = convert_time_to_local_tz(time, city_tz, date);
        format!(
            "{} [{}]",
            time.format(format_str),
            local_time.format(format_str)
        )
    }
}

/// Convert a NaiveTime from `from_tz` to the user's local timezone.
///
/// NaiveTime carries no date or timezone, so the conversion rebuilds a full
/// DateTime from `date` to resolve DST transitions and ambiguous local times.
fn convert_time_to_local_tz(time: NaiveTime, from_tz: &Tz, date: NaiveDate) -> NaiveTime {
    let datetime_in_tz = from_tz
        .from_local_datetime(&date.and_time(time))
        .single()
        .unwrap_or_else(|| from_tz.from_utc_datetime(&date.and_time(time)));

    Local.from_utc_datetime(&datetime_in_tz.naive_utc()).time()
}

/// Whether the coordinate timezone has the same UTC offset as the user's local
/// timezone on `date`.
///
/// Compared at a specific date because DST can make two timezones agree on some
/// dates and differ on others.
fn is_city_timezone_same_as_local(city_tz: &Tz, date: NaiveDate) -> bool {
    let test_time = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
    let test_datetime = date.and_time(test_time);

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
