use anyhow::{Context, Result};
use chrono::{NaiveTime, Timelike};
use serde::Deserialize;
use std::fs::{self};
use std::path::PathBuf;

use crate::constants::*;
use crate::logger::Log;

#[derive(Debug, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Auto,
    Hyprland,
    Wayland,
}

impl Backend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Backend::Auto => "auto",
            Backend::Hyprland => "hyprland",
            Backend::Wayland => "wayland",
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub start_hyprsunset: Option<bool>,
    pub backend: Option<Backend>,
    pub startup_transition: Option<bool>, // whether to enable smooth startup transition
    pub startup_transition_duration: Option<u64>, // seconds for startup transition
    pub latitude: Option<f64>,            // Geographic latitude for geo mode
    pub longitude: Option<f64>,           // Geographic longitude for geo mode
    pub sunset: String,
    pub sunrise: String,
    pub night_temp: Option<u32>,
    pub day_temp: Option<u32>,
    pub night_gamma: Option<f32>,
    pub day_gamma: Option<f32>,
    pub transition_duration: Option<u64>, // minutes
    pub update_interval: Option<u64>,     // seconds during transition
    pub transition_mode: Option<String>,  // "finish_by", "start_at", "center", or "geo"
}

impl Config {
    pub fn get_config_path() -> Result<PathBuf> {
        if cfg!(test) {
            // For library's own unit tests, bypass complex logic
            let config_dir = dirs::config_dir()
                .context("Could not determine config directory for unit tests")?;
            Ok(config_dir.join("sunsetr").join("sunsetr.toml"))
        } else {
            // For binary execution or integration tests (when not a unit test)
            let config_dir = dirs::config_dir().context("Could not determine config directory")?;
            let new_config_path = config_dir.join("sunsetr").join("sunsetr.toml");
            let old_config_path = config_dir.join("hypr").join("sunsetr.toml");

            let new_exists = new_config_path.exists();
            let old_exists = old_config_path.exists();

            match (new_exists, old_exists) {
                (true, true) => {
                    #[cfg(feature = "testing-support")]
                    {
                        Log::log_pipe();
                        anyhow::bail!(
                            "TEST_MODE_CONFLICT: Found configuration files in both new ({}) and old ({}) locations while testing-support feature is active.",
                            new_config_path.display(),
                            old_config_path.display()
                        )
                    }
                    #[cfg(not(feature = "testing-support"))]
                    {
                        Self::choose_config_file(new_config_path, old_config_path)
                    }
                }
                (true, false) => Ok(new_config_path),
                (false, true) => Ok(old_config_path),
                (false, false) => Ok(new_config_path), // Default to new path for creation
            }
        }
    }

    /// Interactive terminal interface for choosing which config file to keep
    #[cfg(not(feature = "testing-support"))]
    fn choose_config_file(new_path: PathBuf, old_path: PathBuf) -> Result<PathBuf> {
        Log::log_pipe();
        Log::log_warning("Configuration conflict detected");
        Log::log_block_start("Please select which config to keep:");

        let options = vec![
            (
                format!("{} (new location)", new_path.display()),
                new_path.clone(),
            ),
            (
                format!("{} (legacy location)", old_path.display()),
                old_path.clone(),
            ),
        ];

        let selected_index = crate::utils::show_dropdown_menu(
            &options,
            None,
            Some("Operation cancelled. Please manually remove one of the config files."),
        )?;
        let (chosen_path, to_remove) = if selected_index == 0 {
            (new_path, old_path)
        } else {
            (old_path, new_path)
        };

        // Confirm deletion
        Log::log_block_start(&format!("You chose: {}", chosen_path.display()));
        Log::log_decorated(&format!("Will remove: {}", to_remove.display()));

        let confirm_options = vec![
            ("Yes, remove the file".to_string(), true),
            ("No, cancel operation".to_string(), false),
        ];

        let confirm_index = crate::utils::show_dropdown_menu(
            &confirm_options,
            None,
            Some("Operation cancelled. Please manually remove one of the config files."),
        )?;
        let should_remove = confirm_options[confirm_index].1;

        if !should_remove {
            Log::log_pipe();
            Log::log_warning(
                "Operation cancelled. Please manually remove one of the config files.",
            );
            std::process::exit(EXIT_FAILURE);
        }

        // Try to use trash-cli first, fallback to direct removal
        let removed_successfully = if Self::try_trash_file(&to_remove) {
            Log::log_block_start(&format!(
                "Successfully moved to trash: {}",
                to_remove.display()
            ));
            true
        } else if let Err(e) = fs::remove_file(&to_remove) {
            Log::log_pipe();
            Log::log_warning(&format!("Failed to remove {}: {}", to_remove.display(), e));
            Log::log_decorated("Please remove it manually to avoid future conflicts.");
            false
        } else {
            Log::log_block_start(&format!("Successfully removed: {}", to_remove.display()));
            true
        };

        if removed_successfully {
            Log::log_block_start(&format!("Using configuration: {}", chosen_path.display()));
        }

        Ok(chosen_path)
    }

    /// Attempt to move file to trash using trash-cli
    #[cfg(not(feature = "testing-support"))]
    fn try_trash_file(path: &PathBuf) -> bool {
        // Try trash-put command (most common)
        if let Ok(status) = std::process::Command::new("trash-put").arg(path).status() {
            return status.success();
        }

        // Try trash command (alternative)
        if let Ok(status) = std::process::Command::new("trash").arg(path).status() {
            return status.success();
        }

        // Try gio trash (GNOME)
        if let Ok(status) = std::process::Command::new("gio")
            .args(["trash", path.to_str().unwrap_or("")])
            .status()
        {
            return status.success();
        }

        false
    }

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

