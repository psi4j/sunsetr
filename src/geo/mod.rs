//! Geographic location-based sunrise/sunset calculations.
//!
//! This module provides functionality for:
//! - Interactive city selection
//! - Timezone-based coordinate detection
//! - Solar calculations for sunrise/sunset times
//! - Civil twilight duration calculations

pub mod city_selector;
pub mod solar;
pub mod timezone;

pub use city_selector::select_city_interactive;
pub use solar::get_sun_times;
pub use timezone::detect_coordinates_from_timezone;

/// Represents a geographic location with coordinates.
#[derive(Debug, Clone)]
pub struct Location {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// Represents calculated sun times for a specific date and location.
#[derive(Debug, Clone)]
pub struct SunTimes {
    /// Time when sun crosses horizon (0 degrees elevation)
    pub sunrise: chrono::NaiveTime,
    /// Time when sun crosses horizon (0 degrees elevation)
    pub sunset: chrono::NaiveTime,
    /// Duration of sunrise transition (-6 to +6 degrees)
    pub sunrise_duration: std::time::Duration,
    /// Duration of sunset transition (+6 to -6 degrees)
    pub sunset_duration: std::time::Duration,
}

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
    use crate::config::Config;
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
        Log::log_pipe();
        Log::log_debug("Detected running sunsetr instance");
        Log::log_indented("Will update configuration and restart after city selection");
    } else {
        Log::log_pipe();
        Log::log_debug("No running instance detected");
        Log::log_indented("Will start sunsetr in background after city selection");
    }

    // Run interactive city selection
    let selection_result = run_city_selection();

    // Handle cancellation
    let (latitude, longitude) = match selection_result {
        Ok(coords) => coords,
        Err(e) => {
            if e.to_string().contains("cancelled") {
                return Ok(GeoSelectionResult::Cancelled);
            }
            return Err(e);
        }
    };

    // Update config with selected coordinates or create new config if none exists
    handle_config_update_with_coordinates(latitude, longitude)?;

    if instance_running {
        Log::log_block_start("Configuration updated successfully");
        Log::log_indented("Signaling running instance to restart with new location");
        Ok(GeoSelectionResult::ConfigUpdated {
            needs_restart: true,
        })
    } else {
        Log::log_block_start("Configuration updated successfully");
        Log::log_decorated("Ready to start sunsetr with new location");
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
/// * `Ok((latitude, longitude))` - Selected city coordinates
/// * `Err(_)` - If selection fails or user cancels
pub fn run_city_selection() -> anyhow::Result<(f64, f64)> {
    use crate::logger::Log;
    use anyhow::Context;
    use chrono::{Duration, Local};

    // Delegate to the city_selector module for the actual implementation
    let (latitude, longitude, city_name) =
        select_city_interactive().context("Failed to run interactive city selection")?;

    // Show calculated sunrise/sunset times using solar module
    let today = Local::now().date_naive();
    let tomorrow = today + Duration::days(1);

    // Calculate times for today and tomorrow
    // Calculate the actual transition windows using our new +6° to -6° method
    match calculate_civil_twilight_times_for_display(latitude, longitude, today) {
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
                "Sun times for {} ({:.4}°, {:.4}°)",
                city_name, latitude, longitude
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
                "Transition duration: {} minutes",
                sunset_duration.as_secs() / 60
            ));
        }
        Err(e) => {
            Log::log_warning(&format!("Could not calculate sun times: {}", e));
            Log::log_indented("Using default transition times");
        }
    }

    Ok((latitude, longitude))
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
fn calculate_civil_twilight_times_for_display(
    latitude: f64,
    longitude: f64,
    date: chrono::NaiveDate,
) -> Result<
    (
        chrono::NaiveTime,
        chrono::NaiveTime,
        chrono::NaiveTime,
        chrono::NaiveTime,
        chrono::NaiveTime,
        chrono::NaiveTime,
        std::time::Duration,
        std::time::Duration,
    ),
    anyhow::Error,
