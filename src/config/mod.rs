//! Configuration system: load, validate, and migrate `sunsetr.toml`, apply defaults, and resolve
//! geographic coordinates.
//!
//! `sunsetr.toml` is resolved from the `--config` directory when set, otherwise
//! `XDG_CONFIG_HOME/sunsetr/sunsetr.toml`. A default is created there if none exists.

pub mod builder;
pub mod loading;
pub mod validation;
pub mod watcher;

use anyhow::Result;
use serde::Deserialize;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::common::constants::*;

/// Update interval strategy for sunset/sunrise transitions.
///
/// Either a fixed interval in seconds, or an adaptive mode that sizes each
/// step to the combined perceptual rate of change in temperature and gamma.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateInterval {
    /// Fixed update interval in seconds (10-300).
    Fixed(u64),
    /// Adaptive interval that adjusts based on the smoothstep derivative and
    /// combined mired/gamma range to keep each step below the just-noticeable
    /// difference threshold.
    Adaptive,
}

impl fmt::Display for UpdateInterval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateInterval::Fixed(secs) => write!(f, "{} seconds", secs),
            UpdateInterval::Adaptive => write!(f, "auto"),
        }
    }
}

impl<'de> Deserialize<'de> for UpdateInterval {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct UpdateIntervalVisitor;

        impl<'de> serde::de::Visitor<'de> for UpdateIntervalVisitor {
            type Value = UpdateInterval;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an integer (10-300) or the string \"auto\"")
            }

            fn visit_i64<E>(self, v: i64) -> Result<UpdateInterval, E>
            where
                E: serde::de::Error,
            {
                if v < 0 {
                    Err(E::custom(format!(
                        "update_interval cannot be negative: {}",
                        v
                    )))
                } else {
                    Ok(UpdateInterval::Fixed(v as u64))
                }
            }

            fn visit_u64<E>(self, v: u64) -> Result<UpdateInterval, E>
            where
                E: serde::de::Error,
            {
                Ok(UpdateInterval::Fixed(v))
            }

            fn visit_str<E>(self, v: &str) -> Result<UpdateInterval, E>
            where
                E: serde::de::Error,
            {
                match v.to_lowercase().as_str() {
                    "auto" => Ok(UpdateInterval::Adaptive),
                    _ => Err(E::custom(format!(
                        "unknown update_interval value: \"{}\". Expected an integer (10-300) or \"auto\"",
                        v
                    ))),
                }
            }
        }

        deserializer.deserialize_any(UpdateIntervalVisitor)
    }
}

pub use loading::{get_custom_config_dir, set_config_dir};
pub use watcher::start_config_watcher;

/// Which configuration fields `log_config` shows, based on `transition_mode`.
#[derive(Debug, Clone, PartialEq)]
enum DisplayMode {
    Static,
    TimeBasedGeo,
    TimeBasedManual { mode: String },
}

/// The optional `geo.toml`, storing coordinates apart from `sunsetr.toml`
/// so the main config can be shared while location data stays private.
#[derive(Debug, Deserialize, Clone)]
pub(crate) struct GeoConfig {
    pub(crate) latitude: Option<f64>,
    pub(crate) longitude: Option<f64>,
}

/// Backend used to control display color temperature.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Auto,
    Hyprland,
    Hyprsunset,
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

/// All settings deserialized from `sunsetr.toml`.
///
/// Every field is optional, and unset fields fall back to defaults on load. Which fields apply
/// depends on `transition_mode`.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Config {
    // Backend
    pub backend: Option<Backend>,
    pub transition_mode: Option<String>,

    // Smoothing
    pub smoothing: Option<bool>,
    pub startup_duration: Option<f64>,
    pub shutdown_duration: Option<f64>,
    pub adaptive_interval: Option<u64>,

    // Time-based
    pub night_temp: Option<u32>,
    pub day_temp: Option<u32>,
    pub night_gamma: Option<f64>,
    pub day_gamma: Option<f64>,
    pub update_interval: Option<UpdateInterval>,

    // Static
    pub static_temp: Option<u32>,
    pub static_gamma: Option<f64>,

    // Manual transitions
    pub sunset: Option<String>,
    pub sunrise: Option<String>,
    pub transition_duration: Option<u64>,

    // Geolocation
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,

    // Deprecated and ignored
    #[serde(default, skip_serializing)]
    pub start_hyprsunset: Option<bool>,
    pub startup_transition: Option<bool>,
    pub startup_transition_duration: Option<f64>,
}

