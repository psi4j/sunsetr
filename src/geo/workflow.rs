//! Workflow orchestration for geo command.
//!
//! This module handles the complete geo selection workflow including:
//! - Preset detection and configuration target selection
//! - City selection coordination
//! - Configuration updates (default or preset)

use anyhow::{Context, Result};

use crate::config::Config;
use crate::geo::{GeoSelectionResult, log_solar_debug_info, select_city_interactive};

/// Configuration target for geo updates.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigTarget {
    /// Update the default configuration
    Default,
    /// Update a specific preset
    Preset(String),
}

/// Workflow orchestrator for geo selection.
pub struct GeoWorkflow {
    debug_enabled: bool,
    target: Option<String>,
}

impl GeoWorkflow {
    /// Create a new workflow orchestrator.
    pub fn new(debug_enabled: bool, target: Option<String>) -> Self {
        Self {
            debug_enabled,
            target,
        }
    }

    /// Run the complete geo selection workflow.
    ///
    /// This orchestrates:
    /// 1. Detect active presets and prompt for update target
    /// 2. Run interactive city selection with fuzzy search
    /// 3. Update configuration (default or preset) with new coordinates
    pub fn run(&self) -> Result<GeoSelectionResult> {
        log_version!();

        if self.debug_enabled {
            log_pipe!();
            log_debug!("Debug mode enabled for geo selection");
        }

        let target = self.determine_target()?;
        if target.is_none() {
            return Ok(GeoSelectionResult::Cancelled);
        }
        let target = target.unwrap();

        let coords = self.select_city()?;
        if coords.is_none() {
            return Ok(GeoSelectionResult::Cancelled);
        }
        let (latitude, longitude, city_name) = coords.unwrap();

        self.update_configuration(latitude, longitude, &city_name, target)?;

        Ok(GeoSelectionResult::Updated)
    }

    /// Determine configuration target (default or preset).
    ///
    /// Returns None if user cancels the operation.
    fn determine_target(&self) -> Result<Option<ConfigTarget>> {
        if let Some(name) = &self.target {
            return Ok(Some(Self::resolve_explicit_target(name)?));
        }

        let active_preset = crate::state::preset::get_active_preset()?;

        if let Some(preset_name) = &active_preset {
            log_pipe!();
            log_info!("Active preset '{}' detected", preset_name);
            log_indented!("The geo command can update either the preset or the default config.");

            let options = vec![
                ("Cancel operation".to_string(), "cancel"),
                ("Update default configuration".to_string(), "default"),
                (format!("Update preset '{}'", preset_name), "preset"),
            ];

            let result = crate::common::utils::show_dropdown_menu(
                &options,
                Some("Which configuration would you like to update?"),
            )?;

            match result {
                crate::common::utils::DropdownResult::Cancelled => Ok(None),
                crate::common::utils::DropdownResult::Selected(selection_index) => {
                    match options[selection_index].1 {
                        "cancel" => {
                            // User explicitly selected cancel - return None without logging
                            // (top-level handler will log)
                            Ok(None)
                        }
                        "default" => Ok(Some(ConfigTarget::Default)),
                        "preset" => Ok(Some(ConfigTarget::Preset(preset_name.clone()))),
                        _ => unreachable!(),
                    }
                }
            }
        } else {
            Ok(Some(ConfigTarget::Default))
        }
    }

    /// Resolve an explicit `--target` to a config target, validating a
    /// named preset the same way get and set do.
    fn resolve_explicit_target(name: &str) -> Result<ConfigTarget> {
        if name == "default" {
            return Ok(ConfigTarget::Default);
        }

        match crate::commands::resolve_target_config_path(Some(name)) {
            Ok(_) => Ok(ConfigTarget::Preset(name.to_string())),
            Err(e) => match e.downcast_ref::<crate::commands::PresetNotFoundError>() {
                Some(preset_error) => crate::commands::handle_preset_not_found_error(preset_error),
                None => Err(e),
            },
        }
    }

