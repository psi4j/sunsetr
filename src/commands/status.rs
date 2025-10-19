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
    println!("Active preset: {}", state.active_preset);

    // Display current period
    match &state.period {
        Period::Day => println!("Current period: Day"),
        Period::Night => println!("Current period: Night"),
        Period::Sunset { progress } => {
            println!(
                "Current period: Sunset transition ({:.0}% complete)",
                progress * 100.0
            );
            display_transition_info(state)?;
        }
        Period::Sunrise { progress } => {
            println!(
                "Current period: Sunrise transition ({:.0}% complete)",
                progress * 100.0
            );
            display_transition_info(state)?;
        }
        Period::Static => println!("Current period: Static"),
    }

    println!("  Temperature: {}K", state.current_temp);
    println!("  Gamma: {:.1}%", state.current_gamma);

    if let Some(next) = &state.next_period {
        let now = chrono::Local::now();
        let duration = *next - now;
        let total_hours = duration.num_hours();
        let minutes = duration.num_minutes() % 60;

        if total_hours > 0 {
            println!(
                "Next period: {} (in {}h{}m)",
                next.format("%H:%M:%S"),
                total_hours,
                minutes
            );
        } else if minutes > 0 {
            println!("Next period: {} (in {}m)", next.format("%H:%M:%S"), minutes);
        } else {
            println!("Next period: {} (soon)", next.format("%H:%M:%S"));
        }
    }

    Ok(())
}

/// Display transition information for transitioning states.
fn display_transition_info(state: &DisplayState) -> Result<()> {
    if state.period.is_transitioning() {
        println!("  Target temperature: {}K", state.target_temp);
        println!("  Target gamma: {:.1}%", state.target_gamma);

        if let Some(remaining) = state.transition_remaining {
            let minutes = remaining / 60;
            let seconds = remaining % 60;
            if minutes > 0 {
                println!("  Time remaining: {} minutes {} seconds", minutes, seconds);
            } else {
                println!("  Time remaining: {} seconds", seconds);
            }
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

        // Format state description
        let state_description = match &display_state.period {
            Period::Day => "day".to_string(),
            Period::Night => "night".to_string(),
            Period::Sunset { progress } => {
                format!("sunset ({:.0}%)", progress * 100.0)
            }
            Period::Sunrise { progress } => {
                format!("sunrise ({:.0}%)", progress * 100.0)
            }
            Period::Static => "static".to_string(),
        };

        print!(
            "{} {} | {}K @ {:.0}%",
            state_description,
            display_state.active_preset,
            display_state.current_temp,
            display_state.current_gamma
        );

        // Show transition information if transitioning
        if display_state.period.is_transitioning() {
            if let Some(remaining) = display_state.transition_remaining {
                let minutes = remaining / 60;
                let seconds = remaining % 60;
                if minutes > 0 {
                    print!(" | {}m{}s remaining", minutes, seconds);
                } else {
                    print!(" | {}s remaining", seconds);
                }
            }
            print!(
                " → {}K @ {:.0}%",
                display_state.target_temp, display_state.target_gamma
            );
        } else {
            // Show time until next transition for stable states
            if let Some(next) = &display_state.next_period {
                let now = chrono::Local::now();
                let duration = *next - now;
                let hours = duration.num_hours();
                let minutes = duration.num_minutes() % 60;

                if hours > 0 {
                    print!(" | {}h{}m until next", hours, minutes);
                } else if minutes > 0 {
                    print!(" | {}m until next", minutes);
                }
            }
        }

        println!(); // End the line
        std::io::stdout().flush()?;
    }
    Ok(())
}
