//! Create default config files and update existing ones with geographic coordinates.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::DocumentMut;

use super::loading::get_config_path;
use super::{Config, TransitionMode};
use crate::common::constants::*;
use crate::common::utils::private_path;

/// Create a default config file at `path`. When `coords` is `Some`, write those coordinates
/// directly. When `None`, attempt timezone-based detection. Coordinates are saved into an
/// existing geo.toml when it parses without them. A geo.toml that cannot be read or parsed
/// stays untouched and the coordinates go to the new sunsetr.toml.
pub(super) fn create_default_config(path: &Path, coords: Option<(f64, f64, String)>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create config directory")?;
    }

    let geo_path = Config::get_geo_path().unwrap_or_else(|_| PathBuf::from(""));
    let use_geo_file = !geo_path.as_os_str().is_empty() && geo_path.exists();

    let (transition_mode, lat, lon, city_name) = if let Some((mut lat, lon, city_name)) = coords {
        if lat.abs() > 65.0 {
            lat = 65.0 * lat.signum();
        }
        (DEFAULT_TRANSITION_MODE, lat, lon, Some(city_name))
    } else {
        let (mode, lat, lon) = determine_default_mode_and_coords();
        (mode, lat, lon, None)
    };

    if let Some(city) = city_name {
        log_indented!("Using selected location for new config: {city}");
    }

    let should_write_coords_to_main = if use_geo_file {
        let existing_geo = fs::read_to_string(&geo_path)
            .ok()
            .and_then(|content| toml::from_str::<super::GeoConfig>(&content).ok());

        match existing_geo {
            Some(cfg) if cfg.latitude.is_some() && cfg.longitude.is_some() => {
                log_indented!("Using existing geo file: {}", private_path(&geo_path));
                false
            }
            Some(_) => {
                let geo_content = format!(
                    "#[Private geo coordinates]\nlatitude = {lat:.6}\nlongitude = {lon:.6}\n"
                );

                fs::write(&geo_path, geo_content).with_context(|| {
                    format!("Failed to write coordinates to {}", geo_path.display())
                })?;

                log_indented!(
                    "Saved coordinates to separate geo file: {}",
                    private_path(&geo_path)
                );
                false
            }
            None => {
                log_warning!(
                    "Could not read coordinates from existing geo.toml. Writing coordinates to sunsetr.toml."
                );
                true
            }
        }
    } else {
        true
    };

    let mut config_content = format!(
        r##"# sunsetr configuration
# Documentation: https://psi4j.github.io/sunsetr/configuration/

#[Backend]
# See https://psi4j.github.io/sunsetr/configuration/backends.html
# and https://psi4j.github.io/sunsetr/configuration/transition-modes.html
backend = "{DEFAULT_BACKEND}"
transition_mode = "{transition_mode}"

#[Smoothing]
# See https://psi4j.github.io/sunsetr/configuration/smoothing.html
smoothing = {DEFAULT_SMOOTHING}
startup_duration = {DEFAULT_STARTUP_DURATION_SEC}
shutdown_duration = {DEFAULT_SHUTDOWN_DURATION_SEC}
adaptive_interval = {DEFAULT_ADAPTIVE_INTERVAL_MS}

#[Time-based config]
# See https://psi4j.github.io/sunsetr/configuration/temperature-gamma.html
night_temp = {DEFAULT_NIGHT_TEMP}
day_temp = {DEFAULT_DAY_TEMP}
night_gamma = {DEFAULT_NIGHT_GAMMA}
day_gamma = {DEFAULT_DAY_GAMMA}
update_interval = "auto"

#[Static config]
# See https://psi4j.github.io/sunsetr/configuration/transition-modes.html#5-static-constant-values
static_temp = {DEFAULT_DAY_TEMP}
static_gamma = {DEFAULT_DAY_GAMMA}

#[Manual transitions]
# See https://psi4j.github.io/sunsetr/configuration/transition-modes.html#manual-transitions
sunset = "{DEFAULT_SUNSET}"
sunrise = "{DEFAULT_SUNRISE}"
transition_duration = {DEFAULT_TRANSITION_DURATION_MIN}

#[Geolocation]
# See https://psi4j.github.io/sunsetr/configuration/geographic.html
"##
    );

    if should_write_coords_to_main {
        config_content.push_str(&format!("latitude = {lat:.6}\nlongitude = {lon:.6}\n"));
    }

    fs::write(path, config_content).context("Failed to write default config file")?;
    Ok(())
}

/// Determine the transition mode and coordinates for a new config: geo mode with detected
/// coordinates, or finish_by mode with Chicago coordinates when timezone detection fails.
fn determine_default_mode_and_coords() -> (TransitionMode, f64, f64) {
    if let Ok((mut lat, lon, city_name)) = crate::geo::detect_coordinates_from_timezone() {
        if lat.abs() > 65.0 {
            lat = 65.0 * lat.signum();
        }

        log_indented!("Auto-detected location for new config: {city_name}");
        (DEFAULT_TRANSITION_MODE, lat, lon)
    } else {
        log_indented!("Timezone detection failed, using manual times with placeholder coordinates");
        log_indented!("Use 'sunsetr geo' to select your actual location");
        (
            crate::common::constants::FALLBACK_DEFAULT_TRANSITION_MODE,
            41.8781,
            -87.6298,
        )
    }
}