    /// Run city selection and return coordinates.
    ///
    /// This method provides a comprehensive city selection experience:
    /// 1. Interactive fuzzy search across 10,000+ world cities
    /// 2. Real-time filtering as the user types
    /// 3. Latitude capping at ±65° for extreme locations
    /// 4. Solar calculation with enhanced twilight transitions (+10° to -2°)
    /// 5. Display of calculated sunrise/sunset times with timezone handling
    ///
    /// # Returns
    /// * `Some((latitude, longitude, city_name))` - Selected coordinates and city name
    /// * `None` - If user cancels the selection
    /// * `Err(_)` - If selection fails or solar calculations error
    fn select_city(&self) -> Result<Option<(f64, f64, String)>> {
        use anyhow::Context;

        let (mut latitude, longitude, city_name) = match select_city_interactive() {
            Ok(coords) => coords,
            Err(e) => {
                if e.to_string().contains("cancelled") {
                    return Ok(None);
                }
                return Err(e).context("Failed to run interactive city selection");
            }
        };

        // Cap latitude at ±65° to avoid solar calculation edge cases
        let was_capped = latitude.abs() > 65.0;
        if was_capped {
            let original_latitude = latitude;
            latitude = 65.0 * latitude.signum();

            log_pipe!();
            log_warning!(
                "⚠️ Latitude capped at 65°{} (selected city was at {:.4}°{})",
                if latitude >= 0.0 { "N" } else { "S" },
                original_latitude.abs(),
                if latitude >= 0.0 { "N" } else { "S" },
            );
            log_indented!("Are you researching extremophile bacteria under the ice caps?");
            log_indented!(
                "Consider using manual sunset/sunrise times for more sensible transitions."
            );
        }

        // Use time source to support simulation mode, and coordinate timezone for correct date
        let city_tz = crate::geo::solar::determine_timezone_from_coordinates(latitude, longitude);
        let now = crate::time::source::now();
        let now_in_tz = now.with_timezone(&city_tz);
        let today = now_in_tz.date_naive();

        // Calculate the actual transition windows using our enhanced +10° to -2° method
        match crate::geo::solar::calculate_civil_twilight_times_for_display(
            latitude,
            longitude,
            today,
            self.debug_enabled,
        ) {
            Ok((
                sunset_time,
                sunset_start,
                sunset_end,
                sunrise_time,
                sunrise_start,
                sunrise_end,
                sunset_duration,
                sunrise_duration,
            )) => {
                log_block_start!(
                    "Sun times for {} ({:.4}°{}, {:.4}°{})",
                    city_name,
                    latitude.abs(),
                    if latitude >= 0.0 { "N" } else { "S" },
                    longitude.abs(),
                    if longitude >= 0.0 { "E" } else { "W" }
                );

                log_indented!(
                    "Today's sunset: {} (transition from {} to {})",
                    sunset_time.format("%H:%M"),
                    sunset_start.format("%H:%M"),
                    sunset_end.format("%H:%M")
                );

                log_indented!(
                    "Tomorrow's sunrise: {} (transition from {} to {})",
                    sunrise_time.format("%H:%M"),
                    sunrise_start.format("%H:%M"),
                    sunrise_end.format("%H:%M")
                );

                log_indented!(
                    "Sunset transition duration: {} minutes",
                    sunset_duration.as_secs() / 60
                );

                log_indented!(
                    "Sunrise transition duration: {} minutes",
                    sunrise_duration.as_secs() / 60
                );

                if self.debug_enabled {
                    let _ = log_solar_debug_info(latitude, longitude);
                }
            }
            Err(e) => {
                log_warning!("Could not calculate sun times: {e}");
                log_indented!("Using default transition times");
            }
        }

        Ok(Some((latitude, longitude, city_name)))
    }

    /// Update the configuration with new coordinates.
    fn update_configuration(
        &self,
        latitude: f64,
        longitude: f64,
        city_name: &str,
        target: ConfigTarget,
    ) -> Result<()> {
        match target {
            ConfigTarget::Default => {
                let config_path = Config::get_config_path()?;

                if config_path.exists() {
                    log_block_start!("Updating default configuration with new location...");
                    Config::update_coordinates(latitude, longitude)?;
                } else {
                    log_block_start!("No existing configuration found");
                    log_indented!("Creating new configuration with selected location");

                    Config::create_default_config(
                        &config_path,
                        Some((latitude, longitude, city_name.to_string())),
                    )?;

                    log_block_start!(
                        "Created new config file: {}",
                        crate::common::utils::private_path(&config_path)
                    );
                    log_indented!("Latitude: {latitude}");
                    log_indented!("Longitude: {longitude}");
                    log_indented!("Transition mode: geo");
                }
            }
            ConfigTarget::Preset(ref preset_name) => {
                log_block_start!("Updating preset '{}' with new location...", preset_name);

                let config_path = Config::get_config_path()?;
                let preset_dir = config_path
                    .parent()
                    .context("Failed to get config directory")?
                    .join("presets")
                    .join(preset_name);

                crate::config::builder::update_coords_in_dir(&preset_dir, latitude, longitude)?;
            }
        }

        Ok(())
    }
}
