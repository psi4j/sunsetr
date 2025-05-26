use anyhow::{Context, Result};
use chrono::{NaiveTime, Timelike}; // Added Timelike import
use serde::Deserialize;
use std::fs::{self};
use std::path::PathBuf;

use crate::constants::*;
use crate::logger::Log;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub start_hyprsunset: Option<bool>,
    pub startup_transition: Option<bool>, // whether to enable smooth startup transition
    pub startup_transition_duration: Option<u64>, // seconds for startup transition
    pub sunset: String,
    pub sunrise: String,
    pub night_temp: Option<u32>,
    pub day_temp: Option<u32>,
    pub night_gamma: Option<f32>,
    pub day_gamma: Option<f32>,
    pub transition_duration: Option<u64>, // minutes
    pub update_interval: Option<u64>,     // seconds during transition
    pub transition_mode: Option<String>,  // "finish_by", "start_at", or "center"
}

impl Config {
    pub fn get_config_path() -> Result<PathBuf> {
        dirs::config_dir()
            .map(|p| p.join("hypr").join("sunsetr.toml"))
            .context("Could not determine config directory")
    }

    pub fn create_default_config(path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Failed to create config directory")?;
        }

        // Calculate the maximum width needed for comment alignment
        // We need to calculate the full "key = value" width for each line
        let config_entries = [
            format!("start_hyprsunset = {}", DEFAULT_START_HYPRSUNSET),
            format!("startup_transition = {}", DEFAULT_STARTUP_TRANSITION),
            format!(
                "startup_transition_duration = {}",
                DEFAULT_STARTUP_TRANSITION_DURATION
            ),
            format!("sunset = \"{}\"", DEFAULT_SUNSET),
            format!("sunrise = \"{}\"", DEFAULT_SUNRISE),
            format!("night_temp = {}", DEFAULT_NIGHT_TEMP),
            format!("day_temp = {}", DEFAULT_DAY_TEMP),
            format!("night_gamma = {}", DEFAULT_NIGHT_GAMMA),
            format!("day_gamma = {}", DEFAULT_DAY_GAMMA),
            format!("transition_duration = {}", DEFAULT_TRANSITION_DURATION),
            format!("update_interval = {}", DEFAULT_UPDATE_INTERVAL),
            format!("transition_mode = \"{}\"", DEFAULT_TRANSITION_MODE),
        ];

        let max_line_width = config_entries.iter().map(|line| line.len()).max().unwrap() + 1; // +1 for extra space

        // Calculate padding for each line to align comments
        let formatted_entries: Vec<String> = config_entries
            .iter()
            .map(|line| {
                let padding_needed = max_line_width - line.len();
                format!("{}{}", line, " ".repeat(padding_needed))
            })
            .collect();

        let default_config: String = format!(
            r#"#[Sunsetr configuration]
{}# Set true if you're not using hyprsunset.service
{}# Enable smooth transition when sunsetr starts
{}# Duration of startup transition in seconds ({}-{})
{}# Time to transition to night mode (HH:MM:SS)
{}# Time to transition to day mode (HH:MM:SS)
{}# Color temperature after sunset ({}-{}) Kelvin
{}# Color temperature during day ({}-{}) Kelvin
{}# Gamma percentage for night ({}-{}%)
{}# Gamma percentage for day ({}-{}%)
{}# Transition duration in minutes ({}-{})
{}# Update frequency during transitions in seconds ({}-{})
{}# Transition timing mode:
{}# "finish_by" - transition completes at sunset/sunrise time
{}# "start_at" - transition starts at sunset/sunrise time
{}# "center" - transition is centered on sunset/sunrise time
"#,
            formatted_entries[0], // start_hyprsunset entry
            formatted_entries[1], // enable_startup_transition entry
            formatted_entries[2], // startup_transition_duration entry
            MINIMUM_STARTUP_TRANSITION_DURATION,
            MAXIMUM_STARTUP_TRANSITION_DURATION, // startup_transition_duration range
            formatted_entries[3],                // sunset entry
            formatted_entries[4],                // sunrise entry
            formatted_entries[5],                // night_temp entry
            MINIMUM_TEMP,
            MAXIMUM_TEMP,         // night_temp range
            formatted_entries[6], // day_temp entry
            MINIMUM_TEMP,
            MAXIMUM_TEMP,         // day_temp range
            formatted_entries[7], // night_gamma entry
            MINIMUM_GAMMA,
            MAXIMUM_GAMMA,        // night_gamma range
            formatted_entries[8], // day_gamma entry
            MINIMUM_GAMMA,
            MAXIMUM_GAMMA,        // day_gamma range
            formatted_entries[9], // transition_duration entry
            MINIMUM_TRANSITION_DURATION,
            MAXIMUM_TRANSITION_DURATION, // transition_duration range
            formatted_entries[10],       // update_interval entry
            MINIMUM_UPDATE_INTERVAL,
            MAXIMUM_UPDATE_INTERVAL,    // update_interval range
            formatted_entries[11],      // transition_mode entry
            " ".repeat(max_line_width), // padding for first comment line
            " ".repeat(max_line_width), // padding for second comment line
            " ".repeat(max_line_width), // padding for third comment line
        );