        // Determine coordinate entries based on whether coordinates were provided
        let (transition_mode, lat, lon) = if let Some((lat, lon, city_name)) = coords {
            // Use provided coordinates from geo selection
            use crate::logger::Log;
            Log::log_indented(&format!(
                "Using selected location for new config: {}",
                city_name
            ));
            (DEFAULT_TRANSITION_MODE, lat, lon)
        } else {
            // Try to auto-detect coordinates via timezone for smart geo mode default
            Self::determine_default_mode_and_coords()
        };

        // Build the config using the builder pattern
        let config_content = ConfigBuilder::new()
            .add_section("Sunsetr configuration")
            .add_setting(
                "backend",
                &format!("\"{}\"", DEFAULT_BACKEND.as_str()),
                "Backend to use: \"auto\", \"hyprland\" or \"wayland\"",
            )
            .add_setting(
                "start_hyprsunset",
                &DEFAULT_START_HYPRSUNSET.to_string(),
                "Set true if you're not using hyprsunset.service",
            )
            .add_setting(
                "startup_transition",
                &DEFAULT_STARTUP_TRANSITION.to_string(),
                "Enable smooth transition when sunsetr starts",
            )
            .add_setting(
                "startup_transition_duration",
                &DEFAULT_STARTUP_TRANSITION_DURATION.to_string(),
                &format!(
                    "Duration of startup transition in seconds ({}-{})",
                    MINIMUM_STARTUP_TRANSITION_DURATION, MAXIMUM_STARTUP_TRANSITION_DURATION
                ),
            )
            .add_setting(
                "night_temp",
                &DEFAULT_NIGHT_TEMP.to_string(),
                &format!(
                    "Color temperature after sunset ({}-{}) Kelvin",
                    MINIMUM_TEMP, MAXIMUM_TEMP
                ),
            )
            .add_setting(
                "day_temp",
                &DEFAULT_DAY_TEMP.to_string(),
                &format!(
                    "Color temperature during day ({}-{}) Kelvin",
                    MINIMUM_TEMP, MAXIMUM_TEMP
                ),
            )
            .add_setting(
                "night_gamma",
                &DEFAULT_NIGHT_GAMMA.to_string(),
                &format!(
                    "Gamma percentage for night ({}-{}%)",
                    MINIMUM_GAMMA, MAXIMUM_GAMMA
                ),
            )
            .add_setting(
                "day_gamma",
                &DEFAULT_DAY_GAMMA.to_string(),
                &format!(
                    "Gamma percentage for day ({}-{}%)",
                    MINIMUM_GAMMA, MAXIMUM_GAMMA
                ),
            )
            .add_setting(
                "update_interval",
                &DEFAULT_UPDATE_INTERVAL.to_string(),
                &format!(
                    "Update frequency during transitions in seconds ({}-{})",
                    MINIMUM_UPDATE_INTERVAL, MAXIMUM_UPDATE_INTERVAL
                ),
            )
            .add_setting(
                "transition_mode",
                &format!("\"{}\"", transition_mode),
                "Select: \"geo\", \"finish_by\", \"start_at\", \"center\"",
            )
            .add_section("Manual transitions")
            .add_setting(
                "sunset",
                &format!("\"{}\"", DEFAULT_SUNSET),
                "Time to transition to night mode (HH:MM:SS) - ignored in geo mode",
            )
            .add_setting(
                "sunrise",
                &format!("\"{}\"", DEFAULT_SUNRISE),
                "Time to transition to day mode (HH:MM:SS) - ignored in geo mode",
            )
            .add_setting(
                "transition_duration",
                &DEFAULT_TRANSITION_DURATION.to_string(),
                &format!(
                    "Transition duration in minutes ({}-{})",
                    MINIMUM_TRANSITION_DURATION, MAXIMUM_TRANSITION_DURATION
                ),
            )
            .add_section("Geolocation-based transitions")
            .add_setting(
                "latitude",
                &format!("{:.6}", lat),
                "Geographic latitude (auto-detected on first run)",
            )
            .add_setting(
                "longitude",
                &format!("{:.6}", lon),
                "Geographic longitude (use 'sunsetr --geo' to change)",
            )
            .build();

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
        use crate::logger::Log;

