//! Implementation of the --simulate flag for testing time-based behavior.
//!
//! Runs the application itself under an accelerated time source, so transitions
//! and geo calculations play out without waiting for wall-clock time.

use crate::common::logger::LoggerGuard;
use crate::common::utils::ProgressBar;
use crate::io::instance::get_running_instance_pid;
use crate::time::source::{SimulatedTimeSource, TimeSource};
use anyhow::Result;
use chrono::{DateTime, Local};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// Guards that must stay alive for the duration of the simulation.
pub struct SimulationGuards {
    logger_guard: Option<LoggerGuard>,
    progress_handle: Option<thread::JoinHandle<()>>,
    progress_shutdown: Arc<AtomicBool>,
    log_to_file: bool,
    is_complete: bool,
}

impl SimulationGuards {
    /// Clean teardown for a simulation that reached its end time.
    pub fn complete_simulation(&mut self) {
        self.is_complete = true;

        self.progress_shutdown.store(true, Ordering::SeqCst);

        if let Some(handle) = self.progress_handle.take() {
            let _ = handle.join();
        }

        // Clear the progress bar line.
        print!("\r\x1B[K");
        std::io::Write::flush(&mut std::io::stdout()).ok();

        if self.log_to_file {
            drop(self.logger_guard.take());

            // Give the logger thread time to flush to the file.
            std::thread::sleep(Duration::from_millis(100));

            // ApplicationRunner already wrote the shutdown lines to the file.
            println!("┣ Simulation complete");
            println!("╹");
        }
    }
}

impl Drop for SimulationGuards {
    fn drop(&mut self) {
        if !self.is_complete {
            self.progress_shutdown.store(true, Ordering::SeqCst);

            if let Some(handle) = self.progress_handle.take() {
                let _ = handle.join();
                // Clear the progress bar line.
                print!("\r\x1B[K");
                std::io::Write::flush(&mut std::io::stdout()).ok();
            }

            if self.log_to_file {
                drop(self.logger_guard.take());

                // Give the logger thread time to flush to the file.
                std::thread::sleep(Duration::from_millis(100));

                println!("┣ Simulation interrupted");
                println!("┃");
                println!("┣ Shutting down sunsetr...");
                println!("╹");
            }
        }
    }
}

