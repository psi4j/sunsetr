//! Workflow orchestration for the geo command.

use anyhow::{Context, Result};

use crate::config::Config;
use crate::geo::{GeoSelectionResult, log_solar_debug_info, select_city_interactive};

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigTarget {
    Default,
    Preset(String),
}

pub struct GeoWorkflow {
    debug_enabled: bool,
    target: Option<String>,
}

impl GeoWorkflow {
    pub fn new(debug_enabled: bool, target: Option<String>) -> Self {
        Self {
            debug_enabled,
            target,
        }
    }

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
                        "cancel" => Ok(None),
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

    /// Run interactive city selection, returning `(latitude, longitude, city_name)`
    /// or `None` if the user cancels.
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

        let city_tz = crate::geo::solar::determine_timezone(latitude, longitude);
        let now = crate::time::source::now();
        let now_in_tz = now.with_timezone(&city_tz);
        let today = now_in_tz.date_naive();

        match crate::geo::solar::calculate_solar_times(latitude, longitude, today) {
            Ok(solar) => {
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
                    solar.sunset_time.format("%H:%M"),
                    solar.sunset_plus_10_start.format("%H:%M"),
                    solar.sunset_minus_2_end.format("%H:%M")
                );

                log_indented!(
                    "Tomorrow's sunrise: {} (transition from {} to {})",
                    solar.sunrise_time.format("%H:%M"),
                    solar.sunrise_minus_2_start.format("%H:%M"),
                    solar.sunrise_plus_10_end.format("%H:%M")
                );

                log_indented!(
                    "Sunset transition duration: {} minutes",
                    solar.sunset_duration.as_secs() / 60
                );

                log_indented!(
                    "Sunrise transition duration: {} minutes",
                    solar.sunrise_duration.as_secs() / 60
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
