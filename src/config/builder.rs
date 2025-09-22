//! Configuration file building and default config creation.
//!
//! Handles creating default configuration files, updating existing files with geo coordinates,
//! and managing the config builder pattern for properly formatted output.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use super::{Config, get_config_path};
use crate::constants::*;
use crate::utils::private_path;

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
    let geo_path = Config::get_geo_path().unwrap_or_else(|_| PathBuf::from(""));
    let use_geo_file = !geo_path.as_os_str().is_empty() && geo_path.exists();

    // Determine coordinate entries based on whether coordinates were provided
    let (transition_mode, lat, lon, city_name) = if let Some((mut lat, lon, city_name)) = coords {
        // Cap latitude at ±65° before saving
        if lat.abs() > 65.0 {
            lat = 65.0 * lat.signum();
        }
        (DEFAULT_TRANSITION_MODE, lat, lon, Some(city_name))
    } else {
        // Try to auto-detect coordinates via timezone for smart geo mode default
        let (mode, lat, lon) = determine_default_mode_and_coords();
        (mode, lat, lon, None)
    };

    // Now handle geo.toml logic for ALL cases
    let should_write_coords_to_main = if use_geo_file {
        // Write coordinates to geo.toml instead of main config
        let geo_content =
            format!("#[Private geo coordinates]\nlatitude = {lat:.6}\nlongitude = {lon:.6}\n");

        fs::write(&geo_path, geo_content)
            .with_context(|| format!("Failed to write coordinates to {}", geo_path.display()))?;

        if let Some(city) = city_name {
            log_indented!("Using selected location for new config: {city}");
        }
        log_indented!(
            "Saved coordinates to separate geo file: {}",
            private_path(&geo_path)
        );

        false // Don't write coords to main config
    } else {
        // No geo.toml, write to main config as usual
        if let Some(city) = city_name {
            log_indented!("Using selected location for new config: {city}");
        }
        true // Write coords to main config
    };

    // Build the config using the builder pattern
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
            &DEFAULT_STARTUP_DURATION.to_string(),
            &format!(
                "Duration of smooth startup in seconds (0.1-{MAXIMUM_SMOOTH_TRANSITION_DURATION} | 0 = instant)"
            ),
        )
        .add_setting(
            "shutdown_duration",
            &DEFAULT_SHUTDOWN_DURATION.to_string(),
            &format!(
                "Duration of smooth shutdown in seconds (0.1-{MAXIMUM_SMOOTH_TRANSITION_DURATION} | 0 = instant)"
            ),
        )
        .add_setting(
            "adaptive_interval",
            &DEFAULT_ADAPTIVE_INTERVAL.to_string(),
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
            &DEFAULT_UPDATE_INTERVAL.to_string(),
            &format!(
                "Update frequency during transitions in seconds ({MINIMUM_UPDATE_INTERVAL}-{MAXIMUM_UPDATE_INTERVAL})"
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
            &DEFAULT_TRANSITION_DURATION.to_string(),
            &format!(
                "Transition duration in minutes ({MINIMUM_TRANSITION_DURATION}-{MAXIMUM_TRANSITION_DURATION})"
            ),
        )
        .add_section("Geolocation");

    // Only add coordinates to main config if they should be written there
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
    // Try timezone detection for automatic coordinates
    if let Ok((mut lat, lon, city_name)) = crate::geo::detect_coordinates_from_timezone() {
        // Cap latitude at ±65°
        if lat.abs() > 65.0 {
            lat = 65.0 * lat.signum();
        }

        log_indented!("Auto-detected location for new config: {city_name}");
        (DEFAULT_TRANSITION_MODE, lat, lon)
    } else {
        // Fall back to finish_by mode with Chicago coordinates as placeholders
        log_indented!("Timezone detection failed, using manual times with placeholder coordinates");
        log_indented!("Use 'sunsetr geo' to select your actual location");
        (
            crate::constants::FALLBACK_DEFAULT_TRANSITION_MODE,
            41.8781,
            -87.6298,
        ) // Chicago coordinates (placeholder)
    }
}