        // Try timezone detection for automatic coordinates
        if let Ok((lat, lon, city_name)) = crate::geo::detect_coordinates_from_timezone() {
            Log::log_indented(&format!(
                "Auto-detected location for new config: {}",
                city_name
            ));
            (DEFAULT_TRANSITION_MODE, lat, lon)
        } else {
            // Fall back to finish_by mode with Chicago coordinates as placeholders
            Log::log_indented(
                "Timezone detection failed, using manual times with placeholder coordinates",
            );
            Log::log_indented("Use 'sunsetr --geo' to select your actual location");
            (
                crate::constants::FALLBACK_DEFAULT_TRANSITION_MODE,
                41.8781,
                -87.6298,
            ) // Chicago coordinates (placeholder)
        }
    }

    // NEW private helper method
    fn apply_defaults_and_validate_fields(config: &mut Config) -> Result<()> {
        // Set default for start_hyprsunset if not specified
        if config.start_hyprsunset.is_none() {
            config.start_hyprsunset = Some(DEFAULT_START_HYPRSUNSET);
        }

        // Set default for backend if not specified
        if config.backend.is_none() {
            config.backend = Some(DEFAULT_BACKEND);
        }

        // Validate time formats
        NaiveTime::parse_from_str(&config.sunset, "%H:%M:%S")
            .context("Invalid sunset time format in config. Use HH:MM:SS format")?;
        NaiveTime::parse_from_str(&config.sunrise, "%H:%M:%S")
            .context("Invalid sunrise time format in config. Use HH:MM:SS format")?;

        // Validate temperature if specified
        if let Some(temp) = config.night_temp {
            if !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp) {
                Log::log_pipe();
                anyhow::bail!(
                    "Night temperature must be between {} and {} Kelvin",
                    MINIMUM_TEMP,
                    MAXIMUM_TEMP
                );
            }
        } else {
            config.night_temp = Some(DEFAULT_NIGHT_TEMP);
        }

        // Validate day temperature if specified
        if let Some(temp) = config.day_temp {
            if !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp) {
                Log::log_pipe();
                anyhow::bail!(
                    "Day temperature must be between {} and {} Kelvin",
                    MINIMUM_TEMP,
                    MAXIMUM_TEMP
                );
            }
        } else {
            config.day_temp = Some(DEFAULT_DAY_TEMP);
        }

        // Validate night gamma if specified
        if let Some(gamma) = config.night_gamma {
            if !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma) {
                Log::log_pipe();
                anyhow::bail!(
                    "Night gamma must be between {}% and {}%",
                    MINIMUM_GAMMA,
                    MAXIMUM_GAMMA
                );
            }
        } else {
            config.night_gamma = Some(DEFAULT_NIGHT_GAMMA);
        }

        // Validate day gamma if specified
        if let Some(gamma) = config.day_gamma {
            if !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma) {
                Log::log_pipe();
                anyhow::bail!(
                    "Day gamma must be between {}% and {}%",
                    MINIMUM_GAMMA,
                    MAXIMUM_GAMMA
                );
            }
        } else {
            config.day_gamma = Some(DEFAULT_DAY_GAMMA);
        }

        // Set defaults for transition fields
        if config.transition_duration.is_none() {
            config.transition_duration = Some(DEFAULT_TRANSITION_DURATION);
        }

        if config.update_interval.is_none() {
            config.update_interval = Some(DEFAULT_UPDATE_INTERVAL);
        }

        if config.transition_mode.is_none() {
            config.transition_mode = Some(DEFAULT_TRANSITION_MODE.to_string());
        }

        // Set defaults for startup transition fields
        if config.startup_transition.is_none() {
            config.startup_transition = Some(DEFAULT_STARTUP_TRANSITION);
        }

        if config.startup_transition_duration.is_none() {
            config.startup_transition_duration = Some(DEFAULT_STARTUP_TRANSITION_DURATION);
        }

        // Validate transition ranges
        if let Some(duration_minutes) = config.transition_duration {
            if !(MINIMUM_TRANSITION_DURATION..=MAXIMUM_TRANSITION_DURATION)
                .contains(&duration_minutes)
            {
                Log::log_pipe();
                anyhow::bail!(
                    "Transition duration must be between {} and {} minutes",
                    MINIMUM_TRANSITION_DURATION,
                    MAXIMUM_TRANSITION_DURATION
                );
            }
        }

        if let Some(interval) = config.update_interval {
            if !(MINIMUM_UPDATE_INTERVAL..=MAXIMUM_UPDATE_INTERVAL).contains(&interval) {
                Log::log_pipe();
                anyhow::bail!(
                    "Update interval must be between {} and {} seconds",
                    MINIMUM_UPDATE_INTERVAL,
                    MAXIMUM_UPDATE_INTERVAL
                );
            }
        }

        // Validate transition mode
        if let Some(ref mode) = config.transition_mode {
            if mode != "finish_by" && mode != "start_at" && mode != "center" && mode != "geo" {
                Log::log_pipe();
                anyhow::bail!(
                    "Transition mode must be 'finish_by', 'start_at', 'center', or 'geo'"
                );
            }
        }

        // Validate startup transition duration
        if let Some(duration_seconds) = config.startup_transition_duration {
            if !(MINIMUM_STARTUP_TRANSITION_DURATION..=MAXIMUM_STARTUP_TRANSITION_DURATION)
                .contains(&duration_seconds)
            {
                Log::log_pipe();
                anyhow::bail!(
                    "Startup transition duration must be between {} and {} seconds",
                    MINIMUM_STARTUP_TRANSITION_DURATION,
                    MAXIMUM_STARTUP_TRANSITION_DURATION
                );
            }
        }
        Ok(())
    }

    // NEW public method for loading from a specific path
    // This version does NOT create a default config if the path doesn't exist.
    pub fn load_from_path(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            Log::log_pipe();
            anyhow::bail!(
                "Configuration file not found at specified path: {}",
                path.display()
            );
        }

        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;

        let mut config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config from {}", path.display()))?;

        Self::apply_defaults_and_validate_fields(&mut config)?;

        // Comprehensive configuration validation (this is the existing public function)
        validate_config(&config)?;

        Ok(config)
    }

    // MODIFIED existing load method
    pub fn load() -> Result<Self> {
        let config_path = Self::get_config_path()?;

        if !config_path.exists() {
            Self::create_default_config(&config_path, None)
                .context("Failed to create default config during load")?;
        }

        // Now that we're sure a file exists (either pre-existing or newly created default),
        // load it using the common path-based loader.
        Self::load_from_path(&config_path).with_context(|| {
            Log::log_pipe();
            format!(
                "Failed to load configuration from {}",
                config_path.display()
            )
        })
    }

    /// Update an existing config file with geo coordinates and mode
    pub fn update_config_with_geo_coordinates(latitude: f64, longitude: f64) -> Result<()> {
        let config_path = Self::get_config_path()?;

        if !config_path.exists() {
            anyhow::bail!("No existing config file found at {}", config_path.display());
        }

        // Read current config content
        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config from {}", config_path.display()))?;

        // Parse as TOML to preserve structure and comments
        let mut updated_content = content.clone();

        // Update or add latitude
        if let Some(lat_line) = find_config_line(&content, "latitude") {
            let new_lat_line =
                preserve_comment_formatting(&lat_line, "latitude", &format!("{:.6}", latitude));
            updated_content = updated_content.replace(&lat_line, &new_lat_line);
        } else {
            // Add latitude after backend line or at beginning
            if let Some(backend_pos) = find_line_position(&content, "backend") {
                updated_content = insert_line_after(
                    &updated_content,
                    backend_pos,
                    &format!("latitude = {:.6}", latitude),
                );
            } else {
                updated_content = format!("latitude = {:.6}\n{}", latitude, updated_content);
            }
        }

        // Update or add longitude
        if let Some(lon_line) = find_config_line(&content, "longitude") {
            let new_lon_line =
                preserve_comment_formatting(&lon_line, "longitude", &format!("{:.6}", longitude));
            updated_content = updated_content.replace(&lon_line, &new_lon_line);
        } else {
            // Add longitude after latitude line
            if let Some(lat_pos) = find_line_position(&updated_content, "latitude") {
                updated_content = insert_line_after(
                    &updated_content,
                    lat_pos,
                    &format!("longitude = {:.6}", longitude),
                );
            } else {
                updated_content = format!("longitude = {:.6}\n{}", longitude, updated_content);
            }
        }

        // Update or add transition_mode to "geo"
        if let Some(mode_line) = find_config_line(&content, "transition_mode") {
            let new_mode_line =
                preserve_comment_formatting(&mode_line, "transition_mode", "\"geo\"");
            updated_content = updated_content.replace(&mode_line, &new_mode_line);
        } else {
            // Add transition_mode at the end
            updated_content = format!("{}transition_mode = \"geo\"\n", updated_content);
        }

        // Write updated content back to file
        fs::write(&config_path, updated_content).with_context(|| {
            format!(
                "Failed to write updated config to {}",
                config_path.display()
            )
        })?;

        Log::log_block_start(&format!(
            "Updated config file: {}",
            crate::utils::path_for_display(&config_path)
        ));
        Log::log_indented(&format!("Latitude: {}", latitude));
        Log::log_indented(&format!("Longitude: {}", longitude));
        Log::log_indented("Transition mode: geo");

        Ok(())
    }

    pub fn log_config(&self) {
        Log::log_block_start("Loaded configuration");
        Log::log_indented(&format!(
            "Backend: {}",
            self.backend.as_ref().unwrap_or(&DEFAULT_BACKEND).as_str()
        ));
        Log::log_indented(&format!(
            "Auto-start hyprsunset: {}",
            self.start_hyprsunset.unwrap_or(DEFAULT_START_HYPRSUNSET)
        ));
        Log::log_indented(&format!(
            "Enable startup transition: {}",
            self.startup_transition
                .unwrap_or(DEFAULT_STARTUP_TRANSITION)
        ));

        // Only show startup transition duration if startup transition is enabled
        if self
            .startup_transition
            .unwrap_or(DEFAULT_STARTUP_TRANSITION)
        {
            Log::log_indented(&format!(
                "Startup transition duration: {} seconds",
                self.startup_transition_duration
                    .unwrap_or(DEFAULT_STARTUP_TRANSITION_DURATION)
            ));
        }

        // Show geographic coordinates if in geo mode
        let mode = self
            .transition_mode
            .as_deref()
            .unwrap_or(DEFAULT_TRANSITION_MODE);
        if mode == "geo" {
            if let (Some(lat), Some(lon)) = (self.latitude, self.longitude) {
                let lat_dir = if lat >= 0.0 { "N" } else { "S" };
                let lon_dir = if lon >= 0.0 { "E" } else { "W" };
                Log::log_indented(&format!(
                    "Location: {:.4}°{}, {:.4}°{}",
                    lat.abs(),
                    lat_dir,
                    lon.abs(),
                    lon_dir
                ));
            } else {
                Log::log_indented("Location: Auto-detected on first run");
            }
        }

        Log::log_indented(&format!("Sunset time: {}", self.sunset));
        Log::log_indented(&format!("Sunrise time: {}", self.sunrise));
        Log::log_indented(&format!(
            "Night temperature: {}K",
            self.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP)
        ));
        Log::log_indented(&format!(
            "Day temperature: {}K",
            self.day_temp.unwrap_or(DEFAULT_DAY_TEMP)
        ));
        Log::log_indented(&format!(
            "Night gamma: {}%",
            self.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA)
        ));
        Log::log_indented(&format!(
            "Day gamma: {}%",
            self.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA)
        ));
        Log::log_indented(&format!(
            "Transition duration: {} minutes",
            self.transition_duration
                .unwrap_or(DEFAULT_TRANSITION_DURATION)
        ));
        Log::log_indented(&format!(
            "Update interval: {} seconds",
            self.update_interval.unwrap_or(DEFAULT_UPDATE_INTERVAL)
        ));
        Log::log_indented(&format!(
            "Transition mode: {}",
            self.transition_mode
                .as_deref()
                .unwrap_or(DEFAULT_TRANSITION_MODE)
        ));
    }
}

