//! Preset command implementation for switching configurations
//!
//! This command allows users to switch between different sunsetr configurations by name.
//! Presets are stored in the config directory under presets/{name}/sunsetr.toml
//! and can toggle on/off with simple toggle behavior.

use anyhow::{Context, Result};
use std::fs;

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
        config_dir.display()
    );

    let preset_marker = config_dir.join(".active_preset");

    // Check current preset
    let current_preset = if preset_marker.exists() {
        fs::read_to_string(&preset_marker)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    };

    // Toggle logic ONLY applies when a process is running
    // When no process is running, always apply the preset (for idempotent scheduling)
    if let Some(pid) = running_pid {
        // Process is running
        if current_preset.as_deref() == Some(preset_name) {
            // Toggle OFF - remove marker to use default
            if let Err(e) = fs::remove_file(&preset_marker) {
                log_pipe!();
                log_error!("Failed to remove active preset marker: {e}");
                anyhow::bail!("Could not deactivate preset");
            }
            log_block_start!(
                "Deactivated preset '{}', restored default configuration",
                preset_name
            );

            // Reload the running process with default config
            reload_running_process(pid)?;
        } else {
            // Switch to different preset or activate first preset
            apply_preset(&preset_marker, preset_name, config_dir)?;

            // Reload the running process with new preset
            reload_running_process(pid)?;
        }
        log_end!();
        Ok(PresetResult::Exit)
    } else {
        // No process running - apply preset and continue with normal execution
        apply_preset(&preset_marker, preset_name, config_dir)?;

        // Return that we should continue with normal execution
        Ok(PresetResult::ContinueExecution)
    }
}

/// Apply a preset by validating it and writing the marker file
fn apply_preset(
    preset_marker: &std::path::Path,
    preset_name: &str,
    config_dir: &std::path::Path,
) -> Result<()> {
    // Verify preset exists
    let preset_config = config_dir
        .join("presets")
        .join(preset_name)
        .join("sunsetr.toml");

    if !preset_config.exists() {
        log_pipe!();
        log_error!(
            "Preset '{}' not found at {}",
            preset_name,
            preset_config.display()
        );
        log_indented!("Create a preset directory and config file first:");
        log_indented!("mkdir -p ~/.config/sunsetr/presets/{}", preset_name);
        log_indented!(
            "# Then create ~/.config/sunsetr/presets/{}/sunsetr.toml with your settings",
            preset_name
        );
        anyhow::bail!("Preset not found");
    }

    // Verify the preset config is valid before activating
    if let Err(e) = crate::config::Config::load_from_path(&preset_config) {
        log_pipe!();
        log_error!("Preset '{}' has invalid configuration: {}", preset_name, e);
        anyhow::bail!("Cannot activate preset with invalid configuration");
    }

    // Write preset name to marker file
    fs::write(preset_marker, preset_name).with_context(|| {
        format!(
            "Failed to write preset marker to {}",
            preset_marker.display()
        )
    })?;

    log_block_start!("Activated preset: {}", preset_name);
    Ok(())
}

/// Handle the special "default" preset which always deactivates any active preset
fn handle_default_preset() -> Result<PresetResult> {
    // Check if sunsetr is already running FIRST
    // This will restore the config directory from the lock file if present
    let running_pid = crate::utils::get_running_sunsetr_pid().ok();

    // NOW get config directory - it will use the restored custom dir if any
    let config_path = crate::config::Config::get_config_path()?;
    let config_dir = config_path
        .parent()
        .context("Failed to get config directory")?;
    let preset_marker = config_dir.join(".active_preset");

    // Check if there's an active preset
    let current_preset = if preset_marker.exists() {
        fs::read_to_string(&preset_marker)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    };

    if let Some(preset_name) = current_preset {
        // Remove the preset marker
        if let Err(e) = fs::remove_file(&preset_marker) {
            log_pipe!();
            log_error!("Failed to remove active preset marker: {e}");
            anyhow::bail!("Could not deactivate preset");
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
        anyhow::bail!("'{}' is a reserved preset name", name);
    }

    // Check for empty or whitespace-only names
    if name.trim().is_empty() {
        anyhow::bail!("Preset name cannot be empty");
    }

    // Invalid characters for directory names
    if name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
        anyhow::bail!(
            "Invalid preset name '{}' - contains forbidden characters",
            name
        );
    }

    // Path traversal prevention
    if name.starts_with('.') || name.contains("..") {
        anyhow::bail!("Preset name cannot start with '.' or contain '..'");
    }

    // Reasonable length limit
    if name.len() > 50 {
        anyhow::bail!("Preset name is too long (max 50 characters)");
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
