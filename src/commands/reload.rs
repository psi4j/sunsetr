//! Implementation of the reload command.
//!
//! This command resets all display gamma across all protocols and then either
//! signals an existing sunsetr process to reload or starts a new instance.

use anyhow::Result;

/// Handle the reload command to reset gamma and signal/spawn sunsetr.
pub fn handle_reload_command(debug_enabled: bool) -> Result<()> {
    log_version!();

    // Check if test mode is active
    let test_lock_path = crate::io::lock::get_test_lock_path();

    // Check if lock file exists and if the PID in it is still running
    if let Ok(contents) = std::fs::read_to_string(&test_lock_path)
        && let Ok(lock_pid) = contents.trim().parse::<u32>()
    {
        // Check if the process that created the lock is still running
        let proc_path = format!("/proc/{}", lock_pid);
        if std::path::Path::new(&proc_path).exists() {
            // Process is still running, test mode is active
            log_pipe!();
            log_warning!("Cannot reload while test mode is active");
            log_indented!("Exit test mode first (press Escape in the test terminal)");
            log_end!();
            return Ok(());
        } else {
            // Process is dead, remove stale lock file
            let _ = std::fs::remove_file(&test_lock_path);
        }
    }

    // Debug logging for reload investigation
    #[cfg(debug_assertions)]
    eprintln!("DEBUG: handle_reload_command() starting");

    // Check for existing sunsetr process FIRST
    // This will restore the config directory from the lock file if present
    let existing_pid_result = crate::common::utils::get_running_sunsetr_pid();

    // NOW load and validate configuration - it will use the restored custom dir if any
    // This ensures we fail fast with a clear error message if config is invalid
    let config = crate::config::Config::load()?;

    #[cfg(debug_assertions)]
    eprintln!("DEBUG: Existing sunsetr process check: {existing_pid_result:?}");

    match existing_pid_result {
        Ok(pid) => {
            // Existing process - just signal reload (it will handle gamma correctly)
            log_block_start!("Signaling existing sunsetr to reload...");

            use nix::sys::signal::{Signal, kill};
            use nix::unistd::Pid;

            match kill(Pid::from_raw(pid as i32), Signal::SIGUSR2) {
                Ok(_) => {
                    log_decorated!("Sent reload signal to sunsetr (PID: {pid})");
                    log_indented!("Existing process will reload configuration");
                }
                Err(e) => {
                    log_error!("Failed to signal existing process: {e}");
                }
            }
        }
        Err(_) => {
            // No existing process - safe to reset gamma and start new instance
            #[cfg(debug_assertions)]
            eprintln!(
                "DEBUG: No existing sunsetr process found, proceeding with gamma reset and spawn"
            );

            // Clean up stale lock file that prevented process detection
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Cleaning up stale lock file");

            let runtime_dir =
                std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
            let lock_path = format!("{runtime_dir}/sunsetr.lock");
            let _ = std::fs::remove_file(&lock_path);

            if debug_enabled {
                log_pipe!();
                log_warning!("Removed stale lock file from previous sunsetr instance");
            }

            // Start Wayland reset and sunsetr spawn in parallel for better performance
            log_block_start!("Resetting gamma and starting new sunsetr instance...");

            // Spawn Wayland reset in background thread
            let config_clone = config.clone();
            let wayland_handle =
                std::thread::spawn(move || reset_wayland_gamma_only(config_clone, debug_enabled));

            // Start new sunsetr instance while Wayland reset happens in parallel
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: About to call spawn_background_process()");

            crate::common::utils::spawn_background_process(debug_enabled)?;
            log_decorated!("New sunsetr instance started");

            // Wait for Wayland reset to complete and log result
            match wayland_handle.join() {
                Ok(Ok(())) => {
                    log_decorated!("Wayland gamma reset completed");
                }
                Ok(Err(e)) => {
                    log_warning!("Wayland reset skipped: {e}");
                }
                Err(_) => {
                    log_warning!("Wayland reset thread panicked");
                }
            }

            #[cfg(debug_assertions)]
            eprintln!("DEBUG: spawn_background_process() completed");
        }
    }

    log_block_start!("Reload complete");
    log_end!();
    Ok(())
}

/// Reset only the Wayland backend to clear residual gamma from compositor switching.
/// This is safer than resetting Hyprland which could spawn conflicting processes.
fn reset_wayland_gamma_only(config: crate::config::Config, debug_enabled: bool) -> Result<()> {
    use crate::backend::ColorTemperatureBackend;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    let running = Arc::new(AtomicBool::new(true));

    match crate::backend::wayland::WaylandBackend::new(&config, debug_enabled) {
        Ok(mut backend) => backend.apply_temperature_gamma(6500, 100.0, &running),
        Err(e) => Err(e),
    }
}

/// Display usage help for the reload command (--help flag)
pub fn show_usage() {
    log_version!();
    log_block_start!("Usage: sunsetr reload");
    log_block_start!("Description:");
    log_indented!("Reset display gamma and reload sunsetr configuration");
    log_pipe!();
    log_info!("For detailed help with examples, try: sunsetr help reload");
    log_end!();
}

/// Display detailed help for the reload command (help subcommand)
pub fn display_help() {
    log_version!();
    log_block_start!("reload - Reset display gamma and reload configuration");
    log_block_start!("Usage: sunsetr reload");
    log_block_start!("Description:");
    log_indented!("Resets all displays to default gamma values and restarts");
    log_indented!("sunsetr with the current configuration. This is useful for:");
    log_pipe!();
    log_indented!("- Switching between compositors");
    log_indented!("- Recovering from display issues");
    log_indented!("- Starting process in the background");
    log_block_start!("Process:");
    log_indented!("1. Resets gamma on all displays");
    log_indented!("2. Kills any running sunsetr instance");
    log_indented!("3. Starts a new sunsetr instance");
    log_block_start!("Examples:");
    log_indented!("# Basic reload");
    log_indented!("sunsetr reload");
    log_pipe!();
    log_indented!("# Reload with debug output");
    log_indented!("sunsetr --debug reload");
    log_end!();
}