/// Comprehensive configuration validation to prevent impossible or problematic setups
pub fn validate_config(config: &Config) -> Result<()> {
    use chrono::NaiveTime;

    // 0. Validate backend configuration compatibility
    let backend = config.backend.as_ref().unwrap_or(&DEFAULT_BACKEND);
    let start_hyprsunset = config.start_hyprsunset.unwrap_or(DEFAULT_START_HYPRSUNSET);

    // Only validate explicit backend choices, Auto will be resolved at runtime
    if *backend == Backend::Wayland && start_hyprsunset {
        Log::log_pipe();
        anyhow::bail!(
            "Incompatible configuration: backend=\"wayland\" and start_hyprsunset=true. \
            When using Wayland protocols (backend=\"wayland\"), hyprsunset should not be started (start_hyprsunset=false). \
            Please set either backend=\"hyprland\" or start_hyprsunset=false."
        );
    }

    let sunset = NaiveTime::parse_from_str(&config.sunset, "%H:%M:%S")
        .context("Invalid sunset time format")?;
    let sunrise = NaiveTime::parse_from_str(&config.sunrise, "%H:%M:%S")
        .context("Invalid sunrise time format")?;

    let transition_duration_mins = config
        .transition_duration
        .unwrap_or(DEFAULT_TRANSITION_DURATION);
    let update_interval_secs = config.update_interval.unwrap_or(DEFAULT_UPDATE_INTERVAL);
    let mode = config
        .transition_mode
        .as_deref()
        .unwrap_or(DEFAULT_TRANSITION_MODE);

    // Validate transition duration (hard limits)
    if !(MINIMUM_TRANSITION_DURATION..=MAXIMUM_TRANSITION_DURATION)
        .contains(&transition_duration_mins)
    {
        Log::log_pipe();
        anyhow::bail!(
            "Transition duration ({} minutes) must be between {} and {} minutes",
            transition_duration_mins,
            MINIMUM_TRANSITION_DURATION,
            MAXIMUM_TRANSITION_DURATION
        );
    }

    // Validate startup transition duration (hard limits)
    if let Some(startup_duration_secs) = config.startup_transition_duration {
        if !(MINIMUM_STARTUP_TRANSITION_DURATION..=MAXIMUM_STARTUP_TRANSITION_DURATION)
            .contains(&startup_duration_secs)
        {
            Log::log_pipe();
            anyhow::bail!(
                "Startup transition duration ({} seconds) must be between {} and {} seconds",
                startup_duration_secs,
                MINIMUM_STARTUP_TRANSITION_DURATION,
                MAXIMUM_STARTUP_TRANSITION_DURATION
            );
        }
    }

    // 0. Validate basic ranges for temperature and gamma (hard limits)
    if let Some(temp) = config.night_temp {
        if !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp) {
            Log::log_pipe();
            anyhow::bail!(
                "Night temperature ({}) must be between {} and {} Kelvin",
                temp,
                MINIMUM_TEMP,
                MAXIMUM_TEMP
            );
        }
    }

    if let Some(temp) = config.day_temp {
        if !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp) {
            Log::log_pipe();
            anyhow::bail!(
                "Day temperature ({}) must be between {} and {} Kelvin",
                temp,
                MINIMUM_TEMP,
                MAXIMUM_TEMP
            );
        }
    }

    if let Some(gamma) = config.night_gamma {
        if !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma) {
            Log::log_pipe();
            anyhow::bail!(
                "Night gamma ({}%) must be between {}% and {}%",
                gamma,
                MINIMUM_GAMMA,
                MAXIMUM_GAMMA
            );
        }
    }

    if let Some(gamma) = config.day_gamma {
        if !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma) {
            Log::log_pipe();
            anyhow::bail!(
                "Day gamma ({}%) must be between {}% and {}%",
                gamma,
                MINIMUM_GAMMA,
                MAXIMUM_GAMMA
            );
        }
    }

    // 1. Check for identical sunset/sunrise times
    if sunset == sunrise {
        Log::log_pipe();
        anyhow::bail!(
            "Sunset and sunrise cannot be the same time ({:?}). \
            There must be a distinction between day and night periods.",
            sunset
        );
    }

    // 2. Calculate time periods and check minimums
    let (day_duration_mins, night_duration_mins) = calculate_day_night_durations(sunset, sunrise);

    if day_duration_mins < 60 {
        Log::log_pipe();
        anyhow::bail!(
            "Day period is too short ({} minutes). \
            Day period must be at least 1 hour. \
            Adjust sunset ({:?}) or sunrise ({:?}) times.",
            day_duration_mins,
            sunset,
            sunrise
        );
    }

    if night_duration_mins < 60 {
        Log::log_pipe();
        anyhow::bail!(
            "Night period is too short ({} minutes). \
            Night period must be at least 1 hour. \
            Adjust sunset ({:?}) or sunrise ({:?}) times.",
            night_duration_mins,
            sunset,
            sunrise
        );
    }

    // 3. Check that transitions fit within their periods
    validate_transitions_fit_periods(sunset, sunrise, transition_duration_mins, mode)?;

    // 4. Check for transition overlaps
    validate_no_transition_overlaps(sunset, sunrise, transition_duration_mins, mode)?;

    // 5. Validate update interval vs transition duration (must come before range check)
    let transition_duration_secs = transition_duration_mins * 60;
    if update_interval_secs > transition_duration_secs {
        Log::log_pipe();
        anyhow::bail!(
            "Update interval ({} seconds) is longer than transition duration ({} seconds). \
            Update interval should be shorter to allow smooth transitions. \
            Reduce update_interval or increase transition_duration.",
            update_interval_secs,
            transition_duration_secs
        );
    }

    // 6. Update interval range check (with warnings for extreme values)
    if update_interval_secs < MINIMUM_UPDATE_INTERVAL {
        Log::log_warning(&format!(
            "Update interval ({} seconds) is below recommended minimum ({} seconds). \
            This may cause excessive system load.",
            update_interval_secs, MINIMUM_UPDATE_INTERVAL
        ));
    } else if update_interval_secs > MAXIMUM_UPDATE_INTERVAL {
        Log::log_warning(&format!(
            "Update interval ({} seconds) is above recommended maximum ({} seconds). \
            Transitions may appear choppy.",
            update_interval_secs, MAXIMUM_UPDATE_INTERVAL
        ));
    }

    // 7. Check for reasonable transition frequency
    if transition_duration_secs < 300 && update_interval_secs < 30 {
        // This would create very frequent updates
        Log::log_warning(&format!(
            "Very short transition duration ({} min) with frequent updates ({} sec) may stress your graphics system.",
            transition_duration_mins, update_interval_secs
        ));
    }

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
            content: format!("#[{}]", title),
            entry_type: EntryType::Section,
        });
        self
    }

    fn add_setting(mut self, key: &str, value: &str, comment: &str) -> Self {
        let line = format!("{} = {}", key, value);
        self.entries.push(ConfigEntry {
            content: line.clone(),
            entry_type: EntryType::Setting {
                line,
                comment: format!("# {}", comment),
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
                    result.push(format!("{}{}{}", line, padding, comment));
                }
            }
        }

        result.join("\n")
    }
}

