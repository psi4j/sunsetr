//! Create default config files and update existing ones with geographic coordinates.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use super::{Config, get_config_path};
use crate::common::constants::*;
use crate::common::utils::private_path;

/// Create a default config file at `path`. When `coords` is `Some`, write those coordinates
/// directly. When `None`, attempt timezone-based detection.
pub fn create_default_config(path: &PathBuf, coords: Option<(f64, f64, String)>) -> Result<()> {
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

    let config_content = ConfigBuilder::new()
        .add_section("Backend")
        .add_setting(
            "backend",
            &format!("\"{}\"", DEFAULT_BACKEND.as_str()),
            "Backend to use: \"auto\", \"hyprland\", \"hyprsunset\" or \"wayland\"",
        )
        .add_setting(
            "transition_mode",
            &format!("\"{transition_mode}\""),
            "Select: \"geo\", \"finish_by\", \"start_at\", \"center\", \"static\"",
        )
        .add_section("Smoothing")
        .add_setting(
            "smoothing",
            &DEFAULT_SMOOTHING.to_string(),
            "Enable smooth transitions during startup and exit",
        )
        .add_setting(
            "startup_duration",
            &DEFAULT_STARTUP_DURATION_SEC.to_string(),
            &format!(
                "Duration of smooth startup in seconds (0.1-{MAXIMUM_SMOOTH_TRANSITION_DURATION_SEC} | 0 = instant)"
            ),
        )
        .add_setting(
            "shutdown_duration",
            &DEFAULT_SHUTDOWN_DURATION_SEC.to_string(),
            &format!(
                "Duration of smooth shutdown in seconds (0.1-{MAXIMUM_SMOOTH_TRANSITION_DURATION_SEC} | 0 = instant)"
            ),
        )
        .add_setting(
            "adaptive_interval",
            &DEFAULT_ADAPTIVE_INTERVAL_MS.to_string(),
            "Adaptive interval base for smooth transitions (1-1000)ms",
        )
        .add_section("Time-based config")
        .add_setting(
            "night_temp",
            &DEFAULT_NIGHT_TEMP.to_string(),
            &format!(
                "Color temperature during night ({MINIMUM_TEMP}-{MAXIMUM_TEMP}) Kelvin"
            ),
        )
        .add_setting(
            "day_temp",
            &DEFAULT_DAY_TEMP.to_string(),
            &format!(
                "Color temperature during day ({MINIMUM_TEMP}-{MAXIMUM_TEMP}) Kelvin"
            ),
        )
        .add_setting(
            "night_gamma",
            &DEFAULT_NIGHT_GAMMA.to_string(),
            &format!(
                "Gamma percentage for night ({MINIMUM_GAMMA}-{MAXIMUM_GAMMA}%)"
            ),
        )
        .add_setting(
            "day_gamma",
            &DEFAULT_DAY_GAMMA.to_string(),
            &format!(
                "Gamma percentage for day ({MINIMUM_GAMMA}-{MAXIMUM_GAMMA}%)"
            ),
        )
        .add_setting(
            "update_interval",
            "\"auto\"",
            &format!(
                "Update frequency during transitions: \"auto\" or integer ({MINIMUM_UPDATE_INTERVAL_SEC}-{MAXIMUM_UPDATE_INTERVAL_SEC}) sec"
            ),
        )
        .add_section("Static config")
        .add_setting(
            "static_temp",
            &DEFAULT_DAY_TEMP.to_string(),
            &format!(
                "Color temperature for static mode ({MINIMUM_TEMP}-{MAXIMUM_TEMP}) Kelvin"
            )
        )
        .add_setting(
            "static_gamma",
            &DEFAULT_DAY_GAMMA.to_string(),
            &format!(
                "Gamma percentage for static mode ({MINIMUM_GAMMA}-{MAXIMUM_GAMMA}%)"
            )
        )
        .add_section("Manual transitions")
        .add_setting(
            "sunset",
            &format!("\"{DEFAULT_SUNSET}\""),
            "Time for manual sunset calculations (HH:MM:SS)",
        )
        .add_setting(
            "sunrise",
            &format!("\"{DEFAULT_SUNRISE}\""),
            "Time for manual sunrise calculations (HH:MM:SS)",
        )
        .add_setting(
            "transition_duration",
            &DEFAULT_TRANSITION_DURATION_MIN.to_string(),
            &format!(
                "Transition duration in minutes ({MINIMUM_TRANSITION_DURATION_MIN}-{MAXIMUM_TRANSITION_DURATION_MIN})"
            ),
        )
        .add_section("Geolocation");

    let config_content = if should_write_coords_to_main {
        config_content
            .add_setting(
                "latitude",
                &format!("{lat:.6}"),
                "Geographic latitude (auto-detected on first run)",
            )
            .add_setting(
                "longitude",
                &format!("{lon:.6}"),
                "Geographic longitude (use 'sunsetr geo' to change)",
            )
    } else {
        config_content
    };

    let config_content = config_content.build();

    fs::write(path, config_content).context("Failed to write default config file")?;
    Ok(())
}

/// Determine the transition mode and coordinates for a new config: geo mode with detected
/// coordinates, or finish_by mode with Chicago coordinates when timezone detection fails.
fn determine_default_mode_and_coords() -> (&'static str, f64, f64) {
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
pub fn update_coordinates(mut latitude: f64, longitude: f64) -> Result<()> {
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

    let lat_exists = find_config_line(&content, "latitude").is_some();
    let lon_exists = find_config_line(&content, "longitude").is_some();

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

/// Builder for configuration files with dynamically-aligned comments.
///
/// Aligns comments by padding to the widest setting line, so the formatting stays correct
/// when the default values in constants.rs change.
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
            content: format!("#[{title}]"),
            entry_type: EntryType::Section,
        });
        self
    }

    fn add_setting(mut self, key: &str, value: &str, comment: &str) -> Self {
        let line = format!("{key} = {value}");
        self.entries.push(ConfigEntry {
            content: line.clone(),
            entry_type: EntryType::Setting {
                line,
                comment: format!("# {comment}"),
            },
        });
        self
    }

    fn build(self) -> String {
        let max_width = self
            .entries
            .iter()
            .filter_map(|entry| match &entry.entry_type {
                EntryType::Setting { line, .. } => Some(line.len()),
                EntryType::Section => None,
            })
            .max()
            .unwrap_or(0)
            + 1;

        let mut result = Vec::new();
        let mut first_section = true;

        for entry in self.entries {
            match entry.entry_type {
                EntryType::Section => {
                    if !first_section {
                        result.push(String::new());
                    }
                    result.push(entry.content);
                    first_section = false;
                }
                EntryType::Setting { line, comment } => {
                    let padding = " ".repeat(max_width - line.len());
                    result.push(format!("{line}{padding}{comment}"));
                }
            }
        }

        result.join("\n")
    }
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
