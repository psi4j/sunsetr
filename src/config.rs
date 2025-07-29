//! Configuration system for sunsetr with validation and geo coordinate integration.
//!
//! This module provides comprehensive configuration management for the sunsetr application,
//! handling TOML-based configuration files, validation, default value generation, and
//! integration with geographic location detection.
//!
//! ## Configuration Sources
//!
//! The configuration system searches for `sunsetr.toml` with backward compatibility support:
//! 1. **XDG_CONFIG_HOME**/sunsetr/sunsetr.toml (preferred new location)
//! 2. **XDG_CONFIG_HOME**/hypr/sunsetr.toml (legacy location for backward compatibility)
//! 3. Interactive selection if both exist (prevents conflicts)
//! 4. Defaults to new location when creating configuration
//!
//! This dual-path system ensures smooth migration from the original Hyprland-specific
//! configuration location to the new sunsetr-specific directory.
//!
//! ## Configuration Structure
//!
//! The configuration supports both manual sunset/sunrise times and automatic geographic
//! location-based calculations:
//!
//! ```toml
//! # Backend configuration
//! backend = "auto"                  # "auto", "hyprland", or "wayland"
//! start_hyprsunset = true           # Whether to start hyprsunset daemon
//!
//! # Geolocation-based transitions (automatic transition times and durations)
//! latitude = 40.7128                # Geographic coordinates
//! longitude = -74.0060
//! transition_mode = "geo"           # Use solar calculations
//!
//! # Manual mode (fixed times)
//! sunset = "19:00:00"               # Manual sunset time
//! sunrise = "06:00:00"              # Manual sunrise time
//! transition_duration = 45          # Manual transition duration (minutes)
//! transition_mode = "finish_by"     # How to apply transitions
//!
//! # Color temperature settings
//! night_temp = 3300                 # Kelvin (warm)
//! day_temp = 6500                   # Kelvin (cool)
//! night_gamma = 90.0                # Brightness percentage
//! day_gamma = 100.0                 # Brightness percentage
//!
//! # Transition behavior
//! update_interval = 60              # Seconds between transtion updates (any mode)
//!
//! # Startup behavior
//! startup_transition = false        # Smooth startup transition
//! startup_transition_duration = 1   # Second(s)
//! ```
//!
//! ## Validation and Error Handling
//!
//! The configuration system performs extensive validation:
//! - **Range validation**: Temperature (1000-20000K), gamma (0-100%), durations (5-120 min)
//! - **Time format validation**: Ensures sunset/sunrise times are parseable
//! - **Geographic validation**: Latitude (-90° to +90°), longitude (-180° to +180°)
//! - **Logical validation**: Prevents impossible configurations
//!
//! Invalid configurations produce helpful error messages with suggestions for fixes.
//!
//! ## Default Configuration Generation
//!
//! When no configuration exists, the system can automatically generate a default
//! configuration with optional geographic coordinates from timezone detection or
//! interactive city selection.

use anyhow::{Context, Result};
use chrono::{NaiveTime, Timelike};
use serde::Deserialize;
use std::fs::{self};
use std::path::{Path, PathBuf};

use crate::constants::*;
use crate::logger::Log;

/// Geographic configuration structure for storing coordinates separately.
///
/// This structure represents the optional geo.toml file that can store
/// latitude and longitude separately from the main configuration file.
/// This allows users to version control their main settings while keeping
/// location data private.
#[derive(Debug, Deserialize, Clone)]
struct GeoConfig {
    /// Geographic latitude in degrees (-90 to +90)
    latitude: Option<f64>,
    /// Geographic longitude in degrees (-180 to +180)
    longitude: Option<f64>,
}

/// Backend selection for color temperature control.
///
/// Determines which backend implementation to use for controlling display
/// color temperature. The backend choice affects how sunsetr communicates
/// with the compositor and what features are available.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// Automatic backend detection based on environment.
    ///
    /// Auto-detection priority: Hyprland → Wayland → error.
    /// This is the recommended setting for most users.
    Auto,
    /// Hyprland compositor backend using hyprsunset daemon.
    ///
    /// Communicates with hyprsunset via Hyprland's IPC socket protocol.
    /// Provides the most stable and feature-complete experience on Hyprland.
    Hyprland,
    /// Generic Wayland backend using wlr-gamma-control-unstable-v1 protocol.
    ///
    /// Works with most wlroots-based compositors (Niri, Sway, river, Wayfire, etc.).
    /// Does not require external helper processes.
    Wayland,
}

impl Backend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Backend::Auto => "auto",
            Backend::Hyprland => "hyprland",
            Backend::Wayland => "wayland",
        }
    }
}

/// Configuration structure for sunsetr application settings.
///
/// This structure represents all configurable options for sunsetr, loaded from
/// the `sunsetr.toml` configuration file. Most fields are optional and will
/// use appropriate defaults when not specified.
///
/// ## Configuration Categories
///
/// - **Backend Control**: `backend`, `start_hyprsunset` (applies to all modes)
/// - **Startup Behavior**: `startup_transition`, `startup_transition_duration` (applies to all modes)
/// - **Color Settings**: `night_temp`, `day_temp`, `night_gamma`, `day_gamma` (applies to all modes)
/// - **Update Frequency**: `update_interval` (applies to all transition modes)
/// - **Geographic Mode Settings**: `latitude`, `longitude` (only used when `transition_mode = "geo"`)
/// - **Manual Mode Settings**: `sunset`, `sunrise`, `transition_duration` (only used for manual modes: "finish_by", "start_at", "center")
/// - **Mode Selection**: `transition_mode` ("geo" vs manual modes: "finish_by", "start_at", "center")
///
/// ## Validation
///
/// All configuration values are validated during loading to ensure they fall
/// within acceptable ranges and don't create impossible configurations (e.g.,
/// overlapping transitions, insufficient time periods).
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    /// Whether sunsetr should start and manage the hyprsunset daemon.
    ///
    /// When `true`, sunsetr will start hyprsunset as a child process.
    /// When `false`, sunsetr expects hyprsunset to be started externally.
    /// Defaults to `true` for Hyprland backend, `false` for Wayland backend.
    pub start_hyprsunset: Option<bool>,

    /// Backend implementation to use for color temperature control.
    ///
    /// Determines how sunsetr communicates with the compositor.
    /// Defaults to `Auto` which detects the appropriate backend automatically.
    pub backend: Option<Backend>,

    /// Whether to enable smooth animated startup transitions.
    ///
    /// When `true`, sunsetr will gradually transition from day values to the
    /// current target state over the startup transition duration.
    /// When `false`, sunsetr applies the correct state immediately.
    pub startup_transition: Option<bool>, // whether to enable smooth startup transition
    pub startup_transition_duration: Option<u64>, // seconds for startup transition
    pub latitude: Option<f64>,                    // Geographic latitude for geo mode
    pub longitude: Option<f64>,                   // Geographic longitude for geo mode
    pub sunset: String,
    pub sunrise: String,
    pub night_temp: Option<u32>,
    pub day_temp: Option<u32>,
    pub night_gamma: Option<f32>,
    pub day_gamma: Option<f32>,
    pub transition_duration: Option<u64>, // minutes
    pub update_interval: Option<u64>,     // seconds during transition
    pub transition_mode: Option<String>,  // "finish_by", "start_at", "center", or "geo"
}

impl Config {
    /// Get the path to the geo.toml file (in the same directory as sunsetr.toml)
    pub fn get_geo_path() -> Result<PathBuf> {
        let config_path = Self::get_config_path()?;
        if let Some(parent) = config_path.parent() {
            Ok(parent.join("geo.toml"))
        } else {
            anyhow::bail!("Could not determine geo.toml path from config path")
        }
    }

