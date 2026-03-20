//! Configuration loading functionality.
//!
//! Handles loading configuration files from various paths, applying defaults,
//! and managing geo.toml overrides.

use anyhow::{Context, Result};
use chrono::NaiveTime;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::validation::validate_config;
use super::{Config, GeoConfig};
use crate::common::constants::*;
use crate::common::utils::private_path;

static CONFIG_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Set the configuration directory for the current process.
/// This can only be called once, typically at startup.
/// Returns an error if already set.
pub fn set_config_dir(dir: Option<String>) -> Result<()> {
    #[cfg(debug_assertions)]
    eprintln!("DEBUG: set_config_dir() called with: {:?}", dir);

    CONFIG_DIR
        .set(dir.map(PathBuf::from))
        .map_err(|_| anyhow::anyhow!("Configuration directory already set"))
}

/// Get the custom configuration directory if one was set.
/// Returns None if using the default directory.
pub fn get_custom_config_dir() -> Option<PathBuf> {
    CONFIG_DIR.get().and_then(|d| d.clone())
}

/// Get the base configuration directory.
/// This returns the directory containing sunsetr.toml, geo.toml, presets/, etc.
pub fn get_config_base_dir() -> Result<PathBuf> {
    let config_path = get_config_path()?;
    config_path
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))
}

fn validate_geo_mode_coordinates(config: &Config) -> Result<()> {
    if config.transition_mode.as_deref() == Some("geo")
        && (config.latitude.is_none() || config.longitude.is_none())
    {
        log_pipe!();
        log_critical!("Geo mode requires coordinates but none are configured");
        log_indented!("Please run 'sunsetr geo' to select your location");
        log_indented!("Or add latitude and longitude to your configuration");
        log_end!();
        std::process::exit(crate::common::constants::EXIT_FAILURE);
    }
    Ok(())
}

/// Load configuration using automatic path detection.
///
/// This function will create a default configuration file if none exists.
/// If a preset is active, it will load from the preset directory instead.
pub fn load() -> Result<Config> {
    let config_path = get_config_path()?;

    #[cfg(debug_assertions)]
    eprintln!(
        "DEBUG: Config::load() - config_path: {}",
        private_path(&config_path)
    );

    if let Some(preset_name) = crate::state::preset::get_active_preset()? {
        #[cfg(debug_assertions)]
        eprintln!(
            "DEBUG: Config::load() - Found active preset: {}",
            preset_name
        );
        let preset_config = config_path
            .parent()
            .context("Failed to get config directory")?
            .join("presets")
            .join(&preset_name)
            .join("sunsetr.toml");

        if preset_config.exists() {
            #[cfg(debug_assertions)]
            eprintln!(
                "DEBUG: Config::load() - Loading preset config from: {}",
                private_path(&preset_config)
            );
            return load_from_path(&preset_config);
        } else {
            log_warning!(
                "Active preset '{}' not found, falling back to default config",
                preset_name
            );
            crate::state::preset::clear_active_preset()?;
        }
    }

    if !config_path.exists() {
        super::builder::create_default_config(&config_path, None)
            .context("Failed to create default config during load")?;
    }

    let config = load_from_path(&config_path).with_context(|| {
        format!(
            "Failed to load configuration from {}",
            private_path(&config_path)
        )
    })?;

    validate_geo_mode_coordinates(&config)?;
    Ok(config)
}

