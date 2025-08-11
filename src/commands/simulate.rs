//! Implementation of the --simulate command for testing time-based behavior.
//!
//! This command sets up a simulated time source, allowing the application to run
//! with accelerated time for testing transitions, geo mode calculations, and other
//! time-dependent functionality without waiting for real time to pass.

use crate::logger::LoggerGuard;
use crate::time_source::{self, SimulatedTimeSource, TimeSource};
use crate::utils::ProgressBar;
use anyhow::Result;
use chrono::{DateTime, Local};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// Guards that need to stay alive for the duration of the simulation
pub struct SimulationGuards {
    logger_guard: Option<LoggerGuard>,
    progress_handle: Option<thread::JoinHandle<()>>,
    progress_shutdown: Arc<AtomicBool>,
    log_to_file: bool,
    is_complete: bool,
}

impl SimulationGuards {
    /// Complete the simulation cleanly with proper output
    pub fn complete_simulation(&mut self) {
        // Mark that we completed naturally
        self.is_complete = true;

        // Signal the progress monitor to stop
        self.progress_shutdown.store(true, Ordering::SeqCst);

        // Wait for progress monitor to finish if it exists
        if let Some(handle) = self.progress_handle.take() {
            let _ = handle.join();
        }

        // If we were logging to file, we need to handle completion carefully
        if self.log_to_file {
            // Drop the logger guard to stop file logging and flush
            // The file will end with the normal shutdown message from ApplicationRunner
            drop(self.logger_guard.take());

            // Give logger thread time to flush to file
            std::thread::sleep(Duration::from_millis(100));

            // The ApplicationRunner already printed shutdown messages to the log file
            // Just print a simple completion indicator on terminal
            println!("┣ Simulation complete");
            println!("╹");
        }
        // For normal simulation (without --log), just let it end naturally
        // The ApplicationRunner will handle the shutdown message
    }
}

impl Drop for SimulationGuards {
    fn drop(&mut self) {
        // Only do cleanup if not already completed
        if !self.is_complete {
            // Ensure cleanup happens even if the simulation is interrupted
            // Signal the progress monitor to stop
            self.progress_shutdown.store(true, Ordering::SeqCst);

            // Wait for progress monitor to finish if it exists
            if let Some(handle) = self.progress_handle.take() {
                // Give it a moment to clean up the progress bar
                let _ = handle.join();
            }

            // If we were interrupted and logging to file,
            // we need to clean up but without the "complete" message
            if self.log_to_file {
                // Drop the logger guard to stop file logging and flush
                drop(self.logger_guard.take());

                // Give logger thread time to flush to file
                std::thread::sleep(Duration::from_millis(100));

                // Print interrupted message to terminal
                println!("┣ Simulation interrupted");
                println!("┃");
                println!("┣ Shutting down sunsetr...");
                println!("╹");
            }
        }
        // Logger guard will drop automatically if not already dropped
    }
}

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
/// * `log_to_file` - Whether to log output to a file with progress bar on terminal
///
/// # Returns
/// Returns SimulationGuards that must be kept alive for the duration of the simulation
pub fn handle_simulate_command(
    start_time: String,
    end_time: String,
    multiplier: f64,
    debug_enabled: bool,
    log_to_file: bool,
) -> Result<SimulationGuards> {
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

    // Create the simulated time source but DON'T initialize it yet if using --log
    let sim_source = Arc::new(SimulatedTimeSource::new(start, end, time_source_multiplier));

    // Set up file logging if requested
    let _logger_guard;
    let _progress_handle;
    let progress_shutdown = Arc::new(AtomicBool::new(false));

    if log_to_file {
        // Show header on terminal BEFORE initializing simulated time source
        // This ensures the terminal header has no timestamps
        log_version!();
        log_block_start!("Simulation Mode");

        // Show simulation details on terminal (without timestamps)
        log_simulation_details(start, end, multiplier);
        log_indented!("Running simulation...");

        // Generate log filename
        let log_filename = format!(
            "sunsetr-simulation-{}.log",
            Local::now().format("%Y%m%d-%H%M%S")
        );

        // Show where output is going
        log_block_start!("Logging simulation output to: {}", log_filename);

        // NOW initialize the simulated time source (after terminal output)
        time_source::init_time_source(sim_source.clone());

        // Set the timezone for dual timestamp display if in geo mode
        if let Some(geo_tz) = geo_tz_opt {
            crate::logger::Log::set_geo_timezone(Some(geo_tz));
        }

        // Start file logging (this routes all logger output to file from this point on)
        _logger_guard = Some(crate::logger::Log::start_file_logging(log_filename)?);

        // Start progress monitor thread (shows on terminal)
        _progress_handle = Some(spawn_progress_monitor(
            sim_source.clone(),
            start,
            end,
            multiplier,
            progress_shutdown.clone(),
        ));

        // Now log to file - repeat the header for the file (with timestamps)
        log_version!();
        log_block_start!("Simulation Mode");
    } else {
        // Initialize simulated time source for normal simulation
        time_source::init_time_source(sim_source.clone());

        // Set the timezone for dual timestamp display if in geo mode
        if let Some(geo_tz) = geo_tz_opt {
            crate::logger::Log::set_geo_timezone(Some(geo_tz));
        }

        _logger_guard = None;
        _progress_handle = None;

        // Normal output to terminal (with timestamps)
        log_version!();
        log_block_start!("Simulation Mode");
    }

    // Show simulation details (already shown on terminal if using --log, but we want it in the file too)
    if log_to_file {
        log_simulation_details(start, end, multiplier);
    } else {
        // Show details for normal simulation
        log_simulation_details(start, end, multiplier);
        log_indented!("Running simulation...");
    }

    if debug_enabled {
        log_pipe!();
        log_debug!("Simulated time source initialized");
    }

    // Don't call Log::log_end() here - ApplicationRunner will handle the full lifecycle

    // Return guards that must stay alive for the duration of the simulation
    Ok(SimulationGuards {
        logger_guard: _logger_guard,
        progress_handle: _progress_handle,
        progress_shutdown,
        log_to_file,
        is_complete: false,
    })
}

