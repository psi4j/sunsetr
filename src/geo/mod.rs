//! Geographic location-based sunrise/sunset calculations.
//!
//! This module provides comprehensive geographic functionality for the sunsetr application,
//! enabling location-aware color temperature transitions based on local solar events.
//!
//! ## Module Structure
//!
//! - [`city_selector`]: Interactive city selection with fuzzy search across 10,000+ cities
//! - [`solar`]: Astronomical calculations for sunrise/sunset with extreme latitude handling
//! - [`timezone`]: Automatic location detection based on system timezone settings
//!
//! ## Key Features
//!
//! - **Interactive city selection**: Users can search and select from a comprehensive
//!   database of world cities for precise coordinate determination
//! - **Automatic location detection**: Falls back to timezone-based detection when
//!   manual selection is not desired
//! - **Solar calculations**: Precise sunrise/sunset times with enhanced twilight
//!   transitions using custom elevation angles (+10° to -2°)
//! - **Extreme latitude handling**: Automatic fallback for polar regions where
//!   standard astronomical calculations fail

pub mod city_selector;
pub mod solar;
pub mod timezone;

pub use city_selector::select_city_interactive;
pub use timezone::detect_coordinates_from_timezone;

/// Result of the geo selection workflow.
#[derive(Debug)]
pub enum GeoSelectionResult {
    /// Configuration was updated, instance needs restart
    ConfigUpdated { needs_restart: bool },
    /// No instance running, start new with given debug mode
    StartNew { debug: bool },
    /// User cancelled the selection
    Cancelled,
}

/// Handle the complete --geo flag workflow
///
/// This function manages the geo selection process:
/// 1. Check if instance is running
/// 2. Interactive city selection
/// 3. Config file updates
/// 4. Return appropriate action for main.rs
///
/// # Arguments
/// * `debug_enabled` - Whether debug mode is enabled
///
/// # Returns
/// * `GeoSelectionResult` indicating what main.rs should do next
pub fn handle_geo_selection(debug_enabled: bool) -> anyhow::Result<GeoSelectionResult> {
    use crate::logger::Log;

    Log::log_version();

    if debug_enabled {
        Log::log_pipe();
        Log::log_debug("Debug mode enabled for geo selection");
    }

    // Check if sunsetr is currently running
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let lock_path = format!("{}/sunsetr.lock", runtime_dir);
    let instance_running = is_sunsetr_running(&lock_path);

    if instance_running {
        if debug_enabled {
            Log::log_pipe();
            Log::log_debug("Detected running sunsetr instance");
            Log::log_indented("Will update configuration and restart after city selection");
        }
    } else if debug_enabled {
        Log::log_pipe();
        Log::log_debug("No running instance detected");
        Log::log_indented("Will start sunsetr in background after city selection");
    }

    // Run interactive city selection
    let selection_result = run_city_selection(debug_enabled);

    // Handle cancellation
    let (latitude, longitude, city_name) = match selection_result {
        Ok(coords) => coords,
        Err(e) => {
            if e.to_string().contains("cancelled") {
                return Ok(GeoSelectionResult::Cancelled);
            }
            return Err(e);
        }
    };

    // Update config with selected coordinates or create new config if none exists
    handle_config_update_with_coordinates(latitude, longitude, &city_name)?;

    if instance_running {
        Ok(GeoSelectionResult::ConfigUpdated {
            needs_restart: true,
        })
    } else {
        Ok(GeoSelectionResult::StartNew {
            debug: debug_enabled,
        })
    }
}

