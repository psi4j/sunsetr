//! Preset command implementation for switching configurations
//!
//! This command allows users to switch between different sunsetr configurations by name.
//! Presets are stored in the config directory under presets/{name}/sunsetr.toml
//! and can toggle on/off with simple toggle behavior.

use crate::args::PresetSubcommand;
use crate::common::utils::private_path;
use anyhow::{Context, Result};
use std::fs;

/// Result of preset command execution
#[derive(Debug, PartialEq)]
pub enum PresetResult {
    /// Command completed, exit the program
    Exit,
    /// Continue with normal sunsetr execution (no process was running)
    ContinueExecution,
    /// Test mode is active, cannot proceed
    TestModeActive,
}

/// Handle preset command with subcommands
///
/// Returns PresetResult indicating whether to exit or continue execution
pub fn handle_preset_command(subcommand: &PresetSubcommand) -> Result<PresetResult> {
    match subcommand {
        PresetSubcommand::Apply { name } => handle_preset_apply(name),
        PresetSubcommand::Active => handle_preset_active(),
        PresetSubcommand::List => handle_preset_list(),
    }
}

/// Handle the original preset apply functionality
fn handle_preset_apply(preset_name: &str) -> Result<PresetResult> {
    // Always print version header since we're handling a preset command
    log_version!();

    // Check if test mode is active
    if crate::io::instance::is_test_mode_active() {
        log_pipe!();
        log_warning!("Cannot switch presets while test mode is active");
        log_indented!("Exit test mode first (press Escape in the test terminal)");
        log_end!();
        return Ok(PresetResult::TestModeActive);
    }

    // Check if sunsetr is already running
    // This will restore the config directory from the lock file if present
    let running_pid = crate::io::instance::get_running_instance_pid().ok();

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
    let current_preset = crate::state::preset::get_active_preset().ok().flatten();

    // Toggle logic ONLY applies when a process is running
    // When no process is running, always apply the preset (for idempotent scheduling)
    if let Some(pid) = running_pid {
        // Process is running
        if current_preset.as_deref() == Some(preset_name) {
            // Toggle OFF - clear state to use default
            if let Err(e) = crate::state::preset::clear_active_preset() {
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
    crate::state::preset::set_active_preset(preset_name)?;

    log_block_start!("Active preset: {}", preset_name);
    Ok(())
}

/// Handle the special "default" preset which always deactivates any active preset
fn handle_default_preset() -> Result<PresetResult> {
    // Check if sunsetr is already running FIRST
    // This will restore the config directory from the lock file if present
    let running_pid = crate::io::instance::get_running_instance_pid().ok();

    // Check if there's an active preset
    let current_preset = crate::state::preset::get_active_preset().ok().flatten();

    if let Some(preset_name) = current_preset {
        // Clear the preset state
        if let Err(e) = crate::state::preset::clear_active_preset() {
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

    crate::io::instance::send_reload_signal(pid)
        .context("Failed to send reload signal to sunsetr process")?;
    log_decorated!("Configuration reloaded");

    Ok(())
}

/// Handle preset active subcommand - show the currently active preset
fn handle_preset_active() -> Result<PresetResult> {
    // Get the active preset from state
    let active_preset = crate::state::preset::get_active_preset().ok().flatten();

    if let Some(preset_name) = active_preset {
        println!("{}", preset_name);
    } else {
        println!("default");
    }

    Ok(PresetResult::Exit)
}

/// Handle preset list subcommand - list all available presets
fn handle_preset_list() -> Result<PresetResult> {
    // Get config directory - it will use the restored custom dir if any
    let config_path = crate::config::Config::get_config_path()?;
    let config_dir = config_path
        .parent()
        .context("Failed to get config directory")?;

    let presets_dir = config_dir.join("presets");
    let mut available_presets = Vec::new();

    if presets_dir.exists()
        && let Ok(entries) = fs::read_dir(&presets_dir)
    {
        for entry in entries.flatten() {
            if entry.path().is_dir()
                && let Some(name) = entry.file_name().to_str()
            {
                // Check if it has a sunsetr.toml file
                if entry.path().join("sunsetr.toml").exists() {
                    available_presets.push(name.to_string());
                }
            }
        }
    }

    available_presets.sort();

    // Simply output the list, one per line
    for preset in available_presets {
        println!("{}", preset);
    }

    Ok(PresetResult::Exit)
}

/// Display usage help for the preset command (--help flag)
pub fn show_usage() {
    log_version!();
    log_block_start!("Usage: sunsetr preset <subcommand|name>");
    log_block_start!("Subcommands:");
    log_indented!("active       Show the currently active preset");
    log_indented!("list         List all available presets");
    log_indented!("<name>       Apply the named preset");
    log_indented!("default      Return to base configuration");
    log_pipe!();
    log_info!("For detailed help with examples, try: sunsetr help preset");
    log_end!();
}

/// Display error message with active preset and available presets
pub fn show_usage_with_context(error_message: &str) {
    log_version!();
    log_pipe!();
    log_error!("{}", error_message);

    // Show active preset
    let active_preset = crate::state::preset::get_active_preset().ok().flatten();
    if let Some(preset_name) = active_preset {
        log_block_start!("Active preset: {}", preset_name);
    } else {
        log_block_start!("Active preset: default");
    }

    // Show available presets
    if let Ok(config_path) = crate::config::Config::get_config_path()
        && let Some(config_dir) = config_path.parent()
    {
        let presets_dir = config_dir.join("presets");
        let mut available_presets = Vec::new();

        if presets_dir.exists()
            && let Ok(entries) = fs::read_dir(&presets_dir)
        {
            for entry in entries.flatten() {
                if entry.path().is_dir()
                    && let Some(name) = entry.file_name().to_str()
                {
                    // Check if it has a sunsetr.toml file
                    if entry.path().join("sunsetr.toml").exists() {
                        available_presets.push(name.to_string());
                    }
                }
            }
        }

        available_presets.sort();

        if !available_presets.is_empty() {
            log_decorated!("Available presets: {}", available_presets.join(", "));
        }
    }

    log_block_start!("Usage: sunsetr preset <subcommand|name>");
    log_pipe!();
    log_info!("For more information, try '--help'.");
    log_end!();
}

/// Display detailed help for the preset command (help subcommand)
pub fn display_help() {
    log_version!();
    log_block_start!("preset - Manage and apply preset configurations");
    log_block_start!("Usage: sunsetr preset <subcommand|name>");
    log_block_start!("Subcommands:");
    log_indented!("active       Show the currently active preset");
    log_indented!("list         List all available presets");
    log_indented!("<name>       Apply the named preset");
    log_indented!("default      Return to base configuration");
    log_block_start!("Description:");
    log_indented!("Presets allow you to switch between different configurations");
    log_indented!("quickly without modifying the base configuration. Each preset");
    log_indented!("is stored as a separate TOML file in the config directory.");
    log_block_start!("Preset Files:");
    log_indented!("Presets are stored in: ~/.config/sunsetr/presets/<name>/sunsetr.toml");
    log_indented!("Each preset can override any configuration field");
    log_indented!("Fields not specified in a preset use the default values");
    log_block_start!("Examples:");
    log_indented!("# Show the currently active preset");
    log_indented!("sunsetr preset active");
    log_pipe!();
    log_indented!("# List all available presets");
    log_indented!("sunsetr preset list");
    log_pipe!();
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