/// Calculate day and night durations in minutes
fn calculate_day_night_durations(sunset: NaiveTime, sunrise: NaiveTime) -> (u32, u32) {
    let sunset_mins = sunset.hour() * 60 + sunset.minute();
    let sunrise_mins = sunrise.hour() * 60 + sunrise.minute();

    if sunset_mins > sunrise_mins {
        // Normal case: sunset after sunrise in the same day
        let day_duration = sunset_mins - sunrise_mins;
        let night_duration = (24 * 60) - day_duration;
        (day_duration, night_duration)
    } else {
        // Overnight case: sunset before sunrise (next day)
        let night_duration = sunrise_mins - sunset_mins;
        let day_duration = (24 * 60) - night_duration;
        (day_duration, night_duration)
    }
}

/// Validate that transitions fit within their respective day/night periods
fn validate_transitions_fit_periods(
    sunset: NaiveTime,
    sunrise: NaiveTime,
    transition_duration_mins: u64,
    mode: &str,
) -> Result<()> {
    let (day_duration_mins, night_duration_mins) = calculate_day_night_durations(sunset, sunrise);

    // For "center" mode, transition spans both day and night periods
    // For "finish_by" and "start_at", transition should fit within the target period

    match mode {
        "center" => {
            // Transition spans across sunset/sunrise time, so we need room on both sides
            let half_transition = transition_duration_mins / 2;

            // Check if transition would exceed either period
            if half_transition >= day_duration_mins.into()
                || half_transition >= night_duration_mins.into()
            {
                Log::log_pipe();
                anyhow::bail!(
                    "Transition duration ({} minutes) is too long for 'center' mode. \
                    With centered transitions, half the duration ({} minutes) must fit in both \
                    day period ({} minutes) and night period ({} minutes). \
                    Reduce transition_duration or adjust sunset/sunrise times.",
                    transition_duration_mins,
                    half_transition,
                    day_duration_mins,
                    night_duration_mins
                );
            }
        }
        "finish_by" | "start_at" => {
            // Transitions should reasonably fit within their periods
            let max_reasonable_ratio = 0.8; // 80% of period
            let max_day_transition = (day_duration_mins as f64 * max_reasonable_ratio) as u64;
            let max_night_transition = (night_duration_mins as f64 * max_reasonable_ratio) as u64;

            if transition_duration_mins > max_day_transition {
                Log::log_warning(&format!(
                    "Transition duration ({} min) is quite long compared to day period ({} min). Consider reducing transition_duration for better experience.",
                    transition_duration_mins, day_duration_mins
                ));
            }

            if transition_duration_mins > max_night_transition {
                Log::log_warning(&format!(
                    "Transition duration ({} min) is quite long compared to night period ({} min). Consider reducing transition_duration for better experience.",
                    transition_duration_mins, night_duration_mins
                ));
            }
        }
        _ => {} // Already validated mode earlier
    }

    Ok(())
}

