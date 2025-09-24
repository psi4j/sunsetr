//! Preset command implementation for switching configurations
//!
//! This command allows users to switch between different sunsetr configurations by name.
//! Presets are stored in the config directory under presets/{name}/sunsetr.toml
//! and can toggle on/off with simple toggle behavior.

use crate::utils::private_path;
use anyhow::{Context, Result};

/// Result of preset command execution
#[derive(Debug, PartialEq)]
pub enum PresetResult {
    /// Command completed, exit the program
    Exit,
    /// Continue with normal sunsetr execution (no process was running)
    ContinueExecution,
}

/// Handle preset command - toggle or switch to named config
///
/// Returns PresetResult indicating whether to exit or continue execution
pub fn handle_preset_command(preset_name: &str) -> Result<PresetResult> {
    // Always print version header since we're handling a preset command
    log_version!();

    // Check if sunsetr is already running
    // This will restore the config directory from the lock file if present
    let running_pid = crate::utils::get_running_sunsetr_pid().ok();

    // Special handling for "default" preset - always deactivates current preset
    if preset_name.to_lowercase() == "default" {
        return handle_default_preset();
    }

    // Validate preset name
    validate_preset_name(preset_name)?;

    // Get config directory - it will use the restored custom dir if any
    let config_path = crate::config::Config::get_config_path()?;
    let config_dir = config_path
        .parent()
        .context("Failed to get config directory")?;

    // Debug: Log which config directory we're using
    #[cfg(debug_assertions)]
    eprintln!(
        "DEBUG: Using config directory for preset: {}",
        private_path(config_dir)
    );

    // Get the current preset from state
    let current_preset = crate::state::get_active_preset().ok().flatten();

    // Toggle logic ONLY applies when a process is running
    // When no process is running, always apply the preset (for idempotent scheduling)
    if let Some(pid) = running_pid {
        // Process is running
        if current_preset.as_deref() == Some(preset_name) {
            // Toggle OFF - clear state to use default
            if let Err(e) = crate::state::clear_active_preset() {
                log_error_exit!("Failed to clear active preset: {e}");
                std::process::exit(1);
            }
            log_block_start!(
                "Deactivated preset '{}', restored default configuration",
                preset_name
            );

            // Reload the running process with default config
            reload_running_process(pid)?;
        } else {
            // Switch to different preset or activate first preset
            apply_preset(preset_name, config_dir)?;

            // Reload the running process with new preset
            reload_running_process(pid)?;
        }
        log_end!();
        Ok(PresetResult::Exit)
    } else {
        // No process running - apply preset and continue with normal execution
        apply_preset(preset_name, config_dir)?;

        // Return that we should continue with normal execution
        Ok(PresetResult::ContinueExecution)
    }
}

/// Apply a preset by validating it and writing the state
fn apply_preset(preset_name: &str, config_dir: &std::path::Path) -> Result<()> {
    // Verify preset exists
    let preset_config = config_dir
        .join("presets")
        .join(preset_name)
        .join("sunsetr.toml");

    if !preset_config.exists() {
        log_pipe!();
        log_error!("Preset '{}' not found at:", preset_name,);
        log_indented!("{}", private_path(&preset_config));
        log_block_start!("Create a preset directory and config file first:");
        log_indented!("mkdir -p ~/.config/sunsetr/presets/{}", preset_name);
        log_indented!(
            "# Then create ~/.config/sunsetr/presets/{}/sunsetr.toml with your settings",
            preset_name
        );
        log_end!();
        std::process::exit(1);
    }

    // Verify the preset config is valid before activating
    if let Err(e) = crate::config::Config::load_from_path(&preset_config) {
        log_pipe!();
        log_error!("Preset '{}' has invalid configuration:", preset_name);
        log_indented!("{}", e);
        log_end!();
        std::process::exit(1);
    }

    // Write preset name to state
    crate::state::set_active_preset(preset_name)?;

    log_block_start!("Activated preset: {}", preset_name);
    Ok(())
}

