//! Cleanly terminate a running sunsetr instance.

use crate::common::constants::*;
use crate::common::error::Silent;
use crate::config::Config;
use anyhow::{Context, Result};

/// Terminate the running instance.
///
/// Exits successfully when no instance is running. Errors when termination fails or the
/// instance does not confirm exit within the timeout.
pub fn handle_stop_command() -> Result<()> {
    log_version!();

    let Some(info) = crate::io::instance::get_running_instance()
        .context("Failed to determine whether a sunsetr instance is running")?
    else {
        log_block_start!("sunsetr isn't running");
        log_end!();
        return Ok(());
    };
    let pid = info.pid;

    let config = Config::load()?;

    log_block_start!("Stopping sunsetr instance (PID: {})...", pid);

    if let Err(e) = crate::io::instance::terminate_instance(pid) {
        log_error_end!("Failed to terminate instance: {}", e);
        return Err(Silent.into());
    }

    let resolved_backend = crate::backend::detect_backend(&config)?;
    let backend_supports_smoothing =
        matches!(resolved_backend, crate::backend::BackendType::Wayland);
    let smoothing_enabled = config.smoothing.unwrap_or(DEFAULT_SMOOTHING);
    let shutdown_duration = config
        .shutdown_duration
        .unwrap_or(DEFAULT_SHUTDOWN_DURATION);

    if backend_supports_smoothing && smoothing_enabled && shutdown_duration >= 0.1 {
        log_block_start!("Shutting down...");
    }

    let base_timeout_ms = 3000u64;
    let additional_timeout_ms =
        if backend_supports_smoothing && smoothing_enabled && shutdown_duration >= 0.1 {
            (shutdown_duration * 1000.0) as u64
        } else {
            0
        };

    let total_timeout_ms = base_timeout_ms + additional_timeout_ms;
    let max_attempts = total_timeout_ms / 100;
    let _terminal_guard = crate::common::utils::TerminalGuard::new();
    let mut attempts = 0;

    while attempts < max_attempts {
        if !crate::io::instance::is_instance_running(pid) {
            log_pipe!();
            log_info!("Process terminated successfully");
            log_end!();
            return Ok(());
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
        attempts += 1;
    }

    log_pipe!();
    log_warning!("Process did not terminate within the expected time");
    log_indented!("The termination signal was sent, but the process may still be shutting down");
    log_end!();
    Err(Silent.into())
}

pub fn show_usage() {
    log_version!();
    log_block_start!("Usage: sunsetr stop");
    log_block_start!("Description:");
    log_indented!("Cleanly terminate the running sunsetr instance");
    log_pipe!();
    log_info!("For detailed help with examples, try: sunsetr help stop");
    log_end!();
}

pub fn display_help() {
    log_version!();
    log_block_start!("stop - Cleanly terminate running sunsetr");
    log_block_start!("Usage: sunsetr stop");
    log_block_start!("Description:");
    log_indented!("Sends a termination signal to the running sunsetr instance,");
    log_indented!("allowing it to shut down gracefully and reset display gamma.");
    log_indented!("Waits up to 3 seconds to confirm the process actually terminates.");
    log_block_start!("Process:");
    log_indented!("1. Locates the running sunsetr process");
    log_indented!("2. Sends SIGTERM for graceful shutdown");
    log_indented!("3. Waits for confirmation that the process terminated");
    log_indented!("4. Reports successful termination and gamma reset");
    log_block_start!("Examples:");
    log_indented!("# Stop running sunsetr");
    log_indented!("sunsetr stop");
    log_pipe!();
    log_indented!("# Stop with debug output");
    log_indented!("sunsetr --debug stop");
    log_end!();
}
