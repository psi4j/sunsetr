//! Status command - display current runtime state.
//!
//! This command provides a way to query sunsetr's current state without
//! interfering with its operation. It can output in human-readable or JSON
//! format, and supports a follow mode for continuous monitoring.

use anyhow::{Context, Result};
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use crate::config::Config;
use crate::core::period::{Period, get_current_period, time_until_next_event};
use crate::geo::times::GeoTimes;
use crate::state::display::DisplayState;

/// Handle the status command.
///
/// This command displays the current runtime state of sunsetr, including:
/// - Current time state (day/night/transitioning)
/// - Temperature and gamma values (current and target)
/// - Transition progress
/// - Next scheduled transition
///
/// # Arguments
/// * `json` - Output in JSON format
/// * `follow` - Follow mode for continuous updates
/// * `config_dir` - Optional custom configuration directory
pub fn handle_status_command(json: bool, follow: bool, config_dir: Option<&str>) -> Result<()> {
    // Set custom config directory if provided
    if let Some(dir) = config_dir {
        crate::config::set_config_dir(Some(dir.to_string()))?;
    }

    // Check if sunsetr is running - this also restores config directory from lock file
    let running_pid_result = crate::io::instance::get_running_instance_pid();

    if running_pid_result.is_err() {
        // No sunsetr process is running
        if json {
            println!(r#"{{"error": "No sunsetr process is running"}}"#);
        } else {
            log_error_standalone!("No sunsetr process is running");
            println!("  Start sunsetr first or use 'sunsetr --debug' to run");
        }
        return Ok(());
    }

    // Load configuration (after checking process, so we use the correct config dir)
    let config = Config::load().context("Failed to load configuration")?;

    // Initialize geo times if in geo mode
    let geo_times = if config.transition_mode.as_deref() == Some("geo") {
        GeoTimes::from_config(&config)?
    } else {
        None
    };

    if follow {
        // Follow mode - continuous monitoring
        handle_follow_mode(config, geo_times, json)
    } else {
        // One-shot query
        display_current_state(&config, geo_times.as_ref(), json)
    }
}

/// Display the current state once.
fn display_current_state(config: &Config, geo_times: Option<&GeoTimes>, json: bool) -> Result<()> {
    // Get current state
    let current_state = get_current_period(config, geo_times);

    // For display purposes, we need to calculate what the actual applied values would be
    // In a real running instance, these would be tracked, but for status command
    // we calculate them based on current state
    let (current_temp, current_gamma) = current_state.values(config);

    // Create DisplayState
    let display_state = DisplayState::new(
        current_state,
        current_temp,
        current_gamma,
        config,
        geo_times,
    );

    if json {
        // JSON output
        println!("{}", display_state.to_json_pretty()?);
    } else {
        // Human-readable output
        print_human_readable(&display_state);
    }

    Ok(())
}

/// Handle follow mode for continuous monitoring.
fn handle_follow_mode(
    mut config: Config,
    mut geo_times: Option<GeoTimes>,
    json: bool,
) -> Result<()> {
    let running = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Set up Ctrl+C handler using signal-hook
    // Note: signal_hook::flag::register sets the flag to true when signal is received
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&running))?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&running))?;

    if !json {
        println!("Following sunsetr state (press Ctrl+C to stop)...\n");
    }

    let mut last_state = None;
    let mut last_progress = None;
    let mut last_preset = None;

    while !running.load(Ordering::SeqCst) {
        // Check if preset has changed and reload config if needed
        let current_preset = Config::get_active_preset().ok().flatten();
        if current_preset != last_preset {
            // Preset changed, reload config
            config = Config::load().context("Failed to reload configuration")?;

            // Reload geo times if in geo mode
            geo_times = if config.transition_mode.as_deref() == Some("geo") {
                GeoTimes::from_config(&config)?
            } else {
                None
            };

            last_preset = current_preset;
        }

        // Get current state
        let current_state = get_current_period(&config, geo_times.as_ref());
        let (current_temp, current_gamma) = current_state.values(&config);

        // Create DisplayState
        let display_state = DisplayState::new(
            current_state,
            current_temp,
            current_gamma,
            &config,
            geo_times.as_ref(),
        );

        // Determine if we should output (state changed or progress changed significantly)
        let should_output = if let Some(ref last) = last_state {
            // Check if state type changed (ignoring progress values)
            let state_changed = !matches!(
                (&last, &display_state.period),
                (Period::Day, Period::Day)
                    | (Period::Night, Period::Night)
                    | (Period::Sunset { .. }, Period::Sunset { .. })
                    | (Period::Sunrise { .. }, Period::Sunrise { .. })
                    | (Period::Static, Period::Static)
            );

            let progress_changed = if display_state.period.is_transitioning() {
                // Get current progress from the Period enum
                let current_progress = display_state.period.progress().unwrap_or(0.0) * 100.0;
                match last_progress {
                    Some(last_p) => {
                        let diff: f32 = current_progress - last_p;
                        diff.abs() >= 1.0
                    }
                    None => true,
                }
            } else {
                false
            };

            state_changed || progress_changed
        } else {
            true // First iteration
        };

        if should_output {
            if json {
                // JSON streaming - one JSON object per line
                println!("{}", display_state.to_json()?);
                std::io::stdout().flush()?;
            } else {
                // Human-readable with timestamp - compact format with all info
                let now = chrono::Local::now();
                print!("[{}] ", now.format("%H:%M:%S"));

                // Format state name without the progress value for cleaner display
                let state_name = match display_state.period {
                    Period::Day => "day",
                    Period::Night => "night",
                    Period::Sunset { .. } => "sunset",
                    Period::Sunrise { .. } => "sunrise",
                    Period::Static => "static",
                };

                if display_state.period.is_transitioning() {
                    let progress = display_state.period.progress().unwrap_or(0.0) * 100.0;
                    // During transition: show progress and time remaining in clock format
                    let remaining = display_state.transition_remaining.unwrap_or(0);
                    let remaining_hours = remaining / 3600;
                    let remaining_mins = (remaining % 3600) / 60;
                    let remaining_secs = remaining % 60;

                    let time_left = if remaining_hours > 0 {
                        format!(
                            "{:02}:{:02}:{:02} left",
                            remaining_hours, remaining_mins, remaining_secs
                        )
                    } else {
                        format!("{:02}:{:02} left", remaining_mins, remaining_secs)
                    };

                    print!(
                        "{} ({:.0}%, {}) {} | {}K→{}K @ {:.0}%→{:.0}%",
                        state_name,
                        progress,
                        time_left,
                        display_state.active_preset,
                        display_state.current_temp,
                        display_state.target_temp,
                        display_state.current_gamma,
                        display_state.target_gamma
                    );
                } else {
                    // For stable states: show current values and time until next transition
                    print!(
                        "{} {} | {}K @ {:.0}%",
                        state_name,
                        display_state.active_preset,
                        display_state.current_temp,
                        display_state.current_gamma
                    );

                    // Show time until next transition for stable states
                    if let Some(next) = &display_state.next_period {
                        let now = chrono::Local::now();
                        let duration = *next - now;
                        let hours = duration.num_hours();
                        let minutes = duration.num_minutes() % 60;

                        if hours > 0 {
                            print!(
                                " | {}h{}m until {}",
                                hours,
                                minutes,
                                match display_state.period {
                                    Period::Day => "sunset",
                                    Period::Night => "sunrise",
                                    _ => "transition",
                                }
                            );
                        } else if minutes > 0 {
                            print!(
                                " | {}m until {}",
                                minutes,
                                match display_state.period {
                                    Period::Day => "sunset",
                                    Period::Night => "sunrise",
                                    _ => "transition",
                                }
                            );
                        }
                    }
                }

                println!(); // End the line
                std::io::stdout().flush()?;
            }

            last_state = Some(display_state.period);
            if display_state.period.is_transitioning() {
                last_progress = Some(display_state.period.progress().unwrap_or(0.0) * 100.0);
            } else {
                last_progress = None;
            }
        }

        // Calculate sleep duration
        let sleep_duration = if current_state.is_transitioning() {
            // During transitions, check more frequently
            Duration::from_secs(config.update_interval.unwrap_or(60).min(10))
        } else {
            // During stable periods, calculate time to next event
            let duration = time_until_next_event(&config, geo_times.as_ref());
            // Cap at 60 seconds for responsiveness
            duration.min(Duration::from_secs(60))
        };

        // Sleep with early exit on Ctrl+C
        let start = std::time::Instant::now();
        while !running.load(Ordering::SeqCst) && start.elapsed() < sleep_duration {
            thread::sleep(Duration::from_millis(100));
        }
    }

    if !json {
        println!("\nStopped monitoring.");
    }

    Ok(())
}

