//! Configuration loading functionality.
//!
//! Handles loading configuration files from various paths, applying defaults,
//! and managing geo.toml overrides.

use anyhow::{Context, Result};
use chrono::NaiveTime;
use std::fs;
use std::path::{Path, PathBuf};

use super::validation::validate_config;
use super::{Config, GeoConfig};
use crate::constants::*;

/// Validate that geo mode has required coordinates configured.
/// This is called after loading any config (default or preset) to ensure
/// geo mode always has the coordinates it requires.
fn validate_geo_mode_coordinates(config: &Config) -> Result<()> {
    if config.transition_mode.as_deref() == Some("geo")
        && (config.latitude.is_none() || config.longitude.is_none())
    {
        log_pipe!();
        log_critical!("Geo mode requires coordinates but none are configured");
        log_indented!("Please run 'sunsetr --geo' to select your location");
        log_indented!("Or add latitude and longitude to your configuration");
        log_end!();
        std::process::exit(crate::constants::EXIT_FAILURE);
    }
    Ok(())
}

/// Load configuration using automatic path detection.
///
/// This function will create a default configuration file if none exists.
/// If a preset is active, it will load from the preset directory instead.
pub fn load() -> Result<Config> {
    let config_path = get_config_path()?;

    // Check for active preset first
    if let Some(preset_name) = get_active_preset()? {
        // Load from preset directory
        let preset_config = config_path
            .parent()
            .context("Failed to get config directory")?
            .join("presets")
            .join(&preset_name)
            .join("sunsetr.toml");

        if preset_config.exists() {
            // Note: Config is loaded from active preset
            return load_from_path(&preset_config);
        } else {
            log_warning!(
                "Active preset '{}' not found, falling back to default config",
                preset_name
            );
            // Clean up invalid marker
            clear_active_preset()?;
        }
    }

    if !config_path.exists() {
        super::builder::create_default_config(&config_path, None)
            .context("Failed to create default config during load")?;
    }

    // Now that we're sure a file exists (either pre-existing or newly created default),
    // load it using the common path-based loader.
    // Note: load_from_path already calls load_geo_override_from_path, so we don't need to call it again
    let config = load_from_path(&config_path).with_context(|| {
        log_pipe!();
        format!(
            "Failed to load configuration from {}",
            config_path.display()
        )
    })?;

    // Validate geo mode has coordinates (same check needed for presets)
    validate_geo_mode_coordinates(&config)?;

    Ok(config)
}

