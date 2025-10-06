//! Workflow orchestration for geo command.
//!
//! This module handles the complete geo selection workflow including:
//! - Process checking and instance management
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
}

impl GeoWorkflow {
    /// Create a new workflow orchestrator.
    pub fn new(debug_enabled: bool) -> Self {
        Self { debug_enabled }
    }

    /// Run the complete geo selection workflow.
    ///
    /// This orchestrates:
    /// 1. Check if sunsetr instance is currently running
    /// 2. Detect active presets and prompt for update target
    /// 3. Run interactive city selection with fuzzy search
    /// 4. Update configuration (default or preset) with new coordinates
    /// 5. Return appropriate action for the CLI dispatcher
    pub fn run(&self) -> Result<GeoSelectionResult> {
        log_version!();

        if self.debug_enabled {
            log_pipe!();
            log_debug!("Debug mode enabled for geo selection");
        }

        // Check prerequisites
        let instance_running = self.check_instance_running()?;

        if instance_running {
            if self.debug_enabled {
                log_pipe!();
                log_debug!("Detected running sunsetr instance");
                log_indented!("Will update configuration and restart after city selection");
            }
        } else if self.debug_enabled {
            log_pipe!();
            log_debug!("No running instance detected");
            log_indented!("Will start sunsetr in background after city selection");
        }

        // Determine configuration target
        let target = self.determine_target()?;
        if target.is_none() {
            // User cancelled
            return Ok(GeoSelectionResult::Cancelled);
        }
        let target = target.unwrap();

        // Run city selection
        let coords = self.select_city()?;
        if coords.is_none() {
            // User cancelled selection
            return Ok(GeoSelectionResult::Cancelled);
        }
        let (latitude, longitude, city_name) = coords.unwrap();

        // Update configuration
        self.update_configuration(latitude, longitude, &city_name, target)?;

        // Return appropriate result based on instance state
        if instance_running {
            Ok(GeoSelectionResult::ConfigUpdated {
                needs_restart: true,
            })
        } else {
            Ok(GeoSelectionResult::StartNew {
                debug: self.debug_enabled,
            })
        }
    }

    /// Check if sunsetr instance is currently running.
    fn check_instance_running(&self) -> Result<bool> {
        // Use io::instance to check for running instance
        Ok(crate::io::instance::get_running_instance()?.is_some())
    }

    /// Determine configuration target (default or preset).
    ///
    /// Returns None if user cancels the operation.
    fn determine_target(&self) -> Result<Option<ConfigTarget>> {
        let active_preset = Config::get_active_preset()?;

        if let Some(preset_name) = &active_preset {
            // We have an active preset - ask user what to update
            log_pipe!();
            log_info!("Active preset '{}' detected", preset_name);
            log_indented!("The geo command can update either the preset or the default config.");

            let options = vec![
                ("Cancel operation".to_string(), "cancel"),
                ("Update default configuration".to_string(), "default"),
                (format!("Update preset '{}'", preset_name), "preset"),
            ];

            let selection_index = crate::common::utils::show_dropdown_menu(
                &options,
                Some("Which configuration would you like to update?"),
                Some("Geo selection cancelled"),
            )?;

            match options[selection_index].1 {
                "cancel" => {
                    log_pipe!();
                    log_info!("Geo selection cancelled");
                    Ok(None)
                }
                "default" => Ok(Some(ConfigTarget::Default)),
                "preset" => Ok(Some(ConfigTarget::Preset(preset_name.clone()))),
                _ => unreachable!(),
            }
        } else {
            // No preset active, update default config
            Ok(Some(ConfigTarget::Default))
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
        use chrono::Local;

        // Delegate to the city_selector module for the actual implementation
        let (mut latitude, longitude, city_name) = match select_city_interactive() {
            Ok(coords) => coords,
            Err(e) => {
                // Check if user cancelled
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

        // Show calculated sunrise/sunset times using solar module
        let today = Local::now().date_naive();

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

                // Display sunset info (happening today)
                log_indented!(
                    "Today's sunset: {} (transition from {} to {})",
                    sunset_time.format("%H:%M"),
                    sunset_start.format("%H:%M"),
                    sunset_end.format("%H:%M")
                );

                // Display sunrise info (happening tomorrow)
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

                // Show detailed solar calculation debug info when debug mode is enabled
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
                    // No config exists, create new config with geo coordinates
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