/// Validate that sunset and sunrise transitions don't overlap
fn validate_no_transition_overlaps(
    sunset: NaiveTime,
    sunrise: NaiveTime,
    transition_duration_mins: u64,
    mode: &str,
) -> Result<()> {
    use std::time::Duration;

    // Calculate transition windows using the same logic as the main code
    let transition_duration = Duration::from_secs(transition_duration_mins * 60);

    let (sunset_start, sunset_end, sunrise_start, sunrise_end) = match mode {
        "center" => {
            let half_transition = transition_duration / 2;
            let half_chrono = chrono::Duration::from_std(half_transition).unwrap();
            (
                sunset - half_chrono,
                sunset + half_chrono,
                sunrise - half_chrono,
                sunrise + half_chrono,
            )
        }
        "start_at" => {
            let full_transition = chrono::Duration::from_std(transition_duration).unwrap();
            (
                sunset,
                sunset + full_transition,
                sunrise,
                sunrise + full_transition,
            )
        }
        "finish_by" => {
            let full_transition = chrono::Duration::from_std(transition_duration).unwrap();
            (
                sunset - full_transition,
                sunset,
                sunrise - full_transition,
                sunrise,
            )
        }
        _ => {
            // Default to "finish_by" mode for any unexpected values
            let full_transition = chrono::Duration::from_std(transition_duration).unwrap();
            (
                sunset - full_transition,
                sunset,
                sunrise - full_transition,
                sunrise,
            )
        }
    };

    // Convert to minutes since midnight for easier comparison
    let sunset_start_mins = sunset_start.hour() * 60 + sunset_start.minute();
    let sunset_end_mins = sunset_end.hour() * 60 + sunset_end.minute();
    let sunrise_start_mins = sunrise_start.hour() * 60 + sunrise_start.minute();
    let sunrise_end_mins = sunrise_end.hour() * 60 + sunrise_end.minute();

    // Check for overlaps - this is complex due to potential midnight crossings
    let overlap = check_time_ranges_overlap(
        sunset_start_mins,
        sunset_end_mins,
        sunrise_start_mins,
        sunrise_end_mins,
    );

    if overlap {
        Log::log_pipe();
        anyhow::bail!(
            "Transition periods overlap! \
            Sunset transition: {:?} → {:?}, Sunrise transition: {:?} → {:?}. \
            \nThis configuration is impossible because transitions would conflict. \
            \nSolutions: \
            \n  1. Reduce transition_duration from {} to {} minutes or less \
            \n  2. Increase time between sunset ({:?}) and sunrise ({:?}) \
            \n  3. Change transition_mode from '{}' to a different mode",
            sunset_start,
            sunset_end,
            sunrise_start,
            sunrise_end,
            transition_duration_mins,
            suggest_max_transition_duration(sunset, sunrise, mode),
            sunset,
            sunrise,
            mode
        );
    }

    Ok(())
}