/// Load configuration from a specific path.
///
/// This version does NOT create a default config if the path doesn't exist.
pub fn load_from_path(path: &PathBuf) -> Result<Config> {
    if !path.exists() {
        anyhow::bail!(
            "Configuration file not found at specified path: {}",
            path.display()
        );
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config from {}", path.display()))?;

    let mut config: Config = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config from {}", path.display()))?;

    // Migrate legacy field names to new ones for backward compatibility
    config.migrate_legacy_fields();

    apply_defaults_and_validate_fields(&mut config)?;

    // Load geo.toml overrides if present - pass the actual config path
    load_geo_override_from_path(&mut config, path)?;

    // Comprehensive configuration validation (this is the existing public function)
    validate_config(&config)?;

    // Validate geo mode has coordinates (needed for presets which use load_from_path)
    validate_geo_mode_coordinates(&config)?;

    Ok(config)
}

/// Get the configuration file path with backward compatibility support.
pub fn get_config_path() -> Result<PathBuf> {
    if cfg!(test) {
        // For library's own unit tests, bypass complex logic
        let config_dir =
            dirs::config_dir().context("Could not determine config directory for unit tests")?;
        Ok(config_dir.join("sunsetr").join("sunsetr.toml"))
    } else {
        // For binary execution or integration tests (when not a unit test)
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
                        new_config_path.display(),
                        old_config_path.display()
                    )
                }
                #[cfg(not(feature = "testing-support"))]
                {
                    choose_config_file(new_config_path, old_config_path)
                }
            }
            (true, false) => Ok(new_config_path),
            (false, true) => Ok(old_config_path),
            (false, false) => Ok(new_config_path), // Default to new path for creation
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
            format!("{} (new location)", new_path.display()),
            new_path.clone(),
        ),
        (
            format!("{} (legacy location)", old_path.display()),
            old_path.clone(),
        ),
    ];

    let selected_index = crate::utils::show_dropdown_menu(
        &options,
        None,
        Some("Operation cancelled. Please manually remove one of the config files."),
    )?;
    let (chosen_path, to_remove) = if selected_index == 0 {
        (new_path, old_path)
    } else {
        (old_path, new_path)
    };

    // Confirm deletion
    log_block_start!("You chose: {}", chosen_path.display());
    log_decorated!("Will remove: {}", to_remove.display());

    let confirm_options = vec![
        ("Yes, remove the file".to_string(), true),
        ("No, cancel operation".to_string(), false),
    ];

    let confirm_index = crate::utils::show_dropdown_menu(
        &confirm_options,
        None,
        Some("Operation cancelled. Please manually remove one of the config files."),
    )?;
    let should_remove = confirm_options[confirm_index].1;

    if !should_remove {
        log_pipe!();
        log_warning!("Operation cancelled. Please manually remove one of the config files.");
        std::process::exit(EXIT_FAILURE);
    }

    // Try to use trash-cli first, fallback to direct removal
    let removed_successfully = if try_trash_file(&to_remove) {
        log_block_start!("Successfully moved to trash: {}", to_remove.display());
        true
    } else if let Err(e) = fs::remove_file(&to_remove) {
        log_pipe!();
        log_warning!("Failed to remove {}: {e}", to_remove.display());
        log_decorated!("Please remove it manually to avoid future conflicts.");
        false
    } else {
        log_block_start!("Successfully removed: {}", to_remove.display());
        true
    };

    if removed_successfully {
        log_block_start!("Using configuration: {}", chosen_path.display());
    }

    Ok(chosen_path)
}

/// Attempt to move file to trash using trash-cli
#[cfg(not(feature = "testing-support"))]
fn try_trash_file(path: &PathBuf) -> bool {
    // Try trash-put command (most common)
    if let Ok(status) = std::process::Command::new("trash-put").arg(path).status() {
        return status.success();
    }

    // Try trash command (alternative)
    if let Ok(status) = std::process::Command::new("trash").arg(path).status() {
        return status.success();
    }

    // Try gio trash (GNOME)
    if let Ok(status) = std::process::Command::new("gio")
        .args(["trash", path.to_str().unwrap_or("")])
        .status()
    {
        return status.success();
    }

    false
}

