//! Status command - display current runtime state via IPC.
//!
//! This command connects to the running sunsetr process via IPC to get the actual
//! DisplayState, ensuring perfect consistency with what's actually applied.
//! Supports JSON and human-readable output, with optional follow mode.

use anyhow::{Context, Result};
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::core::period::Period;
use crate::state::display::DisplayState;
use crate::state::ipc::client::IpcClient;

/// Handle the status command using IPC client approach.
///
/// This command connects to the running sunsetr process via IPC to get the actual
/// DisplayState, ensuring perfect consistency with what's actually applied.
///
/// # Arguments
/// * `json` - Output in JSON format
/// * `follow` - Follow mode for continuous updates
/// * `config_dir` - Optional custom configuration directory (ignored, kept for API compatibility)
pub fn handle_status_command(json: bool, follow: bool, _config_dir: Option<&str>) -> Result<()> {
    // Connect to IPC socket as pure client
    let mut ipc_client = match IpcClient::connect() {
        Ok(client) => client,
        Err(_) => {
            log_error_standalone!("No sunsetr process is running");
            println!("  Start sunsetr first or use 'sunsetr --debug' to run");
            return Ok(());
        }
    };

    if follow {
        // Follow mode: stream live IPC events
        handle_follow_mode_via_ipc(ipc_client, json)
    } else {
        // One-shot: receive current state immediately upon connection
        let display_state = ipc_client
            .current()
            .context("Failed to receive current state from sunsetr process")?;
        output_status(&display_state, json)?;
        Ok(())
    }
}

/// Output the DisplayState in the requested format.
fn output_status(state: &DisplayState, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(state)?);
    } else {
        display_human_readable(state)?;
    }
    Ok(())
}

/// Display DisplayState in human-readable format.
fn display_human_readable(state: &DisplayState) -> Result<()> {
    println!(" Active preset: {}", state.active_preset);

    // Display current period
    match &state.period {
        Period::Day => {
            println!("Current period: Day");
            println!("   Temperature: {}K", state.current_temp);
            println!("         Gamma: {:.1}%", state.current_gamma);
            if let Some(remaining) = state.time_remaining
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
            println!("Current period: Night");
            println!("   Temperature: {}K", state.current_temp);
            println!("         Gamma: {:.1}%", state.current_gamma);
            if let Some(remaining) = state.time_remaining
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
            // Need to calculate progress with RuntimeState since Period no longer has progress field
            // TODO: This should be provided by DisplayState.progress field per specification
            println!("Current period: Sunset transition");
            println!(
                "   Temperature: {}K → {}K",
                state.current_temp, state.target_temp
            );
            println!(
                "         Gamma: {:.1}% → {:.1}%",
                state.current_gamma, state.target_gamma
            );
            if let Some(remaining) = state.time_remaining
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
            // Need to calculate progress with RuntimeState since Period no longer has progress field
            // TODO: This should be provided by DisplayState.progress field per specification
            println!("Current period: Sunrise transition");
            println!(
                "   Temperature: {}K → {}K",
                state.current_temp, state.target_temp
            );
            println!(
                "         Gamma: {:.1}% → {:.1}%",
                state.current_gamma, state.target_gamma
            );
            if let Some(remaining) = state.time_remaining
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
            println!("Current period: Static");
            println!("   Temperature: {}K", state.current_temp);
            println!("         Gamma: {:.1}%", state.current_gamma);
        }
    }

    Ok(())
}

/// Handle follow mode via event-based IPC polling.
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

    // Event-based polling loop
    loop {
        // Check for signal first for responsive exit
        if running.load(Ordering::SeqCst) {
            break;
        }

        // Try to receive any available events (non-blocking)
        match ipc_client.try_receive() {
            Ok(Some(display_state)) => {
                // Event received! Display it
                display_event(&display_state, json)?;
            }
            Ok(None) => {
                // No events available - this is normal, just continue polling
            }
            Err(e) => {
                // Connection error
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

/// Display a state change event in the appropriate format.
fn display_event(display_state: &DisplayState, json: bool) -> Result<()> {
    if json {
        // JSON streaming - one JSON object per line
        println!("{}", serde_json::to_string(display_state)?);
        std::io::stdout().flush()?;
    } else {
        // Human-readable with timestamp
        let now = chrono::Local::now();
        print!("[{}] ", now.format("%H:%M:%S"));

        // Format state description with time remaining inline for transitions
        let state_description = match &display_state.period {
            Period::Day => "day".to_string(),
            Period::Night => "night".to_string(),
            Period::Sunset => {
                let mut desc = "sunset".to_string();
                if let Some(remaining) = display_state.time_remaining {
                    let duration_str = format_duration(remaining);
                    desc.push_str(&format!(" ({})", duration_str));
                }
                desc
            }
            Period::Sunrise => {
                let mut desc = "sunrise".to_string();
                if let Some(remaining) = display_state.time_remaining {
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
                display_state.target_temp, display_state.target_gamma
            );
        } else {
            // Show time until next period for stable states
            if let Some(remaining) = display_state.time_remaining {
                let duration_str = format_duration(remaining);
                print!(" | {} until next", duration_str);
            }
        }

        println!(); // End the line
        std::io::stdout().flush()?;
    }
    Ok(())
}

/// Format time duration consistently across all displays.
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
            // Round up if more than 30 seconds
            format!("{}m", minutes + 1)
        } else {
            format!("{}m", minutes)
        }
    } else {
        format!("{}s", seconds)
    }
}

/// Display help for the status command.
pub fn display_help() {
    log_version!();
    log_block_start!("status - Display current runtime state");
    log_block_start!("Usage: sunsetr status [--json] [--follow]");
    log_block_start!("Description:");
    log_indented!("Shows the current state of the running sunsetr instance via IPC,");
    log_indented!("including temperature, gamma, period, transition progress, and timing.");
    log_indented!("This provides real-time information about what's actually applied.");
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
