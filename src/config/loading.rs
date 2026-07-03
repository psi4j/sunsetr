//! Load configuration from disk, applying defaults and geo.toml overrides.

use anyhow::{Context, Result};
use chrono::NaiveTime;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::validation::validate_config;
use super::{Config, GeoConfig, RawConfig, TransitionMode};
use crate::common::constants::*;
use crate::common::utils::private_path;

static CONFIG_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Set once and return an error if already set.
pub fn set_config_dir(dir: Option<String>) -> Result<()> {
    #[cfg(debug_assertions)]
    eprintln!("DEBUG: set_config_dir() called with: {:?}", dir);

    CONFIG_DIR
        .set(dir.map(PathBuf::from))
        .map_err(|_| anyhow::anyhow!("Configuration directory already set"))
}

pub fn get_custom_config_dir() -> Option<PathBuf> {
    CONFIG_DIR.get().and_then(|d| d.clone())
}

/// The base configuration directory, holding sunsetr.toml, geo.toml, and presets/.
pub fn get_config_base_dir() -> Result<PathBuf> {
    let config_path = get_config_path()?;
    config_path
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))
}

fn validate_geo_mode_coordinates(config: &RawConfig) -> Result<()> {
    if config.transition_mode == TransitionMode::Geo
        && (config.latitude.is_none() || config.longitude.is_none())
    {
        anyhow::bail!(
            "Geo mode requires coordinates but none are configured\n\
             Please run 'sunsetr geo' to select your location\n\
             Or add latitude and longitude to your configuration"
        );
    }
    Ok(())
}

/// Load the active configuration, creating a default file if none exists and preferring an active
/// preset's config when one is set.
pub(super) fn load() -> Result<Config> {
    let config_path = get_config_path()?;

    #[cfg(debug_assertions)]
    eprintln!(
        "DEBUG: Config::load() config_path: {}",
        private_path(&config_path)
    );

    if let Some(preset_name) = crate::state::preset::get_active_preset()? {
        #[cfg(debug_assertions)]
        eprintln!("DEBUG: Config::load() found active preset: {}", preset_name);
        let preset_config = config_path
            .parent()
            .context("Failed to get config directory")?
            .join("presets")
            .join(&preset_name)
            .join("sunsetr.toml");

        if preset_config.exists() {
            #[cfg(debug_assertions)]
            eprintln!(
                "DEBUG: Config::load() loading preset config from: {}",
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

    load_from_path(&config_path).with_context(|| private_path(&config_path))
}

/// Load configuration from `path`, without creating a default when it is missing (unlike [`load`]).
pub(super) fn load_from_path(path: &Path) -> Result<Config> {
    if !path.exists() {
        anyhow::bail!("Configuration file not found at {}", private_path(path));
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config from {}", private_path(path)))?;

    let mut raw: RawConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config from {}", private_path(path)))?;

    raw.migrate_legacy_fields();
    load_geo_override_from_path(&mut raw, path)?;
    raw.resolve()
}

/// Path to the configuration file, under the custom directory when set or the default location.
pub(super) fn get_config_path() -> Result<PathBuf> {
    if let Some(custom_dir) = get_custom_config_dir() {
        #[cfg(debug_assertions)]
        eprintln!(
            "DEBUG: get_config_path() using custom dir: {}",
            private_path(&custom_dir)
        );
        return Ok(custom_dir.join("sunsetr.toml"));
    }

    #[cfg(debug_assertions)]
    eprintln!("DEBUG: get_config_path() using the default config location");

    let config_dir = dirs::config_dir().context("Could not determine config directory")?;
    Ok(config_dir.join("sunsetr").join("sunsetr.toml"))
}

impl RawConfig {
    /// Validate and apply defaults exactly once, producing the runtime [`Config`].
    ///
    /// Runs raw validation, the latitude cap and time-format checks, the
    /// mode-conditional sunset/sunrise defaults, and the geo coordinate check,
    /// in that order.
    pub(crate) fn resolve(mut self) -> Result<Config> {
        validate_config(&self)?;
        apply_modifications(&mut self)?;

        if self.transition_mode != TransitionMode::Static {
            self.sunset
                .get_or_insert_with(|| DEFAULT_SUNSET.to_string());
            self.sunrise
                .get_or_insert_with(|| DEFAULT_SUNRISE.to_string());
        }

        validate_geo_mode_coordinates(&self)?;

        Ok(Config {
            backend: self.backend.unwrap_or(DEFAULT_BACKEND),
            transition_mode: self.transition_mode,
            smoothing: self.smoothing.unwrap_or(DEFAULT_SMOOTHING),
            startup_duration: self
                .startup_duration
                .unwrap_or(DEFAULT_STARTUP_DURATION_SEC),
            shutdown_duration: self
                .shutdown_duration
                .unwrap_or(DEFAULT_SHUTDOWN_DURATION_SEC),
            adaptive_interval: self
                .adaptive_interval
                .unwrap_or(DEFAULT_ADAPTIVE_INTERVAL_MS),
            night_temp: self.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP),
            day_temp: self.day_temp.unwrap_or(DEFAULT_DAY_TEMP),
            night_gamma: self.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA),
            day_gamma: self.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA),
            update_interval: self
                .update_interval
                .unwrap_or(crate::config::UpdateInterval::Adaptive),
            transition_duration: self
                .transition_duration
                .unwrap_or(DEFAULT_TRANSITION_DURATION_MIN),
            static_temp: self.static_temp,
            static_gamma: self.static_gamma,
            sunset: self.sunset,
            sunrise: self.sunrise,
            latitude: self.latitude,
            longitude: self.longitude,
        })
    }
}

fn apply_modifications(config: &mut RawConfig) -> Result<()> {
    if config.transition_mode != TransitionMode::Static {
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

/// Overlay latitude and longitude from a sibling geo.toml onto `config`, if the file is present
/// and parses. A missing or malformed geo.toml is ignored with a warning.
pub(crate) fn load_geo_override_from_path(
    config: &mut RawConfig,
    config_path: &Path,
) -> Result<()> {
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
