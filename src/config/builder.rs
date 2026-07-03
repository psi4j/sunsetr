//! Create default config files and update existing ones with geographic coordinates.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use super::loading::get_config_path;
use super::{Config, TransitionMode};
use crate::common::constants::*;
use crate::common::utils::private_path;

/// Create a default config file at `path`. When `coords` is `Some`, write those coordinates
/// directly. When `None`, attempt timezone-based detection.
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

    let should_write_coords_to_main = if use_geo_file {
        let existing_has_coords = fs::read_to_string(&geo_path)
            .ok()
            .and_then(|content| toml::from_str::<super::GeoConfig>(&content).ok())
            .is_some_and(|cfg| cfg.latitude.is_some() && cfg.longitude.is_some());

        if let Some(city) = city_name {
            log_indented!("Using selected location for new config: {city}");
        }

        if existing_has_coords {
            log_indented!("Using existing geo file: {}", private_path(&geo_path));
        } else {
            let geo_content =
                format!("#[Private geo coordinates]\nlatitude = {lat:.6}\nlongitude = {lon:.6}\n");

            fs::write(&geo_path, geo_content).with_context(|| {
                format!("Failed to write coordinates to {}", geo_path.display())
            })?;

            log_indented!(
                "Saved coordinates to separate geo file: {}",
                private_path(&geo_path)
            );
        }

        false
    } else {
        if let Some(city) = city_name {
            log_indented!("Using selected location for new config: {city}");
        }
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
        let mut updated_content = content.clone();

        if let Some(mode_line) = find_config_line(&content, "transition_mode") {
            let new_mode_line =
                preserve_comment_formatting(&mode_line, "transition_mode", "\"geo\"");
            updated_content = updated_content.replace(&mode_line, &new_mode_line);
        } else {
            if !updated_content.ends_with('\n') {
                updated_content.push('\n');
            }
            updated_content.push_str("transition_mode = \"geo\"\n");
        }

        fs::write(&config_path, updated_content)?;

        log_block_start!("Updated coordinates in {}", private_path(&geo_path));
        log_indented!("Latitude: {latitude:.6}");
        log_indented!("Longitude: {longitude:.6}");

        return Ok(());
    }

    let content = fs::read_to_string(&config_path)?;
    let mut updated_content = content.clone();

    if let Some(lat_line) = find_config_line(&content, "latitude") {
        let target_column = lat_line.find('#').unwrap_or(25);
        let new_lat_line = align_comment_to_column(
            &lat_line,
            "latitude",
            &format!("{latitude:.6}"),
            target_column,
        );
        updated_content = updated_content.replace(&lat_line, &new_lat_line);
    } else {
        if !updated_content.ends_with('\n') {
            updated_content.push('\n');
        }
        updated_content.push_str(&format!("latitude = {latitude:.6}\n"));
    }

    if let Some(lon_line) = find_config_line(&content, "longitude") {
        let target_column = lon_line.find('#').unwrap_or(25);
        let new_lon_line = align_comment_to_column(
            &lon_line,
            "longitude",
            &format!("{longitude:.6}"),
            target_column,
        );
        updated_content = updated_content.replace(&lon_line, &new_lon_line);
    } else {
        if !updated_content.ends_with('\n') {
            updated_content.push('\n');
        }
        updated_content.push_str(&format!("longitude = {longitude:.6}\n"));
    }

    if let Some(mode_line) = find_config_line(&updated_content, "transition_mode") {
        let new_mode_line = preserve_comment_formatting(&mode_line, "transition_mode", "\"geo\"");
        updated_content = updated_content.replace(&mode_line, &new_mode_line);
    } else {
        if !updated_content.ends_with('\n') {
            updated_content.push('\n');
        }
        updated_content.push_str("transition_mode = \"geo\"\n");
    }

    fs::write(&config_path, updated_content)?;

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

        let mut updated_content = content.clone();

        if let Some(mode_line) = find_config_line(&content, "transition_mode") {
            let value_part = mode_line.split('#').next().unwrap_or(&mode_line);
            if !value_part.contains("= \"geo\"") {
                let new_mode_line =
                    preserve_comment_formatting(&mode_line, "transition_mode", "\"geo\"");
                updated_content = updated_content.replace(&mode_line, &new_mode_line);
            }
        } else {
            updated_content = format!("{updated_content}transition_mode = \"geo\"\n");
        }

        if updated_content != content {
            fs::write(&config_path, updated_content).with_context(|| {
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

    let mut updated_content = content.clone();

    let lat_value = format!("{latitude:.6}");
    let lon_value = format!("{longitude:.6}");
    let lat_line = find_config_line(&content, "latitude");
    let lon_line = find_config_line(&content, "longitude");

    let target_column = match (&lat_line, &lon_line) {
        (Some(lat), Some(lon)) => {
            let lat_pos = lat.find('#').unwrap_or(lat.len());
            let lon_pos = lon.find('#').unwrap_or(lon.len());
            lat_pos.max(lon_pos)
        }
        (Some(line), None) | (None, Some(line)) => line.find('#').unwrap_or(25),
        (None, None) => 25,
    };

    let lat_exists = lat_line.is_some();
    let lon_exists = lon_line.is_some();

    if let Some(lat_line) = lat_line {
        let new_lat_line =
            align_comment_to_column(&lat_line, "latitude", &lat_value, target_column);
        updated_content = updated_content.replace(&lat_line, &new_lat_line);
    }

    if let Some(lon_line) = lon_line {
        let new_lon_line =
            align_comment_to_column(&lon_line, "longitude", &lon_value, target_column);
        updated_content = updated_content.replace(&lon_line, &new_lon_line);
    }

    if !lat_exists || !lon_exists {
        if !updated_content.ends_with('\n') {
            updated_content.push('\n');
        }

        if !lat_exists {
            updated_content.push_str(&format!("latitude = {latitude:.6}\n"));
        }
        if !lon_exists {
            updated_content.push_str(&format!("longitude = {longitude:.6}\n"));
        }
    }

    if let Some(mode_line) = find_config_line(&content, "transition_mode") {
        let value_part = mode_line.split('#').next().unwrap_or(&mode_line);
        if !value_part.contains("= \"geo\"") {
            let new_mode_line =
                preserve_comment_formatting(&mode_line, "transition_mode", "\"geo\"");
            updated_content = updated_content.replace(&mode_line, &new_mode_line);
        }
    } else {
        updated_content = format!("{updated_content}transition_mode = \"geo\"\n");
    }

    fs::write(&config_path, updated_content).with_context(|| {
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

pub(crate) fn find_config_line(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(key) && trimmed.contains('=') && !trimmed.starts_with('#') {
            return Some(line.to_string());
        }
    }
    None
}

/// Update a config line's value while preserving the exact spacing between the value and its
/// trailing comment (tabs, spaces, or a mix).
pub(crate) fn preserve_comment_formatting(
    original_line: &str,
    key: &str,
    new_value: &str,
) -> String {
    let key_value_part = format!("{key} = {new_value}");

    if let Some(comment_pos) = original_line.find('#') {
        let comment_part = &original_line[comment_pos..];

        let before_comment = &original_line[..comment_pos];
        let original_spacing =
            if let Some(last_non_space) = before_comment.rfind(|c: char| !c.is_whitespace()) {
                &before_comment[last_non_space + 1..]
            } else {
                " "
            };

        format!("{}{}{}", key_value_part, original_spacing, comment_part)
    } else {
        key_value_part
    }
}

/// Update a config line's value and align its comment to `target_column`, keeping related lines
/// (latitude and longitude) aligned regardless of value length.
fn align_comment_to_column(
    original_line: &str,
    key: &str,
    new_value: &str,
    target_column: usize,
) -> String {
    let key_value_part = format!("{key} = {new_value}");

    if let Some(comment_pos) = original_line.find('#') {
        let comment_part = &original_line[comment_pos..];

        let padding_needed = if key_value_part.len() < target_column {
            target_column - key_value_part.len()
        } else {
            1
        };

        format!(
            "{}{}{}",
            key_value_part,
            " ".repeat(padding_needed),
            comment_part
        )
    } else {
        key_value_part
    }
}