/// Prepares the simulation environment and returns guards that must be kept
/// alive while the caller runs the application under the installed time source.
///
/// `multiplier` is a fast-forward flag at -1.0, the 3600x default at 0, and a
/// literal acceleration factor when positive.
pub fn setup_simulation(
    start_time: String,
    end_time: String,
    multiplier: f64,
    debug_enabled: bool,
    log_to_file: bool,
) -> Result<SimulationGuards> {
    if let Ok(pid) = get_running_instance_pid() {
        log_version!();
        log_error_end!(
            "Cannot run simulation: sunsetr is already running (PID: {})\n   Stop the existing sunsetr instance first with: kill {}",
            pid,
            pid
        );
        std::process::exit(1);
    }

    if crate::io::instance::is_test_mode_active() {
        log_version!();
        log_error_end!(
            "Cannot run simulation: test mode is currently active\n   Exit the test mode first (press Escape in test terminal)"
        );
        std::process::exit(1);
    }

    let loaded_config = crate::config::Config::load();

    if let Ok(config) = &loaded_config
        && config.transition_mode == crate::config::TransitionMode::Static
    {
        log_version!();
        log_error_end!(
            "Cannot run simulation in static transition mode\n   Static mode maintains constant temperature and gamma values\n   There are no transitions to simulate"
        );
        std::process::exit(1);
    }
    // Keep both the parsed times for display and the Local-converted times for
    // the simulation. In geo mode these differ in timezone.
    let (start, end, geo_tz_opt, display_start, display_end) = if let Ok(config) = &loaded_config {
        if config.transition_mode == crate::config::TransitionMode::Geo {
            if let (Some(lat), Some(lon)) = (config.latitude, config.longitude) {
                let geo_tz = crate::geo::solar::determine_timezone(lat, lon);

                let start_tz = crate::time::source::parse_datetime_in_tz(&start_time, geo_tz)
                    .map_err(|e| anyhow::anyhow!("Invalid start time: {}", e))?;
                let end_tz = crate::time::source::parse_datetime_in_tz(&end_time, geo_tz)
                    .map_err(|e| anyhow::anyhow!("Invalid end time: {}", e))?;

                // Convert to Local for SimulatedTimeSource while preserving the instant.
                let start_local = start_tz.with_timezone(&Local);
                let end_local = end_tz.with_timezone(&Local);

                (
                    start_local,
                    end_local,
                    Some(geo_tz),
                    start_tz.format("%Y-%m-%d %H:%M:%S").to_string(),
                    end_tz.format("%Y-%m-%d %H:%M:%S").to_string(),
                )
            } else {
                let start = crate::time::source::parse_datetime(&start_time)
                    .map_err(|e| anyhow::anyhow!("Invalid start time: {}", e))?;
                let end = crate::time::source::parse_datetime(&end_time)
                    .map_err(|e| anyhow::anyhow!("Invalid end time: {}", e))?;
                (
                    start,
                    end,
                    None,
                    start.format("%Y-%m-%d %H:%M:%S").to_string(),
                    end.format("%Y-%m-%d %H:%M:%S").to_string(),
                )
            }
        } else {
            let start = crate::time::source::parse_datetime(&start_time)
                .map_err(|e| anyhow::anyhow!("Invalid start time: {}", e))?;
            let end = crate::time::source::parse_datetime(&end_time)
                .map_err(|e| anyhow::anyhow!("Invalid end time: {}", e))?;
            (
                start,
                end,
                None,
                start.format("%Y-%m-%d %H:%M:%S").to_string(),
                end.format("%Y-%m-%d %H:%M:%S").to_string(),
            )
        }
    } else {
        let start = crate::time::source::parse_datetime(&start_time)
            .map_err(|e| anyhow::anyhow!("Invalid start time: {}", e))?;
        let end = crate::time::source::parse_datetime(&end_time)
            .map_err(|e| anyhow::anyhow!("Invalid end time: {}", e))?;
        (
            start,
            end,
            None,
            start.format("%Y-%m-%d %H:%M:%S").to_string(),
            end.format("%Y-%m-%d %H:%M:%S").to_string(),
        )
    };

    if end <= start {
        log_error_end!("End time must be after start time");
        std::process::exit(1);
    }

    // The -1.0 fast-forward flag from arg parsing maps to the 0.0 mode TimeSource uses.
    let time_source_multiplier = if multiplier == -1.0 { 0.0 } else { multiplier };

    // Create the time source now, but defer init until after terminal output when
    // using --log.
    let sim_source = Arc::new(SimulatedTimeSource::new(start, end, time_source_multiplier));

    let _logger_guard;
    let _progress_handle;
    let progress_shutdown = Arc::new(AtomicBool::new(false));

    if log_to_file {
        log_version!();
        log_block_start!("Simulation Mode");

        log_simulation_details(&display_start, &display_end, multiplier, start, end);
        log_indented!("Running simulation...");

        let log_filename = format!(
            "sunsetr-simulation-{}.log",
            Local::now().format("%Y%m%d-%H%M%S")
        );

        log_block_start!("Logging simulation output to: {}", log_filename);

        // Initialize the time source now, after the terminal output.
        crate::time::source::init_time_source(sim_source.clone());

        if let Some(geo_tz) = geo_tz_opt {
            crate::common::logger::Log::set_geo_timezone(Some(geo_tz));
        }

        // Routes all subsequent logger output to the file.
        _logger_guard = Some(crate::common::logger::Log::start_file_logging(
            log_filename,
        )?);

        _progress_handle = Some(spawn_progress_monitor(
            sim_source.clone(),
            start,
            end,
            multiplier,
            progress_shutdown.clone(),
        ));

        // Repeat the header into the file, now with timestamps.
        log_version!();
        log_block_start!("Simulation Mode");
    } else {
        crate::time::source::init_time_source(sim_source.clone());

        if let Some(geo_tz) = geo_tz_opt {
            crate::common::logger::Log::set_geo_timezone(Some(geo_tz));
        }

        _logger_guard = None;
        _progress_handle = None;

        log_version!();
        log_block_start!("Simulation Mode");
    }

    // Repeat the details into the file when using --log.
    if log_to_file {
        log_simulation_details(&display_start, &display_end, multiplier, start, end);
    } else {
        log_simulation_details(&display_start, &display_end, multiplier, start, end);
        log_indented!("Running simulation...");
    }

    if debug_enabled {
        log_pipe!();
        log_debug!("Simulated time source initialized");
    }

    // ApplicationRunner handles log_end() as part of the full lifecycle.

    Ok(SimulationGuards {
        logger_guard: _logger_guard,
        progress_handle: _progress_handle,
        progress_shutdown,
        log_to_file,
        is_complete: false,
    })
}