/// Run interactive city selection and return the selected coordinates
///
/// This function handles the city selection UI workflow:
/// 1. Display regional selection menu
/// 2. Display cities within selected region  
/// 3. User selects closest city
/// 4. Display calculated sunrise/sunset times
/// 5. Return latitude/longitude coordinates
///
/// # Returns
/// * `Ok((latitude, longitude, city_name))` - Selected city coordinates and name
/// * `Err(_)` - If selection fails or user cancels
pub fn run_city_selection(debug_enabled: bool) -> anyhow::Result<(f64, f64, String)> {
    use crate::logger::Log;
    use anyhow::Context;
    use chrono::Local;

    // Delegate to the city_selector module for the actual implementation
    let (mut latitude, longitude, city_name) =
        select_city_interactive().context("Failed to run interactive city selection")?;

    // Cap latitude at ±65° to avoid solar calculation edge cases
    let was_capped = latitude.abs() > 65.0;
    if was_capped {
        let original_latitude = latitude;
        latitude = 65.0 * latitude.signum();

        Log::log_pipe();
        Log::log_warning(&format!(
            "⚠️ Latitude capped at 65°{} (selected city was at {:.4}°{})",
            if latitude >= 0.0 { "N" } else { "S" },
            original_latitude.abs(),
            if latitude >= 0.0 { "N" } else { "S" },
        ));
        Log::log_indented("Are you researching extremophile bacteria under the ice caps?");
        Log::log_indented("Consider using manual sunset/sunrise times for better accuracy.");
    }

    // Show calculated sunrise/sunset times using solar module
    let today = Local::now().date_naive();

    // Calculate the actual transition windows using our enhanced +10° to -2° method
    match crate::geo::solar::calculate_civil_twilight_times_for_display(
        latitude,
        longitude,
        today,
        debug_enabled,
    ) {
        Ok((
            sunset_time,
            sunset_start,
            sunset_end,
            sunrise_time,
            sunrise_start,
            sunrise_end,
            sunset_duration,
            sunrise_duration,
        )) => {
            Log::log_block_start(&format!(
                "Sun times for {} ({:.4}°{}, {:.4}°{})",
                city_name,
                latitude.abs(),
                if latitude >= 0.0 { "N" } else { "S" },
                longitude.abs(),
                if longitude >= 0.0 { "E" } else { "W" }
            ));

            // Display sunset info (happening today)
            Log::log_indented(&format!(
                "Today's sunset: {} (transition from {} to {})",
                sunset_time.format("%H:%M"),
                sunset_start.format("%H:%M"),
                sunset_end.format("%H:%M")
            ));

            // Display sunrise info (happening tomorrow)
            Log::log_indented(&format!(
                "Tomorrow's sunrise: {} (transition from {} to {})",
                sunrise_time.format("%H:%M"),
                sunrise_start.format("%H:%M"),
                sunrise_end.format("%H:%M")
            ));

            Log::log_indented(&format!(
                "Sunset transition duration: {} minutes",
                sunset_duration.as_secs() / 60
            ));

            Log::log_indented(&format!(
                "Sunrise transition duration: {} minutes",
                sunrise_duration.as_secs() / 60
            ));
        }
        Err(e) => {
            Log::log_warning(&format!("Could not calculate sun times: {}", e));
            Log::log_indented("Using default transition times");
        }
    }

    Ok((latitude, longitude, city_name))
}

/// Handle config update with coordinates, creating new config if none exists.
///
/// This function updates an existing configuration with new geographic coordinates
/// or creates a new configuration file if none exists. It automatically sets the
/// transition mode to "geo" when coordinates are provided.
///
/// # Arguments
/// * `latitude` - Geographic latitude in degrees
/// * `longitude` - Geographic longitude in degrees
/// * `city_name` - Human-readable city name for logging
///
/// # Errors
/// Returns an error if:
/// - Config path cannot be determined
/// - Config file cannot be read/written
/// - Config update fails
fn handle_config_update_with_coordinates(
    latitude: f64,
    longitude: f64,
    city_name: &str,
) -> anyhow::Result<()> {
    use crate::config::Config;
    use crate::logger::Log;

    let config_path = Config::get_config_path()?;

    if config_path.exists() {
        // Config exists, update it
        Config::update_config_with_geo_coordinates(latitude, longitude)?;
    } else {
        // No config exists, create new config with geo coordinates
        Log::log_block_start("No existing configuration found");
        Log::log_indented("Creating new configuration with selected location");

        // Create default config with selected coordinates (skips timezone detection)
        Config::create_default_config(
            &config_path,
            Some((latitude, longitude, city_name.to_string())),
        )?;

        Log::log_block_start(&format!(
            "Created new config file: {}",
            crate::utils::path_for_display(&config_path)
        ));
        Log::log_indented(&format!("Latitude: {}", latitude));
        Log::log_indented(&format!("Longitude: {}", longitude));
        Log::log_indented("Transition mode: geo");
    }

    Ok(())
}