/// Spawn a thread to monitor simulation progress and display a progress bar.
///
/// This thread runs independently and updates the terminal with a progress bar
/// showing the current simulation progress. It writes directly to stdout,
/// bypassing the logger channel to ensure the progress bar always appears
/// on the terminal even when file logging is active.
///
/// # Arguments
/// * `time_source` - The simulated time source to monitor
/// * `start_time` - Start time of the simulation
/// * `end_time` - End time of the simulation
/// * `multiplier` - Time acceleration multiplier for ETA calculation
/// * `shutdown` - Atomic flag to signal the thread to stop
///
/// # Returns
/// A JoinHandle for the spawned thread
fn spawn_progress_monitor(
    time_source: Arc<SimulatedTimeSource>,
    start_time: DateTime<Local>,
    end_time: DateTime<Local>,
    multiplier: f64,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut progress_bar = ProgressBar::new(40);
        let total_duration = end_time.signed_duration_since(start_time);

        loop {
            // Check if we should shutdown
            if shutdown.load(Ordering::SeqCst) {
                // Clear the progress bar line completely
                print!("\r\x1B[K");
                std::io::Write::flush(&mut std::io::stdout()).ok();
                break;
            }

            // Get current simulation time
            let current = time_source.now();
            let elapsed = current.signed_duration_since(start_time);
            let progress = (elapsed.num_seconds() as f64 / total_duration.num_seconds() as f64)
                .clamp(0.0, 1.0);

            // Calculate ETA or show fast-forward mode
            let suffix = if multiplier == 0.0 || multiplier == -1.0 {
                "fast-forward mode".to_string()
            } else {
                let remaining_sim = total_duration - elapsed;
                let remaining_real = remaining_sim.num_seconds() as f64 / multiplier;
                format!("ETA: {remaining_real:.1}s")
            };

            // Update the progress bar (it handles adaptive timing internally)
            progress_bar.update(progress as f32, Some(&suffix));

            // Check if simulation has ended
            if time_source.is_ended() {
                // Clear the progress bar line completely
                print!("\r\x1B[K");
                std::io::Write::flush(&mut std::io::stdout()).ok();
                break;
            }

            // Sleep for the adaptive interval recommended by the progress bar
            thread::sleep(progress_bar.recommended_sleep());
        }
    })
}

/// Log simulation details including time range and acceleration.
///
/// This helper function is used to display consistent simulation information
/// both on terminal and in log files.
fn log_simulation_details(start: DateTime<Local>, end: DateTime<Local>, multiplier: f64) {
    let duration = end.signed_duration_since(start);

    log_decorated!(
        "Simulating from {} to {}",
        start.format("%Y-%m-%d %H:%M:%S"),
        end.format("%Y-%m-%d %H:%M:%S")
    );

    log_indented!(
        "Total simulated time: {} hours {} minutes",
        duration.num_hours(),
        duration.num_minutes() % 60
    );

    // Display time acceleration info
    let (actual_multiplier, is_fast_forward) = if multiplier == -1.0 {
        (0.0, true)
    } else if multiplier <= 0.0 {
        (3600.0, false)
    } else {
        (multiplier, false)
    };

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
}
