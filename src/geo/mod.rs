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
pub use solar::{calculate_sunrise_sunset, calculate_transition_duration};
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
    /// Time when sun reaches +6� elevation (start of day)
    pub sunrise: chrono::NaiveTime,
    /// Time when sun reaches -6� elevation (end of day)
    pub sunset: chrono::NaiveTime,
    /// Duration of sunrise transition (-6� to +6�)
    pub sunrise_duration: std::time::Duration,
    /// Duration of sunset transition (+6� to -6�)
    pub sunset_duration: std::time::Duration,
}

/// Handle the complete --geo flag workflow
///
/// This function manages the entire geo selection process:
/// 1. Interactive city selection
/// 2. Config file updates
/// 3. Process management (restart/start sunsetr)
/// 4. Terminal release (unless debug mode)
///
/// # Arguments
/// * `debug_enabled` - Whether to run final sunsetr instance in debug mode
pub fn handle_geo_selection(debug_enabled: bool) -> anyhow::Result<()> {
    use crate::logger::Log;
    use crate::config::Config;
    use std::fs::File;
    use fs2::FileExt;
    
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
        Log::log_block_start("Detected running sunsetr instance");
        Log::log_indented("Will update configuration and restart after city selection");
    } else {
        Log::log_block_start("No running instance detected");
        Log::log_indented("Will start sunsetr in background after city selection");
    }

    // Run interactive city selection
    let (latitude, longitude) = run_city_selection()?;

    // Update config with selected coordinates
    Config::update_config_with_geo_coordinates(latitude, longitude)?;

    if instance_running {
        // Signal restart by touching a restart flag file
        let restart_flag_path = format!("{}/sunsetr.restart", runtime_dir);
        std::fs::write(&restart_flag_path, "restart")
            .map_err(|e| anyhow::anyhow!("Failed to create restart signal: {}", e))?;
        
        Log::log_block_start("Configuration updated successfully");
        Log::log_indented("Signaled running instance to restart with new coordinates");
    }

    // Start sunsetr in background (or foreground if debug mode)
    if debug_enabled {
        Log::log_block_start("Starting sunsetr in debug mode...");
        // TODO: Import and call run_application(true) - for now, just indicate intent
        Log::log_decorated("TODO: Start sunsetr in foreground debug mode");
        // crate::run_application(true)
        Ok(())
    } else {
        Log::log_block_start("Starting sunsetr in background...");
        
        // Start new process in background and release terminal
        let current_exe = std::env::current_exe()
            .map_err(|e| anyhow::anyhow!("Failed to get current executable path: {}", e))?;
        let child = std::process::Command::new(current_exe)
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to start background sunsetr process: {}", e))?;
            
        Log::log_decorated(&format!("Sunsetr started in background (PID: {})", child.id()));
        Log::log_decorated("Geo selection complete. Terminal released.");
        Ok(())
    }
}

/// Run interactive city selection and return the selected coordinates
///
/// This function handles the city selection UI workflow:
/// 1. Display regional selection menu
/// 2. Display cities within selected region  
/// 3. User selects closest city
/// 4. Return latitude/longitude coordinates
///
/// # Returns
/// * `Ok((latitude, longitude))` - Selected city coordinates
/// * `Err(_)` - If selection fails or user cancels
pub fn run_city_selection() -> anyhow::Result<(f64, f64)> {
    use crate::logger::Log;
    
    Log::log_block_start("Interactive City Selection");
    Log::log_indented("Select your city to determine sunrise/sunset times");
    
    // TODO: Implement actual city selection using city_selector module
    // For now, return placeholder coordinates (New York)
    Log::log_pipe();
    Log::log_warning("TODO: Interactive city selection not yet implemented");
    Log::log_decorated("Using placeholder coordinates for New York City");
    
    let latitude = 40.7128;
    let longitude = -74.0060;
    
    Log::log_block_start(&format!("Selected Location: New York City"));
    Log::log_indented(&format!("Coordinates: {:.4}°N, {:.4}°W", latitude, longitude.abs()));
    Log::log_indented("TODO: Show calculated sunrise/sunset times");
    
    Ok((latitude, longitude))
}

/// Check if sunsetr is currently running by testing the lock file
fn is_sunsetr_running(lock_path: &str) -> bool {
    if let Ok(lock_file) = File::open(lock_path) {
        lock_file.try_lock_exclusive().is_err()
    } else {
        false
    }
}