/// Check if two time ranges overlap, handling midnight crossings
fn check_time_ranges_overlap(
    start1_mins: u32,
    end1_mins: u32,
    start2_mins: u32,
    end2_mins: u32,
) -> bool {
    // Helper function to normalize ranges that cross midnight
    let normalize_range = |start: u32, end: u32| -> Vec<(u32, u32)> {
        if start <= end {
            vec![(start, end)]
        } else {
            // Range crosses midnight, split into two ranges
            vec![(start, 24 * 60), (0, end)]
        }
    };

    let range1 = normalize_range(start1_mins, end1_mins);
    let range2 = normalize_range(start2_mins, end2_mins);

    // Check if any segment from range1 overlaps with any segment from range2
    for (r1_start, r1_end) in &range1 {
        for (r2_start, r2_end) in &range2 {
            if r1_start < r2_end && r2_start < r1_end {
                return true; // Overlap detected
            }
        }
    }

    false
}

/// Suggest a maximum safe transition duration for the given configuration
fn suggest_max_transition_duration(sunset: NaiveTime, sunrise: NaiveTime, mode: &str) -> u64 {
    let (day_duration_mins, night_duration_mins) = calculate_day_night_durations(sunset, sunrise);
    let min_period = day_duration_mins.min(night_duration_mins);

    match mode {
        "center" => {
            // For center mode, half the transition goes in each period
            ((min_period / 2).saturating_sub(1)).into()
        }
        "finish_by" | "start_at" => {
            // For these modes, leave some buffer between transitions
            ((min_period as f64 * 0.8) as u32).into()
        }
        _ => (min_period.saturating_sub(10)).into(),
    }
}

/// Find a config line containing the specified key
fn find_config_line(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(key) && trimmed.contains('=') && !trimmed.starts_with('#') {
            return Some(line.to_string());
        }
    }
    None
}

/// Find the line number (0-indexed) of a config key
fn find_line_position(content: &str, key: &str) -> Option<usize> {
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with(key) && trimmed.contains('=') && !trimmed.starts_with('#') {
            return Some(i);
        }
    }
    None
}

/// Insert a new line after the specified line position
fn insert_line_after(content: &str, line_pos: usize, new_line: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        result.push(line.to_string());
        if i == line_pos {
            result.push(new_line.to_string());
        }
    }

    result.join("\n")
}

