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
//! The configuration supports manual sunset/sunrise times, automatic geographic
//! location-based calculations, and static mode with constant values:
//!
//! ```toml
//! #[Sunsetr configuration]
//! backend = "auto"                  # "auto", "hyprland", or "wayland"
//! startup_transition = true         # Smooth startup transition
//! startup_transition_duration = 1   # Seconds (1-60)
//! transition_mode = "geo"           # "geo", "finish_by", "start_at", "center", or "static"
//!
//! #[Time-based configuration]
//! night_temp = 3300                 # Color temperature during night (1000-20000) Kelvin
//! day_temp = 6500                   # Color temperature during day (1000-20000) Kelvin
//! night_gamma = 90.0                # Gamma percentage for night (10-100%)
//! day_gamma = 100.0                 # Gamma percentage for day (10-100%)
//! update_interval = 60              # Update frequency during transitions in seconds (10-300)
//!
//! #[Static configuration]
//! static_temp = 6500                # Color temperature for static mode (1000-20000) Kelvin
//! static_gamma = 100.0              # Gamma percentage for static mode (10-100%)
//!
//! #[Manual transitions]
//! sunset = "19:00:00"               # Time to transition to night mode (HH:MM:SS) - ignored in geo mode
//! sunrise = "06:00:00"              # Time to transition to day mode (HH:MM:SS) - ignored in geo mode
//! transition_duration = 45          # Transition duration in minutes (5-120)
//!
//! #[Geolocation-based transitions]
//! latitude = 40.7128                # Geographic latitude
//! longitude = -74.0060              # Geographic longitude
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

pub mod builder;
pub mod loading;
pub mod validation;
pub mod watcher;

use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

use crate::constants::*;

// Re-export public API
pub use builder::{create_default_config, update_coordinates};
pub use loading::{get_config_path, get_custom_config_dir, load, load_from_path, set_config_dir};
pub use watcher::start_config_watcher;

/// Display mode for intelligent configuration display.
///
/// This enum determines how the configuration should be displayed based on the
/// active transition mode, allowing only relevant fields to be shown.
#[derive(Debug, Clone, PartialEq)]
enum DisplayMode {
    /// Static mode - shows constant temperature and gamma values
    Static,
    /// Time-based mode with geographic calculations
    TimeBasedGeo,
    /// Time-based mode with manual sunset/sunrise times
    TimeBasedManual { mode: String },
}

