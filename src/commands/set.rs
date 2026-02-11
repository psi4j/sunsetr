//! Set command implementation for modifying configuration fields
//!
//! This command allows users to update individual settings in the active configuration
//! without manually editing files, while preserving comments and leveraging hot-reloading.

use crate::args::SetOperator;
use crate::common::utils::private_path;
use crate::core::period::Period;
use crate::state::ipc::client::IpcClient;
use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

/// Fields that support increment/decrement operators.
const INCREMENTABLE_FIELDS: &[&str] = &[
    "night_temp",
    "day_temp",
    "static_temp",
    "night_gamma",
    "day_gamma",
    "static_gamma",
];

/// Handle the set command - update configuration fields
///
/// # Arguments
/// * `fields` - Field-operator-value triples to update
/// * `target` - Optional target configuration:
///   - None: Use currently active configuration
///   - Some("default"): Use base configuration
///   - Some(name): Use specified preset
pub fn handle_set_command(
    fields: Vec<(String, SetOperator, String)>,
    target: Option<&str>,
) -> Result<()> {
    log_version!();

    if crate::io::instance::is_test_mode_active() {
        log_error_exit!(
            "Cannot modify configuration while test mode is active\n   Exit test mode first (press Escape in the test terminal)"
        );
        return Ok(());
    }

    let has_current_alias = fields
        .iter()
        .any(|(f, _, _)| f == "current_temp" || f == "current_gamma");
    if has_current_alias && target.is_some() {
        log_pipe!();
        log_error!("Cannot use 'current_temp' or 'current_gamma' with --target");
        log_indented!("These aliases resolve based on the running instance's current period");
        log_indented!("Use specific field names instead (night_temp, day_temp, static_temp)");
        log_end!();
        std::process::exit(1);
    }

    if crate::config::get_custom_config_dir().is_none() {
        let _ = crate::io::instance::get_running_instance()?;
    }

    let final_target = if target.is_none() {
        if has_current_alias {
            None
        } else if let Some(preset_name) = crate::state::preset::get_active_preset()? {
            log_pipe!();
            log_info!("Preset '{}' is currently active", preset_name);

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
            let result = crate::common::utils::show_dropdown_menu(&options, Some(prompt))?;

            match result {
                crate::common::utils::DropdownResult::Cancelled => {
                    log_pipe!();
                    log_info!("Operation cancelled");
                    log_end!();
                    return Ok(());
                }
                crate::common::utils::DropdownResult::Selected(selected_index) => {
                    match options[selected_index].1 {
                        None => {
                            log_pipe!();
                            log_info!("Operation cancelled");
                            log_end!();
                            return Ok(());
                        }
                        Some(target_choice) => target_choice,
                    }
                }
            }
        } else {
            None
        }
    } else {
        target
    };

    let config_path = match super::resolve_target_config_path(final_target) {
        Ok(path) => path,
        Err(e) => {
            if let Some(preset_error) = e.downcast_ref::<super::PresetNotFoundError>() {
                super::handle_preset_not_found_error(preset_error);
            } else {
                return Err(e);
            }
        }
    };

    let active_config_path = super::resolve_target_config_path(None)?;
    let is_active_config = config_path == active_config_path;
    let mut fields = fields;
    resolve_current_aliases(&mut fields)?;
    let fields = resolve_relative_operations(fields, &config_path)?;
    let mut validated_fields = Vec::new();

    for (field, value) in &fields {
        match validate_field_value(field, value) {
            Err(e) => {
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
                validated_fields.push((field.as_ref(), formatted_value));
            }
        }
    }

    let geo_path = config_path
        .parent()
        .map(|p| p.join("geo.toml"))
        .unwrap_or_default();

    let mut geo_fields = Vec::new();
    let mut regular_fields = Vec::new();

    if geo_path.exists() {
        for (field, value) in &validated_fields {
            if *field == "latitude" || *field == "longitude" {
                geo_fields.push((*field, value.clone()));
            } else {
                regular_fields.push((*field, value.clone()));
            }
        }
    } else {
        regular_fields = validated_fields
            .iter()
            .map(|(f, v)| (*f, v.clone()))
            .collect();
    }

    let mut changed = false;
    let mut updated_fields = Vec::new();

    #[cfg(debug_assertions)]
    eprintln!(
        "DEBUG [set]: Acquiring lockfile for {}",
        private_path(&config_path)
    );
    let lock_path = crate::io::lock::get_config_lock_path(&config_path);
    let _lockfile = crate::io::lock::LockFile::acquire(lock_path)?;
    #[cfg(debug_assertions)]
    eprintln!("DEBUG [set]: Lockfile acquired");

    if !geo_fields.is_empty() {
        if !geo_path.exists() {
            fs::write(&geo_path, "#[Private geo coordinates]\n")
                .with_context(|| format!("Failed to create geo.toml at {}", geo_path.display()))?;
        }

        let geo_content = fs::read_to_string(&geo_path)
            .with_context(|| format!("Failed to read geo.toml from {}", geo_path.display()))?;
        let mut geo_content = geo_content;

        for (field, formatted_value) in &geo_fields {
            let updated_content = update_field_in_content(&geo_content, field, formatted_value)?;
            if updated_content != geo_content {
                geo_content = updated_content;
                changed = true;
                updated_fields.push((field, formatted_value));
            }
        }

        if changed {
            atomic_write_file(&geo_path, &geo_content)
                .with_context(|| format!("Failed to write geo.toml at {}", geo_path.display()))?;
        }
    }

    if !regular_fields.is_empty() {
        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config from {}", config_path.display()))?;
        let mut content = content;

        for (field, formatted_value) in &regular_fields {
            let updated_content = update_field_in_content(&content, field, formatted_value)?;
            if updated_content != content {
                content = updated_content;
                changed = true;
                updated_fields.push((field, formatted_value));
            }
        }

        if changed {
            atomic_write_file(&config_path, &content)
                .with_context(|| format!("Failed to write config to {}", config_path.display()))?;
        }
    }

    if changed {
        log_block_start!("Updated configuration");
        for (field, value) in &updated_fields {
            log_indented!("{} = {}", field, value);
        }

        if !geo_fields.is_empty() && geo_path.exists() {
            log_indented!("in {}", private_path(&geo_path));
            if !regular_fields.is_empty() {
                log_indented!("and {}", private_path(&config_path));
            }
        } else {
            log_indented!("in {}", private_path(&config_path));
        }

        if is_active_config {
            if let Ok(pid) = crate::io::instance::get_running_instance_pid() {
                log_block_start!("Configuration reloaded successfully (PID: {})", pid);
            } else {
                log_block_start!("Start sunsetr to apply the new configuration");
            }
        } else if target == Some("default") {
            log_block_start!("Updated default configuration (not currently active)");
        } else if let Some(preset_name) = target {
            log_block_start!("Updated preset '{}' (not currently active)", preset_name);
        }
    } else {
        log_block_start!("Configuration unchanged");
        if fields.len() == 1 {
            log_indented!(
                "{} is already set to {}",
                &fields[0].0,
                &validated_fields[0].1
            );
        } else {
            log_indented!("All fields already have the specified values");
        }
    }

    log_end!();
    Ok(())
}