/// Preserve the comment formatting when updating a config line value
fn preserve_comment_formatting(original_line: &str, key: &str, new_value: &str) -> String {
    if let Some(comment_pos) = original_line.find('#') {
        let comment_part = &original_line[comment_pos..];
        let key_value_part = format!("{} = {}", key, new_value);

        // Calculate spacing to align with other comments (aim for around 33 characters total)
        let target_width = 33;
        let padding_needed = if key_value_part.len() < target_width {
            target_width - key_value_part.len()
        } else {
            1 // At least one space
        };

        format!(
            "{}{}{}",
            key_value_part,
            " ".repeat(padding_needed),
            comment_part
        )
    } else {
        format!("{} = {}", key, new_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::test_constants::*;
    use std::fs;
    use tempfile::tempdir;

    #[allow(clippy::too_many_arguments)]
    fn create_test_config(
        sunset: &str,
        sunrise: &str,
        transition_duration: Option<u64>,
        update_interval: Option<u64>,
        transition_mode: Option<&str>,
        night_temp: Option<u32>,
        day_temp: Option<u32>,
        night_gamma: Option<f32>,
        day_gamma: Option<f32>,
    ) -> Config {
        Config {
            start_hyprsunset: Some(false),
            backend: Some(Backend::Auto),
            startup_transition: Some(false),
            startup_transition_duration: Some(10),
            latitude: None,
            longitude: None,
            sunset: sunset.to_string(),
            sunrise: sunrise.to_string(),
            night_temp,
            day_temp,
            night_gamma,
            day_gamma,
            transition_duration,
            update_interval,
            transition_mode: transition_mode.map(|s| s.to_string()),
        }
    }

    #[test]
    fn test_config_load_default_creation() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("sunsetr").join("sunsetr.toml");

        // First load should create default config
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }
        let result = Config::load();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }

        assert!(result.is_ok());
        assert!(config_path.exists());
    }

    #[test]
    fn test_config_validation_basic() {
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_config_validation_backend_compatibility() {
        // Test valid combinations
        let mut config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );

        // Valid: use_wayland=false, start_hyprsunset=true (Hyprland backend)
        config.backend = Some(Backend::Hyprland);
        config.start_hyprsunset = Some(true);
        assert!(validate_config(&config).is_ok());

        // Valid: use_wayland=true, start_hyprsunset=false (Wayland backend)
        config.backend = Some(Backend::Wayland);
        config.start_hyprsunset = Some(false);
        assert!(validate_config(&config).is_ok());

        // Valid: use_wayland=false, start_hyprsunset=false (Hyprland without auto-start)
        config.backend = Some(Backend::Hyprland);
        config.start_hyprsunset = Some(false);
        assert!(validate_config(&config).is_ok());

        // Invalid: use_wayland=true, start_hyprsunset=true (conflicting)
        config.backend = Some(Backend::Wayland);
        config.start_hyprsunset = Some(true);
        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Incompatible configuration")
        );
    }

    #[test]
    fn test_config_validation_identical_times() {
        let config = create_test_config(
            "12:00:00",
            "12:00:00",
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());
        assert!(
            validate_config(&config)
                .unwrap_err()
                .to_string()
                .contains("cannot be the same time")
        );
    }

    #[test]
    fn test_config_validation_extreme_short_day() {
        // 30 minute day period (sunrise 23:45, sunset 00:15)
        let config = create_test_config(
            "00:15:00",
            "23:45:00",
            Some(5),
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());
        assert!(
            validate_config(&config)
                .unwrap_err()
                .to_string()
                .contains("Day period is too short")
        );
    }

    #[test]
    fn test_config_validation_extreme_short_night() {
        // 30 minute night period (sunset 23:45, sunrise 00:15)
        let config = create_test_config(
            "23:45:00",
            "00:15:00",
            Some(5),
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());
        assert!(
            validate_config(&config)
                .unwrap_err()
                .to_string()
                .contains("Night period is too short")
        );
    }

    #[test]
    fn test_config_validation_extreme_temperature_values() {
        // Test minimum temperature boundary
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(MINIMUM_TEMP),
            Some(MAXIMUM_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // Test below minimum temperature
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(MINIMUM_TEMP - 1),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());

        // Test above maximum temperature
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(MAXIMUM_TEMP + 1),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_config_validation_extreme_gamma_values() {
        // Test minimum gamma boundary
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(MINIMUM_GAMMA),
            Some(MAXIMUM_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // Test below minimum gamma
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(MINIMUM_GAMMA - 0.1),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());

        // Test above maximum gamma
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(MAXIMUM_GAMMA + 0.1),
        );
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_config_validation_extreme_transition_durations() {
        // Test minimum transition duration
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(MINIMUM_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // Test maximum transition duration
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(MAXIMUM_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // Test below minimum (should fail validation)
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(MINIMUM_TRANSITION_DURATION - 1),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());

        // Test above maximum (should fail validation)
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(MAXIMUM_TRANSITION_DURATION + 1),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_config_validation_extreme_update_intervals() {
        // Test minimum update interval
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(MINIMUM_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // Test maximum update interval
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(120),
            Some(MAXIMUM_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // Test update interval longer than transition
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(30),
            Some(30 * 60 + 1),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());
        assert!(
            validate_config(&config)
                .unwrap_err()
                .to_string()
                .contains("longer than transition duration")
        );
    }

    #[test]
    fn test_config_validation_center_mode_overlapping() {
        // Center mode with transition duration that would overlap
        // Day period is about 11 hours (06:00-19:00), night is 13 hours
        // Transition of 60 minutes in center mode means 30 minutes each side
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(60),
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some("center"),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // But if we make the transition too long for center mode
        // Let's try a 22-hour transition in center mode (11 hours each side)
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(22 * 60),
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some("center"),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_config_validation_midnight_crossings() {
        // Sunset after midnight, sunrise in evening - valid but extreme
        let config = create_test_config(
            "01:00:00",
            "22:00:00",
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());

        // Very late sunset, very early sunrise
        let config = create_test_config(
            "23:30:00",
            "00:30:00",
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some(TEST_STANDARD_UPDATE_INTERVAL),
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_config_validation_invalid_time_formats() {
        // This should fail during parsing, not validation
        assert!(NaiveTime::parse_from_str("25:00:00", "%H:%M:%S").is_err());
        assert!(NaiveTime::parse_from_str("19:60:00", "%H:%M:%S").is_err());
    }

    #[test]
    fn test_config_validation_transition_overlap_detection() {
        // Test transition overlap detection with extreme short periods
        let config = create_test_config(
            "12:30:00",
            "12:00:00",
            Some(60),
            Some(TEST_STANDARD_TRANSITION_DURATION),
            Some("center"),
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        // Should fail because day period is only 30 minutes, can't fit 1-hour center transition
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_config_validation_performance_warnings() {
        // Test configuration that should generate performance warnings
        let config = create_test_config(
            TEST_STANDARD_SUNSET,
            TEST_STANDARD_SUNRISE,
            Some(5),
            Some(5),
            Some(TEST_STANDARD_MODE), // Very frequent updates
            Some(TEST_STANDARD_NIGHT_TEMP),
            Some(TEST_STANDARD_DAY_TEMP),
            Some(TEST_STANDARD_NIGHT_GAMMA),
            Some(TEST_STANDARD_DAY_GAMMA),
        );
        // Should pass validation but might generate warnings (captured in logs)
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_default_config_file_creation() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("sunsetr.toml");

        Config::create_default_config(&config_path, None).unwrap();
        assert!(config_path.exists());

        let content = fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("start_hyprsunset"));
        assert!(content.contains("sunset"));
        assert!(content.contains("sunrise"));
        assert!(content.contains("night_temp"));
        assert!(content.contains("transition_mode"));
    }

    #[test]
    fn test_config_toml_parsing() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("test_config.toml");

        let config_content = r#"
start_hyprsunset = false
startup_transition = true
startup_transition_duration = 15
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = 3300
day_temp = 6000
night_gamma = 90.0
day_gamma = 100.0
transition_duration = 45
update_interval = 60
transition_mode = "finish_by"
"#;

        fs::write(&config_path, config_content).unwrap();
        let content = fs::read_to_string(&config_path).unwrap();
        let config: Config = toml::from_str(&content).unwrap();

        assert_eq!(config.start_hyprsunset, Some(false));
        assert_eq!(config.sunset, "19:00:00");
        assert_eq!(config.sunrise, "06:00:00");
        assert_eq!(config.night_temp, Some(3300));
        assert_eq!(config.transition_mode, Some("finish_by".to_string()));
    }

    #[test]
    fn test_config_malformed_toml() {
        let malformed_content = r#"
start_hyprsunset = false
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = "not_a_number"  # This should cause parsing to fail
"#;

        let result: Result<Config, _> = toml::from_str(malformed_content);
        assert!(result.is_err());
    }
}