impl Config {
    /// Migrate deprecated fields: `startup_transition` to `smoothing`, `startup_transition_duration`
    /// to `startup_duration`, and `shutdown_duration` defaulted from `startup_duration` when unset.
    pub fn migrate_legacy_fields(&mut self) {
        let has_deprecated_fields = (self.smoothing.is_none() && self.startup_transition.is_some())
            || (self.startup_duration.is_none() && self.startup_transition_duration.is_some())
            || self.start_hyprsunset.is_some();

        if has_deprecated_fields {
            log_pipe!();
        }

        if self.smoothing.is_none() && self.startup_transition.is_some() {
            self.smoothing = self.startup_transition;
            log_warning!(
                "Config field 'startup_transition' is deprecated. Please use 'smoothing' instead."
            );
        }

        if self.startup_duration.is_none() && self.startup_transition_duration.is_some() {
            self.startup_duration = self.startup_transition_duration;
            log_warning!(
                "Config field 'startup_transition_duration' is deprecated. Please use 'startup_duration' instead."
            );
        }

        if self.shutdown_duration.is_none() && self.startup_duration.is_some() {
            self.shutdown_duration = self.startup_duration;
        }

        if self.start_hyprsunset.is_some() {
            log_warning!("Config field 'start_hyprsunset' is deprecated and will be ignored.");
            log_indented!("Remove it from your configuration and use backend selection instead:");
            for option in [
                "• Use backend=\"hyprsunset\" for the hyprsunset process backend",
                "• Use backend=\"hyprland\" for the native CTM protocol (recommended)",
                "• Use backend=\"wayland\" for the Wayland backend",
                "• Use backend=\"auto\" for automatic detection",
            ] {
                log_indented!(option);
            }
            self.start_hyprsunset = None;
        }
    }

    /// Path to geo.toml, alongside sunsetr.toml.
    pub fn get_geo_path() -> Result<PathBuf> {
        Ok(loading::get_config_base_dir()?.join("geo.toml"))
    }

    pub fn load() -> Result<Self> {
        loading::load()
    }

    pub fn load_from_path(path: &Path) -> Result<Self> {
        loading::load_from_path(path)
    }

    pub fn get_config_path() -> Result<PathBuf> {
        loading::get_config_path()
    }

    pub fn create_default_config(path: &Path, coords: Option<(f64, f64, String)>) -> Result<()> {
        builder::create_default_config(path, coords)
    }

    pub fn update_coordinates(latitude: f64, longitude: f64) -> Result<()> {
        builder::update_coordinates(latitude, longitude)
    }

    pub fn log_config(&self, resolved_backend: Option<crate::backend::BackendType>) {
        let active_preset = crate::state::preset::get_active_preset().ok().flatten();
        let (config_source, is_preset) = if let Some(ref preset_name) = active_preset {
            (format!("preset '{}'", preset_name), true)
        } else {
            ("default configuration".to_string(), false)
        };

        let display_mode = self.detect_display_mode();

        log_block_start!("Loaded {}", config_source);

        if matches!(display_mode, DisplayMode::TimeBasedGeo) {
            let geo_path = if is_preset {
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
                Self::get_geo_path().unwrap_or_else(|_| PathBuf::from("~/.config/sunsetr/geo.toml"))
            };

            if geo_path.exists() {
                log_indented!("Loaded coordinates from geo.toml");
            }
        }

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

        let mode_display = match display_mode {
            DisplayMode::Static => "Mode: Static (constant values)".to_string(),
            DisplayMode::TimeBasedGeo => "Mode: Time-based (geo)".to_string(),
            DisplayMode::TimeBasedManual { ref mode } => {
                format!("Mode: Time-based manual ({})", mode)
            }
        };
        log_indented!("{}", mode_display);

        match display_mode {
            DisplayMode::Static => {
                log_indented!(
                    "Constant: {}K @ {}% gamma",
                    self.static_temp.unwrap_or(DEFAULT_DAY_TEMP),
                    self.static_gamma.unwrap_or(DEFAULT_DAY_GAMMA)
                );
            }
            DisplayMode::TimeBasedGeo => {
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
                    "Update interval: {}",
                    self.update_interval
                        .as_ref()
                        .map_or_else(|| UpdateInterval::Adaptive.to_string(), |v| v.to_string(),)
                );
            }
            DisplayMode::TimeBasedManual { .. } => {
                if let Some(ref sunset) = self.sunset {
                    log_indented!("Sunset: {}", sunset);
                }
                if let Some(ref sunrise) = self.sunrise {
                    log_indented!("Sunrise: {}", sunrise);
                }
                log_indented!(
                    "Transition duration: {} minutes",
                    self.transition_duration
                        .unwrap_or(DEFAULT_TRANSITION_DURATION_MIN)
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
                    "Update interval: {}",
                    self.update_interval
                        .as_ref()
                        .map_or_else(|| UpdateInterval::Adaptive.to_string(), |v| v.to_string(),)
                );
            }
        }

        let backend_supports_smoothing = matches!(backend, Backend::Wayland);
        let smoothing_enabled = self.smoothing.unwrap_or(DEFAULT_SMOOTHING);

        if backend_supports_smoothing && smoothing_enabled {
            let startup_duration = self
                .startup_duration
                .unwrap_or(DEFAULT_STARTUP_DURATION_SEC);
            let shutdown_duration = self
                .shutdown_duration
                .unwrap_or(DEFAULT_SHUTDOWN_DURATION_SEC);
            let show_startup = startup_duration >= 0.1;
            let show_shutdown = shutdown_duration >= 0.1;

            if show_startup {
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

            if show_startup || show_shutdown {
                let adaptive_interval = self
                    .adaptive_interval
                    .unwrap_or(DEFAULT_ADAPTIVE_INTERVAL_MS);
                log_indented!("Adaptive interval: {}ms", adaptive_interval);
            }
        }
    }

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