/// Print human-readable state information.
fn print_human_readable(display_state: &DisplayState) {
    println!("Preset: {}", display_state.active_preset);

    println!("State: {}", display_state.period);

    if display_state.period.is_transitioning() {
        let remaining = display_state.transition_remaining.unwrap_or(0);
        let hours = remaining / 3600;
        let minutes = (remaining % 3600) / 60;
        let seconds = remaining % 60;

        if hours > 0 {
            println!("Time remaining: {:02}:{:02}:{:02}", hours, minutes, seconds);
        } else {
            println!("Time remaining: {:02}:{:02}", minutes, seconds);
        }
    }

    println!(
        "Current: {}K @ {:.1}% gamma",
        display_state.current_temp, display_state.current_gamma
    );
    println!(
        "Target: {}K @ {:.1}% gamma",
        display_state.target_temp, display_state.target_gamma
    );

    if let Some(next) = &display_state.next_period {
        let now = chrono::Local::now();
        let duration = *next - now;
        let hours = duration.num_hours();
        let minutes = duration.num_minutes() % 60;

        if hours > 0 {
            println!(
                "Next: {} at {} ({} hours {} minutes)",
                match display_state.period {
                    Period::Day | Period::Sunrise { .. } => "Sunset",
                    Period::Night | Period::Sunset { .. } => "Sunrise",
                    Period::Static => "None",
                },
                next.format("%H:%M:%S"),
                hours,
                minutes
            );
        } else {
            println!(
                "Next: {} at {} ({} minutes)",
                match display_state.period {
                    Period::Day | Period::Sunrise { .. } => "Sunset",
                    Period::Night | Period::Sunset { .. } => "Sunrise",
                    Period::Static => "None",
                },
                next.format("%H:%M:%S"),
                minutes
            );
        }
    } else {
        println!("Next: No transitions scheduled (static mode)");
    }
}