/// Resolve `current_temp` and `current_gamma` aliases to concrete field names.
///
/// These virtual aliases resolve to the field matching the running instance's active period:
/// - Static -> `static_temp` / `static_gamma`
/// - Day -> `day_temp` / `day_gamma`
/// - Night -> `night_temp` / `night_gamma`
/// - Sunset -> `night_temp` / `night_gamma` (transitioning toward night)
/// - Sunrise -> `day_temp` / `day_gamma` (transitioning toward day)
///
/// Requires a running sunsetr instance for IPC state lookup. Exits with an error
/// if no instance is running or communication fails.
fn resolve_current_aliases(fields: &mut [(String, SetOperator, String)]) -> Result<()> {
    let has_alias = fields
        .iter()
        .any(|(f, _, _)| f == "current_temp" || f == "current_gamma");
    if !has_alias {
        return Ok(());
    }

    let mut ipc_client = match IpcClient::connect() {
        Ok(client) => client,
        Err(_) => {
            let alias = fields
                .iter()
                .find(|(f, _, _)| f.starts_with("current_"))
                .map(|(f, _, _)| f.as_str())
                .unwrap_or("current_temp");
            log_pipe!();
            log_error!("Cannot resolve '{}': no running sunsetr instance", alias);
            log_indented!("Use specific field names instead (night_temp, day_temp, static_temp)");
            log_end!();
            std::process::exit(1);
        }
    };

    let display_state = match ipc_client.current() {
        Ok(state) => state,
        Err(_) => {
            let alias = fields
                .iter()
                .find(|(f, _, _)| f.starts_with("current_"))
                .map(|(f, _, _)| f.as_str())
                .unwrap_or("current_temp");
            log_pipe!();
            log_error!(
                "Cannot resolve '{}': failed to read state from running instance",
                alias
            );
            log_indented!("Check if sunsetr is running properly: sunsetr status");
            log_end!();
            std::process::exit(1);
        }
    };

    let (temp_field, gamma_field) = match display_state.period {
        Period::Static => ("static_temp", "static_gamma"),
        Period::Day => ("day_temp", "day_gamma"),
        Period::Night => ("night_temp", "night_gamma"),
        Period::Sunset => ("night_temp", "night_gamma"),
        Period::Sunrise => ("day_temp", "day_gamma"),
    };

    for (field, _, _) in fields.iter_mut() {
        if field == "current_temp" {
            log_block_start!(
                "Resolved current_temp → {} (period: {})",
                temp_field,
                display_state.period
            );
            *field = temp_field.to_string();
        } else if field == "current_gamma" {
            log_block_start!(
                "Resolved current_gamma → {} (period: {})",
                gamma_field,
                display_state.period
            );
            *field = gamma_field.to_string();
        }
    }

    Ok(())
}

