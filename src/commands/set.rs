//! Set command implementation for modifying configuration fields
//!
//! This command allows users to update individual settings in the active configuration
//! without manually editing files, while preserving comments and leveraging hot-reloading.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

/// Handle the set command - update configuration fields
pub fn handle_set_command(fields: &[(String, String)]) -> Result<()> {
    // Always print version header since we're handling a set command
    log_version!();

    // Get the active config path (respects presets and custom config directories)
    let config_path = get_active_config_path()?;

    // Validate all fields first before making any changes
    let mut validated_fields = Vec::new();
    for (field, value) in fields {
        match validate_field_value(field, value) {
            Err(e) => {
                log_pipe!();
                log_error!("Invalid value for field '{}': {}", field, e);
                anyhow::bail!("Configuration validation failed");
            }
            Ok(formatted_value) => {
                validated_fields.push((field.as_str(), formatted_value));
            }
        }
    }

    // Read current config content once
    let mut content = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config from {}", config_path.display()))?;

    // Apply all field updates
    let mut changed = false;
    let mut updated_fields = Vec::new();

    for (field, formatted_value) in &validated_fields {
        let updated_content = update_field_in_content(&content, field, formatted_value)?;
        if updated_content != content {
            content = updated_content;
            changed = true;
            updated_fields.push((field, formatted_value));
        }
    }

    // Write back if changed
    if changed {
        fs::write(&config_path, &content)
            .with_context(|| format!("Failed to write config to {}", config_path.display()))?;

        log_block_start!("Updated configuration");
        for (field, value) in &updated_fields {
            log_indented!("{} = {}", field, value);
        }
        log_indented!("in {}", crate::utils::path_for_display(&config_path));

        // If sunsetr is running, it will automatically reload via file watcher
        if let Ok(pid) = crate::utils::get_running_sunsetr_pid() {
            log_block_start!("Configuration reloaded successfully (PID: {})", pid);
        } else {
            log_block_start!("Start sunsetr to apply the new configuration");
        }
    } else {
        log_block_start!("Configuration unchanged");
        if fields.len() == 1 {
            log_indented!(
                "{} is already set to {}",
                fields[0].0,
                validated_fields[0].1
            );
        } else {
            log_indented!("All fields already have the specified values");
        }
    }

    log_end!();
    Ok(())
}

/// Get the path to the currently active configuration file
fn get_active_config_path() -> Result<PathBuf> {
    // Use the existing config loading logic which handles presets
    let config_path = crate::config::Config::get_config_path()?;

    // Check if there's an active preset using the existing function
    if let Some(preset_name) = crate::config::Config::get_active_preset()? {
        let config_dir = config_path
            .parent()
            .context("Failed to get config directory")?;

        Ok(config_dir
            .join("presets")
            .join(&preset_name)
            .join("sunsetr.toml"))
    } else {
        Ok(config_path)
    }
}

