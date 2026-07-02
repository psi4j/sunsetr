//! Load configuration from disk, applying defaults and geo.toml overrides.

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

fn validate_geo_mode_coordinates(config: &Config) -> Result<()> {
    if config.transition_mode.as_deref() == Some("geo")
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
pub(super) fn load_from_path(path: &PathBuf) -> Result<Config> {
    if !path.exists() {
        anyhow::bail!("Configuration file not found at {}", private_path(path));
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

/// Path to the configuration file, under the custom directory when set or the default location.
pub(super) fn get_config_path() -> Result<PathBuf> {
    if let Some(custom_dir) = CONFIG_DIR.get().and_then(|d| d.clone()) {
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
        config.transition_duration = Some(DEFAULT_TRANSITION_DURATION_MIN);
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
        config.startup_duration = Some(DEFAULT_STARTUP_DURATION_SEC);
    }
    if config.shutdown_duration.is_none() {
        config.shutdown_duration = Some(DEFAULT_SHUTDOWN_DURATION_SEC);
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

/// Validation is handled separately by validation::validate_config.
pub(crate) fn apply_defaults_and_modifications(config: &mut Config) -> Result<()> {
    apply_defaults(config);
    apply_modifications(config)?;
    Ok(())
}

/// Overlay latitude and longitude from a sibling geo.toml onto `config`, if the file is present
/// and parses. A missing or malformed geo.toml is ignored with a warning.
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