/// Apply default values and validate individual fields.
pub(crate) fn apply_defaults_and_validate_fields(config: &mut Config) -> Result<()> {
    // Set default for start_hyprsunset if not specified
    if config.start_hyprsunset.is_none() {
        config.start_hyprsunset = Some(DEFAULT_START_HYPRSUNSET);
    }

    // Set default for backend if not specified
    if config.backend.is_none() {
        config.backend = Some(DEFAULT_BACKEND);
    }

    // Validate time formats and set defaults if not in static mode
    let mode = config
        .transition_mode
        .as_deref()
        .unwrap_or(DEFAULT_TRANSITION_MODE);

    // For static mode, sunset/sunrise are optional
    if mode != "static" {
        // For time-based modes, ensure sunset/sunrise are present
        if config.sunset.is_none() {
            config.sunset = Some(DEFAULT_SUNSET.to_string());
        }
        if config.sunrise.is_none() {
            config.sunrise = Some(DEFAULT_SUNRISE.to_string());
        }

        // Validate the time formats
        if let Some(ref sunset) = config.sunset {
            NaiveTime::parse_from_str(sunset, "%H:%M:%S")
                .context("Invalid sunset time format in config. Use HH:MM:SS format")?;
        }
        if let Some(ref sunrise) = config.sunrise {
            NaiveTime::parse_from_str(sunrise, "%H:%M:%S")
                .context("Invalid sunrise time format in config. Use HH:MM:SS format")?;
        }
    }

    // Validate temperature if specified
    if let Some(temp) = config.night_temp {
        if !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp) {
            anyhow::bail!(
                "Night temperature must be between {} and {} Kelvin",
                MINIMUM_TEMP,
                MAXIMUM_TEMP
            );
        }
    } else {
        config.night_temp = Some(DEFAULT_NIGHT_TEMP);
    }

    // Validate day temperature if specified
    if let Some(temp) = config.day_temp {
        if !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp) {
            anyhow::bail!(
                "Day temperature must be between {} and {} Kelvin",
                MINIMUM_TEMP,
                MAXIMUM_TEMP
            );
        }
    } else {
        config.day_temp = Some(DEFAULT_DAY_TEMP);
    }

    // Validate night gamma if specified
    if let Some(gamma) = config.night_gamma {
        if !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma) {
            anyhow::bail!(
                "Night gamma must be between {}% and {}%",
                MINIMUM_GAMMA,
                MAXIMUM_GAMMA
            );
        }
    } else {
        config.night_gamma = Some(DEFAULT_NIGHT_GAMMA);
    }

    // Validate day gamma if specified
    if let Some(gamma) = config.day_gamma {
        if !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma) {
            anyhow::bail!(
                "Day gamma must be between {}% and {}%",
                MINIMUM_GAMMA,
                MAXIMUM_GAMMA
            );
        }
    } else {
        config.day_gamma = Some(DEFAULT_DAY_GAMMA);
    }

    // Set defaults for transition fields
    if config.transition_duration.is_none() {
        config.transition_duration = Some(DEFAULT_TRANSITION_DURATION);
    }

    if config.update_interval.is_none() {
        config.update_interval = Some(DEFAULT_UPDATE_INTERVAL);
    }

    if config.transition_mode.is_none() {
        config.transition_mode = Some(DEFAULT_TRANSITION_MODE.to_string());
    }

    // Set defaults for smoothing fields (preferred new names)
    if config.smoothing.is_none() {
        config.smoothing = Some(DEFAULT_SMOOTHING);
    }

    if config.startup_duration.is_none() {
        config.startup_duration = Some(DEFAULT_STARTUP_DURATION);
    }

    if config.shutdown_duration.is_none() {
        config.shutdown_duration = Some(DEFAULT_SHUTDOWN_DURATION);
    }

    // Don't set defaults for deprecated fields - only use them if present in the config
    // This prevents false positive deprecation warnings during migration

    // Validate transition ranges
    if let Some(duration_minutes) = config.transition_duration
        && !(MINIMUM_TRANSITION_DURATION..=MAXIMUM_TRANSITION_DURATION).contains(&duration_minutes)
    {
        anyhow::bail!(
            "Transition duration must be between {} and {} minutes",
            MINIMUM_TRANSITION_DURATION,
            MAXIMUM_TRANSITION_DURATION
        );
    }

    if let Some(interval) = config.update_interval
        && !(MINIMUM_UPDATE_INTERVAL..=MAXIMUM_UPDATE_INTERVAL).contains(&interval)
    {
        anyhow::bail!(
            "Update interval must be between {} and {} seconds",
            MINIMUM_UPDATE_INTERVAL,
            MAXIMUM_UPDATE_INTERVAL
        );
    }

    // Validate transition mode
    if let Some(ref mode) = config.transition_mode
        && mode != "finish_by"
        && mode != "start_at"
        && mode != "center"
        && mode != "geo"
        && mode != "static"
    {
        anyhow::bail!(
            "Transition mode must be 'finish_by', 'start_at', 'center', 'geo', or 'static'"
        );
    }

    // Validate smooth transition durations (using new field names internally)
    if let Some(duration_seconds) = config.startup_duration
        && !(MINIMUM_SMOOTH_TRANSITION_DURATION..=MAXIMUM_SMOOTH_TRANSITION_DURATION)
            .contains(&duration_seconds)
    {
        anyhow::bail!(
            "Startup duration must be between {} and {} seconds",
            MINIMUM_SMOOTH_TRANSITION_DURATION,
            MAXIMUM_SMOOTH_TRANSITION_DURATION
        );
    }

    if let Some(duration_seconds) = config.shutdown_duration
        && !(MINIMUM_SMOOTH_TRANSITION_DURATION..=MAXIMUM_SMOOTH_TRANSITION_DURATION)
            .contains(&duration_seconds)
    {
        anyhow::bail!(
            "Shutdown duration must be between {} and {} seconds",
            MINIMUM_SMOOTH_TRANSITION_DURATION,
            MAXIMUM_SMOOTH_TRANSITION_DURATION
        );
    }

    // Validate legacy startup transition duration (for backward compatibility)
    if let Some(duration_seconds) = config.startup_transition_duration
        && !(MINIMUM_STARTUP_TRANSITION_DURATION..=MAXIMUM_STARTUP_TRANSITION_DURATION)
            .contains(&duration_seconds)
    {
        anyhow::bail!(
            "Startup transition duration must be between {} and {} seconds",
            MINIMUM_STARTUP_TRANSITION_DURATION,
            MAXIMUM_STARTUP_TRANSITION_DURATION
        );
    }

    // Validate latitude range (-90 to 90)
    if let Some(lat) = config.latitude {
        if !(-90.0..=90.0).contains(&lat) {
            anyhow::bail!("Latitude must be between -90 and 90 degrees (got {})", lat);
        }
        // Cap latitude at ±65° to avoid solar calculation edge cases
        if lat.abs() > 65.0 {
            log_pipe!();
            log_warning!(
                "⚠️ Latitude capped at 65°{} (config {:.4}°{})",
                if lat >= 0.0 { "N" } else { "S" },
                lat.abs(),
                if lat >= 0.0 { "N" } else { "S" }
            );
            log_indented!("Are you researching extremophile bacteria under the ice caps?");
            log_indented!("Consider using manual sunset/sunrise times for better accuracy.");
            config.latitude = Some(65.0 * lat.signum());
        }
    }

    // Validate longitude range (-180 to 180)
    if let Some(lon) = config.longitude
        && !(-180.0..=180.0).contains(&lon)
    {
        anyhow::bail!(
            "Longitude must be between -180 and 180 degrees (got {})",
            lon
        );
    }

    Ok(())
}