    pub fn get_config_path() -> Result<PathBuf> {
        if cfg!(test) {
            // For library's own unit tests, bypass complex logic
            let config_dir = dirs::config_dir()
                .context("Could not determine config directory for unit tests")?;
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
                        Log::log_pipe();
                        anyhow::bail!(
                            "TEST_MODE_CONFLICT: Found configuration files in both new ({}) and old ({}) locations while testing-support feature is active.",
                            new_config_path.display(),
                            old_config_path.display()
                        )
                    }
                    #[cfg(not(feature = "testing-support"))]
                    {
                        Self::choose_config_file(new_config_path, old_config_path)
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
        Log::log_pipe();
        Log::log_warning("Configuration conflict detected");
        Log::log_block_start("Please select which config to keep:");

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
        Log::log_block_start(&format!("You chose: {}", chosen_path.display()));
        Log::log_decorated(&format!("Will remove: {}", to_remove.display()));

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
            Log::log_pipe();
            Log::log_warning(
                "Operation cancelled. Please manually remove one of the config files.",
            );
            std::process::exit(EXIT_FAILURE);
        }

        // Try to use trash-cli first, fallback to direct removal
        let removed_successfully = if Self::try_trash_file(&to_remove) {
            Log::log_block_start(&format!(
                "Successfully moved to trash: {}",
                to_remove.display()
            ));
            true
        } else if let Err(e) = fs::remove_file(&to_remove) {
            Log::log_pipe();
            Log::log_warning(&format!("Failed to remove {}: {}", to_remove.display(), e));
            Log::log_decorated("Please remove it manually to avoid future conflicts.");
            false
        } else {
            Log::log_block_start(&format!("Successfully removed: {}", to_remove.display()));
            true
        };

        if removed_successfully {
            Log::log_block_start(&format!("Using configuration: {}", chosen_path.display()));
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

    /// Create a default config file with optional coordinate override.
    ///
    /// This function creates a new configuration file. If coordinates are provided,
    /// it uses those directly (for geo selection). If no coordinates are provided,
    /// it attempts timezone-based coordinate detection (normal startup behavior).
    ///
    /// # Arguments
    /// * `path` - Path where the config file should be created
    /// * `coords` - Optional tuple of (latitude, longitude, city_name).
    ///   If provided, skips timezone detection and uses these coordinates.
    ///   If None, performs automatic timezone detection.
    ///
    /// # Returns
    /// Result indicating success or failure of config file creation
    pub fn create_default_config(path: &PathBuf, coords: Option<(f64, f64, String)>) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Failed to create config directory")?;
        }

        // Check if geo.toml exists - we'll use it for ANY coordinate source
        let geo_path = Self::get_geo_path().unwrap_or_else(|_| PathBuf::from(""));
        let use_geo_file = !geo_path.as_os_str().is_empty() && geo_path.exists();

        // Determine coordinate entries based on whether coordinates were provided
        let (transition_mode, lat, lon, city_name) = if let Some((mut lat, lon, city_name)) = coords
        {
            // Cap latitude at ±65° before saving
            if lat.abs() > 65.0 {
                lat = 65.0 * lat.signum();
            }
            (DEFAULT_TRANSITION_MODE, lat, lon, Some(city_name))
        } else {
            // Try to auto-detect coordinates via timezone for smart geo mode default
            let (mode, lat, lon) = Self::determine_default_mode_and_coords();
            (mode, lat, lon, None)
        };

        // Now handle geo.toml logic for ALL cases
        let should_write_coords_to_main = if use_geo_file {
            // Write coordinates to geo.toml instead of main config
            let geo_content = format!(
                "#[Private geo coordinates]\nlatitude = {:.6}\nlongitude = {:.6}\n",
                lat, lon
            );

            fs::write(&geo_path, geo_content).with_context(|| {
                format!("Failed to write coordinates to {}", geo_path.display())
            })?;

            use crate::logger::Log;
            if let Some(city) = city_name {
                Log::log_indented(&format!("Using selected location for new config: {}", city));
            }
            Log::log_indented(&format!(
                "Saved coordinates to separate geo file: {}",
                crate::utils::path_for_display(&geo_path)
            ));

            false // Don't write coords to main config
        } else {
            // No geo.toml, write to main config as usual
            use crate::logger::Log;
            if let Some(city) = city_name {
                Log::log_indented(&format!("Using selected location for new config: {}", city));
            }
            true // Write coords to main config
        };

        // Build the config using the builder pattern
        let config_content = ConfigBuilder::new()
            .add_section("Sunsetr configuration")
            .add_setting(
                "backend",
                &format!("\"{}\"", DEFAULT_BACKEND.as_str()),
                "Backend to use: \"auto\", \"hyprland\" or \"wayland\"",
            )
            .add_setting(
                "start_hyprsunset",
                &DEFAULT_START_HYPRSUNSET.to_string(),
                "Set true if you're not using hyprsunset.service",
            )
            .add_setting(
                "startup_transition",
                &DEFAULT_STARTUP_TRANSITION.to_string(),
                "Enable smooth transition when sunsetr starts",
            )
            .add_setting(
                "startup_transition_duration",
                &DEFAULT_STARTUP_TRANSITION_DURATION.to_string(),
                &format!(
                    "Duration of startup transition in seconds ({}-{})",
                    MINIMUM_STARTUP_TRANSITION_DURATION, MAXIMUM_STARTUP_TRANSITION_DURATION
                ),
            )
            .add_setting(
                "night_temp",
                &DEFAULT_NIGHT_TEMP.to_string(),
                &format!(
                    "Color temperature after sunset ({}-{}) Kelvin",
                    MINIMUM_TEMP, MAXIMUM_TEMP
                ),
            )
            .add_setting(
                "day_temp",
                &DEFAULT_DAY_TEMP.to_string(),
                &format!(
                    "Color temperature during day ({}-{}) Kelvin",
                    MINIMUM_TEMP, MAXIMUM_TEMP
                ),
            )
            .add_setting(
                "night_gamma",
                &DEFAULT_NIGHT_GAMMA.to_string(),
                &format!(
                    "Gamma percentage for night ({}-{}%)",
                    MINIMUM_GAMMA, MAXIMUM_GAMMA
                ),
            )
            .add_setting(
                "day_gamma",
                &DEFAULT_DAY_GAMMA.to_string(),
                &format!(
                    "Gamma percentage for day ({}-{}%)",
                    MINIMUM_GAMMA, MAXIMUM_GAMMA
                ),
            )
            .add_setting(
                "update_interval",
                &DEFAULT_UPDATE_INTERVAL.to_string(),
                &format!(
                    "Update frequency during transitions in seconds ({}-{})",
                    MINIMUM_UPDATE_INTERVAL, MAXIMUM_UPDATE_INTERVAL
                ),
            )
            .add_setting(
                "transition_mode",
                &format!("\"{}\"", transition_mode),
                "Select: \"geo\", \"finish_by\", \"start_at\", \"center\"",
            )
            .add_section("Manual transitions")
            .add_setting(
                "sunset",
                &format!("\"{}\"", DEFAULT_SUNSET),
                "Time to transition to night mode (HH:MM:SS) - ignored in geo mode",
            )
            .add_setting(
                "sunrise",
                &format!("\"{}\"", DEFAULT_SUNRISE),
                "Time to transition to day mode (HH:MM:SS) - ignored in geo mode",
            )
            .add_setting(
                "transition_duration",
                &DEFAULT_TRANSITION_DURATION.to_string(),
                &format!(
                    "Transition duration in minutes ({}-{})",
                    MINIMUM_TRANSITION_DURATION, MAXIMUM_TRANSITION_DURATION
                ),
            )
            .add_section("Geolocation-based transitions");

        // Only add coordinates to main config if they should be written there
        let config_content = if should_write_coords_to_main {
            config_content
                .add_setting(
                    "latitude",
                    &format!("{:.6}", lat),
                    "Geographic latitude (auto-detected on first run)",
                )
                .add_setting(
                    "longitude",
                    &format!("{:.6}", lon),
                    "Geographic longitude (use 'sunsetr --geo' to change)",
                )
        } else {
            // When using geo.toml, don't add coordinates to main config at all
            config_content
        };

        let config_content = config_content.build();

        fs::write(path, config_content).context("Failed to write default config file")?;
        Ok(())
    }