/// Resolve increment/decrement operations to absolute values.
///
/// For `Assign` operations, passes through unchanged. For `Increment` and `Decrement`,
/// reads the current value from the config file, computes the new absolute value,
/// and returns it as a plain field=value pair for the existing validation pipeline.
fn resolve_relative_operations(
    fields: Vec<(String, SetOperator, String)>,
    config_path: &Path,
) -> Result<Vec<(String, String)>> {
    let has_relative = fields.iter().any(|(_, op, _)| *op != SetOperator::Assign);
    if !has_relative {
        return Ok(fields
            .into_iter()
            .map(|(field, _, value)| (field, value))
            .collect());
    }

    let content = if config_path.exists() {
        fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read config from {}", config_path.display()))?
    } else {
        String::new()
    };

    let mut resolved = Vec::with_capacity(fields.len());

    for (field, op, value) in fields {
        match op {
            SetOperator::Assign => {
                resolved.push((field, value));
            }
            SetOperator::Increment | SetOperator::Decrement => {
                if !INCREMENTABLE_FIELDS.contains(&field.as_str()) {
                    log_pipe!();
                    log_error!(
                        "Increment/decrement operators are only supported for temperature and gamma fields"
                    );
                    log_indented!("Supported: {}", INCREMENTABLE_FIELDS.join(", "));
                    log_end!();
                    std::process::exit(1);
                }

                let current_line = crate::config::builder::find_config_line(&content, &field);
                let current_str = match current_line {
                    Some(ref line) => extract_value_from_line(line),
                    None => {
                        let op_word = match op {
                            SetOperator::Increment => "increment",
                            SetOperator::Decrement => "decrement",
                            SetOperator::Assign => unreachable!(),
                        };
                        log_pipe!();
                        log_error!(
                            "Cannot {} '{}': field is not set in configuration",
                            op_word,
                            field
                        );
                        log_indented!("Set an explicit value first: sunsetr set {}=<value>", field);
                        log_end!();
                        std::process::exit(1);
                    }
                };

                let new_value = if field.ends_with("_temp") {
                    let current: i64 = current_str.trim().parse().with_context(|| {
                        format!(
                            "Current value for '{}' is not a valid integer: '{}'",
                            field, current_str
                        )
                    })?;
                    let delta: i64 = value.trim().parse().map_err(|_| {
                        log_error_exit!(
                            "Invalid {} value for '{}': '{}' is not a valid number",
                            if op == SetOperator::Increment {
                                "increment"
                            } else {
                                "decrement"
                            },
                            field,
                            value
                        );
                        std::process::exit(1);
                    })?;
                    let result = match op {
                        SetOperator::Increment => current + delta,
                        SetOperator::Decrement => current - delta,
                        SetOperator::Assign => unreachable!(),
                    };
                    result.to_string()
                } else {
                    let current: f64 = current_str.trim().parse().with_context(|| {
                        format!(
                            "Current value for '{}' is not a valid number: '{}'",
                            field, current_str
                        )
                    })?;
                    let delta: f64 = value.trim().parse().map_err(|_| {
                        log_error_exit!(
                            "Invalid {} value for '{}': '{}' is not a valid number",
                            if op == SetOperator::Increment {
                                "increment"
                            } else {
                                "decrement"
                            },
                            field,
                            value
                        );
                        std::process::exit(1);
                    })?;
                    let result = match op {
                        SetOperator::Increment => current + delta,
                        SetOperator::Decrement => current - delta,
                        SetOperator::Assign => unreachable!(),
                    };
                    if result.fract() == 0.0 {
                        (result as i64).to_string()
                    } else {
                        format!("{:.1}", result)
                    }
                };

                resolved.push((field, new_value));
            }
        }
    }

    Ok(resolved)
}