/// Handle the special "default" preset which always deactivates any active preset
fn handle_default_preset() -> Result<PresetResult> {
    // Check if sunsetr is already running FIRST
    // This will restore the config directory from the lock file if present
    let running_pid = crate::utils::get_running_sunsetr_pid().ok();

    // Check if there's an active preset
    let current_preset = crate::state::get_active_preset().ok().flatten();

    if let Some(preset_name) = current_preset {
        // Clear the preset state
        if let Err(e) = crate::state::clear_active_preset() {
            log_error_exit!("Failed to remove active preset marker: {e}");
            std::process::exit(1);
        }
        log_block_start!(
            "Deactivated preset '{}', using default configuration",
            preset_name
        );

        // If process is running, reload it
        if let Some(pid) = running_pid {
            reload_running_process(pid)?;
            log_end!();
            Ok(PresetResult::Exit)
        } else {
            // Don't use log_end!() when continuing execution
            Ok(PresetResult::ContinueExecution)
        }
    } else {
        // No active preset to deactivate
        log_block_start!("No active preset to deactivate, already using default configuration");
        if running_pid.is_some() {
            log_end!();
            Ok(PresetResult::Exit)
        } else {
            // Don't use log_end!() when continuing execution
            Ok(PresetResult::ContinueExecution)
        }
    }
}

/// Validate preset name to ensure it's safe to use as a directory name
fn validate_preset_name(name: &str) -> Result<()> {
    // Reserved names that could conflict with system operations
    // Note: "default" is handled specially and doesn't need validation
    const RESERVED: &[&str] = &["none", "off", "auto", "config", "backup"];
    if RESERVED.contains(&name.to_lowercase().as_str()) {
        log_error_exit!("'{}' is a reserved preset name", name);
        std::process::exit(1);
    }

    // Check for empty or whitespace-only names
    if name.trim().is_empty() {
        log_error_exit!("Preset name cannot be empty");
        std::process::exit(1);
    }

    // Invalid characters for directory names
    if name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
        log_error_exit!(
            "Invalid preset name '{}' - contains forbidden characters",
            name
        );
        std::process::exit(1);
    }

    // Path traversal prevention
    if name.starts_with('.') || name.contains("..") {
        log_error_exit!("Preset name cannot start with '.' or contain '..'");
        std::process::exit(1);
    }

    // Reasonable length limit
    if name.len() > 50 {
        log_error_exit!("Preset name is too long (max 50 characters)");
        std::process::exit(1);
    }

    Ok(())
}

/// Send SIGUSR2 to running sunsetr process to trigger config reload
fn reload_running_process(pid: u32) -> Result<()> {
    log_block_start!("Signaling configuration reload...");

    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    kill(Pid::from_raw(pid as i32), Signal::SIGUSR2)
        .context("Failed to send reload signal to sunsetr process")?;
    log_decorated!("Configuration reloaded");

    Ok(())
}

/// Display usage help for the preset command (--help flag)
pub fn show_usage() {
    log_version!();
    log_block_start!("Usage: sunsetr preset <name>");
    log_block_start!("Arguments:");
    log_indented!("<name>  Name of the preset to apply");
    log_indented!("        Use same name or 'default' to return to base configuration");
    log_pipe!();
    log_info!("For detailed help with examples, try: sunsetr help preset");
    log_end!();
}

/// Display detailed help for the preset command (help subcommand)
pub fn display_help() {
    log_version!();
    log_block_start!("preset - Apply a named preset configuration");
    log_block_start!("Usage: sunsetr preset <name>");
    log_block_start!("Arguments:");
    log_indented!("<name>  Name of the preset to apply");
    log_indented!("        Use same name or 'default' to return to base configuration");
    log_block_start!("Description:");
    log_indented!("Presets allow you to switch between different configurations");
    log_indented!("quickly without modifying the base configuration. Each preset");
    log_indented!("is stored as a separate TOML file in the config directory.");
    log_block_start!("Preset Files:");
    log_indented!("Presets are stored in: ~/.config/sunsetr/presets/<name>/sunsetr.toml");
    log_indented!("Each preset can override any configuration field");
    log_indented!("Fields not specified in a preset use the default values");
    log_block_start!("Examples:");
    log_indented!("# Apply a gaming preset");
    log_indented!("sunsetr preset gaming");
    log_pipe!();
    log_indented!("# Apply a night-time preset");
    log_indented!("sunsetr preset night");
    log_pipe!();
    log_indented!("# Return to default configuration");
    log_indented!("sunsetr preset default");
    log_pipe!();
    log_indented!("# Create a new preset by copying and editing");
    log_indented!(
        "cp ~/.config/sunsetr/sunsetr.toml ~/.config/sunsetr/presets/mypreset/sunsetr.toml"
    );
    log_indented!("# Then edit the new sunsetr.toml and apply with: sunsetr preset mypreset");
    log_end!();
}
