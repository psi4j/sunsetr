//! Workflow orchestration for the geo command.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::time::Duration;

use crate::config::{Config, LocationSource, builder};
use crate::geo::geoclue::{FIRST_FIX_WAIT, FixWait, GeoclueMonitor};
use crate::geo::{GeoSelectionResult, log_solar_debug_info, select_city_interactive};

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigTarget {
    Default,
    Preset(String),
}

impl ConfigTarget {
    fn config_path(&self) -> Result<PathBuf> {
        let config_path = Config::get_config_path()?;
        match self {
            ConfigTarget::Default => Ok(config_path),
            ConfigTarget::Preset(preset_name) => Ok(config_path
                .parent()
                .context("Failed to get config directory")?
                .join("presets")
                .join(preset_name)
                .join("sunsetr.toml")),
        }
    }
}

pub struct GeoWorkflow {
    debug_enabled: bool,
    target: Option<String>,
    source: LocationSource,
}

impl GeoWorkflow {
    pub fn new(debug_enabled: bool, target: Option<String>, source: LocationSource) -> Self {
        Self {
            debug_enabled,
            target,
            source,
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

        if self.source == LocationSource::Geoclue {
            self.configure_geoclue(target)?;
            return Ok(GeoSelectionResult::Updated);
        }

        let coords = self.select_city()?;
        if coords.is_none() {
            return Ok(GeoSelectionResult::Cancelled);
        }
        let (latitude, longitude, city_name) = coords.unwrap();

        self.update_configuration(latitude, longitude, &city_name, &target)?;

        let config_path = target.config_path()?;
        if builder::location_source_in(&config_path)? == LocationSource::Geoclue {
            builder::write_location_source(&config_path, LocationSource::Manual)?;
            log_indented!("Location source: manual");
        }

        Ok(GeoSelectionResult::Updated)
    }

    /// Switch the target to GeoClue, creating the config when there is none,
    /// then show what GeoClue reports for this session.
    fn configure_geoclue(&self, target: ConfigTarget) -> Result<()> {
        let config_path = target.config_path()?;
        if !config_path.exists() {
            if target != ConfigTarget::Default {
                anyhow::bail!("No config file found at {}", config_path.display());
            }
            log_block_start!("No existing configuration found");
            log_indented!("Creating new configuration");
            Config::create_default_config(&config_path, None)?;
        }
        builder::write_location_source(&config_path, LocationSource::Geoclue)?;
        log_block_start!(
            "Updated {}",
            crate::common::utils::private_path(&config_path)
        );
        log_indented!("Location source: geoclue");
        log_indented!("Transition mode: geo");

        let (sender, _receiver) = std::sync::mpsc::channel();
        let mut monitor = GeoclueMonitor::new(sender, self.debug_enabled);
        monitor.set_active(true);
        // One extra second lets the thread's missing-agent notice fire
        // inside the wait.
        if let FixWait::Fix(latitude, longitude) =
            monitor.wait_for_fix(FIRST_FIX_WAIT + Duration::from_secs(1))
        {
            self.log_sun_times("GeoClue location", latitude, longitude);
        }
        Ok(())
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

        let (latitude, longitude, city_name) = match select_city_interactive() {
            Ok(coords) => coords,
            Err(e) => {
                if e.to_string().contains("cancelled") {
                    return Ok(None);
                }
                return Err(e).context("Failed to run interactive city selection");
            }
        };

        let latitude = crate::geo::cap_latitude(latitude, "selected city");

        self.log_sun_times(&city_name, latitude, longitude);

        Ok(Some((latitude, longitude, city_name)))
    }

    fn log_sun_times(&self, location: &str, latitude: f64, longitude: f64) {
        let city_tz = crate::geo::solar::determine_timezone(latitude, longitude);
        let now = crate::time::source::now();
        let now_in_tz = now.with_timezone(&city_tz);
        let today = now_in_tz.date_naive();

        match crate::geo::solar::calculate_solar_times(latitude, longitude, today) {
            Ok(solar) => {
                log_block_start!(
                    "Sun times for {} ({})",
                    location,
                    crate::common::utils::format_coordinates(latitude, longitude)
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
    }

    fn update_configuration(
        &self,
        latitude: f64,
        longitude: f64,
        city_name: &str,
        target: &ConfigTarget,
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
            ConfigTarget::Preset(preset_name) => {
                log_block_start!("Updating preset '{}' with new location...", preset_name);

                let preset_dir = target
                    .config_path()?
                    .parent()
                    .context("Failed to get preset directory")?
                    .to_path_buf();

                builder::update_coords_in_dir(&preset_dir, latitude, longitude)?;
            }
        }

        Ok(())
    }
}
