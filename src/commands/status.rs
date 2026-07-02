//! Monitors runtime state via IPC events.
//!
//! Connects to the running sunsetr process and receives typed state events, starting with an
//! initial StateApplied event on connection. Supports one-shot and follow modes with JSON or
//! text output.

use anyhow::{Context, Result};
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::core::period::Period;
use crate::state::display::DisplayState;
use crate::state::ipc::client::{ConnectionClosed, IpcClient};
use crate::state::ipc::events::IpcEvent;
use crate::utils::format_progress_percentage;

/// Time remaining until the next period, rounded up to whole seconds.
fn calculate_time_remaining(state: &DisplayState) -> Option<u64> {
    if let Some(next_period) = &state.next_period {
        let now = chrono::Local::now();
        let duration = *next_period - now;
        if duration.num_seconds() > 0 {
            Some(crate::utils::format_chrono_duration_seconds_ceil(duration))
        } else {
            None
        }
    } else {
        None
    }
}

/// Connect over IPC and either print the current state once or, in follow mode, stream
/// events until interrupted.
pub fn handle_status_command(json: bool, follow: bool) -> Result<()> {
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

    if state.period.is_transitioning() {
        println!(
            "Current period: {} {} ({})",
            state.period.display_name(),
            state.period.symbol(),
            format_progress_percentage(
                state
                    .progress
                    .expect("transitioning period should always have progress"),
                None
            )
        );
        println!("         State: {}", state.period_type);
        println!(
            "   Temperature: {}K → {}K",
            state.current_temp,
            state
                .target_temp
                .expect("transitioning period should always have target_temp")
        );
        println!(
            "         Gamma: {:.1}% → {:.1}%",
            state.current_gamma,
            state
                .target_gamma
                .expect("transitioning period should always have target_gamma")
        );
    } else {
        println!(
            "Current period: {} {}",
            state.period.display_name(),
            state.period.symbol()
        );
        println!("         State: {}", state.period_type);
        println!("   Temperature: {}K", state.current_temp);
        println!("         Gamma: {:.1}%", state.current_gamma);
    }

    if !matches!(state.period, Period::Static)
        && let Some(remaining) = calculate_time_remaining(state)
        && let Some(next) = &state.next_period
    {
        let duration_str = format_duration(remaining);
        println!(
            "   Next period: {} (in {})",
            next.format("%H:%M:%S"),
            duration_str
        );
    }

    Ok(())
}

/// Poll and display IPC events continuously until interrupted, tracking progress for
/// rate-of-change indicators.
fn handle_follow_mode_via_ipc(mut ipc_client: IpcClient, json: bool) -> Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop))?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop))?;

    if !json {
        println!("Following sunsetr state changes (press Ctrl+C to stop)...\n");
    }

    ipc_client
        .set_nonblocking(true)
        .context("Failed to set IPC socket to non-blocking mode")?;

    let mut previous_progress: Option<f32> = None;

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }

        match ipc_client.try_receive_event() {
            Ok(Some(event)) => {
                display_ipc_event(&event, json, &mut previous_progress)?;
            }
            Ok(None) => {}
            Err(e) => {
                if !json {
                    if e.downcast_ref::<ConnectionClosed>().is_some() {
                        eprintln!("Sunsetr process stopped. Exiting follow mode.");
                    } else {
                        eprintln!("IPC error: {e}");
                        eprintln!("Exiting follow mode.");
                    }
                }
                break;
            }
        }

        thread::sleep(Duration::from_millis(10));
    }

    if !json && stop.load(Ordering::SeqCst) {
        println!("\nStopped following sunsetr state.");
    }

    Ok(())
}

/// Display one IPC event as JSON or text, tracking previous progress for rate-of-change
/// indicators in transitions.
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
            IpcEvent::ConfigChanged {
                target_period,
                target_temp,
                target_gamma,
            } => {
                display_config_changed_event(target_period, *target_temp, *target_gamma)?;
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

    let label = display_state.period.display_name().to_lowercase();
    let state_description = if display_state.period.is_transitioning() {
        let progress = display_state
            .progress
            .expect("transitioning period should always have progress");
        let mut desc = format!(
            "{label} {}",
            format_progress_percentage(progress, *previous_progress)
        );
        if let Some(remaining) = calculate_time_remaining(display_state) {
            let duration_str = format_duration(remaining);
            desc.push_str(&format!(" ({})", duration_str));
        }
        desc
    } else {
        label
    };

    print!(
        "{} {} | {}K @ {:.1}%",
        display_state.active_preset,
        state_description,
        display_state.current_temp,
        display_state.current_gamma
    );

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
    } else if let Some(remaining) = calculate_time_remaining(display_state) {
        let duration_str = format_duration(remaining);
        print!(" | {} until next", duration_str);
    }

    println!();
    std::io::stdout().flush()?;

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
    target_gamma: f64,
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

fn display_config_changed_event(
    target_period: &Period,
    target_temp: u32,
    target_gamma: f64,
) -> Result<()> {
    let now = chrono::Local::now();
    print!("[{}] ", now.format("%H:%M:%S"));

    print!("CONFIG: {} ", target_period.symbol());

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
    log_pipe!();
    log_info!("For detailed help with examples, try: sunsetr help status");
    log_end!();
}

pub fn display_help() {
    log_version!();
    log_block_start!("Display current runtime state");
    log_block_start!("Usage: sunsetr status [--json] [--follow]");
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