/// Runs a full simulation by setting up the environment, running the application
/// under accelerated time, then finalizing.
///
/// `complete_simulation` is only called when the run reached the simulated end
/// time. An interrupted run leaves `simulation_ended` false, so the guards'
/// `Drop` handles the interrupted teardown instead.
pub fn run_simulation(
    start_time: String,
    end_time: String,
    multiplier: f64,
    debug_enabled: bool,
    log_to_file: bool,
) -> Result<()> {
    let mut guards =
        setup_simulation(start_time, end_time, multiplier, debug_enabled, log_to_file)?;

    crate::Sunsetr::new(debug_enabled)
        .without_lock()
        .without_headers()
        .run()?;

    if crate::time::source::simulation_ended() {
        guards.complete_simulation();
    }

    Ok(())
}

/// Spawns a thread that renders the simulation progress bar.
///
/// It writes straight to stdout, bypassing the logger channel, so the bar stays
/// on the terminal even while file logging is active. ETA comes from the measured
/// progress rate, so it accounts for system and processing overhead.
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

        let monitor_start = std::time::Instant::now();
        let expected_total_real_secs = if multiplier > 0.0 {
            total_duration.num_milliseconds() as f64 / 1000.0 / multiplier
        } else {
            0.0
        };

        loop {
            if shutdown.load(Ordering::SeqCst) {
                // Leave the bar for SimulationGuards to clear.
                break;
            }

            let current = time_source.now();
            let elapsed = current.signed_duration_since(start_time);
            let progress = (elapsed.num_milliseconds() as f64
                / total_duration.num_milliseconds() as f64)
                .clamp(0.0, 1.0);

            let suffix = if multiplier == 0.0 || multiplier == -1.0 {
                "fast-forward mode".to_string()
            } else {
                let real_elapsed = monitor_start.elapsed().as_secs_f64();
                if progress > 0.0 && progress < 1.0 {
                    let estimated_total = real_elapsed / progress;
                    let remaining_real = (estimated_total - real_elapsed).max(0.0);
                    format!("ETA: {remaining_real:.1}s")
                } else if progress >= 1.0 {
                    "completing...".to_string()
                } else {
                    // Before any progress, fall back to the expected time.
                    format!("ETA: {expected_total_real_secs:.1}s")
                }
            };

            progress_bar.update(progress as f32, Some(&suffix));

            if time_source.is_ended() {
                // Leave the bar at 100% for SimulationGuards to clear, keeping it
                // visible during cleanup like gamma reset.
                break;
            }

            thread::sleep(progress_bar.recommended_sleep());
        }
    })
}

/// Logs the simulation time range and acceleration, shared by the terminal and
/// file output paths.
fn log_simulation_details(
    display_start: &str,
    display_end: &str,
    multiplier: f64,
    start: DateTime<Local>,
    end: DateTime<Local>,
) {
    let duration = end.signed_duration_since(start);

    log_decorated!("Simulating from {} to {}", display_start, display_end);

    log_indented!(
        "Total simulated time: {} hours {} minutes",
        duration.num_hours(),
        duration.num_minutes() % 60
    );

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
        let theoretical_duration_secs =
            duration.num_milliseconds() as f64 / 1000.0 / actual_multiplier;
        log_indented!(
            "Time acceleration: {}x (theoretical: ~{:.1} seconds)",
            actual_multiplier as u64,
            theoretical_duration_secs
        );
        log_indented!("Note: Actual time may vary due to system and processing overhead");
    }
}