/// Extract the value portion from a config line like "field = value # comment".
///
/// Strips the key, equals sign, and any inline comment, returning just the raw value string.
fn extract_value_from_line(line: &str) -> &str {
    let value_part = line.split_once('=').map(|(_, v)| v).unwrap_or("");
    let value_part = if let Some(hash_pos) = value_part.find('#') {
        let before_hash = &value_part[..hash_pos];
        let quote_count = before_hash.chars().filter(|&c| c == '"').count();
        if quote_count % 2 == 0 {
            before_hash
        } else {
            value_part
        }
    } else {
        value_part
    };
    value_part.trim()
}

fn atomic_write_file(path: &std::path::Path, content: &str) -> Result<()> {
    let parent = path
        .parent()
        .context("Failed to get parent directory for atomic write")?;

    let mut temp_file = NamedTempFile::new_in(parent)
        .context("Failed to create temporary file for atomic write")?;

    temp_file
        .write_all(content.as_bytes())
        .context("Failed to write to temporary file")?;

    temp_file
        .flush()
        .context("Failed to flush temporary file")?;

    temp_file
        .persist(path)
        .map_err(|e| anyhow::anyhow!("Failed to atomically replace file: {}", e))?;

    Ok(())
}

fn validate_field_value(field: &str, value: &str) -> Result<String> {
    let toml_value = match field {
        "sunset" | "sunrise" | "backend" | "transition_mode" => {
            if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                value.to_string()
            } else {
                format!("\"{}\"", value)
            }
        }
        _ => value.to_string(),
    };

    let test_toml = format!("{} = {}", field, toml_value);

    let parsed_value: toml::Value = test_toml
        .parse()
        .with_context(|| format!("Invalid TOML syntax for field '{}'", field))?;

    let field_value = parsed_value
        .get(field)
        .context("Failed to extract field value")?;

    match field {
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

        "night_gamma" | "day_gamma" | "static_gamma" => {
            let gamma = field_value
                .as_float()
                .or_else(|| field_value.as_integer().map(|i| i as f64))
                .context("Gamma must be a number")?;
            if !(crate::common::constants::MINIMUM_GAMMA..=crate::common::constants::MAXIMUM_GAMMA)
                .contains(&gamma)
            {
                anyhow::bail!(
                    "Gamma must be between {}% and {}%",
                    crate::common::constants::MINIMUM_GAMMA,
                    crate::common::constants::MAXIMUM_GAMMA
                );
            }
            if field_value.is_integer() || gamma.fract() == 0.0 {
                Ok((gamma as i64).to_string())
            } else {
                Ok(format!("{:.1}", gamma))
            }
        }

        "sunset" | "sunrise" => {
            let time_str = field_value.as_str().context("Time must be a string")?;

            use chrono::NaiveTime;
            NaiveTime::parse_from_str(time_str, "%H:%M:%S")
                .or_else(|_| NaiveTime::parse_from_str(time_str, "%H:%M"))
                .with_context(|| {
                    format!("Invalid time format: {} (use HH:MM or HH:MM:SS)", time_str)
                })?;

            let formatted = if time_str.matches(':').count() == 1 {
                format!("\"{}:00\"", time_str)
            } else {
                format!("\"{}\"", time_str)
            };
            Ok(formatted)
        }

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
            if field_value.is_integer() || duration.fract() == 0.0 {
                Ok((duration as i64).to_string())
            } else {
                Ok(format!("{:.1}", duration))
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

        "smoothing" => {
            let bool_value = field_value.as_bool().context("Must be true or false")?;
            Ok(bool_value.to_string())
        }

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
            anyhow::bail!("Unknown field '{}'", field)
        }
    }
}

