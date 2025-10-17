//! Set command implementation for modifying configuration fields
//!
//! This command allows users to update individual settings in the active configuration
//! without manually editing files, while preserving comments and leveraging hot-reloading.

use crate::common::utils::private_path;
use anyhow::{Context, Result};
use std::fs;

/// Handle the set command - update configuration fields
///
/// # Arguments
/// * `fields` - Field-value pairs to update
/// * `target` - Optional target configuration:
///   - None: Use currently active configuration
///   - Some("default"): Use base configuration
///   - Some(name): Use specified preset
pub fn handle_set_command(fields: &[(String, String)], target: Option<&str>) -> Result<()> {
    // Always print version header since we're handling a set command
    log_version!();

    // Check if test mode is active
    if crate::io::instance::is_test_mode_active() {
        log_pipe!();
        log_warning!("Cannot modify configuration while test mode is active");
        log_indented!("Exit test mode first (press Escape in the test terminal)");
        log_end!();
        return Ok(());
    }

    // If no --config flag was provided, try to use the config directory from the running instance
    // This ensures we modify the same configuration that the running sunsetr is using
    if crate::config::get_custom_config_dir().is_none()
        && let Some(info) = crate::io::instance::get_running_instance()?
        && let Some(ref config_dir) = info.config_dir
        && let Err(e) =
            crate::config::set_config_dir(Some(config_dir.to_string_lossy().to_string()))
    {
        log_error_exit!("{}", e);
    }

    // If no target was specified and a preset is active, prompt the user
    // to confirm they want to edit the preset instead of the default config
    let final_target = if target.is_none() {
        if let Some(preset_name) = crate::config::Config::get_active_preset()? {
            log_pipe!();
            log_warning!("Preset '{}' is currently active", preset_name);

            let options = vec![
                (
                    format!("Edit the active preset '{}'", preset_name),
                    Some(None),
                ),
                (
                    "Edit the default configuration".to_string(),
                    Some(Some("default")),
                ),
                ("Cancel".to_string(), None),
            ];

            let prompt = "Which configuration would you like to modify?";
            let selected_index = crate::common::utils::show_dropdown_menu(
                &options,
                Some(prompt),
                Some("Operation cancelled"),
            )?;

            // Check if user chose to cancel
            match options[selected_index].1 {
                None => {
                    log_pipe!();
                    log_info!("Operation cancelled");
                    log_end!();
                    return Ok(());
                }
                Some(target_choice) => target_choice,
            }
        } else {
            None
        }
    } else {
        target
    };

    // Get the config path based on the final target and check if it's the active one
    let config_path = match super::resolve_target_config_path(final_target) {
        Ok(path) => path,
        Err(e) => {
            // Check if it's a PresetNotFoundError
            if let Some(preset_error) = e.downcast_ref::<super::PresetNotFoundError>() {
                super::handle_preset_not_found_error(preset_error);
            } else {
                return Err(e);
            }
        }
    };

    // Determine if we're updating the currently active configuration
    let active_config_path = super::resolve_target_config_path(None)?;
    let is_active_config = config_path == active_config_path;

    // Validate all fields first before making any changes
    let mut validated_fields = Vec::new();
    for (field, value) in fields {
        match validate_field_value(field, value) {
            Err(e) => {
                // Check if it's an unknown field error
                if e.to_string().starts_with("Unknown field") {
                    log_pipe!();
                    log_error!("Unknown configuration field: '{}'", field);
                    log_block_start!("Available fields:");
                    log_indented!("backend, transition_mode");
                    log_indented!(
                        "smoothing, startup_duration, shutdown_duration, adaptive_interval"
                    );
                    log_indented!("night_temp, day_temp, night_gamma, day_gamma, update_interval");
                    log_indented!("static_temp, static_gamma");
                    log_indented!("sunset, sunrise, transition_duration");
                    log_indented!("latitude, longitude");
                } else {
                    let error_msg = e.to_string();
                    // Handle multi-line errors (ones with \n)
                    if let Some((first_line, rest)) = error_msg.split_once('\n') {
                        log_error_exit!("{}: {}", field, first_line);
                        for line in rest.lines() {
                            println!("  {}", line);
                        }
                    } else {
                        log_error_exit!("{}: {}", field, error_msg);
                    }
                }
                std::process::exit(1);
            }
            Ok(formatted_value) => {
                validated_fields.push((field.as_str(), formatted_value));
            }
        }
    }

    // Check if we need to handle coordinates specially (via geo.toml)
    let geo_path = config_path
        .parent()
        .map(|p| p.join("geo.toml"))
        .unwrap_or_default();

    let mut geo_fields = Vec::new();
    let mut regular_fields = Vec::new();

    // Separate geo fields from regular fields if geo.toml exists
    if geo_path.exists() {
        for (field, value) in &validated_fields {
            if *field == "latitude" || *field == "longitude" {
                geo_fields.push((*field, value.clone()));
            } else {
                regular_fields.push((*field, value.clone()));
            }
        }
    } else {
        // No geo.toml, all fields go to main config
        regular_fields = validated_fields
            .iter()
            .map(|(f, v)| (*f, v.clone()))
            .collect();
    }

    let mut changed = false;
    let mut updated_fields = Vec::new();

    // Update geo.toml if needed
    if !geo_fields.is_empty() {
        let mut geo_content = fs::read_to_string(&geo_path)
            .unwrap_or_else(|_| "#[Private geo coordinates]\n".to_string());

        for (field, formatted_value) in &geo_fields {
            let updated_content = update_field_in_content(&geo_content, field, formatted_value)?;
            if updated_content != geo_content {
                geo_content = updated_content;
                changed = true;
                updated_fields.push((field, formatted_value));
            }
        }

        if changed {
            fs::write(&geo_path, &geo_content)
                .with_context(|| format!("Failed to write geo.toml at {}", geo_path.display()))?;
        }
    }

    // Update main config for non-geo fields
    if !regular_fields.is_empty() {
        let mut content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config from {}", config_path.display()))?;

        for (field, formatted_value) in &regular_fields {
            let updated_content = update_field_in_content(&content, field, formatted_value)?;
            if updated_content != content {
                content = updated_content;
                changed = true;
                updated_fields.push((field, formatted_value));
            }
        }

        if changed {
            fs::write(&config_path, &content)
                .with_context(|| format!("Failed to write config to {}", config_path.display()))?;
        }
    }

    // Report what was updated
    if changed {
        log_block_start!("Updated configuration");
        for (field, value) in &updated_fields {
            log_indented!("{} = {}", field, value);
        }

        // Show where updates were written
        if !geo_fields.is_empty() && geo_path.exists() {
            log_indented!("in {}", private_path(&geo_path));
            if !regular_fields.is_empty() {
                log_indented!("and {}", private_path(&config_path));
            }
        } else {
            log_indented!("in {}", private_path(&config_path));
        }

        // Only show reload message if we updated the active configuration
        if is_active_config {
            // If sunsetr is running and we updated the active config, it will automatically reload via file watcher
            if let Ok(pid) = crate::io::instance::get_running_instance_pid() {
                log_block_start!("Configuration reloaded successfully (PID: {})", pid);
            } else {
                log_block_start!("Start sunsetr to apply the new configuration");
            }
        } else {
            // Updated a non-active configuration
            if target == Some("default") {
                log_block_start!("Updated default configuration (not currently active)");
            } else if let Some(preset_name) = target {
                log_block_start!("Updated preset '{}' (not currently active)", preset_name);
            }
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
            if temp < crate::common::constants::MINIMUM_TEMP as i64
                || temp > crate::common::constants::MAXIMUM_TEMP as i64
            {
                anyhow::bail!(
                    "Temperature must be between {} and {} K",
                    crate::common::constants::MINIMUM_TEMP,
                    crate::common::constants::MAXIMUM_TEMP
                );
            }
            Ok(temp.to_string())
        }

        // Gamma fields (stored as percentage 10-200)
        "night_gamma" | "day_gamma" | "static_gamma" => {
            let gamma = field_value
                .as_float()
                .or_else(|| field_value.as_integer().map(|i| i as f64))
                .context("Gamma must be a number")?;
            if gamma < crate::common::constants::MINIMUM_GAMMA as f64
                || gamma > crate::common::constants::MAXIMUM_GAMMA as f64
            {
                anyhow::bail!(
                    "Gamma must be between {}% and {}%",
                    crate::common::constants::MINIMUM_GAMMA,
                    crate::common::constants::MAXIMUM_GAMMA
                );
            }
            // Preserve the user's format - if they passed an integer, keep it as an integer
            if field_value.is_integer() {
                Ok(gamma as i64).map(|i| i.to_string())
            } else {
                // Only use decimal if the user provided a decimal value
                if gamma.fract() == 0.0 {
                    Ok((gamma as i64).to_string())
                } else {
                    Ok(format!("{:.1}", gamma))
                }
            }
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
            if duration < crate::common::constants::MINIMUM_TRANSITION_DURATION as i64
                || duration > crate::common::constants::MAXIMUM_TRANSITION_DURATION as i64
            {
                anyhow::bail!(
                    "Transition duration must be between {} and {} minutes",
                    crate::common::constants::MINIMUM_TRANSITION_DURATION,
                    crate::common::constants::MAXIMUM_TRANSITION_DURATION
                );
            }
            Ok(duration.to_string())
        }

        "startup_duration" | "shutdown_duration" => {
            let duration = field_value
                .as_float()
                .or_else(|| field_value.as_integer().map(|i| i as f64))
                .context("Duration must be a number (seconds)")?;
            if !(crate::common::constants::MINIMUM_SMOOTH_TRANSITION_DURATION
                ..=crate::common::constants::MAXIMUM_SMOOTH_TRANSITION_DURATION)
                .contains(&duration)
            {
                anyhow::bail!(
                    "Smooth transition duration must be between {} and {} seconds",
                    crate::common::constants::MINIMUM_SMOOTH_TRANSITION_DURATION,
                    crate::common::constants::MAXIMUM_SMOOTH_TRANSITION_DURATION
                );
            }
            // Preserve the user's format
            if field_value.is_integer() {
                Ok((duration as i64).to_string())
            } else {
                // Only use decimal if necessary
                if duration.fract() == 0.0 {
                    Ok((duration as i64).to_string())
                } else {
                    Ok(format!("{:.1}", duration))
                }
            }
        }

        "update_interval" => {
            let interval = field_value
                .as_integer()
                .context("Update interval must be an integer (seconds)")?;
            if interval < crate::common::constants::MINIMUM_UPDATE_INTERVAL as i64
                || interval > crate::common::constants::MAXIMUM_UPDATE_INTERVAL as i64
            {
                anyhow::bail!(
                    "Update interval must be between {} and {} seconds",
                    crate::common::constants::MINIMUM_UPDATE_INTERVAL,
                    crate::common::constants::MAXIMUM_UPDATE_INTERVAL
                );
            }
            Ok(interval.to_string())
        }

        "adaptive_interval" => {
            let interval = field_value
                .as_integer()
                .context("Adaptive interval must be an integer (milliseconds)")?;
            if interval < crate::common::constants::MINIMUM_ADAPTIVE_INTERVAL as i64
                || interval > crate::common::constants::MAXIMUM_ADAPTIVE_INTERVAL as i64
            {
                anyhow::bail!(
                    "Adaptive interval must be between {} and {} milliseconds",
                    crate::common::constants::MINIMUM_ADAPTIVE_INTERVAL,
                    crate::common::constants::MAXIMUM_ADAPTIVE_INTERVAL
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
                    "'{}' is not a valid backend\nUse: auto, hyprland, hyprsunset, or wayland",
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
                    "'{}' is not a valid transition mode\nUse: geo, finish_by, start_at, center, or static",
                    mode
                ),
            }
        }

        // Coordinate fields
        "latitude" => {
            let lat = field_value
                .as_float()
                .or_else(|| field_value.as_integer().map(|i| i as f64))
                .context("Latitude must be a number")?;
            if !(-90.0..=90.0).contains(&lat) {
                anyhow::bail!("Latitude must be between -90 and 90 degrees");
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

        _ => {
            // Return error that will be caught and displayed by calling code
            anyhow::bail!("Unknown field '{}'", field)
        }
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

/// Display usage help for the set command (--help flag)
pub fn show_usage() {
    log_version!();
    log_block_start!("Usage: sunsetr set [OPTIONS] <field>=<value> [<field>=<value>...]");
    log_block_start!("Options:");
    log_indented!("-t, --target <name>  Target configuration to modify");
    log_indented!("                     'default' = base configuration");
    log_indented!("                     <name> = named preset");
    log_indented!("                     (omit to use active configuration)");
    log_block_start!("Arguments:");
    log_indented!("<field>=<value>      Configuration field and its new value");
    log_indented!("                     Multiple pairs can be specified");
    log_block_start!("For detailed help with examples, try: sunsetr help set");
    log_end!();
}

/// Display detailed help for the set command (help subcommand)
pub fn display_help() {
    log_version!();
    log_block_start!("set - Update configuration fields");
    log_block_start!("Usage: sunsetr set [OPTIONS] <field>=<value> [<field>=<value>...]");
    log_block_start!("Options:");
    log_indented!("-t, --target <name>  Target configuration to modify");
    log_indented!("                     'default' = base configuration");
    log_indented!("                     <name> = named preset");
    log_indented!("                     (omit to use active configuration)");
    log_block_start!("Arguments:");
    log_indented!("<field>=<value>      Configuration field and its new value");
    log_indented!("                     Multiple pairs can be specified");
    log_block_start!("Available Fields:");
    log_indented!("backend              Backend: auto, hyprland, or wayland");
    log_indented!("transition_mode      Mode: geo, static, center, finish_by, start_at");
    log_indented!("smoothing            Enable smooth transitions (true/false)");
    log_indented!("startup_duration     Smooth startup time in seconds");
    log_indented!("shutdown_duration    Smooth shutdown time in seconds");
    log_indented!("adaptive_interval    Smooth transition interval in seconds");
    log_indented!("night_temp           Night color temperature (1000-10000)");
    log_indented!("night_gamma          Night gamma percentage (10-200)");
    log_indented!("day_temp             Day color temperature (1000-10000)");
    log_indented!("day_gamma            Day gamma percentage (10-200)");
    log_indented!("update_interval      Main update interval in seconds");
    log_indented!("static_temp          Static mode temperature (1000-10000)");
    log_indented!("static_gamma         Static mode gamma percentage (10-200)");
    log_indented!("sunset               Sunset time (HH:MM:SS format)");
    log_indented!("sunrise              Sunrise time (HH:MM:SS format)");
    log_indented!("transition_duration  Transition time in minutes");
    log_indented!("latitude             Geographic latitude (-90 to 90)");
    log_indented!("longitude            Geographic longitude (-180 to 180)");
    log_block_start!("Examples:");
    log_indented!("# Update active configuration");
    log_indented!("sunsetr set night_temp=3500 night_gamma=85");
    log_pipe!();
    log_indented!("# Update specific preset");
    log_indented!("sunsetr set --target gaming static_temp=3000");
    log_indented!("sunsetr set -t night night_temp=2800");
    log_pipe!();
    log_indented!("# Update default configuration while preset is active");
    log_indented!("sunsetr set --target default day_temp=6500");
    log_pipe!();
    log_indented!("# Update configuration in custom base directory");
    log_indented!("sunsetr --config ~/.dotfiles/sunsetr/ set --target default day_temp=6500");
    log_pipe!();
    log_indented!("# Set multiple fields at once");
    log_indented!("sunsetr set night_temp=3000 day_temp=6500 transition_duration=60");
    log_end!();
}
