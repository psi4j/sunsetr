//! Status command - monitor runtime state via IPC events.
//!
//! Connects to the running sunsetr process to receive typed events (StateApplied,
//! PeriodChanged, PresetChanged). The server sends an initial StateApplied event
//! on connection. Supports one-shot and follow modes with JSON or text output.

use anyhow::{Context, Result};
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::core::period::Period;
use crate::state::display::DisplayState;
use crate::state::ipc::client::IpcClient;
use crate::state::ipc::events::IpcEvent;
use crate::utils::format_progress_percentage;

/// Calculate time remaining until next period starts.
///
/// This calculates the time remaining based on the current time and the next_period
/// timestamp, providing an accurate value regardless of when the status command is run.
/// Rounds up fractional seconds for display (4.7s shows as 5s).
fn calculate_time_remaining(state: &DisplayState) -> Option<u64> {
    if let Some(next_period) = &state.next_period {
        let now = chrono::Local::now();
        let duration = *next_period - now;
        if duration.num_seconds() > 0 {
            // Use centralized duration formatting with ceiling rounding
            Some(crate::utils::format_chrono_duration_seconds_ceil(duration))
        } else {
            None
        }
    } else {
        None
    }
}

/// Handle the status command via IPC events.
///
/// Receives an initial StateApplied event on connection (one-shot mode) or
/// continues streaming all events in follow mode.
///
/// # Arguments
/// * `json` - Output in JSON format
/// * `follow` - Stream events continuously vs one-shot
/// * `config_dir` - Unused, kept for API compatibility
pub fn handle_status_command(json: bool, follow: bool, _config_dir: Option<&str>) -> Result<()> {
    let mut ipc_client = match IpcClient::connect() {
        Ok(client) => client,
        Err(_) => {
            log_error_standalone!("No sunsetr process is running");
            println!("  Start sunsetr first or use 'sunsetr --debug' to run");
            return Ok(());
        }
    };

    if follow {
        handle_follow_mode_via_ipc(ipc_client, json)
    } else {
        let display_state = ipc_client
            .current()
            .context("Failed to receive current state from sunsetr process")?;
        output_status(&display_state, json)?;
        Ok(())
    }
}

fn output_status(state: &DisplayState, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(state)?);
    } else {
        display_human_readable(state)?;
    }
    Ok(())
}

fn display_human_readable(state: &DisplayState) -> Result<()> {
    println!(" Active preset: {}", state.active_preset);

    match &state.period {
        Period::Day => {
            println!(
                "Current period: {} {}",
                state.period.display_name(),
                state.period.symbol()
            );
            println!("         State: {}", state.period_type);
            println!("   Temperature: {}K", state.current_temp);
            println!("         Gamma: {:.1}%", state.current_gamma);
            if let Some(remaining) = calculate_time_remaining(state)
                && let Some(next) = &state.next_period
            {
                let duration_str = format_duration(remaining);
                println!(
                    "   Next period: {} (in {})",
                    next.format("%H:%M:%S"),
                    duration_str
                );
            }
        }
        Period::Night => {
            println!(
                "Current period: {} {}",
                state.period.display_name(),
                state.period.symbol()
            );
            println!("         State: {}", state.period_type);
            println!("   Temperature: {}K", state.current_temp);
            println!("         Gamma: {:.1}%", state.current_gamma);
            if let Some(remaining) = calculate_time_remaining(state)
                && let Some(next) = &state.next_period
            {
                let duration_str = format_duration(remaining);
                println!(
                    "   Next period: {} (in {})",
                    next.format("%H:%M:%S"),
                    duration_str
                );
            }
        }
        Period::Sunset => {
            println!(
                "Current period: {} {}({})",
                state.period.display_name(),
                state.period.symbol(),
                format_progress_percentage(
                    state
                        .progress
                        .expect("Sunset period should always have progress"),
                    None
                )
            );
            println!("         State: {}", state.period_type);
            println!(
                "   Temperature: {}K → {}K",
                state.current_temp,
                state
                    .target_temp
                    .expect("Sunset period should always have target_temp")
            );
            println!(
                "         Gamma: {:.1}% → {:.1}%",
                state.current_gamma,
                state
                    .target_gamma
                    .expect("Sunset period should always have target_gamma")
            );
            if let Some(remaining) = calculate_time_remaining(state)
                && let Some(next) = &state.next_period
            {
                let duration_str = format_duration(remaining);
                println!(
                    "   Next period: {} (in {})",
                    next.format("%H:%M:%S"),
                    duration_str
                );
            }
        }
        Period::Sunrise => {
            println!(
                "Current period: {} {} ({})",
                state.period.display_name(),
                state.period.symbol(),
                format_progress_percentage(
                    state
                        .progress
                        .expect("Sunrise period should always have progress"),
                    None
                )
            );
            println!("         State: {}", state.period_type);
            println!(
                "   Temperature: {}K → {}K",
                state.current_temp,
                state
                    .target_temp
                    .expect("Sunrise period should always have target_temp")
            );
            println!(
                "         Gamma: {:.1}% → {:.1}%",
                state.current_gamma,
                state
                    .target_gamma
                    .expect("Sunrise period should always have target_gamma")
            );
            if let Some(remaining) = calculate_time_remaining(state)
                && let Some(next) = &state.next_period
            {
                let duration_str = format_duration(remaining);
                println!(
                    "   Next period: {} (in {})",
                    next.format("%H:%M:%S"),
                    duration_str
                );
            }
        }
        Period::Static => {
            println!(
                "Current period: {} {}",
                state.period.display_name(),
                state.period.symbol()
            );
            println!("         State: {}", state.period_type);
            println!("   Temperature: {}K", state.current_temp);
            println!("         Gamma: {:.1}%", state.current_gamma);
        }
    }

    Ok(())
}