fn update_field_in_content(content: &str, field: &str, value: &str) -> Result<String> {
    let existing_line = crate::config::builder::find_config_line(content, field);

    if let Some(line) = existing_line {
        let new_line = crate::config::builder::preserve_comment_formatting(&line, field, value);
        Ok(content.replace(&line, &new_line))
    } else {
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
    log_block_start!("Usage: sunsetr set [OPTIONS] <field>[+|-]=<value> [...]");
    log_block_start!("Options:");
    log_indented!("-t, --target <name>  Target configuration to modify");
    log_indented!("                     'default' = base configuration");
    log_indented!("                     <name> = named preset");
    log_indented!("                     (omit to use active configuration)");
    log_block_start!("Operators:");
    log_indented!("<field>=<value>      Set field to value");
    log_indented!("<field>+=<value>     Increment field by value (temp/gamma only)");
    log_indented!("<field>-=<value>     Decrement field by value (temp/gamma only)");
    log_block_start!("Aliases:");
    log_indented!("current_temp             Resolves to active period's temp field");
    log_indented!("current_gamma            Resolves to active period's gamma field");
    log_block_start!("For detailed help with examples, try: sunsetr help set");
    log_end!();
}

/// Display detailed help for the set command (help subcommand)
pub fn display_help() {
    log_version!();
    log_block_start!("set - Update configuration fields");
    log_block_start!("Usage: sunsetr set [OPTIONS] <field>[+|-]=<value> [...]");
    log_block_start!("Options:");
    log_indented!("-t, --target <name>  Target configuration to modify");
    log_indented!("                     'default' = base configuration");
    log_indented!("                     <name> = named preset");
    log_indented!("                     (omit to use active configuration)");
    log_block_start!("Operators:");
    log_indented!("<field>=<value>      Set field to value");
    log_indented!("<field>+=<value>     Increment field by value (temp/gamma only)");
    log_indented!("<field>-=<value>     Decrement field by value (temp/gamma only)");
    log_block_start!("Available Fields:");
    log_indented!("backend              Backend: auto, hyprland, or wayland");
    log_indented!("transition_mode      Mode: geo, static, center, finish_by, start_at");
    log_indented!("smoothing            Enable smooth transitions (true/false)");
    log_indented!("startup_duration     Smooth startup time in seconds");
    log_indented!("shutdown_duration    Smooth shutdown time in seconds");
    log_indented!("adaptive_interval    Smooth transition interval in seconds");
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
    log_block_start!("Aliases (require running instance):");
    log_indented!("current_temp         Resolves to active period's temp field");
    log_indented!("current_gamma        Resolves to active period's gamma field");
    log_indented!(
        "                     Day/Sunrise → day_*, Night/Sunset → night_*, Static → static_*"
    );
    log_block_start!("Examples:");
    log_indented!("# Update active configuration");
    log_indented!("sunsetr set night_temp=3500 night_gamma=85");
    log_pipe!();
    log_indented!("# Increment/decrement values");
    log_indented!("sunsetr set night_temp+=500 night_gamma+=10");
    log_indented!("sunsetr set static_temp-=100 static_gamma-=2");
    log_pipe!();
    log_indented!("# Adjust current period's temperature without knowing the period");
    log_indented!("sunsetr set current_temp+=500");
    log_indented!("sunsetr set current_temp=3500 current_gamma=90");
    log_pipe!();
    log_indented!("# Mix operators in a single command");
    log_indented!("sunsetr set night_temp+=200 day_temp=6500 static_gamma-=5");
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
