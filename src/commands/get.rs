//! Get command implementation for reading configuration fields
//!
//! This command allows users to read configuration values programmatically
//! in both human-readable and JSON formats.

use anyhow::{Context, Result};
use serde_json::json;
use std::fs;

/// Handle the get command - read configuration fields
///
/// # Arguments
/// * `fields` - Field names to retrieve (or special values like "all" or "active")
/// * `target` - Optional target configuration:
///   - None: Use currently active configuration
///   - Some("default"): Use base configuration
///   - Some(name): Use specified preset
/// * `json` - Whether to output in JSON format
pub fn handle_get_command(fields: &[String], target: Option<&str>, json: bool) -> Result<()> {
    // Don't print version header for get command - we want clean output

    // If no --config flag was provided, check if there's a running instance
    // get_running_instance() will automatically set the config directory from the lock file
    if crate::config::get_custom_config_dir().is_none() {
        let _ = crate::io::instance::get_running_instance()?;
    }

    // Get the config path and load configuration
    let config_path = match super::resolve_target_config_path(target) {
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
    let config_content = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config from {}", config_path.display()))?;

    // Check for geo.toml for coordinate fields
    let geo_path = config_path
        .parent()
        .map(|p| p.join("geo.toml"))
        .unwrap_or_default();

    let geo_content = if geo_path.exists() {
        Some(
            fs::read_to_string(&geo_path)
                .with_context(|| format!("Failed to read geo.toml from {}", geo_path.display()))?,
        )
    } else {
        None
    };

    // Parse configs as TOML
    let config_toml: toml::Value = config_content
        .parse()
        .with_context(|| "Failed to parse configuration as TOML")?;

    let geo_toml: Option<toml::Value> = geo_content
        .map(|content| content.parse())
        .transpose()
        .with_context(|| "Failed to parse geo.toml as TOML")?;

    // Determine which fields to retrieve
    let fields_to_get: Vec<String> = if fields.len() == 1 && fields[0] == "all" {
        // Get all available fields
        get_all_field_names()
    } else {
        // Use specified fields
        fields.to_vec()
    };

    // Retrieve field values
    let mut values = Vec::new();
    let mut errors = Vec::new();

    for field in &fields_to_get {
        match get_field_value(field, &config_toml, geo_toml.as_ref()) {
            Ok(value) => values.push((field.clone(), value)),
            Err(e) => errors.push((field.clone(), e.to_string())),
        }
    }

    // Handle errors - but only for non-"all" requests
    // When requesting "all", we expect some fields might not exist
    if !(errors.is_empty() || (fields.len() == 1 && fields[0] == "all")) {
        if json {
            // For JSON mode, output error as JSON
            let error_msg = if errors.len() == 1 {
                format!("Unknown field: {}", errors[0].0)
            } else {
                format!(
                    "Unknown fields: {}",
                    errors
                        .iter()
                        .map(|(f, _)| f.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };

            let error_json = if errors
                .iter()
                .any(|(f, _)| !get_all_field_names().contains(f))
            {
                // Unknown field error - include available fields
                json!({
                    "error": error_msg,
                    "type": "UnknownField",
                    "available": get_all_field_names()
                })
            } else {
                json!({
                    "error": error_msg,
                    "type": "ConfigError"
                })
            };

            eprintln!("{}", serde_json::to_string(&error_json)?);
            std::process::exit(1);
        } else {
            // Human-readable error
            for (field, _) in &errors {
                if !get_all_field_names().contains(field) {
                    log_pipe!();
                    log_error!("Unknown configuration field: '{}'", field);
                    log_block_start!("Available fields:");
                    log_indented!("all (special: returns all fields)");
                    log_indented!("backend, transition_mode");
                    log_indented!(
                        "smoothing, startup_duration, shutdown_duration, adaptive_interval"
                    );
                    log_indented!("night_temp, day_temp, night_gamma, day_gamma, update_interval");
                    log_indented!("static_temp, static_gamma");
                    log_indented!("sunset, sunrise, transition_duration");
                    log_indented!("latitude, longitude");
                    log_end!();
                    std::process::exit(1);
                }
            }
        }
    }

    // Output results
    if json {
        // JSON output
        let mut json_obj = serde_json::Map::new();
        for (field, value) in values {
            json_obj.insert(field, json!(value));
        }
        println!("{}", serde_json::to_string(&json_obj)?);
    } else {
        // Human-readable output
        if values.len() == 1 && fields.len() == 1 && fields[0] != "all" {
            // Single field - just output the value
            println!("{}", values[0].1);
        } else {
            // Multiple fields - output as key=value pairs
            for (field, value) in values {
                println!("{}={}", field, value);
            }
        }
    }

    Ok(())
}

/// Get a field value from the configuration
fn get_field_value(field: &str, config: &toml::Value, geo: Option<&toml::Value>) -> Result<String> {
    // Check geo.toml first for coordinate fields
    if (field == "latitude" || field == "longitude")
        && let Some(geo_toml) = geo
        && let Some(value) = geo_toml.get(field)
    {
        return format_toml_value(value);
    }

    // Check main config
    if let Some(value) = config.get(field) {
        format_toml_value(value)
    } else {
        anyhow::bail!("Field '{}' not found in configuration", field)
    }
}

/// Format a TOML value as a string for output
fn format_toml_value(value: &toml::Value) -> Result<String> {
    match value {
        toml::Value::String(s) => Ok(s.clone()),
        toml::Value::Integer(i) => Ok(i.to_string()),
        toml::Value::Float(f) => {
            // Format floats cleanly - no unnecessary decimals
            if f.fract() == 0.0 {
                Ok((*f as i64).to_string())
            } else {
                Ok(format!("{:.1}", f))
            }
        }
        toml::Value::Boolean(b) => Ok(b.to_string()),
        toml::Value::Datetime(dt) => Ok(dt.to_string()),
        toml::Value::Array(_) | toml::Value::Table(_) => {
            anyhow::bail!("Complex values (arrays/tables) are not supported")
        }
    }
}

/// Get all available field names
fn get_all_field_names() -> Vec<String> {
    vec![
        "backend".to_string(),
        "transition_mode".to_string(),
        "smoothing".to_string(),
        "startup_duration".to_string(),
        "shutdown_duration".to_string(),
        "adaptive_interval".to_string(),
        "night_temp".to_string(),
        "day_temp".to_string(),
        "night_gamma".to_string(),
        "day_gamma".to_string(),
        "update_interval".to_string(),
        "static_temp".to_string(),
        "static_gamma".to_string(),
        "sunset".to_string(),
        "sunrise".to_string(),
        "transition_duration".to_string(),
        "latitude".to_string(),
        "longitude".to_string(),
    ]
}

/// Display usage help for the get command (--help flag)
pub fn show_usage() {
    log_version!();
    log_block_start!("Usage: sunsetr get [OPTIONS] <field> [<field>...]");
    log_block_start!("Options:");
    log_indented!("-t, --target <name>  Target configuration to read");
    log_indented!("                     'default' = base configuration");
    log_indented!("                     <name> = named preset");
    log_indented!("                     (omit to use active configuration)");
    log_indented!("-j, --json           Output in JSON format");
    log_block_start!("Arguments:");
    log_indented!("<field>              Configuration field(s) to retrieve");
    log_indented!("                     Use 'all' to get all fields");
    log_block_start!("For detailed help with examples, try: sunsetr help get");
    log_end!();
}

/// Display detailed help for the get command (help subcommand)
pub fn display_help() {
    log_version!();
    log_block_start!("get - Read configuration fields");
    log_block_start!("Usage: sunsetr get [OPTIONS] <field> [<field>...]");
    log_block_start!("Options:");
    log_indented!("-t, --target <name>  Target configuration to read");
    log_indented!("                     'default' = base configuration");
    log_indented!("                     <name> = named preset");
    log_indented!("                     (omit to use active configuration)");
    log_indented!("-j, --json           Output in JSON format");
    log_block_start!("Special Fields:");
    log_indented!("all                  Get all configuration fields");
    log_block_start!("Available Fields:");
    log_indented!("backend              Backend: auto, hyprland, or wayland");
    log_indented!("transition_mode      Mode: geo, static, center, finish_by, start_at");
    log_indented!("smoothing            Enable smooth transitions (true/false)");
    log_indented!("startup_duration     Smooth startup time in seconds");
    log_indented!("shutdown_duration    Smooth shutdown time in seconds");
    log_indented!("adaptive_interval    Smooth transition interval in milliseconds");
    log_indented!("night_temp           Night color temperature (1000-20000)");
    log_indented!("night_gamma          Night gamma percentage (10-200)");
    log_indented!("day_temp             Day color temperature (1000-20000)");
    log_indented!("day_gamma            Day gamma percentage (10-200)");
    log_indented!("update_interval      Main update interval in seconds");
    log_indented!("static_temp          Static mode temperature (1000-20000)");
    log_indented!("static_gamma         Static mode gamma percentage (10-200)");
    log_indented!("sunset               Sunset time (HH:MM:SS format)");
    log_indented!("sunrise              Sunrise time (HH:MM:SS format)");
    log_indented!("transition_duration  Transition time in minutes");
    log_indented!("latitude             Geographic latitude (-90 to 90)");
    log_indented!("longitude            Geographic longitude (-180 to 180)");
    log_block_start!("Examples:");
    log_indented!("# Get single field value");
    log_indented!("sunsetr get night_temp");
    log_indented!("3500");
    log_pipe!();
    log_indented!("# Get multiple field values");
    log_indented!("sunsetr get night_temp day_temp");
    log_indented!("night_temp=3500");
    log_indented!("day_temp=6500");
    log_pipe!();
    log_indented!("# Get all configuration fields");
    log_indented!("sunsetr get all");
    log_pipe!();
    log_indented!("# Get values from specific preset");
    log_indented!("sunsetr get --target gaming night_temp");
    log_indented!("3000");
    log_pipe!();
    log_indented!("# Get values in JSON format");
    log_indented!("sunsetr get --json night_temp day_temp");
    log_indented!("{{\"night_temp\":\"3500\",\"day_temp\":\"6500\"}}");
    log_pipe!();
    log_indented!("# Get all values from preset as JSON");
    log_indented!("sunsetr get -t night --json all");
    log_end!();
}
