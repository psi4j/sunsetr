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
    let (latitude, longitude, city_name) =
        select_city_interactive().context("Failed to run interactive city selection")?;

    // Show calculated sunrise/sunset times using solar module
    let today = Local::now().date_naive();

    // Calculate the actual transition windows using our new +6° to -6° method
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
            // Show detailed solar calculation debug if debug mode is enabled
            if debug_enabled {
                // Get the detailed solar calculations for debug output
                if let Ok((
                    sunset_time_calc,
                    sunset_duration_calc,
                    sunrise_time_calc,
                    sunrise_duration_calc,
                )) =
                    crate::geo::solar::calculate_geo_center_times_and_durations(latitude, longitude)
                {
                    use chrono::Local;
                    use sunrise::{Coordinates, DawnType, SolarDay, SolarEvent};

                    let coord = Coordinates::new(latitude, longitude).unwrap();
                    let solar_day = SolarDay::new(coord, today);

                    // Calculate all the solar times for debug display
                    let civil_dawn_utc = solar_day.event_time(SolarEvent::Dawn(DawnType::Civil));
                    let civil_dawn = civil_dawn_utc.with_timezone(&Local).time();

                    let civil_dusk_utc = solar_day.event_time(SolarEvent::Dusk(DawnType::Civil));
                    let civil_dusk = civil_dusk_utc.with_timezone(&Local).time();

                    // Calculate both traditional golden hour (+6°/-6°) and new window (+10°/-2°)
                    let sunset_to_civil_dusk_duration = if civil_dusk > sunset_time_calc {
                        civil_dusk.signed_duration_since(sunset_time_calc)
                    } else {
                        chrono::Duration::zero()
                    };

                    // Traditional golden hour (+6° to -6°) using symmetry
                    let golden_hour_start = sunset_time_calc - sunset_to_civil_dusk_duration;

                    // New window calculations
                    // +10° is (10/6) times the duration before sunset
                    let duration_to_plus_10 = sunset_to_civil_dusk_duration * 10 / 6;
                    let plus_10_deg_start = sunset_time_calc - duration_to_plus_10;

                    // -2° is (2/6) times the duration after sunset
                    let duration_to_minus_2 = sunset_to_civil_dusk_duration * 2 / 6;
                    let minus_2_deg_end = sunset_time_calc + duration_to_minus_2;

                    let civil_dawn_to_sunrise_duration = if sunrise_time_calc > civil_dawn {
                        sunrise_time_calc.signed_duration_since(civil_dawn)
                    } else {
                        chrono::Duration::zero()
                    };

                    // Traditional golden hour end
                    let golden_hour_end = sunrise_time_calc + civil_dawn_to_sunrise_duration;

                    // For sunrise: -2° before, +10° after
                    let duration_from_minus_2 = civil_dawn_to_sunrise_duration * 2 / 6;
                    let minus_2_deg_start_dawn = sunrise_time_calc - duration_from_minus_2;

                    let duration_from_plus_10 = civil_dawn_to_sunrise_duration * 10 / 6;
                    let plus_10_deg_end_dawn = sunrise_time_calc + duration_from_plus_10;

                    // Get timezone and UTC times for display
                    use crate::geo::solar::determine_timezone_from_coordinates;
                    let timezone = determine_timezone_from_coordinates(latitude, longitude);

                    let sunrise_utc = solar_day.event_time(SolarEvent::Sunrise);
                    let sunset_utc = solar_day.event_time(SolarEvent::Sunset);

                    // Calculate night duration (-2° evening to -2° morning)
                    let night_duration = if minus_2_deg_start_dawn > minus_2_deg_end {
                        // Same day
                        minus_2_deg_start_dawn.signed_duration_since(minus_2_deg_end)
                    } else {
                        // Crosses midnight
                        let time_to_midnight = chrono::NaiveTime::from_hms_opt(23, 59, 59)
                            .unwrap()
                            .signed_duration_since(minus_2_deg_end);
                        let time_from_midnight = minus_2_deg_start_dawn.signed_duration_since(
                            chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                        );
                        time_to_midnight + time_from_midnight + chrono::Duration::seconds(1)
                    };

                    // Calculate day duration (+10° morning to +10° evening next day)
                    let day_duration = if plus_10_deg_start > plus_10_deg_end_dawn {
                        // Same day
                        plus_10_deg_start.signed_duration_since(plus_10_deg_end_dawn)
                    } else {
                        // Crosses midnight
                        let time_to_midnight = chrono::NaiveTime::from_hms_opt(23, 59, 59)
                            .unwrap()
                            .signed_duration_since(plus_10_deg_end_dawn);
                        let time_from_midnight = plus_10_deg_start.signed_duration_since(
                            chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                        );
                        time_to_midnight + time_from_midnight + chrono::Duration::seconds(1)
                    };

                    Log::log_pipe();
                    Log::log_debug("Solar calculation details:");
                    Log::log_indented(&format!(
                        "        Raw coordinates: {:.4}°, {:.4}°",
                        latitude, longitude
                    ));
                    Log::log_indented(&format!(
                        "            Sunrise UTC: {}",
                        sunrise_utc.format("%H:%M")
                    ));
                    Log::log_indented(&format!(
                        "             Sunset UTC: {} ",
                        sunset_utc.format("%H:%M")
                    ));
                    Log::log_indented(&format!("               Timezone: {}", timezone));

                    // Sunset sequence (descending elevation order)
                    Log::log_indented("--- Sunset (descending) ---");
                    Log::log_indented(&format!(
                        "Transition start (+10°): {}",
                        plus_10_deg_start.format("%H:%M:%S")
                    ));
                    Log::log_indented(&format!(
                        "Golden hour start (+6°): {}",
                        golden_hour_start.format("%H:%M:%S")
                    ));
                    Log::log_indented(&format!(
                        "            Sunset (0°): {}",
                        sunset_time_calc.format("%H:%M:%S")
                    ));
                    Log::log_indented(&format!(
                        "   Transition end (-2°): {}",
                        minus_2_deg_end.format("%H:%M:%S")
                    ));
                    Log::log_indented(&format!(
                        "       Civil dusk (-6°): {}",
                        civil_dusk.format("%H:%M:%S")
                    ));

                    // Night duration
                    Log::log_indented(&format!(
                        "         Night duration: {} hours {} minutes",
                        night_duration.num_hours(),
                        night_duration.num_minutes() % 60
                    ));

                    // Sunrise sequence (ascending elevation order)
                    Log::log_indented("--- Sunrise (ascending) ---");
                    Log::log_indented(&format!(
                        "       Civil dawn (-6°): {}",
                        civil_dawn.format("%H:%M:%S")
                    ));
                    Log::log_indented(&format!(
                        " Transition start (-2°): {}",
                        minus_2_deg_start_dawn.format("%H:%M:%S")
                    ));
                    Log::log_indented(&format!(
                        "           Sunrise (0°): {}",
                        sunrise_time_calc.format("%H:%M:%S")
                    ));
                    Log::log_indented(&format!(
                        "  Golden hour end (+6°): {}",
                        golden_hour_end.format("%H:%M:%S")
                    ));
                    Log::log_indented(&format!(
                        "  Transition end (+10°): {}",
                        plus_10_deg_end_dawn.format("%H:%M:%S")
                    ));

                    // Day duration
                    Log::log_indented(&format!(
                        "           Day duration: {} hours {} minutes",
                        day_duration.num_hours(),
                        day_duration.num_minutes() % 60
                    ));
                    Log::log_indented(&format!(
                        "        Sunset duration: {} minutes",
                        sunset_duration_calc.as_secs() / 60
                    ));
                    Log::log_indented(&format!(
                        "       Sunrise duration: {} minutes",
                        sunrise_duration_calc.as_secs() / 60
                    ));
                }
            }

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

/// Handle config update with coordinates, creating new config if none exists
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