/// Handle follow mode - poll and display IPC events continuously.
///
/// Event types: StateApplied (state updates), PeriodChanged (period transitions),
/// PresetChanged (config changes). Tracks progress for rate-of-change indicators.
fn handle_follow_mode_via_ipc(mut ipc_client: IpcClient, json: bool) -> Result<()> {
    // Set up Ctrl+C handler with proper signal inversion
    let running = Arc::new(AtomicBool::new(false)); // Start false, becomes true on signal
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&running))?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&running))?;

    if !json {
        println!("Following sunsetr state changes (press Ctrl+C to stop)...\n");
    }

    // Set socket to non-blocking mode for event-based polling
    ipc_client
        .set_nonblocking(true)
        .context("Failed to set IPC socket to non-blocking mode")?;

    // Track previous progress for Rate of Change calculation
    let mut previous_progress: Option<f32> = None;

    // Event-based polling loop
    loop {
        // Check for signal first for responsive exit
        if running.load(Ordering::SeqCst) {
            break;
        }

        // Try to receive any available events (non-blocking)
        match ipc_client.try_receive_event() {
            Ok(Some(event)) => {
                // Event received! Display it based on type
                display_ipc_event(&event, json, &mut previous_progress)?;
            }
            Ok(None) => {
                // No events available - this is normal, just continue polling
            }
            Err(e) => {
                let is_connection_error = e.to_string().contains("Connection closed")
                    || e.to_string().contains("Connection refused")
                    || e.to_string().contains("No such file or directory");

                if is_connection_error {
                    if !json {
                        eprintln!("Sunsetr process stopped. Exiting follow mode.");
                    }
                    break;
                } else {
                    if !json {
                        eprintln!("IPC error: {}", e);
                        eprintln!("Exiting follow mode.");
                    }
                    break;
                }
            }
        }

        // Small delay for polling loop (100ms = very responsive)
        thread::sleep(Duration::from_millis(100));
    }

    if !json {
        println!("\nStopped following sunsetr state.");
    }

    Ok(())
}

/// Display an IPC event in the appropriate format.
///
/// Handles three event types:
/// - StateApplied: Updates temperature/gamma values and progress
/// - PeriodChanged: Shows period transitions with symbolic indicators
/// - PresetChanged: Displays configuration changes with new target values
///
/// Tracks previous progress for rate of change indicators in transitions.
fn display_ipc_event(
    event: &IpcEvent,
    json: bool,
    previous_progress: &mut Option<f32>,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string(event)?);
        std::io::stdout().flush()?;
    } else {
        match event {
            IpcEvent::StateApplied { state } => {
                display_state_event(state, previous_progress)?;
            }
            IpcEvent::PeriodChanged {
                from_period,
                to_period,
            } => {
                display_period_changed_event(from_period, to_period)?;
                // Reset previous progress when period changes to avoid stale comparisons
                *previous_progress = None;
            }
            IpcEvent::PresetChanged {
                from_preset,
                to_preset,
                target_period,
                target_temp,
                target_gamma,
            } => {
                display_preset_changed_event(
                    from_preset,
                    to_preset,
                    target_period,
                    *target_temp,
                    *target_gamma,
                )?;
            }
        }
    }
    Ok(())
}

