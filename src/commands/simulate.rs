//! Implementation of the --simulate command for testing time-based behavior.
//!
//! This command sets up a simulated time source, allowing the application to run
//! with accelerated time for testing transitions, geo mode calculations, and other
//! time-dependent functionality without waiting for real time to pass.

use crate::time_source::{self, SimulatedTimeSource};
use anyhow::Result;
use chrono::Local;
use std::sync::Arc;

/// Handle the --simulate command by setting up a simulated time source.
///
/// This function prepares the simulation environment and returns control to main.rs,
/// which will then run the application normally but with accelerated simulated time.
///
/// # Arguments
/// * `start_time` - Start time in format "YYYY-MM-DD HH:MM:SS"
/// * `end_time` - End time in format "YYYY-MM-DD HH:MM:SS"
/// * `multiplier` - Time acceleration factor (0 = default 3600x, >0 = custom multiplier)
/// * `debug_enabled` - Whether debug mode is enabled
pub fn handle_simulate_command(
    start_time: String,
    end_time: String,
    multiplier: f64,
    debug_enabled: bool,
) -> Result<()> {
    // Check if we're in geo mode to determine timezone for parsing
    let (start, end, geo_tz_opt) = if let Ok(config) = crate::config::Config::load() {
        if config.transition_mode.as_deref() == Some("geo") {
            if let (Some(lat), Some(lon)) = (config.latitude, config.longitude) {
                let geo_tz = crate::geo::solar::determine_timezone_from_coordinates(lat, lon);

                // Parse times in coordinate timezone
                let start_tz = time_source::parse_datetime_in_tz(&start_time, geo_tz)
                    .map_err(|e| anyhow::anyhow!("Invalid start time: {}", e))?;
                let end_tz = time_source::parse_datetime_in_tz(&end_time, geo_tz)
                    .map_err(|e| anyhow::anyhow!("Invalid end time: {}", e))?;

                // Convert to Local for SimulatedTimeSource (but preserving the actual moment in time)
                let start_local = start_tz.with_timezone(&Local);
                let end_local = end_tz.with_timezone(&Local);

                (start_local, end_local, Some(geo_tz))
            } else {
                // Geo mode but no coordinates, fall back to local parsing
                let start = time_source::parse_datetime(&start_time)
                    .map_err(|e| anyhow::anyhow!("Invalid start time: {}", e))?;
                let end = time_source::parse_datetime(&end_time)
                    .map_err(|e| anyhow::anyhow!("Invalid end time: {}", e))?;
                (start, end, None)
            }
        } else {
            // Not in geo mode, parse as local
            let start = time_source::parse_datetime(&start_time)
                .map_err(|e| anyhow::anyhow!("Invalid start time: {}", e))?;
            let end = time_source::parse_datetime(&end_time)
                .map_err(|e| anyhow::anyhow!("Invalid end time: {}", e))?;
            (start, end, None)
        }
    } else {
        // Config load failed, fall back to local parsing
        let start = time_source::parse_datetime(&start_time)
            .map_err(|e| anyhow::anyhow!("Invalid start time: {}", e))?;
        let end = time_source::parse_datetime(&end_time)
            .map_err(|e| anyhow::anyhow!("Invalid end time: {}", e))?;
        (start, end, None)
    };

    // Validate that end is after start
    if end <= start {
        anyhow::bail!("End time must be after start time");
    }

    // Convert -1.0 (fast-forward flag) to 0.0 (fast-forward mode for TimeSource)
    let time_source_multiplier = if multiplier == -1.0 { 0.0 } else { multiplier };

    // Initialize the simulated time source BEFORE any logging
    let sim_source = Arc::new(SimulatedTimeSource::new(start, end, time_source_multiplier));
    time_source::init_time_source(sim_source);

    // Set the timezone for dual timestamp display if in geo mode
    // This must happen BEFORE any logging so timestamps are correct from the start
    if let Some(geo_tz) = geo_tz_opt {
        crate::logger::Log::set_geo_timezone(Some(geo_tz));
    }

    // Now we can start logging - timestamps will be shown from the beginning
    log_version!();
    log_block_start!("Simulation Mode");

    // Calculate simulation duration
    let duration = end.signed_duration_since(start);
    log_decorated!(
        "Simulating from {} to {}",
        start.format("%Y-%m-%d %H:%M:%S"),
        end.format("%Y-%m-%d %H:%M:%S")
    );

    // Display time acceleration info
    let (actual_multiplier, is_fast_forward) = if multiplier == -1.0 {
        // Fast-forward mode: use a special marker
        (0.0, true)
    } else if multiplier <= 0.0 {
        (3600.0, false)
    } else {
        (multiplier, false)
    };

    log_indented!(
        "Total simulated time: {} hours {} minutes",
        duration.num_hours(),
        duration.num_minutes() % 60
    );

    if is_fast_forward {
        log_indented!("Time acceleration: fast-forward (instant execution)");
    } else {
        let real_duration_secs = duration.num_seconds() as f64 / actual_multiplier;
        log_indented!(
            "Time acceleration: {}x (will complete in ~{:.1} seconds)",
            actual_multiplier as u64,
            real_duration_secs
        );
    }

    // Suggest log file name
    let log_filename = format!(
        "sunsetr-simulation-{}.log",
        Local::now().format("%Y%m%d-%H%M%S")
    );

    log_indented!("Running simulation...");
    log_pipe!();
    log_decorated!("To save output to a file, run:");
    log_indented!(
        "sunsetr -S \"{}\" \"{}\" {} > {}",
        start_time,
        end_time,
        if debug_enabled { "-d" } else { "" },
        log_filename
    );

    if debug_enabled {
        log_pipe!();
        log_debug!("Simulated time source initialized");
    }

    // Don't call Log::log_end() here - ApplicationRunner will handle the full lifecycle

    // Return control to main.rs
    Ok(())
}