/// Update coordinates in a specific directory (for preset support).
///
/// This function updates coordinates in any config directory, respecting
/// geo.toml if it exists. Used internally for updating preset configs.
pub fn update_coords_in_dir(config_dir: &Path, mut latitude: f64, longitude: f64) -> Result<()> {
    let config_path = config_dir.join("sunsetr.toml");
    let geo_path = config_dir.join("geo.toml");

    if !config_path.exists() {
        anyhow::bail!("No config file found at {}", private_path(&config_path));
    }

    // Cap latitude at ±65° before saving
    if latitude.abs() > 65.0 {
        latitude = 65.0 * latitude.signum();
    }

    // Check if geo.toml exists - if it does, update there instead
    if geo_path.exists() {
        // Update geo.toml with new coordinates
        let geo_content = format!(
            "#[Private geo coordinates]\nlatitude = {latitude:.6}\nlongitude = {longitude:.6}\n"
        );

        fs::write(&geo_path, geo_content)
            .with_context(|| format!("Failed to write geo.toml at {}", geo_path.display()))?;

        // Also update transition_mode to "geo" in main config
        let content = fs::read_to_string(&config_path)?;
        let mut updated_content = content.clone();

        if let Some(mode_line) = find_config_line(&content, "transition_mode") {
            let new_mode_line =
                preserve_comment_formatting(&mode_line, "transition_mode", "\"geo\"");
            updated_content = updated_content.replace(&mode_line, &new_mode_line);
        } else {
            // Add transition_mode at the end
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

    // geo.toml doesn't exist, update main config
    let content = fs::read_to_string(&config_path)?;
    let mut updated_content = content.clone();

    // Update latitude
    if let Some(lat_line) = find_config_line(&content, "latitude") {
        // Preserve comment formatting
        let target_column = lat_line.find('#').unwrap_or(25);
        let new_lat_line = align_comment_to_column(
            &lat_line,
            "latitude",
            &format!("{latitude:.6}"),
            target_column,
        );
        updated_content = updated_content.replace(&lat_line, &new_lat_line);
    } else {
        // Add latitude if missing
        if !updated_content.ends_with('\n') {
            updated_content.push('\n');
        }
        updated_content.push_str(&format!("latitude = {latitude:.6}\n"));
    }

    // Update longitude
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
        // Add longitude if missing
        if !updated_content.ends_with('\n') {
            updated_content.push('\n');
        }
        updated_content.push_str(&format!("longitude = {longitude:.6}\n"));
    }

    // Update transition_mode to "geo"
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

/// Update an existing config file with geo coordinates and mode
pub fn update_coordinates(mut latitude: f64, longitude: f64) -> Result<()> {
    let config_path = get_config_path()?;
    let geo_path = Config::get_geo_path()?;

    if !config_path.exists() {
        anyhow::bail!(
            "No existing config file found at {}",
            private_path(&config_path)
        );
    }

    // Cap latitude at ±65° before saving
    if latitude.abs() > 65.0 {
        latitude = 65.0 * latitude.signum();
    }

    // Check if geo.toml exists - if it does, update there instead
    if geo_path.exists() {
        // Update geo.toml with new coordinates
        let geo_content = format!(
            "#[Private geo coordinates]\nlatitude = {latitude:.6}\nlongitude = {longitude:.6}\n"
        );

        fs::write(&geo_path, geo_content)
            .with_context(|| format!("Failed to write coordinates to {}", geo_path.display()))?;

        // Also ensure transition_mode is set to "geo" in main config
        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config from {}", config_path.display()))?;

        let mut updated_content = content.clone();

        // Update or add transition_mode to "geo"
        if let Some(mode_line) = find_config_line(&content, "transition_mode") {
            // Check if it's already set to "geo" (only check the value part, not comments)
            let value_part = mode_line.split('#').next().unwrap_or(&mode_line);
            if !value_part.contains("= \"geo\"") {
                let new_mode_line =
                    preserve_comment_formatting(&mode_line, "transition_mode", "\"geo\"");
                updated_content = updated_content.replace(&mode_line, &new_mode_line);
            }
        } else {
            // Add transition_mode at the end
            updated_content = format!("{updated_content}transition_mode = \"geo\"\n");
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

        log_block_start!("Updated geo coordinates in {}", private_path(&geo_path));
        log_indented!("Latitude: {latitude}");
        log_indented!("Longitude: {longitude}");

        return Ok(());
    }

    // geo.toml doesn't exist, update main config as before
    // Read current config content
    let content = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config from {}", config_path.display()))?;

    // Parse as TOML to preserve structure and comments
    let mut updated_content = content.clone();

    // Format the coordinate values
    let lat_value = format!("{latitude:.6}");
    let lon_value = format!("{longitude:.6}");

    // Find existing coordinate lines
    let lat_line = find_config_line(&content, "latitude");
    let lon_line = find_config_line(&content, "longitude");

    // Determine comment alignment - preserve existing or use sensible default
    let target_column = match (&lat_line, &lon_line) {
        (Some(lat), Some(lon)) => {
            // Both exist: use the rightmost comment position
            let lat_pos = lat.find('#').unwrap_or(lat.len());
            let lon_pos = lon.find('#').unwrap_or(lon.len());
            lat_pos.max(lon_pos)
        }
        (Some(line), None) | (None, Some(line)) => {
            // One exists: preserve its comment position
            line.find('#').unwrap_or(25) // Default to column 25 if no comment
        }
        (None, None) => {
            // Neither exists: use standard alignment
            25 // Matches ConfigBuilder default
        }
    };

    // Update or add latitude
    if let Some(lat_line) = lat_line {
        let new_lat_line =
            align_comment_to_column(&lat_line, "latitude", &lat_value, target_column);
        updated_content = updated_content.replace(&lat_line, &new_lat_line);
    } else {
        // Latitude doesn't exist, will add at the end
    }

    // Update or add longitude
    if let Some(lon_line) = lon_line {
        let new_lon_line =
            align_comment_to_column(&lon_line, "longitude", &lon_value, target_column);
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
            updated_content.push_str(&format!("latitude = {latitude:.6}\n"));
        }
        if !lon_exists {
            updated_content.push_str(&format!("longitude = {longitude:.6}\n"));
        }
    }

    // Update transition_mode to "geo" only if it's not already set to "geo"
    if let Some(mode_line) = find_config_line(&content, "transition_mode") {
        // Check if it's already set to "geo" (only check the value part, not comments)
        let value_part = mode_line.split('#').next().unwrap_or(&mode_line);
        if !value_part.contains("= \"geo\"") {
            let new_mode_line =
                preserve_comment_formatting(&mode_line, "transition_mode", "\"geo\"");
            updated_content = updated_content.replace(&mode_line, &new_mode_line);
        }
    } else {
        // Add transition_mode at the end
        updated_content = format!("{updated_content}transition_mode = \"geo\"\n");
    }

    // Write updated content back to file
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
                    result.push(format!("{line}{padding}{comment}"));
                }
            }
        }

        result.join("\n")
    }
}

