//! Transient temperature and gamma adjustments for the running instance. An
//! adjustment is never written to a config file.

use crate::core::runtime_state::{AdjustOp, AdjustRequest, Adjustment};
use crate::state::ipc::client::IpcClient;
use anyhow::{Context, Result, bail};

/// Send the running instance one request. The instance resolves it against
/// the adjustment it holds, so overlapping calls all count.
///
/// An absolute value without `--static` is sent only when the instance
/// reports the field as already held, since an Assign always holds.
pub fn handle_adjust_command(
    temp: Option<AdjustOp<i32>>,
    gamma: Option<AdjustOp<f64>>,
    is_static: bool,
    reset: bool,
) -> Result<()> {
    log_version!();

    if crate::io::instance::is_test_mode_active() {
        log_error_end!(
            "Cannot adjust while test mode is active\n   Exit test mode first (press Escape in the test terminal)"
        );
        return Ok(());
    }

    let pid = crate::io::instance::get_running_instance_pid()?;
    if !is_static {
        check_assigns_are_held(temp, gamma)?;
    }
    let request = AdjustRequest {
        reset,
        is_static,
        temp,
        gamma,
    };

    crate::io::instance::send_adjust_signal(pid, &request)
        .context("Failed to send the adjustment to the running instance")?;

    log_pipe!();
    if reset {
        log_info!("Adjustment cleared");
    } else if temp.is_none() && gamma.is_none() {
        log_info!("Holding the current values");
    } else {
        let mut parts = Vec::new();
        if let Some(op) = temp {
            parts.push(match op {
                AdjustOp::Assign(value) => format!("temp={value}K"),
                AdjustOp::Delta(delta) if delta < 0 => format!("temp-={}K", -delta),
                AdjustOp::Delta(delta) => format!("temp+={delta}K"),
            });
        }
        if let Some(op) = gamma {
            parts.push(match op {
                AdjustOp::Assign(value) => format!("gamma={value:.1}%"),
                AdjustOp::Delta(delta) if delta < 0.0 => format!("gamma-={:.1}%", -delta),
                AdjustOp::Delta(delta) => format!("gamma+={delta:.1}%"),
            });
        }
        let mode = if is_static { "Holding" } else { "Adjusting" };
        log_info!("{mode} {}", parts.join(", "));
    }
    log_end!();
    Ok(())
}

fn check_assigns_are_held(temp: Option<AdjustOp<i32>>, gamma: Option<AdjustOp<f64>>) -> Result<()> {
    let temp_assign = match temp {
        Some(AdjustOp::Assign(value)) => Some(format!("{value}K")),
        _ => None,
    };
    let gamma_assign = match gamma {
        Some(AdjustOp::Assign(value)) => Some(format!("{value:.1}%")),
        _ => None,
    };
    if temp_assign.is_none() && gamma_assign.is_none() {
        return Ok(());
    }

    let held: Adjustment = IpcClient::connect()
        .and_then(|mut client| client.current())
        .map(|state| state.adjustment)
        .context("Cannot tell whether the field is held (pass --static to hold it)")?;

    for (field, shown, is_hold) in [
        ("temp", temp_assign, held.temp.is_hold()),
        ("gamma", gamma_assign, held.gamma.is_hold()),
    ] {
        if let Some(shown) = shown
            && !is_hold
        {
            bail!("{field} is following the schedule (pass --static to hold it at {shown})");
        }
    }
    Ok(())
}

pub fn show_usage() {
    log_version!();
    log_block_start!(
        "Usage: sunsetr adjust [--static] <field>[+|-]=<value> [...] | --static | --reset"
    );
    log_pipe!();
    log_info!("For detailed help with examples, try: sunsetr help adjust");
    log_end!();
}

pub fn display_help() {
    log_version!();
    log_block_start!(
        "Adjust the running instance's temperature or gamma without changing the config"
    );
    log_block_start!("Usage: sunsetr adjust [--static] <field>[+|-]=<value> [...]");
    log_indented!("       sunsetr adjust [--static] <field> <[+|-]value> [...]");
    log_indented!("       sunsetr adjust --static|-s");
    log_indented!("       sunsetr adjust --reset|-r");
    log_block_start!("Fields:");
    log_indented!("temp, current_temp      Temperature in Kelvin");
    log_indented!("gamma, current_gamma    Gamma in percent");
    log_block_start!("Operators:");
    log_indented!("<field>+=<value>        Move the field up by value");
    log_indented!("<field>-=<value>        Move the field down by value");
    log_indented!("<field>=<value>         Hold the field at value (needs --static unless held)");
    log_indented!("<field> <+value|-value> Same as += and -=");
    log_indented!("<field> <value>         Same as =");
    log_block_start!("Options:");
    log_indented!("--static, -s            Stop the field from following the schedule, or");
    log_indented!("                        on its own hold the current values");
    log_indented!("--reset, -r             Clear every adjustment");
    log_block_start!("Behavior:");
    log_indented!("Without --static the amount is an offset on top of the scheduled value,");
    log_indented!("so a transition keeps its shape and the display moves by exactly the");
    log_indented!("amount given. With --static the result is held until --reset. A held");
    log_indented!("field takes later values and moves without the flag. Every adjustment");
    log_indented!("stays through period changes and config reloads, and is cleared by");
    log_indented!("--reset, a preset switch, or a restart.");
    log_block_start!("Examples:");
    log_indented!("# Dim by 5 percent for now, either spelling");
    log_indented!("sunsetr adjust gamma-=5");
    log_indented!("sunsetr adjust gamma -5");
    log_pipe!();
    log_indented!("# Warmer and dimmer in one step");
    log_indented!("sunsetr adjust temp-=300 gamma-=5");
    log_pipe!();
    log_indented!("# Hold 3500K until reset, then nudge the held value");
    log_indented!("sunsetr adjust --static temp=3500");
    log_indented!("sunsetr adjust --static temp-=100");
    log_pipe!();
    log_indented!("# Freeze whatever is on screen right now");
    log_indented!("sunsetr adjust --static");
    log_pipe!();
    log_indented!("# Back to the scheduled values");
    log_indented!("sunsetr adjust --reset");
    log_end!();
}
