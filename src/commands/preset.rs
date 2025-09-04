//! Preset command implementation for switching configurations
//!
//! This command allows users to switch between different sunsetr configurations by name.
//! Presets are stored in the config directory under presets/{name}/sunsetr.toml
//! and can toggle on/off with simple toggle behavior.

use anyhow::{Context, Result};
use std::fs;

/// Handle preset command - toggle or switch to named config
pub fn handle_preset_command(preset_name: &str) -> Result<()> {
    log_version!();

    // Validate preset name
    validate_preset_name(preset_name)?;

    // Get config directory from config file path
    let config_path = crate::config::Config::get_config_path()?;
    let config_dir = config_path
        .parent()
        .context("Failed to get config directory")?;
    let preset_marker = config_dir.join(".active_preset");

    // Check if sunsetr is already running
    let running_pid = crate::utils::get_running_sunsetr_pid().ok();

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
    } else {
        // No process running - always apply preset and start
        apply_preset(&preset_marker, preset_name, config_dir)?;

        // Start new process with preset active
        start_sunsetr_process()?;
    }

    log_end!();
    Ok(())
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

/// Validate preset name to ensure it's safe to use as a directory name
fn validate_preset_name(name: &str) -> Result<()> {
    // Reserved names that could conflict with system operations
    const RESERVED: &[&str] = &["default", "none", "off", "auto", "config", "backup"];
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

/// Start a new sunsetr process with the current configuration
fn start_sunsetr_process() -> Result<()> {
    log_block_start!("Starting sunsetr with preset...");

    // Fork a new sunsetr process
    use std::process::Command;

    // Get the current executable path
    let exe_path = std::env::current_exe().context("Failed to get current executable path")?;

    // Start sunsetr in the background (detached)
    Command::new(exe_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("Failed to start sunsetr process")?;

    Ok(())
}
