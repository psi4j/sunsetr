//! Implementation of the --reload command.
//!
//! This command resets all display gamma across all protocols and then either
//! signals an existing sunsetr process to reload or starts a new instance.

use anyhow::Result;

/// Handle the --reload command to reset gamma and signal/spawn sunsetr.
pub fn handle_reload_command(debug_enabled: bool) -> Result<()> {
    log_version!();

    // Check if test mode is active
    let test_lock_path = "/tmp/sunsetr-test.lock";

    // Check if lock file exists and if the PID in it is still running
    if let Ok(contents) = std::fs::read_to_string(test_lock_path)
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
            let _ = std::fs::remove_file(test_lock_path);
        }
    }

    // Debug logging for reload investigation
    #[cfg(debug_assertions)]
    eprintln!("DEBUG: handle_reload_command() starting");

    // Load and validate configuration first
    // This ensures we fail fast with a clear error message if config is invalid
    let config = crate::config::Config::load()?;

    // Check for existing sunsetr process first
    let existing_pid_result = crate::utils::get_running_sunsetr_pid();

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

            // Check for orphaned hyprsunset and fail with same error as normal startup
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Checking for orphaned hyprsunset processes");

            if crate::backend::hyprland::is_hyprsunset_running() {
                log_pipe!();
                log_warning!(
                    "hyprsunset is already running but start_hyprsunset is enabled in config."
                );
                log_pipe!();
                anyhow::bail!(
                    "This indicates a configuration conflict. Please choose one:\n\
                    • Kill the existing hyprsunset process: pkill hyprsunset\n\
                    • Change start_hyprsunset = false in sunsetr.toml\n\
                    \n\
                    Choose the first option if you want sunsetr to manage hyprsunset.\n\
                    Choose the second option if you're using another method to start hyprsunset.",
                );
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

            crate::utils::spawn_background_process(debug_enabled)?;
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
