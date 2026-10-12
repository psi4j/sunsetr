//! Transient temperature and gamma offsets for the running instance. The offset
//! is never written to a config file.

use crate::core::runtime_state::{AdjustRequest, Adjustment};
use anyhow::{Context, Result};

/// Send the running instance a delta or a reset. The instance adds the delta
/// to the offset it holds, so overlapping calls all count.
pub fn handle_adjust_command(
    temp_delta: Option<i32>,
    gamma_delta: Option<f64>,
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
    let request = AdjustRequest {
        reset,
        delta: Adjustment {
            temp_offset: temp_delta.unwrap_or(0),
            gamma_offset: gamma_delta.unwrap_or(0.0),
        },
    };

    crate::io::instance::send_adjust_signal(pid, &request)
        .context("Failed to send the adjustment to the running instance")?;

    log_pipe!();
    if reset {
        log_info!("Adjustment cleared");
    } else {
        log_info!("Adjusting by {}", request.delta);
    }
    log_end!();
    Ok(())
}

pub fn show_usage() {
    log_version!();
    log_block_start!("Usage: sunsetr adjust <field>[+|-]=<N> [...] | --reset|-r");
    log_pipe!();
    log_info!("For detailed help with examples, try: sunsetr help adjust");
    log_end!();
}

pub fn display_help() {
    log_version!();
    log_block_start!(
        "Offset the running instance's temperature or gamma without changing the config"
    );
    log_block_start!("Usage: sunsetr adjust <field>[+|-]=<N> [...]");
    log_indented!("       sunsetr adjust <field> <+N|-N> [...]");
    log_indented!("       sunsetr adjust --reset|-r");
    log_block_start!("Fields:");
    log_indented!("temp, current_temp      Temperature in Kelvin");
    log_indented!("gamma, current_gamma    Gamma in percent");
    log_block_start!("Operators:");
    log_indented!("<field>+=<N>            Add N to the current offset");
    log_indented!("<field>-=<N>            Subtract N from the current offset");
    log_indented!("<field> <+N|-N>         Same as += and -=");
    log_block_start!("Options:");
    log_indented!("--reset, -r             Clear the offset");
    log_block_start!("Behavior:");
    log_indented!("The offset is added on top of the scheduled value, so a transition keeps");
    log_indented!("its shape and the applied value moves by exactly the amount given.");
    log_indented!("It stays through period changes and config reloads, and is cleared by");
    log_indented!("--reset, a preset switch, or a restart.");
    log_block_start!("Examples:");
    log_indented!("# Dim by 5 percent for now, either spelling");
    log_indented!("sunsetr adjust gamma-=5");
    log_indented!("sunsetr adjust gamma -5");
    log_pipe!();
    log_indented!("# Warmer and dimmer in one step");
    log_indented!("sunsetr adjust temp-=300 gamma-=5");
    log_pipe!();
    log_indented!("# Back to the scheduled values");
    log_indented!("sunsetr adjust --reset");
    log_end!();
}
