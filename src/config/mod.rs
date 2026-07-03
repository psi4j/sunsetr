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
    TimeBasedManual { mode: TransitionMode },
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

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Backend::Auto => "auto",
            Backend::Hyprland => "hyprland",
            Backend::Hyprsunset => "hyprsunset",
            Backend::Wayland => "wayland",
        })
    }
}

impl std::str::FromStr for Backend {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "auto" => Backend::Auto,
            "hyprland" => Backend::Hyprland,
            "hyprsunset" => Backend::Hyprsunset,
            "wayland" => Backend::Wayland,
            _ => anyhow::bail!(
                "'{s}' is not a valid backend\nUse: auto, hyprland, hyprsunset, or wayland"
            ),
        })
    }
}

/// How transitions are placed around sunset and sunrise, or a fixed static color.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TransitionMode {
    #[default]
    Geo,
    FinishBy,
    StartAt,
    Center,
    Static,
}

impl fmt::Display for TransitionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TransitionMode::Geo => "geo",
            TransitionMode::FinishBy => "finish_by",
            TransitionMode::StartAt => "start_at",
            TransitionMode::Center => "center",
            TransitionMode::Static => "static",
        })
    }
}

impl std::str::FromStr for TransitionMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "geo" => TransitionMode::Geo,
            "finish_by" => TransitionMode::FinishBy,
            "start_at" => TransitionMode::StartAt,
            "center" => TransitionMode::Center,
            "static" => TransitionMode::Static,
            _ => anyhow::bail!(
                "'{s}' is not a valid transition mode\nUse: geo, finish_by, start_at, center, or static"
            ),
        })
    }
}

/// All settings as deserialized from `sunsetr.toml`, before defaults.
///
/// The sole serde target. `None` means the key was absent in the TOML.
/// [`RawConfig::resolve`] validates and applies defaults once, producing the
/// runtime [`Config`].
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct RawConfig {
    // Backend
    pub backend: Option<Backend>,
    #[serde(default)]
    pub transition_mode: TransitionMode,

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
    pub start_hyprsunset: Option<bool>,
    pub startup_transition: Option<bool>,
    pub startup_transition_duration: Option<f64>,
}

/// Resolved runtime configuration, produced by [`RawConfig::resolve`].
///
/// Always-defaulted fields are concrete values. Mode-conditional fields stay
/// `Option` because no blanket default exists for them. Which fields apply
/// depends on `transition_mode`.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    // Backend
    pub backend: Backend,
    pub transition_mode: TransitionMode,

    // Smoothing
    pub smoothing: bool,
    pub startup_duration: f64,
    pub shutdown_duration: f64,
    pub adaptive_interval: u64,

    // Time-based
    pub night_temp: u32,
    pub day_temp: u32,
    pub night_gamma: f64,
    pub day_gamma: f64,
    pub update_interval: UpdateInterval,

    // Static
    pub static_temp: Option<u32>,
    pub static_gamma: Option<f64>,

    // Manual transitions
    pub sunset: Option<String>,
    pub sunrise: Option<String>,
    pub transition_duration: u64,

    // Geolocation
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

impl RawConfig {
    /// Migrate deprecated fields: `startup_transition` to `smoothing`, `startup_transition_duration`
    /// to `startup_duration`, and `shutdown_duration` defaulted from `startup_duration` when unset.
    pub(crate) fn migrate_legacy_fields(&mut self) {
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
}

impl Config {
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

        let backend = self.backend;
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
            DisplayMode::TimeBasedManual { mode } => {
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

                log_indented!("Night: {}K @ {}% gamma", self.night_temp, self.night_gamma);
                log_indented!("Day: {}K @ {}% gamma", self.day_temp, self.day_gamma);
                log_indented!("Update interval: {}", self.update_interval);
            }
            DisplayMode::TimeBasedManual { .. } => {
                if let Some(ref sunset) = self.sunset {
                    log_indented!("Sunset: {}", sunset);
                }
                if let Some(ref sunrise) = self.sunrise {
                    log_indented!("Sunrise: {}", sunrise);
                }
                log_indented!("Transition duration: {} minutes", self.transition_duration);
                log_indented!("Night: {}K @ {}% gamma", self.night_temp, self.night_gamma);
                log_indented!("Day: {}K @ {}% gamma", self.day_temp, self.day_gamma);
                log_indented!("Update interval: {}", self.update_interval);
            }
        }

        let backend_supports_smoothing = matches!(backend, Backend::Wayland);

        if backend_supports_smoothing && self.smoothing {
            let startup_duration = self.startup_duration;
            let shutdown_duration = self.shutdown_duration;
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
                log_indented!("Adaptive interval: {}ms", self.adaptive_interval);
            }
        }
    }

    fn detect_display_mode(&self) -> DisplayMode {
        match self.transition_mode {
            TransitionMode::Static => DisplayMode::Static,
            TransitionMode::Geo => DisplayMode::TimeBasedGeo,
            mode => DisplayMode::TimeBasedManual { mode },
        }
    }
}

#[cfg(test)]
mod tests;