/// Load geo.toml from a specific config path
pub(crate) fn load_geo_override_from_path(config: &mut Config, config_path: &Path) -> Result<()> {
    // Derive geo.toml path from the config path
    let geo_path = if let Some(parent) = config_path.parent() {
        parent.join("geo.toml")
    } else {
        return Ok(()); // Can't determine geo path, skip
    };

    if !geo_path.exists() {
        // geo.toml is optional, no error if missing
        return Ok(());
    }

    // Try to read and parse geo.toml
    match fs::read_to_string(&geo_path) {
        Ok(content) => {
            match toml::from_str::<GeoConfig>(&content) {
                Ok(geo_config) => {
                    // Override coordinates if present in geo.toml
                    if let Some(lat) = geo_config.latitude {
                        config.latitude = Some(lat);
                    }
                    if let Some(lon) = geo_config.longitude {
                        config.longitude = Some(lon);
                    }
                }
                Err(e) => {
                    // Malformed geo.toml - log warning and continue
                    log_warning!(
                        "Failed to parse geo.toml: {e}. Using coordinates from main config."
                    );
                }
            }
        }
        Err(e) => {
            // Permission error or other read error - log warning and continue
            log_warning!("Failed to read geo.toml: {e}. Using coordinates from main config.");
        }
    }

    Ok(())
}

/// Get the currently active preset name, if any.
pub fn get_active_preset() -> Result<Option<String>> {
    let config_path = get_config_path()?;
    let marker_path = config_path
        .parent()
        .context("Failed to get config directory")?
        .join(".active_preset");

    if marker_path.exists() {
        match fs::read_to_string(&marker_path) {
            Ok(content) => {
                let preset_name = content.trim().to_string();
                if preset_name.is_empty() {
                    // Empty file, clean it up
                    let _ = fs::remove_file(&marker_path);
                    Ok(None)
                } else {
                    Ok(Some(preset_name))
                }
            }
            Err(_) => {
                // Can't read file, treat as no preset
                Ok(None)
            }
        }
    } else {
        Ok(None)
    }
}

/// Clear the active preset marker file.
pub fn clear_active_preset() -> Result<()> {
    let config_path = get_config_path()?;
    let marker_path = config_path
        .parent()
        .context("Failed to get config directory")?
        .join(".active_preset");

    // Remove the marker file (ignore errors if file doesn't exist)
    let _ = fs::remove_file(&marker_path);
    Ok(())
}
