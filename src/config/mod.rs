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
//! #[Backend]
//! backend = "auto"         # Backend to use: "auto", "hyprland", "hyprsunset", "wayland"
//! transition_mode = "geo"  # Select: "geo", "finish_by", "start_at", "center", "static"
//!
//! #[Smoothing]
//! smoothing = true         # Enable smooth transitions during startup and exit
//! startup_duration = 0.5   # Duration of smooth startup in seconds (0.1-60 | 0 = instant)
//! shutdown_duration = 0.5  # Duration of smooth shutdown in seconds (0.1-60 | 0 = instant)
//! adaptive_interval = 1    # Adaptive interval base for smooth transitions (1-1000)ms
//!
//! #[Time-based config]
//! night_temp = 3300        # Color temperature during night (1000-20000) Kelvin
//! day_temp = 6500          # Color temperature during day (1000-20000) Kelvin
//! night_gamma = 90         # Gamma percentage for night (10-100%)
//! day_gamma = 100          # Gamma percentage for day (10-100%)
//! update_interval = 60     # Update frequency during transitions in seconds (10-300)
//!
//! #[Static config]
//! static_temp = 6500       # Color temperature for static mode (1000-20000) Kelvin
//! static_gamma = 100       # Gamma percentage for static mode (10-100%)
//!
//! #[Manual transitions]
//! sunset = "19:00:00"      # Time for manual sunset calculations (HH:MM:SS)
//! sunrise = "06:00:00"     # Time for manual sunrise calculations (HH:MM:SS)
//! transition_duration = 45 # Transition duration in minutes (5-120)
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

use crate::common::constants::*;

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
mod tests;