/// Load configuration from a specific path.
///
/// This version does not create a default config if the path doesn't exist.
pub fn load_from_path(path: &PathBuf) -> Result<Config> {
    if !path.exists() {
        log_pipe!();
        log_error_exit!("Configuration file not found at specified path:",);
        log_indented!("{}", private_path(path));
        log_end!();
        std::process::exit(1);
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config from {}", private_path(path)))?;

    let mut config: Config = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config from {}", private_path(path)))?;

    config.migrate_legacy_fields();
    load_geo_override_from_path(&mut config, path)?;
    validate_config(&config)?;
    apply_defaults_and_modifications(&mut config)?;
    validate_geo_mode_coordinates(&config)?;
    Ok(config)
}

/// Get the configuration file path with backward compatibility support.
pub fn get_config_path() -> Result<PathBuf> {
    if let Some(custom_dir) = CONFIG_DIR.get().and_then(|d| d.clone()) {
        #[cfg(debug_assertions)]
        eprintln!(
            "DEBUG: get_config_path() - using custom dir: {}",
            private_path(&custom_dir)
        );
        return Ok(custom_dir.join("sunsetr.toml"));
    }

    #[cfg(debug_assertions)]
    eprintln!("DEBUG: get_config_path() - no custom dir set, using default");

    if cfg!(test) {
        let config_dir =
            dirs::config_dir().context("Could not determine config directory for unit tests")?;
        Ok(config_dir.join("sunsetr").join("sunsetr.toml"))
    } else {
        let config_dir = dirs::config_dir().context("Could not determine config directory")?;
        let new_config_path = config_dir.join("sunsetr").join("sunsetr.toml");
        let old_config_path = config_dir.join("hypr").join("sunsetr.toml");
        let new_exists = new_config_path.exists();
        let old_exists = old_config_path.exists();

        match (new_exists, old_exists) {
            (true, true) => {
                #[cfg(feature = "testing-support")]
                {
                    log_pipe!();
                    anyhow::bail!(
                        "TEST_MODE_CONFLICT: Found configuration files in both new ({}) and old ({}) locations while testing-support feature is active.",
                        private_path(&new_config_path),
                        private_path(&old_config_path)
                    )
                }
                #[cfg(not(feature = "testing-support"))]
                {
                    choose_config_file(new_config_path, old_config_path)
                }
            }
            (true, false) => Ok(new_config_path),
            (false, true) => Ok(old_config_path),
            (false, false) => Ok(new_config_path),
        }
    }
}

/// Interactive terminal interface for choosing which config file to keep
#[cfg(not(feature = "testing-support"))]
fn choose_config_file(new_path: PathBuf, old_path: PathBuf) -> Result<PathBuf> {
    log_pipe!();
    log_warning!("Configuration conflict detected");
    log_block_start!("Please select which config to keep:");

    let options = vec![
        (
            format!("{} (new location)", private_path(&new_path)),
            new_path.clone(),
        ),
        (
            format!("{} (legacy location)", private_path(&old_path)),
            old_path.clone(),
        ),
    ];

    let result = crate::common::utils::show_dropdown_menu(&options, None)?;
    let (chosen_path, to_remove) = match result {
        crate::common::utils::DropdownResult::Cancelled => {
            log_pipe!();
            log_warning!("Operation cancelled. Please manually remove one of the config files.");
            log_end!();
            std::process::exit(EXIT_FAILURE);
        }
        crate::common::utils::DropdownResult::Selected(selected_index) => {
            if selected_index == 0 {
                (new_path, old_path)
            } else {
                (old_path, new_path)
            }
        }
    };

    log_block_start!("You chose: {}", private_path(&chosen_path));
    log_decorated!("Will remove: {}", private_path(&to_remove));

    let confirm_options = vec![
        ("Yes, remove the file".to_string(), true),
        ("No, cancel operation".to_string(), false),
    ];

    let result = crate::common::utils::show_dropdown_menu(&confirm_options, None)?;

    match result {
        crate::common::utils::DropdownResult::Cancelled => {
            log_pipe!();
            log_warning!("Operation cancelled. Please manually remove one of the config files.");
            log_end!();
            std::process::exit(EXIT_FAILURE);
        }
        crate::common::utils::DropdownResult::Selected(confirm_index) => {
            let should_remove = confirm_options[confirm_index].1;
            if !should_remove {
                log_pipe!();
                log_warning!(
                    "Operation cancelled. Please manually remove one of the config files."
                );
                log_end!();
                std::process::exit(EXIT_FAILURE);
            }
        }
    }

    let removed_successfully = if try_trash_file(&to_remove) {
        log_block_start!("Successfully moved to trash: {}", private_path(&to_remove));
        true
    } else if let Err(e) = fs::remove_file(&to_remove) {
        log_pipe!();
        log_warning!("Failed to remove {}: {e}", private_path(&to_remove));
        log_decorated!("Please remove it manually to avoid future conflicts.");
        false
    } else {
        log_block_start!("Successfully removed: {}", private_path(&to_remove));
        true
    };

    if removed_successfully {
        log_block_start!("Using configuration: {}", private_path(&chosen_path));
    }

    Ok(chosen_path)
}

/// Attempt to move file to trash using trash-cli
#[cfg(not(feature = "testing-support"))]
fn try_trash_file(path: &PathBuf) -> bool {
    if let Ok(status) = std::process::Command::new("trash-put").arg(path).status() {
        return status.success();
    }

    if let Ok(status) = std::process::Command::new("trash").arg(path).status() {
        return status.success();
    }

    if let Ok(status) = std::process::Command::new("gio")
        .args(["trash", path.to_str().unwrap_or("")])
        .status()
    {
        return status.success();
    }

    false
}

fn apply_defaults(config: &mut Config) {
    if config.backend.is_none() {
        config.backend = Some(DEFAULT_BACKEND);
    }

    let mode = config
        .transition_mode
        .as_deref()
        .unwrap_or(DEFAULT_TRANSITION_MODE);

    if mode != "static" {
        if config.sunset.is_none() {
            config.sunset = Some(DEFAULT_SUNSET.to_string());
        }
        if config.sunrise.is_none() {
            config.sunrise = Some(DEFAULT_SUNRISE.to_string());
        }
    }

    if config.night_temp.is_none() {
        config.night_temp = Some(DEFAULT_NIGHT_TEMP);
    }
    if config.day_temp.is_none() {
        config.day_temp = Some(DEFAULT_DAY_TEMP);
    }

    if config.night_gamma.is_none() {
        config.night_gamma = Some(DEFAULT_NIGHT_GAMMA);
    }
    if config.day_gamma.is_none() {
        config.day_gamma = Some(DEFAULT_DAY_GAMMA);
    }

    if config.transition_duration.is_none() {
        config.transition_duration = Some(DEFAULT_TRANSITION_DURATION);
    }
    if config.update_interval.is_none() {
        config.update_interval = Some(crate::config::UpdateInterval::Adaptive);
    }
    if config.transition_mode.is_none() {
        config.transition_mode = Some(DEFAULT_TRANSITION_MODE.to_string());
    }

    if config.smoothing.is_none() {
        config.smoothing = Some(DEFAULT_SMOOTHING);
    }
    if config.startup_duration.is_none() {
        config.startup_duration = Some(DEFAULT_STARTUP_DURATION);
    }
    if config.shutdown_duration.is_none() {
        config.shutdown_duration = Some(DEFAULT_SHUTDOWN_DURATION);
    }
}

fn apply_modifications(config: &mut Config) -> Result<()> {
    let mode = config
        .transition_mode
        .as_deref()
        .unwrap_or(DEFAULT_TRANSITION_MODE);

    if mode != "static" {
        if let Some(ref sunset) = config.sunset {
            NaiveTime::parse_from_str(sunset, "%H:%M:%S")
                .context("Invalid sunset time format in config. Use HH:MM:SS format")?;
        }
        if let Some(ref sunrise) = config.sunrise {
            NaiveTime::parse_from_str(sunrise, "%H:%M:%S")
                .context("Invalid sunrise time format in config. Use HH:MM:SS format")?;
        }
    }

    if let Some(lat) = config.latitude
        && lat.abs() > 65.0
    {
        log_pipe!();
        log_warning!(
            "⚠️ Latitude capped at 65°{} (config {:.4}°{})",
            if lat >= 0.0 { "N" } else { "S" },
            lat.abs(),
            if lat >= 0.0 { "N" } else { "S" }
        );
        log_indented!("Are you researching extremophile bacteria under the ice caps?");
        log_indented!("Consider using manual sunset/sunrise times for more sensible transitions.");
        config.latitude = Some(65.0 * lat.signum());
    }

    Ok(())
}

/// Apply default values and field modifications to the configuration.
/// All validation is handled by validation::validate_config.
pub(crate) fn apply_defaults_and_modifications(config: &mut Config) -> Result<()> {
    apply_defaults(config);
    apply_modifications(config)?;
    Ok(())
}

/// Load geo.toml from a specific config path
pub(crate) fn load_geo_override_from_path(config: &mut Config, config_path: &Path) -> Result<()> {
    let geo_path = if let Some(parent) = config_path.parent() {
        parent.join("geo.toml")
    } else {
        return Ok(());
    };

    if !geo_path.exists() {
        return Ok(());
    }

    let content_result = fs::read_to_string(&geo_path);

    match content_result {
        Ok(content) => match toml::from_str::<GeoConfig>(&content) {
            Ok(geo_config) => {
                if let Some(lat) = geo_config.latitude {
                    config.latitude = Some(lat);
                }
                if let Some(lon) = geo_config.longitude {
                    config.longitude = Some(lon);
                }
            }
            Err(e) => {
                log_warning!("Failed to parse geo.toml: {e}. Using coordinates from main config.");
            }
        },
        Err(e) => {
            log_warning!("Failed to read geo.toml: {e}. Using coordinates from main config.");
        }
    }

    Ok(())
}