/// Geographic configuration structure for storing coordinates separately.
///
/// This structure represents the optional geo.toml file that can store
/// latitude and longitude separately from the main configuration file.
/// This allows users to version control their main settings while keeping
/// location data private.
#[derive(Debug, Deserialize, Clone)]
pub(crate) struct GeoConfig {
    /// Geographic latitude in degrees (-90 to +90)
    pub(crate) latitude: Option<f64>,
    /// Geographic longitude in degrees (-180 to +180)
    pub(crate) longitude: Option<f64>,
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
    /// Auto-detection priority: Hyprland (native) → Wayland → error.
    /// This is the recommended setting for most users.
    Auto,
    /// Native Hyprland backend using hyprland-ctm-control-v1 protocol.
    ///
    /// Directly controls CTM (Color Transform Matrix) without external processes.
    /// Provides smooth animations via Hyprland's built-in CTM animation system.
    Hyprland,
    /// Hyprsunset backend using the hyprsunset daemon.
    ///
    /// Manages hyprsunset as a child process or connects to existing instance.
    /// Legacy backend that will be deprecated once native backend is stable.
    Hyprsunset,
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
            Backend::Hyprsunset => "hyprsunset",
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
/// - **Backend Control**: `backend` (applies to all modes)
/// - **Startup Behavior**: `startup_transition`, `startup_transition_duration` (applies to all modes)
/// - **Mode Selection**: `transition_mode` ("geo", "finish_by", "start_at", "center", or "static")
/// - **Time-based Configuration**: `night_temp`, `day_temp`, `night_gamma`, `day_gamma`, `update_interval` (used by time-based modes: geo, finish_by, start_at, center)
/// - **Static Configuration**: `static_temp`, `static_gamma` (only used when `transition_mode = "static"`)
/// - **Manual Transitions**: `sunset`, `sunrise`, `transition_duration` (only used for manual time-based modes: "finish_by", "start_at", "center")
/// - **Geolocation-based Transitions**: `latitude`, `longitude` (only used when `transition_mode = "geo"`)
///
/// ## Validation
///
/// All configuration values are validated during loading to ensure they fall
/// within acceptable ranges and don't create impossible configurations (e.g.,
/// overlapping transitions, insufficient time periods).
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Config {
    /// Backend implementation to use for color temperature control.
    ///
    /// Determines how sunsetr communicates with the compositor.
    /// Defaults to `Auto` which detects the appropriate backend automatically.
    pub backend: Option<Backend>,

    /// Whether to enable smooth transitions (new name for startup_transition).
    ///
    /// When `true`, sunsetr will gradually transition from day values to the
    /// current target state over the startup transition duration.
    /// When `false`, sunsetr applies the correct state immediately.
    pub smoothing: Option<bool>, // whether to enable smooth transitions
    /// Duration for startup smooth transitions in seconds (new name for startup_transition_duration).
    pub startup_duration: Option<f64>, // seconds for startup transition (supports decimals like 0.5)
    /// Duration for shutdown smooth transitions in seconds.
    pub shutdown_duration: Option<f64>, // seconds for shutdown transition (supports decimals like 0.5)

    /// Whether to enable smooth animated startup transitions (deprecated - use smoothing instead).
    ///
    /// When `true`, sunsetr will gradually transition from day values to the
    /// current target state over the startup transition duration.
    /// When `false`, sunsetr applies the correct state immediately.
    pub startup_transition: Option<bool>, // whether to enable smooth startup transition (deprecated)
    pub startup_transition_duration: Option<f64>, // seconds for startup transition (deprecated - use startup_duration instead)

    /// Whether to start the hyprsunset daemon (deprecated - use backend selection instead).
    ///
    /// This field is deprecated and ignored. Use `backend = "hyprsunset"` to use the hyprsunset backend.
    #[serde(default, skip_serializing)]
    pub start_hyprsunset: Option<bool>,
    pub adaptive_interval: Option<u64>, // milliseconds minimum between updates during transitions (1-1000)
    pub transition_mode: Option<String>, // "finish_by", "start_at", "center", "geo", or "static"
    pub night_temp: Option<u32>,
    pub day_temp: Option<u32>,
    pub night_gamma: Option<f32>,
    pub day_gamma: Option<f32>,
    pub update_interval: Option<u64>, // seconds during transition
    pub static_temp: Option<u32>,     // Temperature for static mode only
    pub static_gamma: Option<f32>,    // Gamma for static mode only
    pub sunset: Option<String>,
    pub sunrise: Option<String>,
    pub transition_duration: Option<u64>, // minutes
    pub latitude: Option<f64>,            // Geographic latitude for geo mode
    pub longitude: Option<f64>,           // Geographic longitude for geo mode
}

impl Config {
    /// Migrate legacy field names to new ones for backward compatibility.
    ///
    /// This method handles the transition from old field names to new ones:
    /// - `startup_transition` → `smoothing`
    /// - `startup_transition_duration` → `startup_duration`
    /// - Sets `shutdown_duration` to match `startup_duration` if not specified
    pub fn migrate_legacy_fields(&mut self) {
        // Check if we need to show deprecation warnings
        let has_deprecated_fields = (self.smoothing.is_none() && self.startup_transition.is_some())
            || (self.startup_duration.is_none() && self.startup_transition_duration.is_some())
            || self.start_hyprsunset.is_some();

        // Add spacing before warnings if we have any deprecated fields
        if has_deprecated_fields {
            log_pipe!();
        }

        // Migrate startup_transition → smoothing
        if self.smoothing.is_none() && self.startup_transition.is_some() {
            self.smoothing = self.startup_transition;
            if self.startup_transition.is_some() {
                log_warning!(
                    "Config field 'startup_transition' is deprecated. Please use 'smoothing' instead."
                );
            }
        }

        // Migrate startup_transition_duration → startup_duration
        if self.startup_duration.is_none() && self.startup_transition_duration.is_some() {
            self.startup_duration = self.startup_transition_duration;
            if self.startup_transition_duration.is_some() {
                log_warning!(
                    "Config field 'startup_transition_duration' is deprecated. Please use 'startup_duration' instead."
                );
            }
        }

        // Default shutdown_duration to startup_duration if not specified
        if self.shutdown_duration.is_none() && self.startup_duration.is_some() {
            self.shutdown_duration = self.startup_duration;
        }

        // Warn about deprecated start_hyprsunset field
        if self.start_hyprsunset.is_some() {
            log_warning!(
                "Config field 'start_hyprsunset' is deprecated and will be ignored.\n\
                ┃ Please remove it from your configuration and use backend selection instead:\n\
                ┃ • Use backend=\"hyprsunset\" for the hyprsunset daemon backend\n\
                ┃ • Use backend=\"hyprland\" for the native CTM protocol (recommended)\n\
                ┃ • Use backend=\"wayland\" for the Wayland backend\n\
                ┃ • Use backend=\"auto\" for automatic detection"
            );
            // Clear the field after warning
            self.start_hyprsunset = None;
        }
    }