    /// Determine the default transition mode and coordinates for new configs.
    ///
    /// This function implements smart defaults:
    /// 1. Try timezone detection for automatic geo mode
    /// 2. If successful, return geo mode with populated coordinates
    /// 3. If failed, fallback to finish_by mode with Chicago coordinates
    ///
    /// # Returns
    /// Tuple of (transition_mode, latitude, longitude)
    fn determine_default_mode_and_coords() -> (&'static str, f64, f64) {
        use crate::logger::Log;

        // Try timezone detection for automatic coordinates
        if let Ok((mut lat, lon, city_name)) = crate::geo::detect_coordinates_from_timezone() {
            // Cap latitude at ±65°
            if lat.abs() > 65.0 {
                lat = 65.0 * lat.signum();
            }

            Log::log_indented(&format!(
                "Auto-detected location for new config: {}",
                city_name
            ));
            (DEFAULT_TRANSITION_MODE, lat, lon)
        } else {
            // Fall back to finish_by mode with Chicago coordinates as placeholders
            Log::log_indented(
                "Timezone detection failed, using manual times with placeholder coordinates",
            );
            Log::log_indented("Use 'sunsetr --geo' to select your actual location");
            (
                crate::constants::FALLBACK_DEFAULT_TRANSITION_MODE,
                41.8781,
                -87.6298,
            ) // Chicago coordinates (placeholder)
        }
    }

    // NEW private helper method
    fn apply_defaults_and_validate_fields(config: &mut Config) -> Result<()> {
        // Set default for start_hyprsunset if not specified
        if config.start_hyprsunset.is_none() {
            config.start_hyprsunset = Some(DEFAULT_START_HYPRSUNSET);
        }

        // Set default for backend if not specified
        if config.backend.is_none() {
            config.backend = Some(DEFAULT_BACKEND);
        }

        // Validate time formats
        NaiveTime::parse_from_str(&config.sunset, "%H:%M:%S")
            .context("Invalid sunset time format in config. Use HH:MM:SS format")?;
        NaiveTime::parse_from_str(&config.sunrise, "%H:%M:%S")
            .context("Invalid sunrise time format in config. Use HH:MM:SS format")?;

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

        // Set defaults for startup transition fields
        if config.startup_transition.is_none() {
            config.startup_transition = Some(DEFAULT_STARTUP_TRANSITION);
        }

        if config.startup_transition_duration.is_none() {
            config.startup_transition_duration = Some(DEFAULT_STARTUP_TRANSITION_DURATION);
        }

        // Validate transition ranges
        if let Some(duration_minutes) = config.transition_duration {
            if !(MINIMUM_TRANSITION_DURATION..=MAXIMUM_TRANSITION_DURATION)
                .contains(&duration_minutes)
            {
                anyhow::bail!(
                    "Transition duration must be between {} and {} minutes",
                    MINIMUM_TRANSITION_DURATION,
                    MAXIMUM_TRANSITION_DURATION
                );
            }
        }

        if let Some(interval) = config.update_interval {
            if !(MINIMUM_UPDATE_INTERVAL..=MAXIMUM_UPDATE_INTERVAL).contains(&interval) {
                anyhow::bail!(
                    "Update interval must be between {} and {} seconds",
                    MINIMUM_UPDATE_INTERVAL,
                    MAXIMUM_UPDATE_INTERVAL
                );
            }
        }

        // Validate transition mode
        if let Some(ref mode) = config.transition_mode {
            if mode != "finish_by" && mode != "start_at" && mode != "center" && mode != "geo" {
                anyhow::bail!(
                    "Transition mode must be 'finish_by', 'start_at', 'center', or 'geo'"
                );
            }
        }

        // Validate startup transition duration
        if let Some(duration_seconds) = config.startup_transition_duration {
            if !(MINIMUM_STARTUP_TRANSITION_DURATION..=MAXIMUM_STARTUP_TRANSITION_DURATION)
                .contains(&duration_seconds)
            {
                anyhow::bail!(
                    "Startup transition duration must be between {} and {} seconds",
                    MINIMUM_STARTUP_TRANSITION_DURATION,
                    MAXIMUM_STARTUP_TRANSITION_DURATION
                );
            }
        }

        // Validate latitude range (-90 to 90)
        if let Some(lat) = config.latitude {
            if !(-90.0..=90.0).contains(&lat) {
                anyhow::bail!(
                    "Latitude must be between -90 and 90 degrees (got {})",
                    lat
                );
            }
            // Cap latitude at ±65° to avoid solar calculation edge cases
            if lat.abs() > 65.0 {
                Log::log_pipe();
                Log::log_warning(&format!(
                    "⚠️ Latitude capped at 65°{} (config {:.4}°{})",
                    if lat >= 0.0 { "N" } else { "S" },
                    lat.abs(),
                    if lat >= 0.0 { "N" } else { "S" },
                ));
                Log::log_indented("Are you researching extremophile bacteria under the ice caps?");
                Log::log_indented(
                    "Consider using manual sunset/sunrise times for better accuracy.",
                );
                config.latitude = Some(65.0 * lat.signum());
            }
        }
        
        // Validate longitude range (-180 to 180)
        if let Some(lon) = config.longitude {
            if !(-180.0..=180.0).contains(&lon) {
                anyhow::bail!(
                    "Longitude must be between -180 and 180 degrees (got {})",
                    lon
                );
            }
        }

        Ok(())
    }

    // NEW public method for loading from a specific path
    // This version does NOT create a default config if the path doesn't exist.
    pub fn load_from_path(path: &PathBuf) -> Result<Self> {
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

        Self::apply_defaults_and_validate_fields(&mut config)?;

        // Load geo.toml overrides if present - pass the actual config path
        Self::load_geo_override_from_path(&mut config, path)?;

        // Comprehensive configuration validation (this is the existing public function)
        validate_config(&config)?;

        Ok(config)
    }