/// Update coordinates for the config in `config_dir`, writing to its geo.toml when present.
/// Used for preset configs.
pub fn update_coords_in_dir(config_dir: &Path, mut latitude: f64, longitude: f64) -> Result<()> {
    let config_path = config_dir.join("sunsetr.toml");
    let geo_path = config_dir.join("geo.toml");

    if !config_path.exists() {
        anyhow::bail!("No config file found at {}", private_path(&config_path));
    }

    if latitude.abs() > 65.0 {
        latitude = 65.0 * latitude.signum();
    }

    if geo_path.exists() {
        let geo_content = format!(
            "#[Private geo coordinates]\nlatitude = {latitude:.6}\nlongitude = {longitude:.6}\n"
        );

        fs::write(&geo_path, geo_content)
            .with_context(|| format!("Failed to write geo.toml at {}", geo_path.display()))?;

        let content = fs::read_to_string(&config_path)?;
        let mut doc: DocumentMut = content
            .parse()
            .with_context(|| format!("Failed to parse {}", config_path.display()))?;
        set_field(&mut doc, "transition_mode", "geo".into());
        fs::write(&config_path, doc.to_string())?;

        log_block_start!("Updated coordinates in {}", private_path(&geo_path));
        log_indented!("Latitude: {latitude:.6}");
        log_indented!("Longitude: {longitude:.6}");

        return Ok(());
    }

    let content = fs::read_to_string(&config_path)?;
    let mut doc: DocumentMut = content
        .parse()
        .with_context(|| format!("Failed to parse {}", config_path.display()))?;
    set_field(&mut doc, "latitude", coord_value(latitude));
    set_field(&mut doc, "longitude", coord_value(longitude));
    set_field(&mut doc, "transition_mode", "geo".into());
    fs::write(&config_path, doc.to_string())?;

    log_block_start!("Updated config at {}", private_path(&config_path));
    log_indented!("Latitude: {latitude:.6}");
    log_indented!("Longitude: {longitude:.6}");
    log_indented!("Transition mode: geo");

    Ok(())
}

/// Update the active config file with geo coordinates and switch it to geo mode.
pub(super) fn update_coordinates(mut latitude: f64, longitude: f64) -> Result<()> {
    let config_path = get_config_path()?;
    let geo_path = Config::get_geo_path()?;

    if !config_path.exists() {
        anyhow::bail!(
            "No existing config file found at {}",
            private_path(&config_path)
        );
    }

    if latitude.abs() > 65.0 {
        latitude = 65.0 * latitude.signum();
    }

    if geo_path.exists() {
        let geo_content = format!(
            "#[Private geo coordinates]\nlatitude = {latitude:.6}\nlongitude = {longitude:.6}\n"
        );

        fs::write(&geo_path, geo_content)
            .with_context(|| format!("Failed to write coordinates to {}", geo_path.display()))?;

        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config from {}", config_path.display()))?;
        let mut doc: DocumentMut = content
            .parse()
            .with_context(|| format!("Failed to parse {}", config_path.display()))?;

        if doc.get("transition_mode").and_then(|item| item.as_str()) != Some("geo") {
            set_field(&mut doc, "transition_mode", "geo".into());
            fs::write(&config_path, doc.to_string()).with_context(|| {
                format!(
                    "Failed to write updated config to {}",
                    config_path.display()
                )
            })?;
        }

        log_block_start!("Updated geo coordinates in {}", private_path(&geo_path));
        log_indented!("Latitude: {latitude}");
        log_indented!("Longitude: {longitude}");

        return Ok(());
    }

    let content = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config from {}", config_path.display()))?;
    let mut doc: DocumentMut = content
        .parse()
        .with_context(|| format!("Failed to parse {}", config_path.display()))?;

    set_field(&mut doc, "latitude", coord_value(latitude));
    set_field(&mut doc, "longitude", coord_value(longitude));
    if doc.get("transition_mode").and_then(|item| item.as_str()) != Some("geo") {
        set_field(&mut doc, "transition_mode", "geo".into());
    }

    fs::write(&config_path, doc.to_string()).with_context(|| {
        format!(
            "Failed to write updated config to {}",
            config_path.display()
        )
    })?;

    log_block_start!("Updated config file: {}", private_path(&config_path));
    log_indented!("Latitude: {latitude}");
    log_indented!("Longitude: {longitude}");
    log_indented!("Transition mode: geo");

    Ok(())
}

/// Set `key` to `value` in the document's root table. When the key already exists,
/// the new value inherits the old value's decor, so surrounding whitespace and any
/// trailing comment survive the edit. A missing key is appended at the end.
///
/// In a document with no keys yet, comments parse as trailing content, which would
/// render below an appended key. Move them onto the new key instead so a header
/// comment (the geo.toml privacy header) stays at the top of the file.
pub(crate) fn set_field(doc: &mut DocumentMut, key: &str, mut value: toml_edit::Value) {
    value.decor_mut().clear();
    if let Some(existing) = doc.get(key).and_then(toml_edit::Item::as_value) {
        *value.decor_mut() = existing.decor().clone();
    }
    let was_empty = doc.is_empty();
    doc[key] = toml_edit::Item::Value(value);

    if was_empty
        && let Some(header) = doc
            .trailing()
            .as_str()
            .filter(|s| !s.is_empty())
            .map(String::from)
        && let Some(mut new_key) = doc.key_mut(key)
    {
        new_key.leaf_decor_mut().set_prefix(header);
        doc.set_trailing("");
    }
}

/// A coordinate as a TOML value with the fixed six-decimal representation used
/// everywhere coordinates are written.
fn coord_value(coord: f64) -> toml_edit::Value {
    format!("{coord:.6}")
        .parse()
        .expect("fixed-precision float is a valid TOML value")
}