/// Validate a field value by attempting to parse it as TOML
fn validate_field_value(field: &str, value: &str) -> Result<String> {
    // For string-type fields, wrap in quotes if not already quoted
    let toml_value = match field {
        // String fields that need quotes
        "sunset" | "sunrise" | "backend" | "transition_mode" => {
            // Check if already properly quoted
            if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                value.to_string()
            } else {
                format!("\"{}\"", value)
            }
        }
        // Boolean fields
        "smoothing" => {
            // Accept various boolean representations
            match value.to_lowercase().as_str() {
                "true" | "yes" | "on" | "1" => "true".to_string(),
                "false" | "no" | "off" | "0" => "false".to_string(),
                _ => value.to_string(), // Let TOML parsing handle the error
            }
        }
        // Numeric fields - pass through as-is
        _ => value.to_string(),
    };

    // Create a minimal TOML document with just this field
    let test_toml = format!("{} = {}", field, toml_value);

    // Try to parse it as a generic TOML value first
    let parsed_value: toml::Value = test_toml
        .parse()
        .with_context(|| format!("Invalid TOML syntax for field '{}'", field))?;

    // Extract the actual value
    let field_value = parsed_value
        .get(field)
        .context("Failed to extract field value")?;

    // Validate based on field type using existing Config struct constraints
    match field {
        // Temperature fields
        "night_temp" | "day_temp" | "static_temp" => {
            let temp = field_value
                .as_integer()
                .context("Temperature must be an integer")?;
            if temp < crate::constants::MINIMUM_TEMP as i64
                || temp > crate::constants::MAXIMUM_TEMP as i64
            {
                anyhow::bail!(
                    "Temperature must be between {} and {} K",
                    crate::constants::MINIMUM_TEMP,
                    crate::constants::MAXIMUM_TEMP
                );
            }
            Ok(temp.to_string())
        }

        // Gamma fields (stored as percentage 0-100)
        "night_gamma" | "day_gamma" | "static_gamma" => {
            let gamma = field_value
                .as_float()
                .or_else(|| field_value.as_integer().map(|i| i as f64))
                .context("Gamma must be a number")?;
            if gamma < crate::constants::MINIMUM_GAMMA as f64
                || gamma > crate::constants::MAXIMUM_GAMMA as f64
            {
                anyhow::bail!(
                    "Gamma must be between {}% and {}%",
                    crate::constants::MINIMUM_GAMMA,
                    crate::constants::MAXIMUM_GAMMA
                );
            }
            Ok(format!("{:.1}", gamma))
        }

        // Time fields
        "sunset" | "sunrise" => {
            let time_str = field_value.as_str().context("Time must be a string")?;

            // Validate time format using chrono
            use chrono::NaiveTime;
            NaiveTime::parse_from_str(time_str, "%H:%M:%S")
                .or_else(|_| {
                    // Also accept HH:MM format and convert to HH:MM:SS
                    NaiveTime::parse_from_str(time_str, "%H:%M")
                })
                .with_context(|| {
                    format!("Invalid time format: {} (use HH:MM or HH:MM:SS)", time_str)
                })?;

            // Always store in HH:MM:SS format
            let formatted = if time_str.matches(':').count() == 1 {
                format!("\"{}:00\"", time_str)
            } else {
                format!("\"{}\"", time_str)
            };
            Ok(formatted)
        }

        // Duration fields
        "transition_duration" => {
            let duration = field_value
                .as_integer()
                .context("Duration must be an integer (minutes)")?;
            if duration < crate::constants::MINIMUM_TRANSITION_DURATION as i64
                || duration > crate::constants::MAXIMUM_TRANSITION_DURATION as i64
            {
                anyhow::bail!(
                    "Transition duration must be between {} and {} minutes",
                    crate::constants::MINIMUM_TRANSITION_DURATION,
                    crate::constants::MAXIMUM_TRANSITION_DURATION
                );
            }
            Ok(duration.to_string())
        }

        "startup_duration" | "shutdown_duration" => {
            let duration = field_value
                .as_float()
                .or_else(|| field_value.as_integer().map(|i| i as f64))
                .context("Duration must be a number (seconds)")?;
            if !(crate::constants::MINIMUM_SMOOTH_TRANSITION_DURATION
                ..=crate::constants::MAXIMUM_SMOOTH_TRANSITION_DURATION)
                .contains(&duration)
            {
                anyhow::bail!(
                    "Smooth transition duration must be between {} and {} seconds",
                    crate::constants::MINIMUM_SMOOTH_TRANSITION_DURATION,
                    crate::constants::MAXIMUM_SMOOTH_TRANSITION_DURATION
                );
            }
            Ok(format!("{:.1}", duration))
        }

        "update_interval" => {
            let interval = field_value
                .as_integer()
                .context("Update interval must be an integer (seconds)")?;
            if interval < crate::constants::MINIMUM_UPDATE_INTERVAL as i64
                || interval > crate::constants::MAXIMUM_UPDATE_INTERVAL as i64
            {
                anyhow::bail!(
                    "Update interval must be between {} and {} seconds",
                    crate::constants::MINIMUM_UPDATE_INTERVAL,
                    crate::constants::MAXIMUM_UPDATE_INTERVAL
                );
            }
            Ok(interval.to_string())
        }

        "adaptive_interval" => {
            let interval = field_value
                .as_integer()
                .context("Adaptive interval must be an integer (milliseconds)")?;
            if interval < crate::constants::MINIMUM_ADAPTIVE_INTERVAL as i64
                || interval > crate::constants::MAXIMUM_ADAPTIVE_INTERVAL as i64
            {
                anyhow::bail!(
                    "Adaptive interval must be between {} and {} milliseconds",
                    crate::constants::MINIMUM_ADAPTIVE_INTERVAL,
                    crate::constants::MAXIMUM_ADAPTIVE_INTERVAL
                );
            }
            Ok(interval.to_string())
        }

        // Boolean field
        "smoothing" => {
            let bool_value = field_value.as_bool().context("Must be true or false")?;
            Ok(bool_value.to_string())
        }

        // String enum fields
        "backend" => {
            let backend_str = field_value.as_str().context("Backend must be a string")?;
            match backend_str {
                "auto" | "hyprland" | "hyprsunset" | "wayland" => {
                    Ok(format!("\"{}\"", backend_str))
                }
                _ => anyhow::bail!(
                    "Invalid backend: {} (use auto, hyprland, hyprsunset, or wayland)",
                    backend_str
                ),
            }
        }

        "transition_mode" => {
            let mode = field_value
                .as_str()
                .context("Transition mode must be a string")?;
            match mode {
                "geo" | "finish_by" | "start_at" | "center" | "static" => {
                    Ok(format!("\"{}\"", mode))
                }
                _ => anyhow::bail!(
                    "Invalid transition mode: {} (use geo, finish_by, start_at, center, or static)",
                    mode
                ),
            }
        }

        // Coordinate fields
        "latitude" => {
            let mut lat = field_value
                .as_float()
                .or_else(|| field_value.as_integer().map(|i| i as f64))
                .context("Latitude must be a number")?;
            if !(-90.0..=90.0).contains(&lat) {
                anyhow::bail!("Latitude must be between -90 and 90 degrees");
            }
            // Cap at ±65° like the geo command does
            if lat.abs() > 65.0 {
                lat = 65.0 * lat.signum();
                log_warning!("Latitude capped at 65° (extreme latitudes can cause issues)");
            }
            Ok(format!("{:.6}", lat))
        }

        "longitude" => {
            let lon = field_value
                .as_float()
                .or_else(|| field_value.as_integer().map(|i| i as f64))
                .context("Longitude must be a number")?;
            if !(-180.0..=180.0).contains(&lon) {
                anyhow::bail!("Longitude must be between -180 and 180 degrees");
            }
            Ok(format!("{:.6}", lon))
        }

        _ => anyhow::bail!("Unknown configuration field: '{}'", field),
    }
}

/// Update a field in the config content while preserving comments
fn update_field_in_content(content: &str, field: &str, value: &str) -> Result<String> {
    // Use the existing helper function from config::builder
    let existing_line = crate::config::builder::find_config_line(content, field);

    if let Some(line) = existing_line {
        // Preserve comment formatting using existing function
        let new_line = crate::config::builder::preserve_comment_formatting(&line, field, value);
        Ok(content.replace(&line, &new_line))
    } else {
        // Field doesn't exist, add it at the end
        let mut updated = content.to_string();
        if !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(&format!("{} = {}\n", field, value));
        Ok(updated)
    }
}