fn display_state_event(
    display_state: &DisplayState,
    previous_progress: &mut Option<f32>,
) -> Result<()> {
    let now = chrono::Local::now();
    print!("[{}] ", now.format("%H:%M:%S"));

    // Format state description with time remaining inline for transitions
    let state_description = match &display_state.period {
        Period::Day => "day".to_string(),
        Period::Night => "night".to_string(),
        Period::Sunset => {
            let progress = display_state
                .progress
                .expect("Sunset period should always have progress");
            let mut desc = format!(
                "sunset {}",
                format_progress_percentage(progress, *previous_progress)
            );
            if let Some(remaining) = calculate_time_remaining(display_state) {
                let duration_str = format_duration(remaining);
                desc.push_str(&format!(" ({})", duration_str));
            }
            desc
        }
        Period::Sunrise => {
            let progress = display_state
                .progress
                .expect("Sunrise period should always have progress");
            let mut desc = format!(
                "sunrise {}",
                format_progress_percentage(progress, *previous_progress)
            );
            if let Some(remaining) = calculate_time_remaining(display_state) {
                let duration_str = format_duration(remaining);
                desc.push_str(&format!(" ({})", duration_str));
            }
            desc
        }
        Period::Static => "static".to_string(),
    };

    print!(
        "{} {} | {}K @ {:.1}%",
        display_state.active_preset,
        state_description,
        display_state.current_temp,
        display_state.current_gamma
    );

    // Show target values if transitioning, or time until next for stable states
    if display_state.period.is_transitioning() {
        print!(
            " → {}K @ {:.1}%",
            display_state
                .target_temp
                .expect("Transitioning period should always have target_temp"),
            display_state
                .target_gamma
                .expect("Transitioning period should always have target_gamma")
        );
    } else {
        // Show time until next period for stable states
        if let Some(remaining) = calculate_time_remaining(display_state) {
            let duration_str = format_duration(remaining);
            print!(" | {} until next", duration_str);
        }
    }

    println!(); // End the line
    std::io::stdout().flush()?;

    // Update previous progress for next Rate of Change calculation
    if display_state.period.is_transitioning() {
        *previous_progress = display_state.progress;
    }

    Ok(())
}

fn display_period_changed_event(from_period: &Period, to_period: &Period) -> Result<()> {
    let now = chrono::Local::now();
    print!("[{}] ", now.format("%H:%M:%S"));

    println!(
        "PERIOD: {} {} → {} {}",
        from_period.display_name().to_lowercase(),
        from_period.symbol(),
        to_period.display_name().to_lowercase(),
        to_period.symbol()
    );
    std::io::stdout().flush()?;
    Ok(())
}

fn display_preset_changed_event(
    from_preset: &Option<String>,
    to_preset: &Option<String>,
    target_period: &Period,
    target_temp: u32,
    target_gamma: f32,
) -> Result<()> {
    let now = chrono::Local::now();
    print!("[{}] ", now.format("%H:%M:%S"));

    let from_name = from_preset.as_deref().unwrap_or("default");
    let to_name = to_preset.as_deref().unwrap_or("default");

    print!(
        "PRESET: {} → {} {} ",
        from_name,
        to_name,
        target_period.symbol()
    );

    println!("(target: {}K @ {:.1}%)", target_temp, target_gamma);
    std::io::stdout().flush()?;
    Ok(())
}

fn format_duration(total_seconds: u64) -> String {
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        if minutes > 0 {
            format!("{}h{}m", hours, minutes)
        } else {
            format!("{}h", hours)
        }
    } else if minutes > 0 {
        if seconds > 30 {
            format!("{}m", minutes + 1)
        } else {
            format!("{}m", minutes)
        }
    } else {
        format!("{}s", seconds)
    }
}

pub fn show_usage() {
    log_version!();
    log_block_start!("Usage: sunsetr status [--json] [--follow]");
    log_block_start!("Description:");
    log_indented!("Display current runtime state of the running sunsetr instance");
    log_pipe!();
    log_info!("For detailed help with examples, try: sunsetr help status");
    log_end!();
}

pub fn display_help() {
    log_version!();
    log_block_start!("status - Display current runtime state");
    log_block_start!("Usage: sunsetr status [--json] [--follow]");
    log_block_start!("Description:");
    log_indented!("Shows the current state of the running sunsetr instance via IPC,");
    log_indented!("including temperature, gamma, period, transition progress, and timing.");
    log_indented!("In follow mode, streams live events (state updates, period changes,");
    log_indented!("preset changes) providing real-time monitoring of all state transitions.");
    log_block_start!("Options:");
    log_indented!("--json     Output state information in JSON format");
    log_indented!("--follow   Continuously monitor and display state changes");
    log_block_start!("Examples:");
    log_indented!("# Show current state once");
    log_indented!("sunsetr status");
    log_pipe!();
    log_indented!("# Show state in JSON format");
    log_indented!("sunsetr status --json");
    log_pipe!();
    log_indented!("# Monitor state changes in real-time");
    log_indented!("sunsetr status --follow");
    log_pipe!();
    log_indented!("# Follow mode with JSON output");
    log_indented!("sunsetr status --json --follow");
    log_end!();
}
