//! Implementation of the --reload command.
//!
//! This command resets all display gamma across all protocols and then either
//! signals an existing sunsetr process to reload or starts a new instance.

use anyhow::Result;
use crate::logger::Log;

/// Handle the --reload command to reset gamma and signal/spawn sunsetr.
pub fn handle_reload_command(debug_enabled: bool) -> Result<()> {
    Log::log_version();
    
    // Check for existing sunsetr process first
    match crate::utils::get_running_sunsetr_pid() {
        Ok(pid) => {
            // Existing process - just signal reload (it will handle gamma correctly)
            Log::log_block_start("Signaling existing sunsetr to reload...");
            
            use nix::sys::signal::{kill, Signal};
            use nix::unistd::Pid;
            
            match kill(Pid::from_raw(pid as i32), Signal::SIGUSR2) {
                Ok(_) => {
                    Log::log_decorated(&format!(
                        "Sent reload signal to sunsetr (PID: {})", pid
                    ));
                    Log::log_indented("Existing process will reload configuration");
                }
                Err(e) => {
                    Log::log_error(&format!(
                        "Failed to signal existing process: {}", e
                    ));
                }
            }
        }
        Err(_) => {
            // No existing process - safe to reset gamma and start new instance
            Log::log_block_start("Resetting display gamma across all protocols...");
            
            let (wayland_result, hyprland_result) = super::reset_all_gamma_parallel(debug_enabled);
            
            // Log successful operations only
            if wayland_result.is_ok() {
                Log::log_decorated("Wayland gamma reset to defaults");
            }
            
            if hyprland_result.is_ok() {
                Log::log_decorated("Hyprland gamma reset to defaults");
            }
            
            Log::log_block_start("Starting new sunsetr instance...");
            crate::utils::spawn_background_process(debug_enabled)?;
            Log::log_decorated("New sunsetr instance started");
        }
    }
    
    Log::log_block_start("Reload complete");
    Log::log_end();
    Ok(())
}