> {
    use chrono::TimeZone;
    use chrono_tz::Tz;
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
    use crate::logger::Log;
    Log::log_indented(&format!(
        "DEBUG - Sunrise UTC: {}, Local: {}, TZ: {}",
        sunrise_utc.format("%H:%M"),
        sunrise_time.format("%H:%M"),
        timezone
    ));
    Log::log_indented(&format!(
        "DEBUG - Sunset UTC: {}, Local: {}, TZ: {}",
        sunset_utc.format("%H:%M"),
        sunset_time.format("%H:%M"),
        timezone
    ));

    // Try to get civil twilight times (-6° elevation)
    let civil_dawn_utc = solar_day.event_time(SolarEvent::Dawn(DawnType::Civil));
    let civil_dawn = civil_dawn_utc.with_timezone(&timezone).time();

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

/// Correct known coordinate errors in the cities database.
///
/// Some cities in the database have incorrect coordinate signs.
pub fn correct_coordinates(city_name: &str, country: &str, lat: f64, lon: f64) -> (f64, f64) {
    match (city_name, country) {
        // Mumbai should be 72.8°E, not W
        ("Mumbai", "India") => (lat, lon.abs()),
        // Add other known corrections here
        _ => (lat, lon),
    }
}

/// Determine the timezone for given coordinates.
///
/// This is a simplified mapping for major regions. For production use,
/// you'd want a more comprehensive timezone database.
fn determine_timezone_from_coordinates(latitude: f64, longitude: f64) -> chrono_tz::Tz {
    use chrono_tz::Tz;

    // Simplified timezone mapping based on coordinates
    // This covers major regions - for production you'd want a proper timezone database

    // Alaska
    if latitude > 55.0 && longitude < -130.0 && longitude > -180.0 {
        return Tz::America__Anchorage;
    }

    // US West Coast
    if latitude > 32.0 && latitude < 49.0 && longitude < -114.0 && longitude > -125.0 {
        return Tz::America__Los_Angeles;
    }

    // US Mountain
    if latitude > 31.0 && latitude < 49.0 && longitude < -102.0 && longitude > -114.0 {
        return Tz::America__Denver;
    }

    // US Central
    if latitude > 25.0 && latitude < 49.0 && longitude < -84.0 && longitude > -102.0 {
        return Tz::America__Chicago;
    }

    // US Eastern
    if latitude > 25.0 && latitude < 49.0 && longitude < -67.0 && longitude > -84.0 {
        return Tz::America__New_York;
    }

    // Europe
    if latitude > 35.0 && latitude < 70.0 && longitude > -10.0 && longitude < 40.0 {
        return Tz::Europe__London; // Default to London for Europe
    }

    // India
    if latitude > 6.0 && latitude < 37.0 && longitude > 68.0 && longitude < 97.0 {
        return Tz::Asia__Kolkata;
    }

    // China
    if latitude > 18.0 && latitude < 54.0 && longitude > 73.0 && longitude < 135.0 {
        return Tz::Asia__Shanghai;
    }

    // Japan
    if latitude > 24.0 && latitude < 46.0 && longitude > 123.0 && longitude < 146.0 {
        return Tz::Asia__Tokyo;
    }

    // Australia (rough approximation)
    if latitude > -44.0 && latitude < -10.0 && longitude > 113.0 && longitude < 154.0 {
        return Tz::Australia__Sydney;
    }

    // Default fallback - try to use system timezone or UTC
    match std::env::var("TZ") {
        Ok(tz_str) => tz_str.parse().unwrap_or(Tz::UTC),
        Err(_) => Tz::UTC,
    }
}

/// Handle config update with coordinates, creating new config if none exists
fn handle_config_update_with_coordinates(latitude: f64, longitude: f64) -> anyhow::Result<()> {
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

        // First create the default config
        Config::create_default_config(&config_path)?;

        // Then update it with the selected coordinates
        Config::update_config_with_geo_coordinates(latitude, longitude)?;

        Log::log_decorated(&format!(
            "Created new config file: {}",
            config_path.display()
        ));
    }

    Ok(())
}

/// Check if sunsetr is currently running by testing the lock file
fn is_sunsetr_running(lock_path: &str) -> bool {
    use fs2::FileExt;
    use std::fs::File;

    if let Ok(lock_file) = File::open(lock_path) {
        lock_file.try_lock_exclusive().is_err()
    } else {
        false
    }
}