        fs::write(path, default_config).context("Failed to write default config file")?;
        Ok(())
    }

    pub fn load() -> Result<Self> {
        let config_path = Self::get_config_path()?;

        if !config_path.exists() {
            Self::create_default_config(&config_path)?;
        }

        let content = fs::read_to_string(&config_path).context("Failed to read sunsetr.toml")?;

        let mut config: Config = toml::from_str(&content).context("Failed to parse config file")?;

        // Set default for start_hyprsunset if not specified
        if config.start_hyprsunset.is_none() {
            config.start_hyprsunset = Some(DEFAULT_START_HYPRSUNSET);
        }

        // Validate time formats
        NaiveTime::parse_from_str(&config.sunset, "%H:%M:%S")
            .context("Invalid sunset time format in config. Use HH:MM:SS format")?;
        NaiveTime::parse_from_str(&config.sunrise, "%H:%M:%S")
            .context("Invalid sunrise time format in config. Use HH:MM:SS format")?;

        // Validate temperature if specified
        if let Some(temp) = config.night_temp {
            if !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp) {
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
                anyhow::bail!(
                    "Transition duration must be between {} and {} minutes",
                    MINIMUM_TRANSITION_DURATION,
                    MAXIMUM_TRANSITION_DURATION
                );
            }
        }

        if let Some(interval) = config.update_interval {
            if !(MINIMUM_UPDATE_INTERVAL..=MAXIMUM_UPDATE_INTERVAL).contains(&interval) {
                anyhow::bail!(
                    "Update interval must be between {} and {} seconds",
                    MINIMUM_UPDATE_INTERVAL,
                    MAXIMUM_UPDATE_INTERVAL
                );
            }
        }

        // Validate transition mode
        if let Some(ref mode) = config.transition_mode {
            if mode != "finish_by" && mode != "start_at" && mode != "center" {
                anyhow::bail!("Transition mode must be 'finish_by', 'start_at', or 'center'");
            }
        }

        // Validate startup transition duration
        if let Some(duration_seconds) = config.startup_transition_duration {
            if !(MINIMUM_STARTUP_TRANSITION_DURATION..=MAXIMUM_STARTUP_TRANSITION_DURATION)
                .contains(&duration_seconds)
            {
                anyhow::bail!(
                    "Startup transition duration must be between {} and {} seconds",
                    MINIMUM_STARTUP_TRANSITION_DURATION,
                    MAXIMUM_STARTUP_TRANSITION_DURATION
                );
            }
        }

        // Comprehensive configuration validation
        validate_config(&config)?;

        Ok(config)
    }

    pub fn log_config(&self) {
        Log::log_block_start("Loaded configuration");
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
        if self.startup_transition.unwrap_or(DEFAULT_STARTUP_TRANSITION) {
            Log::log_indented(&format!(
                "Startup transition duration: {} seconds",
                self.startup_transition_duration
                    .unwrap_or(DEFAULT_STARTUP_TRANSITION_DURATION)
            ));
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
fn validate_config(config: &Config) -> Result<()> {
    use chrono::NaiveTime;

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

    // 0. Validate basic ranges for temperature and gamma (hard limits)
    if let Some(temp) = config.night_temp {
        if !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp) {
            anyhow::bail!(
                "Night temperature ({}) must be between {} and {} Kelvin",
                temp, MINIMUM_TEMP, MAXIMUM_TEMP
            );
        }
    }

    if let Some(temp) = config.day_temp {
        if !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp) {
            anyhow::bail!(
                "Day temperature ({}) must be between {} and {} Kelvin",
                temp, MINIMUM_TEMP, MAXIMUM_TEMP
            );
        }
    }

    if let Some(gamma) = config.night_gamma {
        if !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma) {
            anyhow::bail!(
                "Night gamma ({}%) must be between {}% and {}%",
                gamma, MINIMUM_GAMMA, MAXIMUM_GAMMA
            );
        }
    }

    if let Some(gamma) = config.day_gamma {
        if !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma) {
            anyhow::bail!(
                "Day gamma ({}%) must be between {}% and {}%",
                gamma, MINIMUM_GAMMA, MAXIMUM_GAMMA
            );
        }
    }

    // Validate transition duration (hard limits)
    if !(MINIMUM_TRANSITION_DURATION..=MAXIMUM_TRANSITION_DURATION).contains(&transition_duration_mins) {
        anyhow::bail!(
            "Transition duration ({} minutes) must be between {} and {} minutes",
            transition_duration_mins, MINIMUM_TRANSITION_DURATION, MAXIMUM_TRANSITION_DURATION
        );
    }

    // 1. Check for identical sunset/sunrise times
    if sunset == sunrise {
        anyhow::bail!(
            "Sunset and sunrise cannot be the same time ({:?}). \
            There must be a distinction between day and night periods.",
            sunset
        );
    }

    // 2. Calculate time periods and check minimums
    let (day_duration_mins, night_duration_mins) = calculate_day_night_durations(sunset, sunrise);

    if day_duration_mins < 60 {
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
            startup_transition: Some(false),
            startup_transition_duration: Some(10),
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
        let config_path = temp_dir.path().join("hypr").join("sunsetr.toml");
        
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
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(TEST_STANDARD_TRANSITION_DURATION), 
            Some(TEST_STANDARD_UPDATE_INTERVAL), Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_config_validation_identical_times() {
        let config = create_test_config(
            "12:00:00", "12:00:00", Some(TEST_STANDARD_TRANSITION_DURATION), 
            Some(TEST_STANDARD_UPDATE_INTERVAL), Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_err());
        assert!(validate_config(&config).unwrap_err().to_string().contains("cannot be the same time"));
    }

    #[test]
    fn test_config_validation_extreme_short_day() {
        // 30 minute day period (sunrise 23:45, sunset 00:15)
        let config = create_test_config(
            "00:15:00", "23:45:00", Some(5), Some(TEST_STANDARD_TRANSITION_DURATION), Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_err());
        assert!(validate_config(&config).unwrap_err().to_string().contains("Day period is too short"));
    }

    #[test]
    fn test_config_validation_extreme_short_night() {
        // 30 minute night period (sunset 23:45, sunrise 00:15)
        let config = create_test_config(
            "23:45:00", "00:15:00", Some(5), Some(TEST_STANDARD_TRANSITION_DURATION), Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_err());
        assert!(validate_config(&config).unwrap_err().to_string().contains("Night period is too short"));
    }

    #[test]
    fn test_config_validation_extreme_temperature_values() {
        // Test minimum temperature boundary
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(TEST_STANDARD_TRANSITION_DURATION), 
            Some(TEST_STANDARD_UPDATE_INTERVAL), Some(TEST_STANDARD_MODE),
            Some(MINIMUM_TEMP), Some(MAXIMUM_TEMP), Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_ok());

        // Test below minimum temperature
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(TEST_STANDARD_TRANSITION_DURATION), 
            Some(TEST_STANDARD_UPDATE_INTERVAL), Some(TEST_STANDARD_MODE),
            Some(MINIMUM_TEMP - 1), Some(TEST_STANDARD_DAY_TEMP), Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_err());

        // Test above maximum temperature
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(TEST_STANDARD_TRANSITION_DURATION), 
            Some(TEST_STANDARD_UPDATE_INTERVAL), Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(MAXIMUM_TEMP + 1), Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_config_validation_extreme_gamma_values() {
        // Test minimum gamma boundary
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(TEST_STANDARD_TRANSITION_DURATION), 
            Some(TEST_STANDARD_UPDATE_INTERVAL), Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), Some(MINIMUM_GAMMA), Some(MAXIMUM_GAMMA)
        );
        assert!(validate_config(&config).is_ok());

        // Test below minimum gamma
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(TEST_STANDARD_TRANSITION_DURATION), 
            Some(TEST_STANDARD_UPDATE_INTERVAL), Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), Some(MINIMUM_GAMMA - 0.1), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_err());

        // Test above maximum gamma  
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(TEST_STANDARD_TRANSITION_DURATION), 
            Some(TEST_STANDARD_UPDATE_INTERVAL), Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), Some(TEST_STANDARD_NIGHT_GAMMA), Some(MAXIMUM_GAMMA + 0.1)
        );
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_config_validation_extreme_transition_durations() {
        // Test minimum transition duration
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(MINIMUM_TRANSITION_DURATION), 
            Some(TEST_STANDARD_UPDATE_INTERVAL), Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_ok());

        // Test maximum transition duration
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(MAXIMUM_TRANSITION_DURATION), 
            Some(TEST_STANDARD_UPDATE_INTERVAL), Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_ok());

        // Test below minimum (should fail validation)
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(MINIMUM_TRANSITION_DURATION - 1), 
            Some(TEST_STANDARD_UPDATE_INTERVAL), Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_err());

        // Test above maximum (should fail validation)
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(MAXIMUM_TRANSITION_DURATION + 1), 
            Some(TEST_STANDARD_UPDATE_INTERVAL), Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_config_validation_extreme_update_intervals() {
        // Test minimum update interval
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(TEST_STANDARD_TRANSITION_DURATION), 
            Some(MINIMUM_UPDATE_INTERVAL), Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_ok());

        // Test maximum update interval
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(120), Some(MAXIMUM_UPDATE_INTERVAL), 
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_ok());

        // Test update interval longer than transition
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(30), Some(30 * 60 + 1), 
            Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_err());
        assert!(validate_config(&config).unwrap_err().to_string().contains("longer than transition duration"));
    }

    #[test]
    fn test_config_validation_center_mode_overlapping() {
        // Center mode with transition duration that would overlap
        // Day period is about 11 hours (06:00-19:00), night is 13 hours
        // Transition of 60 minutes in center mode means 30 minutes each side
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(60), Some(TEST_STANDARD_TRANSITION_DURATION), 
            Some("center"),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_ok());

        // But if we make the transition too long for center mode
        // Let's try a 22-hour transition in center mode (11 hours each side)
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(22 * 60), Some(TEST_STANDARD_TRANSITION_DURATION), 
            Some("center"),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_config_validation_midnight_crossings() {
        // Sunset after midnight, sunrise in evening - valid but extreme
        let config = create_test_config(
            "01:00:00", "22:00:00", Some(TEST_STANDARD_TRANSITION_DURATION), 
            Some(TEST_STANDARD_UPDATE_INTERVAL), Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        assert!(validate_config(&config).is_ok());

        // Very late sunset, very early sunrise
        let config = create_test_config(
            "23:30:00", "00:30:00", Some(TEST_STANDARD_TRANSITION_DURATION), 
            Some(TEST_STANDARD_UPDATE_INTERVAL), Some(TEST_STANDARD_MODE),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
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
            "12:30:00", "12:00:00", Some(60), Some(TEST_STANDARD_TRANSITION_DURATION), Some("center"),
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        // Should fail because day period is only 30 minutes, can't fit 1-hour center transition
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_config_validation_performance_warnings() {
        // Test configuration that should generate performance warnings
        let config = create_test_config(
            TEST_STANDARD_SUNSET, TEST_STANDARD_SUNRISE, Some(5), Some(5), Some(TEST_STANDARD_MODE), // Very frequent updates
            Some(TEST_STANDARD_NIGHT_TEMP), Some(TEST_STANDARD_DAY_TEMP), 
            Some(TEST_STANDARD_NIGHT_GAMMA), Some(TEST_STANDARD_DAY_GAMMA)
        );
        // Should pass validation but might generate warnings (captured in logs)
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_default_config_file_creation() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("sunsetr.toml");
        
        Config::create_default_config(&config_path).unwrap();
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