/// Convert a NaiveTime from one timezone to another by reconstructing the full datetime.
///
/// Since NaiveTime doesn't have date/timezone info, we need to reconstruct it with the
/// proper date and timezone to convert correctly. This handles DST transitions and
/// timezone ambiguities gracefully.
///
/// # Arguments
/// * `time` - The time to convert (naive, no timezone info)
/// * `from_tz` - The source timezone
/// * `date` - The date context for proper DST handling
///
/// # Returns
/// The equivalent time in the user's local timezone
fn convert_time_to_local_tz(
    time: chrono::NaiveTime,
    from_tz: &chrono_tz::Tz,
    date: chrono::NaiveDate,
) -> chrono::NaiveTime {
    use chrono::{Local, TimeZone};

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
/// This is used to optimize debug output by avoiding redundant timezone conversions
/// and display when both timezones are the same. The comparison is done by checking
/// UTC offsets at a specific date/time to handle DST correctly.
///
/// # Arguments
/// * `city_tz` - The timezone to compare against local
/// * `date` - The date for offset comparison (important for DST)
///
/// # Returns
/// `true` if the timezones have the same UTC offset at the given date
fn is_city_timezone_same_as_local(city_tz: &chrono_tz::Tz, date: chrono::NaiveDate) -> bool {
    use chrono::{Local, Offset, TimeZone};

    // Use a test time to compare timezone offsets
    let test_time = chrono::NaiveTime::from_hms_opt(12, 0, 0).unwrap();
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

/// Format a time with optional timezone conversion and display.
///
/// This helper function formats times for display, showing both the city's local time
/// and the user's local time when they differ. This is crucial for geo mode where
/// the selected coordinates might be in a different timezone than the user.
///
/// # Display Format
/// - Same timezone: "HH:MM:SS"
/// - Different timezones: "HH:MM:SS \[HH:MM:SS\]" (city time \[user local time\])
///
/// # Arguments
/// * `time` - The time to format (in city timezone)
/// * `city_tz` - The city's timezone
/// * `date` - The date context for timezone conversion
/// * `format_str` - The time format string (e.g., "%H:%M:%S")
///
/// # Returns
/// Formatted string with optional local time in brackets
fn format_time_with_optional_local(
    time: chrono::NaiveTime,
    city_tz: &chrono_tz::Tz,
    date: chrono::NaiveDate,
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

/// Check if sunsetr is currently running by testing the lock file.
///
/// This function attempts to acquire an exclusive lock on the lock file to determine
/// if another instance is running. If the lock cannot be acquired, another instance
/// is active.
///
/// # Arguments
/// * `lock_path` - Path to the lock file (typically in XDG_RUNTIME_DIR or /tmp)
///
/// # Returns
/// `true` if another instance is running, `false` otherwise
fn is_sunsetr_running(lock_path: &str) -> bool {
    use fs2::FileExt;
    use std::fs::File;

    if let Ok(lock_file) = File::open(lock_path) {
        lock_file.try_lock_exclusive().is_err()
    } else {
        false
    }
}

/// Log detailed solar calculation debug information for given coordinates
///
/// This function calculates and displays comprehensive solar timing information
/// including sunrise/sunset times, transition boundaries, and durations.
/// It also warns if extreme latitude fallback values are used.
pub fn log_solar_debug_info(latitude: f64, longitude: f64) -> anyhow::Result<()> {
    use crate::logger::Log;

    let solar_result = crate::geo::solar::calculate_solar_times_unified(latitude, longitude)?;

    // Check if extreme latitude fallback was used and warn the user
    if solar_result.used_extreme_latitude_fallback {
        Log::log_pipe();
        Log::log_warning("⚠️ Using extreme latitude fallback values");
        Log::log_indented(&format!(
            "({})",
            if solar_result.fallback_duration_minutes <= 25 {
                "Summer polar approximation"
            } else {
                "Winter polar approximation"
            }
        ));
    }

    let today = chrono::Local::now().date_naive();
    let city_tz = solar_result.city_timezone;

    // Calculate night duration (-2° evening to -2° morning)
    let night_duration = if solar_result.sunrise_minus_2_start > solar_result.sunset_minus_2_end {
        // Same day
        solar_result
            .sunrise_minus_2_start
            .signed_duration_since(solar_result.sunset_minus_2_end)
    } else {
        // Crosses midnight
        let time_to_midnight = chrono::NaiveTime::from_hms_opt(23, 59, 59)
            .unwrap()
            .signed_duration_since(solar_result.sunset_minus_2_end);
        let time_from_midnight = solar_result
            .sunrise_minus_2_start
            .signed_duration_since(chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap());
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
        let time_to_midnight = chrono::NaiveTime::from_hms_opt(23, 59, 59)
            .unwrap()
            .signed_duration_since(solar_result.sunrise_plus_10_end);
        let time_from_midnight = solar_result
            .sunset_plus_10_start
            .signed_duration_since(chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        time_to_midnight + time_from_midnight + chrono::Duration::seconds(1)
    };

    Log::log_pipe();
    Log::log_debug("Solar calculation details:");
    Log::log_indented(&format!(
        "        Raw coordinates: {:.4}°, {:.4}°",
        latitude, longitude
    ));

    // Get sunrise/sunset UTC times
    use sunrise::{Coordinates, SolarDay, SolarEvent};
    let coord = Coordinates::new(latitude, longitude)
        .ok_or_else(|| anyhow::anyhow!("Invalid coordinates"))?;
    let solar_day = SolarDay::new(coord, today);
    let sunrise_utc = solar_day.event_time(SolarEvent::Sunrise);
    let sunset_utc = solar_day.event_time(SolarEvent::Sunset);

    Log::log_indented(&format!(
        "            Sunrise UTC: {}",
        sunrise_utc.format("%H:%M")
    ));
    Log::log_indented(&format!(
        "             Sunset UTC: {}",
        sunset_utc.format("%H:%M")
    ));
    // Format city timezone with both name and offset
    let city_offset_secs = {
        use chrono::{Offset, TimeZone};
        let test_datetime = today.and_time(chrono::NaiveTime::from_hms_opt(12, 0, 0).unwrap());
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
        format!("{:+03}:00", city_offset_hours)
    } else {
        format!("{:+03}:{:02}", city_offset_hours, city_offset_minutes)
    };

    Log::log_indented(&format!(
        "    Coordinate Timezone: {} ({})",
        city_tz, city_offset_str
    ));

    // Show timezone comparison info only if timezones differ
    if !is_city_timezone_same_as_local(&city_tz, today) {
        use chrono::{Local, Offset};

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
            format!("{:+03}:00", local_offset_hours)
        } else {
            format!("{:+03}:{:02}", local_offset_hours, local_offset_minutes)
        };

        Log::log_indented(&format!(
            "         Local timezone: {} ({})",
            local_tz_name, local_offset_str
        ));
        Log::log_indented(&format!(
            "  Current time (Coords): {}",
            now_city.format("%H:%M:%S")
        ));
        Log::log_indented(&format!(
            "   Current time (Local): {}",
            now_local.format("%H:%M:%S")
        ));

        let diff_sign = if hours_diff >= 0 { "+" } else { "" };
        if minutes_diff == 0 {
            Log::log_indented(&format!(
                "        Time difference: {}{} hours",
                diff_sign, hours_diff
            ));
        } else {
            Log::log_indented(&format!(
                "        Time difference: {}{} hours {} minutes",
                diff_sign,
                hours_diff,
                minutes_diff.abs()
            ));
        }
    }

    // Sunset sequence (descending elevation order)
    Log::log_indented("--- Sunset (descending) ---");

    Log::log_indented(&format!(
        "Transition start (+10°): {}",
        format_time_with_optional_local(
            solar_result.sunset_plus_10_start,
            &city_tz,
            today,
            "%H:%M:%S"
        )
    ));
    Log::log_indented(&format!(
        "Golden hour start (+6°): {}",
        format_time_with_optional_local(
            solar_result.golden_hour_start,
            &city_tz,
            today,
            "%H:%M:%S"
        )
    ));
    Log::log_indented(&format!(
        "            Sunset (0°): {}",
        format_time_with_optional_local(solar_result.sunset_time, &city_tz, today, "%H:%M:%S")
    ));
    Log::log_indented(&format!(
        "   Transition end (-2°): {}",
        format_time_with_optional_local(
            solar_result.sunset_minus_2_end,
            &city_tz,
            today,
            "%H:%M:%S"
        )
    ));
    Log::log_indented(&format!(
        "       Civil dusk (-6°): {}",
        format_time_with_optional_local(solar_result.civil_dusk, &city_tz, today, "%H:%M:%S")
    ));
    Log::log_indented(&format!(
        "         Night duration: {} hours {} minutes",
        night_duration.num_hours(),
        night_duration.num_minutes() % 60
    ));

    // Sunrise sequence (ascending elevation order)
    Log::log_indented("--- Sunrise (ascending) ---");

    let tomorrow = today + chrono::Duration::days(1);

    Log::log_indented(&format!(
        "       Civil dawn (-6°): {}",
        format_time_with_optional_local(solar_result.civil_dawn, &city_tz, tomorrow, "%H:%M:%S")
    ));
    Log::log_indented(&format!(
        " Transition start (-2°): {}",
        format_time_with_optional_local(
            solar_result.sunrise_minus_2_start,
            &city_tz,
            tomorrow,
            "%H:%M:%S"
        )
    ));
    Log::log_indented(&format!(
        "           Sunrise (0°): {}",
        format_time_with_optional_local(solar_result.sunrise_time, &city_tz, tomorrow, "%H:%M:%S")
    ));
    Log::log_indented(&format!(
        "  Golden hour end (+6°): {}",
        format_time_with_optional_local(
            solar_result.golden_hour_end,
            &city_tz,
            tomorrow,
            "%H:%M:%S"
        )
    ));
    Log::log_indented(&format!(
        "  Transition end (+10°): {}",
        format_time_with_optional_local(
            solar_result.sunrise_plus_10_end,
            &city_tz,
            tomorrow,
            "%H:%M:%S"
        )
    ));
    Log::log_indented(&format!(
        "           Day duration: {} hours {} minutes",
        day_duration.num_hours(),
        day_duration.num_minutes() % 60
    ));
    Log::log_indented(&format!(
        "        Sunset duration: {} minutes",
        solar_result.sunset_duration.as_secs() / 60
    ));
    Log::log_indented(&format!(
        "       Sunrise duration: {} minutes",
        solar_result.sunrise_duration.as_secs() / 60
    ));

    Ok(())
}