    /// Load geo.toml from a specific config path
    fn load_geo_override_from_path(config: &mut Config, config_path: &Path) -> Result<()> {
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

                        // Log that we loaded geo overrides
                        Log::log_indented(&format!(
                            "Loaded geographic overrides from {}",
                            crate::utils::path_for_display(&geo_path)
                        ));
                    }
                    Err(e) => {
                        // Malformed geo.toml - log warning and continue
                        Log::log_warning(&format!(
                            "Failed to parse geo.toml: {}. Using coordinates from main config.",
                            e
                        ));
                    }
                }
            }
            Err(e) => {
                // Permission error or other read error - log warning and continue
                Log::log_warning(&format!(
                    "Failed to read geo.toml: {}. Using coordinates from main config.",
                    e
                ));
            }
        }

        Ok(())
    }

    // MODIFIED existing load method
    pub fn load() -> Result<Self> {
        let config_path = Self::get_config_path()?;

        if !config_path.exists() {
            Self::create_default_config(&config_path, None)
                .context("Failed to create default config during load")?;
        }

        // Now that we're sure a file exists (either pre-existing or newly created default),
        // load it using the common path-based loader.
        // Note: load_from_path already calls load_geo_override_from_path, so we don't need to call it again
        let mut config = Self::load_from_path(&config_path).with_context(|| {
            Log::log_pipe();
            format!(
                "Failed to load configuration from {}",
                config_path.display()
            )
        })?;

        // Check if we have geo mode but missing coordinates
        if config.transition_mode.as_deref() == Some("geo")
            && (config.latitude.is_none() || config.longitude.is_none())
        {
            // Try to detect coordinates from timezone
            if let Ok((lat, lon, city_name)) = crate::geo::detect_coordinates_from_timezone() {
                // Update the config file with detected coordinates
                Log::log_pipe();
                Log::log_block_start("Missing coordinates for geo mode");
                Log::log_indented(&format!("Auto-detected location: {}", city_name));
                Log::log_indented("Updating configuration with detected coordinates...");

                // Update the config file
                Self::update_config_with_geo_coordinates(lat, lon)?;

                // Update our in-memory config
                config.latitude = Some(lat);
                config.longitude = Some(lon);
            } else {
                Log::log_pipe();
                Log::log_error("Geo mode requires coordinates but none are configured");
                Log::log_indented("Please run 'sunsetr --geo' to select your location");
                std::process::exit(crate::constants::EXIT_FAILURE);
            }
        }

        Ok(config)
    }

    /// Update an existing config file with geo coordinates and mode
    pub fn update_config_with_geo_coordinates(mut latitude: f64, longitude: f64) -> Result<()> {
        let config_path = Self::get_config_path()?;
        let geo_path = Self::get_geo_path()?;

        if !config_path.exists() {
            anyhow::bail!("No existing config file found at {}", config_path.display());
        }

        // Cap latitude at ±65° before saving
        if latitude.abs() > 65.0 {
            latitude = 65.0 * latitude.signum();
        }

        // Check if geo.toml exists - if it does, update there instead
        if geo_path.exists() {
            // Update geo.toml with new coordinates
            let geo_content = format!(
                "#[Private geo coordinates]\nlatitude = {:.6}\nlongitude = {:.6}\n",
                latitude, longitude
            );

            fs::write(&geo_path, geo_content).with_context(|| {
                format!("Failed to write coordinates to {}", geo_path.display())
            })?;

            // Also ensure transition_mode is set to "geo" in main config
            let content = fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read config from {}", config_path.display()))?;

            let mut updated_content = content.clone();

            // Update or add transition_mode to "geo"
            if let Some(mode_line) = find_config_line(&content, "transition_mode") {
                let new_mode_line =
                    preserve_comment_formatting(&mode_line, "transition_mode", "\"geo\"");
                updated_content = updated_content.replace(&mode_line, &new_mode_line);
            } else {
                // Add transition_mode at the end
                updated_content = format!("{}transition_mode = \"geo\"\n", updated_content);
            }

            // Write back only if we changed transition_mode
            if updated_content != content {
                fs::write(&config_path, updated_content).with_context(|| {
                    format!(
                        "Failed to write updated config to {}",
                        config_path.display()
                    )
                })?;
            }

            Log::log_block_start(&format!(
                "Updated geo coordinates in {}",
                crate::utils::path_for_display(&geo_path)
            ));
            Log::log_indented(&format!("Latitude: {}", latitude));
            Log::log_indented(&format!("Longitude: {}", longitude));

            return Ok(());
        }

        // geo.toml doesn't exist, update main config as before
        // Read current config content
        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config from {}", config_path.display()))?;

        // Parse as TOML to preserve structure and comments
        let mut updated_content = content.clone();

        // Update or add latitude
        if let Some(lat_line) = find_config_line(&content, "latitude") {
            let new_lat_line =
                preserve_comment_formatting(&lat_line, "latitude", &format!("{:.6}", latitude));
            updated_content = updated_content.replace(&lat_line, &new_lat_line);
        } else {
            // Latitude doesn't exist, will add at the end
        }

        // Update or add longitude
        if let Some(lon_line) = find_config_line(&content, "longitude") {
            let new_lon_line =
                preserve_comment_formatting(&lon_line, "longitude", &format!("{:.6}", longitude));
            updated_content = updated_content.replace(&lon_line, &new_lon_line);
        } else {
            // Longitude doesn't exist, will add at the end
        }

        // If either coordinate is missing, append both at the end
        let lat_exists = find_config_line(&content, "latitude").is_some();
        let lon_exists = find_config_line(&content, "longitude").is_some();

        if !lat_exists || !lon_exists {
            // Ensure file ends with newline
            if !updated_content.ends_with('\n') {
                updated_content.push('\n');
            }

            // Add coordinates
            if !lat_exists {
                updated_content.push_str(&format!("latitude = {:.6}\n", latitude));
            }
            if !lon_exists {
                updated_content.push_str(&format!("longitude = {:.6}\n", longitude));
            }
        }

        // Update transition_mode to "geo" only if it's not already set to "geo"
        if let Some(mode_line) = find_config_line(&content, "transition_mode") {
            // Check if it's already set to "geo"
            if !mode_line.contains("\"geo\"") {
                let new_mode_line =
                    preserve_comment_formatting(&mode_line, "transition_mode", "\"geo\"");
                updated_content = updated_content.replace(&mode_line, &new_mode_line);
            }
        } else {
            // Add transition_mode at the end
            updated_content = format!("{}transition_mode = \"geo\"\n", updated_content);
        }

        // Write updated content back to file
        fs::write(&config_path, updated_content).with_context(|| {
            format!(
                "Failed to write updated config to {}",
                config_path.display()
            )
        })?;

        Log::log_block_start(&format!(
            "Updated config file: {}",
            crate::utils::path_for_display(&config_path)
        ));
        Log::log_indented(&format!("Latitude: {}", latitude));
        Log::log_indented(&format!("Longitude: {}", longitude));
        Log::log_indented("Transition mode: geo");

        Ok(())
    }

    pub fn log_config(&self) {
        let config_path = Self::get_config_path()
            .unwrap_or_else(|_| PathBuf::from("~/.config/sunsetr/sunsetr.toml"));
        let geo_path =
            Self::get_geo_path().unwrap_or_else(|_| PathBuf::from("~/.config/sunsetr/geo.toml"));

        Log::log_block_start(&format!(
            "Loaded configuration from {}",
            crate::utils::path_for_display(&config_path)
        ));

        // Check if geo.toml exists to show appropriate message
        if geo_path.exists() {
            Log::log_indented(&format!(
                "Loaded geo coordinates from {}",
                crate::utils::path_for_display(&geo_path)
            ));
        }

        Log::log_indented(&format!(
            "Backend: {}",
            self.backend.as_ref().unwrap_or(&DEFAULT_BACKEND).as_str()
        ));
        Log::log_indented(&format!(
            "Auto-start hyprsunset: {}",
            self.start_hyprsunset.unwrap_or(DEFAULT_START_HYPRSUNSET)
        ));
        Log::log_indented(&format!(
            "Enable startup transition: {}",
            self.startup_transition
                .unwrap_or(DEFAULT_STARTUP_TRANSITION)
        ));

        // Only show startup transition duration if startup transition is enabled
        if self
            .startup_transition
            .unwrap_or(DEFAULT_STARTUP_TRANSITION)
        {
            Log::log_indented(&format!(
                "Startup transition duration: {} seconds",
                self.startup_transition_duration
                    .unwrap_or(DEFAULT_STARTUP_TRANSITION_DURATION)
            ));
        }

        // Show geographic coordinates if in geo mode
        let mode = self
            .transition_mode
            .as_deref()
            .unwrap_or(DEFAULT_TRANSITION_MODE);
        if mode == "geo" {
            if let (Some(lat), Some(lon)) = (self.latitude, self.longitude) {
                let lat_dir = if lat >= 0.0 { "N" } else { "S" };
                let lon_dir = if lon >= 0.0 { "E" } else { "W" };
                Log::log_indented(&format!(
                    "Location: {:.4}°{}, {:.4}°{}",
                    lat.abs(),
                    lat_dir,
                    lon.abs(),
                    lon_dir
                ));
            } else {
                Log::log_indented("Location: Auto-detected on first run");
            }
        }

        Log::log_indented(&format!("Sunset time: {}", self.sunset));
        Log::log_indented(&format!("Sunrise time: {}", self.sunrise));
        Log::log_indented(&format!(
            "Night temperature: {}K",
            self.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP)
        ));
        Log::log_indented(&format!(
            "Day temperature: {}K",
            self.day_temp.unwrap_or(DEFAULT_DAY_TEMP)
        ));
        Log::log_indented(&format!(
            "Night gamma: {}%",
            self.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA)
        ));
        Log::log_indented(&format!(
            "Day gamma: {}%",
            self.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA)
        ));
        Log::log_indented(&format!(
            "Transition duration: {} minutes",
            self.transition_duration
                .unwrap_or(DEFAULT_TRANSITION_DURATION)
        ));
        Log::log_indented(&format!(
            "Update interval: {} seconds",
            self.update_interval.unwrap_or(DEFAULT_UPDATE_INTERVAL)
        ));
        Log::log_indented(&format!(
            "Transition mode: {}",
            self.transition_mode
                .as_deref()
                .unwrap_or(DEFAULT_TRANSITION_MODE)
        ));
    }
}

