//! Switch between named presets stored under presets/<name>/sunsetr.toml.

use crate::args::PresetSubcommand;
use anyhow::{Context, Result};

#[derive(Debug, PartialEq)]
pub enum PresetResult {
    Exit,
    ContinueExecution,
    TestModeActive,
}

pub fn handle_preset_command(subcommand: &PresetSubcommand) -> Result<PresetResult> {
    match subcommand {
        PresetSubcommand::Apply { name } => handle_preset_apply(name),
        PresetSubcommand::Active => handle_preset_active(),
        PresetSubcommand::List => handle_preset_list(),
    }
}

/// Apply a preset by name. When a process is running and the preset is already active, toggle it
/// off and restore the default configuration. With no process running the preset is always applied
/// so scheduled invocations stay idempotent.
fn handle_preset_apply(preset_name: &str) -> Result<PresetResult> {
    log_version!();

    if crate::io::instance::is_test_mode_active() {
        log_error_end!(
            "Cannot switch presets while test mode is active\n   Exit test mode first (press Escape in the test terminal)"
        );
        return Ok(PresetResult::TestModeActive);
    }

    let running_pid = crate::io::instance::get_running_instance_pid().ok();

    if preset_name.to_lowercase() == "default" {
        return handle_default_preset();
    }

    validate_preset_name(preset_name)?;

    let config_path = crate::config::Config::get_config_path()?;
    let config_dir = config_path
        .parent()
        .context("Failed to get config directory")?;

    let current_preset = crate::state::preset::get_active_preset().ok().flatten();

    if let Some(pid) = running_pid {
        if current_preset.as_deref() == Some(preset_name) {
            if let Err(e) = crate::state::preset::clear_active_preset() {
                log_error_end!("Failed to clear active preset: {e}");
                std::process::exit(1);
            }
            log_block_start!(
                "Deactivated preset '{}', restored default configuration",
                preset_name
            );

            reload_running_process(pid)?;
        } else {
            apply_preset(preset_name, config_dir)?;
            reload_running_process(pid)?;
        }
        log_end!();
        Ok(PresetResult::Exit)
    } else {
        apply_preset(preset_name, config_dir)?;
        Ok(PresetResult::ContinueExecution)
    }
}

fn apply_preset(preset_name: &str, config_dir: &std::path::Path) -> Result<()> {
    let preset_config = config_dir
        .join("presets")
        .join(preset_name)
        .join("sunsetr.toml");

    if !preset_config.exists() {
        let available_presets = super::list_available_presets(config_dir)?;

        let error = super::PresetNotFoundError {
            preset_name: preset_name.to_string(),
            available_presets,
            expected_path: preset_config,
        };

        super::handle_preset_not_found_error(&error);
    }

    if let Err(e) = crate::config::Config::load_from_path(&preset_config) {
        log_pipe!();
        log_error!("Preset '{}' has invalid configuration:", preset_name);
        log_indented!("{}", e);
        log_end!();
        std::process::exit(1);
    }

    crate::state::preset::set_active_preset(preset_name)?;

    log_block_start!("Active preset: {}", preset_name);
    Ok(())
}

/// Deactivate any active preset, restoring the base configuration.
fn handle_default_preset() -> Result<PresetResult> {
    let running_pid = crate::io::instance::get_running_instance_pid().ok();

    let current_preset = crate::state::preset::get_active_preset().ok().flatten();

    if let Some(preset_name) = current_preset {
        if let Err(e) = crate::state::preset::clear_active_preset() {
            log_error_end!("Failed to remove active preset marker: {e}");
            std::process::exit(1);
        }
        log_block_start!(
            "Deactivated preset '{}', using default configuration",
            preset_name
        );

        if let Some(pid) = running_pid {
            reload_running_process(pid)?;
            log_end!();
            Ok(PresetResult::Exit)
        } else {
            Ok(PresetResult::ContinueExecution)
        }
    } else {
        log_block_start!("No active preset to deactivate, already using default configuration");
        if running_pid.is_some() {
            log_end!();
            Ok(PresetResult::Exit)
        } else {
            Ok(PresetResult::ContinueExecution)
        }
    }
}

/// Validate preset name to ensure it's safe to use as a directory name
pub(crate) fn validate_preset_name(name: &str) -> Result<()> {
    // Reserved names that could collide with system operations. "default"
    // is handled specially upstream and never reaches this check.
    const RESERVED: &[&str] = &["none", "off", "auto", "config", "backup"];
    if RESERVED.contains(&name.to_lowercase().as_str()) {
        log_error_end!("'{}' is a reserved preset name", name);
        std::process::exit(1);
    }

    if name.trim().is_empty() {
        log_error_end!("Preset name cannot be empty");
        std::process::exit(1);
    }

    if name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
        log_error_end!(
            "Invalid preset name '{}' - contains forbidden characters",
            name
        );
        std::process::exit(1);
    }

    // Path traversal prevention.
    if name.starts_with('.') || name.contains("..") {
        log_error_end!("Preset name cannot start with '.' or contain '..'");
        std::process::exit(1);
    }

    if name.len() > 50 {
        log_error_end!("Preset name is too long (max 50 characters)");
        std::process::exit(1);
    }

    Ok(())
}

fn reload_running_process(pid: u32) -> Result<()> {
    log_block_start!("Signaling configuration reload...");

    crate::io::instance::send_reload_signal(pid)
        .context("Failed to send reload signal to sunsetr process")?;
    log_decorated!("Configuration reloaded");

    Ok(())
}

fn handle_preset_active() -> Result<PresetResult> {
    let active_preset = crate::state::preset::get_active_preset().ok().flatten();

    if let Some(preset_name) = active_preset {
        println!("{}", preset_name);
    } else {
        println!("default");
    }

    Ok(PresetResult::Exit)
}

fn handle_preset_list() -> Result<PresetResult> {
    let config_path = crate::config::Config::get_config_path()?;
    let config_dir = config_path
        .parent()
        .context("Failed to get config directory")?;

    let available_presets = super::list_available_presets(config_dir)?;

    for preset in available_presets {
        println!("{}", preset);
    }

    Ok(PresetResult::Exit)
}

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

pub fn show_usage_with_context(error_message: &str) {
    log_version!();
    log_pipe!();
    log_error!("{}", error_message);

    let active_preset = crate::state::preset::get_active_preset().ok().flatten();
    if let Some(preset_name) = active_preset {
        log_block_start!("Active preset: {}", preset_name);
    } else {
        log_block_start!("Active preset: default");
    }

    if let Ok(config_path) = crate::config::Config::get_config_path()
        && let Some(config_dir) = config_path.parent()
        && let Ok(available_presets) = super::list_available_presets(config_dir)
        && !available_presets.is_empty()
    {
        log_decorated!("Available presets: {}", available_presets.join(", "));
    }

    log_block_start!("Usage: sunsetr preset <subcommand|name>");
    log_pipe!();
    log_info!("For more information, try '--help'.");
    log_end!();
}

pub fn display_help() {
    log_version!();
    log_block_start!("Manage and apply preset configurations");
    log_block_start!("Usage: sunsetr preset <subcommand|name>");
    log_block_start!("Subcommands:");
    log_indented!("active       Show the currently active preset");
    log_indented!("list         List all available presets");
    log_indented!("<name>       Apply the named preset");
    log_indented!("default      Return to base configuration");
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
