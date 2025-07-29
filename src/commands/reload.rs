//! Implementation of the --reload command.
//!
//! This command resets all display gamma across all protocols and then either
//! signals an existing sunsetr process to reload or starts a new instance.

use crate::logger::Log;
use anyhow::Result;

/// Handle the --reload command to reset gamma and signal/spawn sunsetr.
pub fn handle_reload_command(debug_enabled: bool) -> Result<()> {
    Log::log_version();

    // Debug logging for reload investigation
    #[cfg(debug_assertions)]
    eprintln!("DEBUG: handle_reload_command() starting");

    // Load and validate configuration first
    // This ensures we fail fast with a clear error message if config is invalid
    let config = crate::config::Config::load()?;

    // Check for existing sunsetr process first
    let existing_pid_result = crate::utils::get_running_sunsetr_pid();

    #[cfg(debug_assertions)]
    eprintln!(
        "DEBUG: Existing sunsetr process check: {:?}",
        existing_pid_result
    );

    match existing_pid_result {
        Ok(pid) => {
            // Existing process - just signal reload (it will handle gamma correctly)
            Log::log_block_start("Signaling existing sunsetr to reload...");

            use nix::sys::signal::{Signal, kill};
            use nix::unistd::Pid;

            match kill(Pid::from_raw(pid as i32), Signal::SIGUSR2) {
                Ok(_) => {
                    Log::log_decorated(&format!("Sent reload signal to sunsetr (PID: {})", pid));
                    Log::log_indented("Existing process will reload configuration");
                }
                Err(e) => {
                    Log::log_error(&format!("Failed to signal existing process: {}", e));
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
            let lock_path = format!("{}/sunsetr.lock", runtime_dir);
            let _ = std::fs::remove_file(&lock_path);

            if debug_enabled {
                Log::log_pipe();
                Log::log_warning("Removed stale lock file from previous sunsetr instance");
            }

            // Check for orphaned hyprsunset and fail with same error as normal startup
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Checking for orphaned hyprsunset processes");

            if crate::backend::hyprland::is_hyprsunset_running() {
                Log::log_pipe();
                Log::log_warning(
                    "hyprsunset is already running but start_hyprsunset is enabled in config.",
                );
                Log::log_pipe();
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
            Log::log_block_start("Resetting gamma and starting new sunsetr instance...");

            // Spawn Wayland reset in background thread
            let config_clone = config.clone();
            let wayland_handle =
                std::thread::spawn(move || reset_wayland_gamma_only(config_clone, debug_enabled));

            // Start new sunsetr instance while Wayland reset happens in parallel
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: About to call spawn_background_process()");

            crate::utils::spawn_background_process(debug_enabled)?;
            Log::log_decorated("New sunsetr instance started");

            // Wait for Wayland reset to complete and log result
            match wayland_handle.join() {
                Ok(Ok(())) => {
                    Log::log_decorated("Wayland gamma reset completed");
                }
                Ok(Err(e)) => {
                    Log::log_warning(&format!("Wayland reset skipped: {}", e));
                }
                Err(_) => {
                    Log::log_warning("Wayland reset thread panicked");
                }
            }

            #[cfg(debug_assertions)]
            eprintln!("DEBUG: spawn_background_process() completed");
        }
    }

    Log::log_block_start("Reload complete");
    Log::log_end();
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