/// Comprehensive configuration validation to prevent impossible or problematic setups
pub fn validate_config(config: &Config) -> Result<()> {
    use chrono::NaiveTime;

    // 0. Validate backend configuration compatibility
    let backend = config.backend.as_ref().unwrap_or(&DEFAULT_BACKEND);
    let start_hyprsunset = config.start_hyprsunset.unwrap_or(DEFAULT_START_HYPRSUNSET);

    // Only validate explicit backend choices, Auto will be resolved at runtime
    if *backend == Backend::Wayland && start_hyprsunset {
        anyhow::bail!(
            "Incompatible configuration: backend=\"wayland\" and start_hyprsunset=true. \
            When using Wayland protocols (backend=\"wayland\"), hyprsunset should not be started (start_hyprsunset=false). \
            Please set either backend=\"hyprland\" or start_hyprsunset=false."
        );
    }

    let sunset = NaiveTime::parse_from_str(&config.sunset, "%H:%M:%S")
        .context("Invalid sunset time format")?;
    let sunrise = NaiveTime::parse_from_str(&config.sunrise, "%H:%M:%S")
        .context("Invalid sunrise time format")?;

    let transition_duration_mins = config
        .transition_duration
        .unwrap_or(DEFAULT_TRANSITION_DURATION);
    let update_interval_secs = config.update_interval.unwrap_or(DEFAULT_UPDATE_INTERVAL);
    let mode = config
        .transition_mode
        .as_deref()
        .unwrap_or(DEFAULT_TRANSITION_MODE);

    // Validate transition duration (hard limits)
    if !(MINIMUM_TRANSITION_DURATION..=MAXIMUM_TRANSITION_DURATION)
        .contains(&transition_duration_mins)
    {
        anyhow::bail!(
            "Transition duration ({} minutes) must be between {} and {} minutes",
            transition_duration_mins,
            MINIMUM_TRANSITION_DURATION,
            MAXIMUM_TRANSITION_DURATION
        );
    }

    // Validate startup transition duration (hard limits)
    if let Some(startup_duration_secs) = config.startup_transition_duration {
        if !(MINIMUM_STARTUP_TRANSITION_DURATION..=MAXIMUM_STARTUP_TRANSITION_DURATION)
            .contains(&startup_duration_secs)
        {
            anyhow::bail!(
                "Startup transition duration ({} seconds) must be between {} and {} seconds",
                startup_duration_secs,
                MINIMUM_STARTUP_TRANSITION_DURATION,
                MAXIMUM_STARTUP_TRANSITION_DURATION
            );
        }
    }

    // 0. Validate basic ranges for temperature and gamma (hard limits)
    if let Some(temp) = config.night_temp {
        if !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp) {
            anyhow::bail!(
                "Night temperature ({}) must be between {} and {} Kelvin",
                temp,
                MINIMUM_TEMP,
                MAXIMUM_TEMP
            );
        }
    }

    if let Some(temp) = config.day_temp {
        if !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp) {
            anyhow::bail!(
                "Day temperature ({}) must be between {} and {} Kelvin",
                temp,
                MINIMUM_TEMP,
                MAXIMUM_TEMP
            );
        }
    }

    if let Some(gamma) = config.night_gamma {
        if !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma) {
            anyhow::bail!(
                "Night gamma ({}%) must be between {}% and {}%",
                gamma,
                MINIMUM_GAMMA,
                MAXIMUM_GAMMA
            );
        }
    }

    if let Some(gamma) = config.day_gamma {
        if !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma) {
            anyhow::bail!(
                "Day gamma ({}%) must be between {}% and {}%",
                gamma,
                MINIMUM_GAMMA,
                MAXIMUM_GAMMA
            );
        }
    }

    // 1. Check for identical sunset/sunrise times
    if sunset == sunrise {
        anyhow::bail!(
            "Sunset and sunrise cannot be the same time ({:?}). \
            There must be a distinction between day and night periods.",
            sunset
        );
    }

    // 2. Calculate time periods and check minimums
    let (day_duration_mins, night_duration_mins) = calculate_day_night_durations(sunset, sunrise);

    if day_duration_mins < 60 {
        anyhow::bail!(
            "Day period is too short ({} minutes). \
            Day period must be at least 1 hour. \
            Adjust sunset ({:?}) or sunrise ({:?}) times.",
            day_duration_mins,
            sunset,
            sunrise
        );
    }

    if night_duration_mins < 60 {
        anyhow::bail!(
            "Night period is too short ({} minutes). \
            Night period must be at least 1 hour. \
            Adjust sunset ({:?}) or sunrise ({:?}) times.",
            night_duration_mins,
            sunset,
            sunrise
        );
    }

    // 3. Check that transitions fit within their periods
    validate_transitions_fit_periods(sunset, sunrise, transition_duration_mins, mode)?;

    // 4. Check for transition overlaps
    validate_no_transition_overlaps(sunset, sunrise, transition_duration_mins, mode)?;

    // 5. Validate update interval vs transition duration (must come before range check)
    let transition_duration_secs = transition_duration_mins * 60;
    if update_interval_secs > transition_duration_secs {
        anyhow::bail!(
            "Update interval ({} seconds) is longer than transition duration ({} seconds). \
            Update interval should be shorter to allow smooth transitions. \
            Reduce update_interval or increase transition_duration.",
            update_interval_secs,
            transition_duration_secs
        );
    }

    // 6. Update interval range check (with warnings for extreme values)
    if update_interval_secs < MINIMUM_UPDATE_INTERVAL {
        Log::log_warning(&format!(
            "Update interval ({} seconds) is below recommended minimum ({} seconds). \
            This may cause excessive system load.",
            update_interval_secs, MINIMUM_UPDATE_INTERVAL
        ));
    } else if update_interval_secs > MAXIMUM_UPDATE_INTERVAL {
        Log::log_warning(&format!(
            "Update interval ({} seconds) is above recommended maximum ({} seconds). \
            Transitions may appear choppy.",
            update_interval_secs, MAXIMUM_UPDATE_INTERVAL
        ));
    }

    // 7. Check for reasonable transition frequency
    if transition_duration_secs < 300 && update_interval_secs < 30 {
        // This would create very frequent updates
        Log::log_warning(&format!(
            "Very short transition duration ({} min) with frequent updates ({} sec) may stress your graphics system.",
            transition_duration_mins, update_interval_secs
        ));
    }

    Ok(())
}

/// Builder for creating dynamically-aligned configuration files.
///
/// This builder maintains proper comment alignment by calculating the maximum
/// width of all setting lines and applying consistent padding. This ensures
/// that when constants change in constants.rs, the config file formatting
/// remains correct.
struct ConfigBuilder {
    entries: Vec<ConfigEntry>,
}

#[derive(Clone)]
struct ConfigEntry {
    content: String,
    entry_type: EntryType,
}

#[derive(Clone)]
enum EntryType {
    Section,
    Setting { line: String, comment: String },
}

impl ConfigBuilder {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn add_section(mut self, title: &str) -> Self {
        self.entries.push(ConfigEntry {
            content: format!("#[{}]", title),
            entry_type: EntryType::Section,
        });
        self
    }

    fn add_setting(mut self, key: &str, value: &str, comment: &str) -> Self {
        let line = format!("{} = {}", key, value);
        self.entries.push(ConfigEntry {
            content: line.clone(),
            entry_type: EntryType::Setting {
                line,
                comment: format!("# {}", comment),
            },
        });
        self
    }