    /// Get the path to the geo.toml file (in the same directory as sunsetr.toml)
    pub fn get_geo_path() -> Result<PathBuf> {
        Ok(loading::get_config_base_dir()?.join("geo.toml"))
    }

    /// Load configuration using the module's load function
    pub fn load() -> Result<Self> {
        load()
    }

    /// Load from path using the module's load_from_path function
    pub fn load_from_path(path: &PathBuf) -> Result<Self> {
        load_from_path(path)
    }

    /// Get configuration path using the module's get_config_path function
    pub fn get_config_path() -> Result<PathBuf> {
        get_config_path()
    }

    /// Create default config using the module's create_default_config function
    pub fn create_default_config(path: &PathBuf, coords: Option<(f64, f64, String)>) -> Result<()> {
        create_default_config(path, coords)
    }

    /// Update config with geo coordinates using the module's function
    pub fn update_coordinates(latitude: f64, longitude: f64) -> Result<()> {
        update_coordinates(latitude, longitude)
    }

    /// Get the currently active preset name, if any
    pub fn get_active_preset() -> Result<Option<String>> {
        loading::get_active_preset()
    }

    /// Clear the active preset marker
    pub fn clear_active_preset() -> Result<()> {
        loading::clear_active_preset()
    }

    pub fn log_config(&self, resolved_backend: Option<crate::backend::BackendType>) {
        // Detect configuration source (preset vs default)
        // Cache the active preset result to avoid redundant calls
        let active_preset = Self::get_active_preset().ok().flatten();
        let (config_source, is_preset) = if let Some(ref preset_name) = active_preset {
            (format!("preset '{}'", preset_name), true)
        } else {
            ("default configuration".to_string(), false)
        };

        // Detect display mode for intelligent field filtering
        let display_mode = self.detect_display_mode();

        log_block_start!("Loaded {}", config_source);

        // Check for geo.toml in the appropriate directory
        // For presets, check in the preset directory; for default, check main config dir
        if matches!(display_mode, DisplayMode::TimeBasedGeo) {
            let geo_path = if is_preset {
                // For presets, check if geo.toml exists in the preset directory
                if let Some(ref preset_name) = active_preset {
                    if let Ok(config_path) = Self::get_config_path() {
                        if let Some(config_dir) = config_path.parent() {
                            config_dir
                                .join("presets")
                                .join(preset_name)
                                .join("geo.toml")
                        } else {
                            PathBuf::from("geo.toml")
                        }
                    } else {
                        PathBuf::from("geo.toml")
                    }
                } else {
                    PathBuf::from("geo.toml")
                }
            } else {
                // For default config, use the standard geo path
                Self::get_geo_path().unwrap_or_else(|_| PathBuf::from("~/.config/sunsetr/geo.toml"))
            };

            if geo_path.exists() {
                log_indented!("Loaded coordinates from geo.toml");
            }
        }

        // Always show backend and mode
        let backend = self.backend.as_ref().unwrap_or(&DEFAULT_BACKEND);
        let backend_display = format!(
            "Backend: {}",
            match backend {
                Backend::Auto => {
                    if let Some(resolved) = resolved_backend {
                        match resolved {
                            crate::backend::BackendType::Hyprland => "Auto (Hyprland)",
                            crate::backend::BackendType::Wayland => "Auto (Wayland)",
                            crate::backend::BackendType::Hyprsunset => {
                                unreachable!(
                                    "Auto-detection should never select Hyprsunset backend"
                                )
                            }
                        }
                    } else {
                        "Auto"
                    }
                }
                Backend::Hyprland => "Hyprland",
                Backend::Hyprsunset => "Hyprsunset",
                Backend::Wayland => "Wayland",
            }
        );

        log_indented!("{}", backend_display);

        // Show mode with user-friendly labels
        let mode_display = match display_mode {
            DisplayMode::Static => "Mode: Static (constant values)".to_string(),
            DisplayMode::TimeBasedGeo => "Mode: Time-based (geo)".to_string(),
            DisplayMode::TimeBasedManual { ref mode } => {
                format!("Mode: Time-based manual ({})", mode)
            }
        };
        log_indented!("{}", mode_display);

        // Mode-specific field display
        match display_mode {
            DisplayMode::Static => {
                // Static mode: show temp and gamma inline
                log_indented!(
                    "Constant: {}K @ {}% gamma",
                    self.static_temp.unwrap_or(DEFAULT_DAY_TEMP),
                    self.static_gamma.unwrap_or(DEFAULT_DAY_GAMMA)
                );
            }
            DisplayMode::TimeBasedGeo => {
                // Geo mode: show coordinates and day/night values, skip manual times
                if let (Some(lat), Some(lon)) = (self.latitude, self.longitude) {
                    let lat_dir = if lat >= 0.0 { "N" } else { "S" };
                    let lon_dir = if lon >= 0.0 { "E" } else { "W" };
                    log_indented!(
                        "Location: {:.3}°{}, {:.3}°{}",
                        lat.abs(),
                        lat_dir,
                        lon.abs(),
                        lon_dir
                    );
                }

                log_indented!(
                    "Night: {}K @ {}% gamma",
                    self.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP),
                    self.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA)
                );
                log_indented!(
                    "Day: {}K @ {}% gamma",
                    self.day_temp.unwrap_or(DEFAULT_DAY_TEMP),
                    self.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA)
                );
                log_indented!(
                    "Update interval: {} seconds",
                    self.update_interval.unwrap_or(DEFAULT_UPDATE_INTERVAL)
                );
            }
            DisplayMode::TimeBasedManual { .. } => {
                // Manual mode: show sunset/sunrise times, transition duration, day/night values
                if let Some(ref sunset) = self.sunset {
                    log_indented!("Sunset: {}", sunset);
                }
                if let Some(ref sunrise) = self.sunrise {
                    log_indented!("Sunrise: {}", sunrise);
                }
                log_indented!(
                    "Transition duration: {} minutes",
                    self.transition_duration
                        .unwrap_or(DEFAULT_TRANSITION_DURATION)
                );
                log_indented!(
                    "Night: {}K @ {}% gamma",
                    self.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP),
                    self.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA)
                );
                log_indented!(
                    "Day: {}K @ {}% gamma",
                    self.day_temp.unwrap_or(DEFAULT_DAY_TEMP),
                    self.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA)
                );
                log_indented!(
                    "Update interval: {} seconds",
                    self.update_interval.unwrap_or(DEFAULT_UPDATE_INTERVAL)
                );
            }
        }

        // Show smoothing settings only if backend supports it and it's enabled
        // Only Wayland backend supports smooth transitions
        let backend_supports_smoothing = matches!(backend, Backend::Wayland);
        let smoothing_enabled = self.smoothing.unwrap_or(DEFAULT_SMOOTHING);

        if backend_supports_smoothing && smoothing_enabled {
            let startup_duration = self.startup_duration.unwrap_or(DEFAULT_STARTUP_DURATION);
            let shutdown_duration = self.shutdown_duration.unwrap_or(DEFAULT_SHUTDOWN_DURATION);

            // Only show durations that are >= 0.1 (below that is instant)
            let show_startup = startup_duration >= 0.1;
            let show_shutdown = shutdown_duration >= 0.1;

            if show_startup {
                // Format duration nicely - show as integer if it's a whole number
                let duration_str = if startup_duration.fract() == 0.0 {
                    format!("{}", startup_duration as u64)
                } else {
                    format!("{:.1}", startup_duration)
                };
                let duration_label = if startup_duration == 1.0 {
                    "second"
                } else {
                    "seconds"
                };
                log_indented!("Startup duration: {} {}", duration_str, duration_label);
            }

            if show_shutdown {
                // Format duration nicely - show as integer if it's a whole number
                let duration_str = if shutdown_duration.fract() == 0.0 {
                    format!("{}", shutdown_duration as u64)
                } else {
                    format!("{:.1}", shutdown_duration)
                };
                let duration_label = if shutdown_duration == 1.0 {
                    "second"
                } else {
                    "seconds"
                };
                log_indented!("Shutdown duration: {} {}", duration_str, duration_label);
            }

            // Show adaptive interval only if at least one duration is shown
            if show_startup || show_shutdown {
                let adaptive_interval = self.adaptive_interval.unwrap_or(DEFAULT_ADAPTIVE_INTERVAL);
                log_indented!("Adaptive interval: {}ms", adaptive_interval);
            }
        }
    }

    /// Detect the display mode based on transition_mode configuration
    fn detect_display_mode(&self) -> DisplayMode {
        match self.transition_mode.as_deref() {
            Some("static") => DisplayMode::Static,
            Some("geo") => DisplayMode::TimeBasedGeo,
            Some(mode) => DisplayMode::TimeBasedManual {
                mode: mode.to_string(),
            },
            None => DisplayMode::TimeBasedManual {
                mode: "center".to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::validation::validate_config;
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
            backend: Some(Backend::Auto),
            smoothing: Some(false),
            startup_duration: Some(10.0),
            shutdown_duration: Some(10.0),
            startup_transition: Some(false),
            startup_transition_duration: Some(10.0),
            start_hyprsunset: None,
            adaptive_interval: None,
            latitude: None,
            longitude: None,
            sunset: Some(sunset.to_string()),
            sunrise: Some(sunrise.to_string()),
            night_temp,
            day_temp,
            night_gamma,
            day_gamma,
            static_temp: None,
            static_gamma: None,
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

        if let Err(e) = &result {
            eprintln!("Config::load() failed: {:?}", e);
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

        // Valid: Hyprland backend
        config.backend = Some(Backend::Hyprland);
        assert!(validate_config(&config).is_ok());

        // Valid: Hyprsunset backend
        config.backend = Some(Backend::Hyprsunset);
        assert!(validate_config(&config).is_ok());

        // Valid: Wayland backend
        config.backend = Some(Backend::Wayland);
        assert!(validate_config(&config).is_ok());
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
                .contains("longer than transition_duration")
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
        use chrono::NaiveTime;
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
        // Test configuration with edge case values
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(5),  // Short transition duration (will generate warning)
            Some(10), // Minimum allowed update interval
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        // Should pass validation with minimum allowed update_interval
        assert!(validate_config(&config).is_ok());

        // Test that update_interval below minimum fails
        let config_too_low = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(5),
            Some(5), // Below minimum
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        // Should fail validation with update_interval too low
        assert!(validate_config(&config_too_low).is_err());
    }

    #[test]
    fn test_default_config_file_creation() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("sunsetr.toml");

        Config::create_default_config(&config_path, None).unwrap();
        assert!(config_path.exists());

        let content = fs::read_to_string(&config_path).unwrap();
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

        assert_eq!(config.sunset, Some("19:00:00".to_string()));
        assert_eq!(config.sunrise, Some("06:00:00".to_string()));
        assert_eq!(config.night_temp, Some(3300));
        assert_eq!(config.transition_mode, Some("finish_by".to_string()));
    }

    #[test]
    fn test_config_malformed_toml() {
        let malformed_content = r#"
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
        Config::update_coordinates(52.5200, 13.4050).unwrap();

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

        // Create config with coordinates (simulating geo command)
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
}