/// Find a config line containing the specified key
pub(crate) fn find_config_line(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(key) && trimmed.contains('=') && !trimmed.starts_with('#') {
            return Some(line.to_string());
        }
    }
    None
}

/// Preserve the original comment formatting when updating a config line value.
///
/// This function maintains the exact spacing that was between the value and comment
/// in the original line, preserving tabs, spaces, or any combination thereof.
pub(crate) fn preserve_comment_formatting(
    original_line: &str,
    key: &str,
    new_value: &str,
) -> String {
    let key_value_part = format!("{key} = {new_value}");

    if let Some(comment_pos) = original_line.find('#') {
        let comment_part = &original_line[comment_pos..];

        // Extract the original spacing between the value and the comment
        let before_comment = &original_line[..comment_pos];
        let original_spacing =
            if let Some(last_non_space) = before_comment.rfind(|c: char| !c.is_whitespace()) {
                &before_comment[last_non_space + 1..]
            } else {
                " " // Default to single space if we can't determine
            };

        format!("{}{}{}", key_value_part, original_spacing, comment_part)
    } else {
        key_value_part
    }
}

/// Align a comment to a specific column position when updating a config line value.
///
/// This function is used when multiple related lines (like latitude/longitude)
/// need to maintain consistent comment alignment regardless of value lengths.
fn align_comment_to_column(
    original_line: &str,
    key: &str,
    new_value: &str,
    target_column: usize,
) -> String {
    let key_value_part = format!("{key} = {new_value}");

    if let Some(comment_pos) = original_line.find('#') {
        let comment_part = &original_line[comment_pos..];

        // Calculate padding to reach the target column
        let padding_needed = if key_value_part.len() < target_column {
            target_column - key_value_part.len()
        } else {
            1 // At least one space if the value is longer than expected
        };

        // Add padding to reach the target column
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