    fn build(self) -> String {
        // Calculate the maximum width of all setting lines for alignment
        let max_width = self
            .entries
            .iter()
            .filter_map(|entry| match &entry.entry_type {
                EntryType::Setting { line, .. } => Some(line.len()),
                EntryType::Section => None,
            })
            .max()
            .unwrap_or(0)
            + 1; // +1 for one space between setting and comment

        let mut result = Vec::new();
        let mut first_section = true;

        for entry in self.entries {
            match entry.entry_type {
                EntryType::Section => {
                    if !first_section {
                        result.push(String::new()); // Empty line before new section
                    }
                    result.push(entry.content);
                    first_section = false;
                }
                EntryType::Setting { line, comment } => {
                    let padding = " ".repeat(max_width - line.len());
                    result.push(format!("{}{}{}", line, padding, comment));
                }
            }
        }

        result.join("\n")
    }
}

/// Calculate day and night durations in minutes
fn calculate_day_night_durations(sunset: NaiveTime, sunrise: NaiveTime) -> (u32, u32) {
    let sunset_mins = sunset.hour() * 60 + sunset.minute();
    let sunrise_mins = sunrise.hour() * 60 + sunrise.minute();

    if sunset_mins > sunrise_mins {
        // Normal case: sunset after sunrise in the same day
        let day_duration = sunset_mins - sunrise_mins;
        let night_duration = (24 * 60) - day_duration;
        (day_duration, night_duration)
    } else {
        // Overnight case: sunset before sunrise (next day)
        let night_duration = sunrise_mins - sunset_mins;
        let day_duration = (24 * 60) - night_duration;
        (day_duration, night_duration)
    }
}

/// Validate that transitions fit within their respective day/night periods
fn validate_transitions_fit_periods(
    sunset: NaiveTime,
    sunrise: NaiveTime,
    transition_duration_mins: u64,
    mode: &str,
) -> Result<()> {
    let (day_duration_mins, night_duration_mins) = calculate_day_night_durations(sunset, sunrise);

    // For "center" mode, transition spans both day and night periods
    // For "finish_by" and "start_at", transition should fit within the target period

    match mode {
        "center" => {
            // Transition spans across sunset/sunrise time, so we need room on both sides
            let half_transition = transition_duration_mins / 2;

            // Check if transition would exceed either period
            if half_transition >= day_duration_mins.into()
                || half_transition >= night_duration_mins.into()
            {
                anyhow::bail!(
                    "Transition duration ({} minutes) is too long for 'center' mode. \
                    With centered transitions, half the duration ({} minutes) must fit in both \
                    day period ({} minutes) and night period ({} minutes). \
                    Reduce transition_duration or adjust sunset/sunrise times.",
                    transition_duration_mins,
                    half_transition,
                    day_duration_mins,
                    night_duration_mins
                );
            }
        }
        "finish_by" | "start_at" => {
            // Transitions should reasonably fit within their periods
            let max_reasonable_ratio = 0.8; // 80% of period
            let max_day_transition = (day_duration_mins as f64 * max_reasonable_ratio) as u64;
            let max_night_transition = (night_duration_mins as f64 * max_reasonable_ratio) as u64;

            if transition_duration_mins > max_day_transition {
                Log::log_warning(&format!(
                    "Transition duration ({} min) is quite long compared to day period ({} min). Consider reducing transition_duration for better experience.",
                    transition_duration_mins, day_duration_mins
                ));
            }

            if transition_duration_mins > max_night_transition {
                Log::log_warning(&format!(
                    "Transition duration ({} min) is quite long compared to night period ({} min). Consider reducing transition_duration for better experience.",
                    transition_duration_mins, night_duration_mins
                ));
            }
        }
        _ => {} // Already validated mode earlier
    }

    Ok(())
}

/// Validate that sunset and sunrise transitions don't overlap
fn validate_no_transition_overlaps(
    sunset: NaiveTime,
    sunrise: NaiveTime,
    transition_duration_mins: u64,
    mode: &str,
) -> Result<()> {
    use std::time::Duration;

    // Calculate transition windows using the same logic as the main code
    let transition_duration = Duration::from_secs(transition_duration_mins * 60);

    let (sunset_start, sunset_end, sunrise_start, sunrise_end) = match mode {
        "center" => {
            let half_transition = transition_duration / 2;
            let half_chrono = chrono::Duration::from_std(half_transition).unwrap();
            (
                sunset - half_chrono,
                sunset + half_chrono,
                sunrise - half_chrono,
                sunrise + half_chrono,
            )
        }
        "start_at" => {
            let full_transition = chrono::Duration::from_std(transition_duration).unwrap();
            (
                sunset,
                sunset + full_transition,
                sunrise,
                sunrise + full_transition,
            )
        }
        "finish_by" => {
            let full_transition = chrono::Duration::from_std(transition_duration).unwrap();
            (
                sunset - full_transition,
                sunset,
                sunrise - full_transition,
                sunrise,
            )
        }
        _ => {
            // Default to "finish_by" mode for any unexpected values
            let full_transition = chrono::Duration::from_std(transition_duration).unwrap();
            (
                sunset - full_transition,
                sunset,
                sunrise - full_transition,
                sunrise,
            )
        }
    };

    // Convert to minutes since midnight for easier comparison
    let sunset_start_mins = sunset_start.hour() * 60 + sunset_start.minute();
    let sunset_end_mins = sunset_end.hour() * 60 + sunset_end.minute();
    let sunrise_start_mins = sunrise_start.hour() * 60 + sunrise_start.minute();
    let sunrise_end_mins = sunrise_end.hour() * 60 + sunrise_end.minute();

    // Check for overlaps - this is complex due to potential midnight crossings
    let overlap = check_time_ranges_overlap(
        sunset_start_mins,
        sunset_end_mins,
        sunrise_start_mins,
        sunrise_end_mins,
    );

    if overlap {
        anyhow::bail!(
            "Transition periods overlap! \
            Sunset transition: {:?} → {:?}, Sunrise transition: {:?} → {:?}. \
            \nThis configuration is impossible because transitions would conflict. \
            \nSolutions: \
            \n  1. Reduce transition_duration from {} to {} minutes or less \
            \n  2. Increase time between sunset ({:?}) and sunrise ({:?}) \
            \n  3. Change transition_mode from '{}' to a different mode",
            sunset_start,
            sunset_end,
            sunrise_start,
            sunrise_end,
            transition_duration_mins,
            suggest_max_transition_duration(sunset, sunrise, mode),
            sunset,
            sunrise,
            mode
        );
    }

    Ok(())
}

/// Check if two time ranges overlap, handling midnight crossings
fn check_time_ranges_overlap(
    start1_mins: u32,
    end1_mins: u32,
    start2_mins: u32,
    end2_mins: u32,
) -> bool {
    // Helper function to normalize ranges that cross midnight
    let normalize_range = |start: u32, end: u32| -> Vec<(u32, u32)> {
        if start <= end {
            vec![(start, end)]
        } else {
            // Range crosses midnight, split into two ranges
            vec![(start, 24 * 60), (0, end)]
        }
    };

    let range1 = normalize_range(start1_mins, end1_mins);
    let range2 = normalize_range(start2_mins, end2_mins);

    // Check if any segment from range1 overlaps with any segment from range2
    for (r1_start, r1_end) in &range1 {
        for (r2_start, r2_end) in &range2 {
            if r1_start < r2_end && r2_start < r1_end {
                return true; // Overlap detected
            }
        }
    }

    false
}

/// Suggest a maximum safe transition duration for the given configuration
fn suggest_max_transition_duration(sunset: NaiveTime, sunrise: NaiveTime, mode: &str) -> u64 {
    let (day_duration_mins, night_duration_mins) = calculate_day_night_durations(sunset, sunrise);
    let min_period = day_duration_mins.min(night_duration_mins);

    match mode {
        "center" => {
            // For center mode, half the transition goes in each period
            ((min_period / 2).saturating_sub(1)).into()
        }
        "finish_by" | "start_at" => {
            // For these modes, leave some buffer between transitions
            ((min_period as f64 * 0.8) as u32).into()
        }
        _ => (min_period.saturating_sub(10)).into(),
    }
}

/// Find a config line containing the specified key
fn find_config_line(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(key) && trimmed.contains('=') && !trimmed.starts_with('#') {
            return Some(line.to_string());
        }
    }
    None
}

/// Preserve the comment formatting when updating a config line value
fn preserve_comment_formatting(original_line: &str, key: &str, new_value: &str) -> String {
    if let Some(comment_pos) = original_line.find('#') {
        let comment_part = &original_line[comment_pos..];
        let key_value_part = format!("{} = {}", key, new_value);

        // Calculate spacing to align with other comments (aim for column 32)
        let target_width = 32;
        let padding_needed = if key_value_part.len() < target_width {
            target_width - key_value_part.len()
        } else {
            1 // At least one space
        };

        format!(
            "{}{}{}",
            key_value_part,
            " ".repeat(padding_needed),
            comment_part
        )
    } else {
        format!("{} = {}", key, new_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::test_constants::*;
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;

    #[allow(clippy::too_many_arguments)]
    fn create_test_config(
        sunset: &str,
        sunrise: &str,
        transition_duration: Option<u64>,
        update_interval: Option<u64>,
        transition_mode: Option<&str>,
        night_temp: Option<u32>,
        day_temp: Option<u32>,
        night_gamma: Option<f32>,
        day_gamma: Option<f32>,
    ) -> Config {
        Config {
            start_hyprsunset: Some(false),
            backend: Some(Backend::Auto),
            startup_transition: Some(false),
            startup_transition_duration: Some(10),
            latitude: None,
            longitude: None,
            sunset: sunset.to_string(),
            sunrise: sunrise.to_string(),
            night_temp,
            day_temp,
            night_gamma,
            day_gamma,
            transition_duration,
            update_interval,
            transition_mode: transition_mode.map(|s| s.to_string()),
        }
    }

    #[test]
    #[serial]
    fn test_config_load_default_creation() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("sunsetr").join("sunsetr.toml");

        // Save and restore XDG_CONFIG_HOME
        let original = std::env::var("XDG_CONFIG_HOME").ok();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        // First load should create default config
        let result = Config::load();

        // Restore original
        unsafe {
            match original {
                Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }

        assert!(result.is_ok());
        assert!(config_path.exists());
    }

    #[test]
    fn test_config_validation_basic() {
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_config_validation_backend_compatibility() {
        // Test valid combinations
        let mut config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );

        // Valid: use_wayland=false, start_hyprsunset=true (Hyprland backend)
        config.backend = Some(Backend::Hyprland);
        config.start_hyprsunset = Some(true);
        assert!(validate_config(&config).is_ok());

        // Valid: use_wayland=true, start_hyprsunset=false (Wayland backend)
        config.backend = Some(Backend::Wayland);
        config.start_hyprsunset = Some(false);
        assert!(validate_config(&config).is_ok());

        // Valid: use_wayland=false, start_hyprsunset=false (Hyprland without auto-start)
        config.backend = Some(Backend::Hyprland);
        config.start_hyprsunset = Some(false);
        assert!(validate_config(&config).is_ok());

        // Invalid: use_wayland=true, start_hyprsunset=true (conflicting)
        config.backend = Some(Backend::Wayland);
        config.start_hyprsunset = Some(true);
        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Incompatible configuration")
        );
    }

    #[test]
    fn test_config_validation_identical_times() {
        let config = create_test_config(
            "12:00:00",
            "12:00:00",
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());
        assert!(
            validate_config(&config)
                .unwrap_err()
                .to_string()
                .contains("cannot be the same time")
        );
    }

    #[test]
    fn test_config_validation_extreme_short_day() {
        // 30 minute day period (sunrise 23:45, sunset 00:15)
        let config = create_test_config(
            "00:15:00",
            "23:45:00",
            Some(5),
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());
        assert!(
            validate_config(&config)
                .unwrap_err()
                .to_string()
                .contains("Day period is too short")
        );
    }

    #[test]
    fn test_config_validation_extreme_short_night() {
        // 30 minute night period (sunset 23:45, sunrise 00:15)
        let config = create_test_config(
            "23:45:00",
            "00:15:00",
            Some(5),
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());
        assert!(
            validate_config(&config)
                .unwrap_err()
                .to_string()
                .contains("Night period is too short")
        );
    }

    #[test]
    fn test_config_validation_extreme_temperature_values() {
        // Test minimum temperature boundary
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(MINIMUM_TEMP),
            Some(MAXIMUM_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // Test below minimum temperature
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(MINIMUM_TEMP - 1),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());

        // Test above maximum temperature
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(MAXIMUM_TEMP + 1),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_config_validation_extreme_gamma_values() {
        // Test minimum gamma boundary
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(MINIMUM_GAMMA),
            Some(MAXIMUM_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // Test below minimum gamma
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(MINIMUM_GAMMA - 0.1),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());

        // Test above maximum gamma
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(MAXIMUM_GAMMA + 0.1),
        );
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_config_validation_extreme_transition_durations() {
        // Test minimum transition duration
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(MINIMUM_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // Test maximum transition duration
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(MAXIMUM_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // Test below minimum (should fail validation)
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(MINIMUM_TRANSITION_DURATION - 1),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());

        // Test above maximum (should fail validation)
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(MAXIMUM_TRANSITION_DURATION + 1),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_config_validation_extreme_update_intervals() {
        // Test minimum update interval
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(MINIMUM_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // Test maximum update interval
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(120),
            Some(MAXIMUM_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // Test update interval longer than transition
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(30),
            Some(30 * 60 + 1),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());
        assert!(
            validate_config(&config)
                .unwrap_err()
                .to_string()
                .contains("longer than transition duration")
        );
    }

    #[test]
    fn test_config_validation_center_mode_overlapping() {
        // Center mode with transition duration that would overlap
        // Day period is about 11 hours (06:00-19:00), night is 13 hours
        // Transition of 60 minutes in center mode means 30 minutes each side
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(60),
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some("center"),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // But if we make the transition too long for center mode
        // Let's try a 22-hour transition in center mode (11 hours each side)
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(22 * 60),
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some("center"),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_config_validation_midnight_crossings() {
        // Sunset after midnight, sunrise in evening - valid but extreme
        let config = create_test_config(
            "01:00:00",
            "22:00:00",
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // Very late sunset, very early sunrise
        let config = create_test_config(
            "23:30:00",
            "00:30:00",
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_config_validation_invalid_time_formats() {
        // This should fail during parsing, not validation
        assert!(NaiveTime::parse_from_str("25:00:00", "%H:%M:%S").is_err());
        assert!(NaiveTime::parse_from_str("19:60:00", "%H:%M:%S").is_err());
    }

    #[test]
    fn test_config_validation_transition_overlap_detection() {
        // Test transition overlap detection with extreme short periods
        let config = create_test_config(
            "12:30:00",
            "12:00:00",
            Some(60),
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some("center"),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        // Should fail because day period is only 30 minutes, can't fit 1-hour center transition
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_config_validation_performance_warnings() {
        // Test configuration that should generate performance warnings
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(5),
            Some(5),
            Some(TEST_STANDARD_MODE), // Very frequent updates
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        // Should pass validation but might generate warnings (captured in logs)
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_default_config_file_creation() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("sunsetr.toml");

        Config::create_default_config(&config_path, None).unwrap();
        assert!(config_path.exists());

        let content = fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("start_hyprsunset"));
        assert!(content.contains("sunset"));
        assert!(content.contains("sunrise"));
        assert!(content.contains("night_temp"));
        assert!(content.contains("transition_mode"));
    }

    #[test]
    fn test_config_toml_parsing() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("test_config.toml");

        let config_content = r#"
start_hyprsunset = false
startup_transition = true
startup_transition_duration = 15
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = 3300
day_temp = 6000
night_gamma = 90.0
day_gamma = 100.0
transition_duration = 45
update_interval = 60
transition_mode = "finish_by"
"#;

        fs::write(&config_path, config_content).unwrap();
        let content = fs::read_to_string(&config_path).unwrap();
        let config: Config = toml::from_str(&content).unwrap();

        assert_eq!(config.start_hyprsunset, Some(false));
        assert_eq!(config.sunset, "19:00:00");
        assert_eq!(config.sunrise, "06:00:00");
        assert_eq!(config.night_temp, Some(3300));
        assert_eq!(config.transition_mode, Some("finish_by".to_string()));
    }

    #[test]
    fn test_config_malformed_toml() {
        let malformed_content = r#"
start_hyprsunset = false
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = "not_a_number"  # This should cause parsing to fail
"#;

        let result: Result<Config, _> = toml::from_str(malformed_content);
        assert!(result.is_err());
    }

    #[test]
    fn test_geo_toml_loading() {
        let temp_dir = tempdir().unwrap();
        let config_dir = temp_dir.path().join("sunsetr");
        fs::create_dir_all(&config_dir).unwrap();

        let config_path = config_dir.join("sunsetr.toml");
        let geo_path = config_dir.join("geo.toml");

        // Create main config without coordinates
        let config_content = r#"
start_hyprsunset = false
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = 3300
day_temp = 6500
transition_mode = "geo"
"#;
        fs::write(&config_path, config_content).unwrap();

        // Create geo.toml with coordinates
        let geo_content = r#"
# Geographic coordinates
latitude = 51.5074
longitude = -0.1278
"#;
        fs::write(&geo_path, geo_content).unwrap();

        // Load config from path - directly load with the path
        let config = Config::load_from_path(&config_path).unwrap();

        // Check that coordinates were loaded from geo.toml
        assert_eq!(config.latitude, Some(51.5074));
        assert_eq!(config.longitude, Some(-0.1278));
    }

    #[test]
    fn test_geo_toml_overrides_main_config() {
        let temp_dir = tempdir().unwrap();
        let config_dir = temp_dir.path().join("sunsetr");
        fs::create_dir_all(&config_dir).unwrap();

        let config_path = config_dir.join("sunsetr.toml");
        let geo_path = config_dir.join("geo.toml");

        // Create main config with coordinates
        let config_content = r#"
start_hyprsunset = false
sunset = "19:00:00"
sunrise = "06:00:00"
latitude = 40.7128
longitude = -74.0060
transition_mode = "geo"
"#;
        fs::write(&config_path, config_content).unwrap();

        // Create geo.toml with different coordinates
        let geo_content = r#"
latitude = 51.5074
longitude = -0.1278
"#;
        fs::write(&geo_path, geo_content).unwrap();

        // Load config directly from path (no env var needed)
        let config = Config::load_from_path(&config_path).unwrap();

        // Check that geo.toml coordinates override main config
        assert_eq!(config.latitude, Some(51.5074));
        assert_eq!(config.longitude, Some(-0.1278));
    }

    #[test]
    #[serial]
    fn test_update_geo_coordinates_with_geo_toml() {
        let temp_dir = tempdir().unwrap();
        let config_dir = temp_dir.path().join("sunsetr");
        fs::create_dir_all(&config_dir).unwrap();

        let config_path = config_dir.join("sunsetr.toml");
        let geo_path = config_dir.join("geo.toml");

        // Create main config
        let config_content = r#"
start_hyprsunset = false
sunset = "19:00:00"
sunrise = "06:00:00"
transition_mode = "manual"
"#;
        fs::write(&config_path, config_content).unwrap();

        // Create empty geo.toml
        fs::write(&geo_path, "").unwrap();

        // Save and restore XDG_CONFIG_HOME
        let original = std::env::var("XDG_CONFIG_HOME").ok();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        // Update coordinates
        Config::update_config_with_geo_coordinates(52.5200, 13.4050).unwrap();

        // Restore original
        unsafe {
            match original {
                Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }

        // Check that geo.toml was updated
        let geo_content = fs::read_to_string(&geo_path).unwrap();
        assert!(geo_content.contains("latitude = 52.52"));
        assert!(geo_content.contains("longitude = 13.405"));

        // Check that main config transition_mode was updated
        let main_content = fs::read_to_string(&config_path).unwrap();
        assert!(main_content.contains("transition_mode = \"geo\""));
    }

    #[test]
    fn test_malformed_geo_toml_fallback() {
        let temp_dir = tempdir().unwrap();
        let config_dir = temp_dir.path().join("sunsetr");
        fs::create_dir_all(&config_dir).unwrap();

        let config_path = config_dir.join("sunsetr.toml");
        let geo_path = config_dir.join("geo.toml");

        // Create main config with coordinates
        let config_content = r#"
start_hyprsunset = false
sunset = "19:00:00"
sunrise = "06:00:00"
latitude = 40.7128
longitude = -74.0060
transition_mode = "geo"
"#;
        fs::write(&config_path, config_content).unwrap();

        // Create malformed geo.toml
        let geo_content = r#"
latitude = "not a number"
longitude = -0.1278
"#;
        fs::write(&geo_path, geo_content).unwrap();

        // Load config - should use main config coordinates
        let config = Config::load_from_path(&config_path).unwrap();

        // Check that main config coordinates were used
        assert_eq!(config.latitude, Some(40.7128));
        assert_eq!(config.longitude, Some(-74.0060));
    }

    #[test]
    #[serial]
    fn test_geo_toml_exists_before_config_creation() {
        let temp_dir = tempdir().unwrap();
        let config_dir = temp_dir.path().join("sunsetr");
        fs::create_dir_all(&config_dir).unwrap();

        let config_path = config_dir.join("sunsetr.toml");
        let geo_path = config_dir.join("geo.toml");

        // Create empty geo.toml BEFORE creating config
        fs::write(&geo_path, "").unwrap();

        // Save and restore XDG_CONFIG_HOME
        let original = std::env::var("XDG_CONFIG_HOME").ok();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        // Create config with coordinates (simulating --geo command)
        Config::create_default_config(&config_path, Some((52.5200, 13.4050, "Berlin".to_string())))
            .unwrap();

        // Restore original
        unsafe {
            match original {
                Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }

        // Check that coordinates went to geo.toml
        let geo_content = fs::read_to_string(&geo_path).unwrap();
        assert!(geo_content.contains("latitude = 52.52"));
        assert!(geo_content.contains("longitude = 13.405"));

        // Check that main config does NOT have coordinates
        let main_content = fs::read_to_string(&config_path).unwrap();
        assert!(!main_content.contains("latitude = 52.52"));
        assert!(!main_content.contains("longitude = 13.405"));

        // But it should have geo transition mode
        assert!(main_content.contains("transition_mode = \"geo\""));
    }

    #[test]
    #[serial]
    fn test_missing_coordinates_auto_save() {
        let temp_dir = tempdir().unwrap();
        let config_dir = temp_dir.path().join("sunsetr");
        fs::create_dir_all(&config_dir).unwrap();

        let config_path = config_dir.join("sunsetr.toml");

        // Create config with geo mode but NO coordinates
        let config_content = r#"
start_hyprsunset = false
sunset = "19:00:00"
sunrise = "06:00:00"
transition_mode = "geo"
"#;
        fs::write(&config_path, config_content).unwrap();

        // Save and restore XDG_CONFIG_HOME
        let original = std::env::var("XDG_CONFIG_HOME").ok();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        // Load config - should trigger coordinate detection and auto-save
        let config = Config::load();

        // Restore original
        unsafe {
            match original {
                Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }

        // If detection succeeded, config should have coordinates
        if let Ok(loaded_config) = config {
            assert!(loaded_config.latitude.is_some());
            assert!(loaded_config.longitude.is_some());

            // Check that coordinates were saved to the file
            let updated_content = fs::read_to_string(&config_path).unwrap();
            assert!(updated_content.contains("latitude = "));
            assert!(updated_content.contains("longitude = "));
        }
        // If detection failed, the load would have exited, so we can't test that path
    }
}
