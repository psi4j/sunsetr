//! Runtime display state tracking for IPC and external integrations.
//!
//! This module provides the DisplayState struct which tracks the current
//! runtime state of sunsetr, including interpolated temperature/gamma values,
//! transition progress, and scheduling information. This data structure is
//! designed for real-time communication with external applications through
//! IPC mechanisms.
use chrono::{DateTime, Local, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::config::Config;
use crate::core::period::{Period, PeriodType};
use crate::geo::times::GeoTimes;

/// Runtime display state that changes during transitions.
///
/// This struct captures all dynamic runtime values that external applications
/// might need to react to sunsetr's state changes. It's designed to be
/// serializable for IPC communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayState {
    /// Currently active preset name (or "default" if using base configuration)
    pub active_preset: String,

    /// Current time-based state
    pub period: Period,

    /// Period type for presentation layer categorization
    #[serde(rename = "state")]
    pub period_type: PeriodType,

    /// Transition progress (0.0 to 1.0) for transitioning periods, None for stable periods
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<f32>,

    /// Currently applied temperature in Kelvin
    pub current_temp: u32,

    /// Currently applied gamma as percentage
    pub current_gamma: f32,

    /// Target temperature we're transitioning to (only present during transitions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_temp: Option<u32>,

    /// Target gamma we're transitioning to (only present during transitions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_gamma: Option<f32>,

    /// Next scheduled period time (None for static mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_period: Option<DateTime<Local>>,
}

impl DisplayState {
    /// Create a new DisplayState from RuntimeState.
    ///
    /// # Arguments
    /// * `runtime_state` - Current RuntimeState containing all runtime context
    pub fn new(runtime_state: &crate::core::runtime_state::RuntimeState) -> Self {
        let current_state = runtime_state.period();
        let config = runtime_state.config();
        let geo_times = runtime_state.geo_times();

        // Calculate target values - what we're transitioning TO (only for transitioning periods)
        let (target_temp, target_gamma) = if current_state.is_transitioning() {
            match current_state {
                Period::Sunset => {
                    // Transitioning to night - calculate night values from config
                    let night_temp = config
                        .night_temp
                        .unwrap_or(crate::common::constants::DEFAULT_NIGHT_TEMP);
                    let night_gamma = config
                        .night_gamma
                        .unwrap_or(crate::common::constants::DEFAULT_NIGHT_GAMMA);
                    (Some(night_temp), Some(night_gamma))
                }
                Period::Sunrise => {
                    // Transitioning to day - calculate day values from config
                    let day_temp = config
                        .day_temp
                        .unwrap_or(crate::common::constants::DEFAULT_DAY_TEMP);
                    let day_gamma = config
                        .day_gamma
                        .unwrap_or(crate::common::constants::DEFAULT_DAY_GAMMA);
                    (Some(day_temp), Some(day_gamma))
                }
                _ => {
                    // Shouldn't reach here for transitioning states
                    (None, None)
                }
            }
        } else {
            // For stable/static states, no target values (already at destination)
            (None, None)
        };

        // Calculate next period time
        let next_period = Self::calculate_next_period(current_state, config, geo_times);

        // Get progress for transitioning periods only
        let progress = if current_state.is_transitioning() {
            runtime_state.progress()
        } else {
            None
        };

        // Get the active preset name
        let active_preset = crate::state::preset::get_active_preset()
            .ok()
            .flatten()
            .unwrap_or_else(|| "default".to_string());

        // Extract current applied values from RuntimeState
        let (current_temp, current_gamma) = runtime_state.values();

        DisplayState {
            active_preset,
            period: current_state,
            period_type: current_state.period_type(),
            progress,
            current_temp,
            current_gamma,
            target_temp,
            target_gamma,
            next_period,
        }
    }

    /// Calculate when the next period in the logical sequence will start.
    ///
    /// Uses the Period.next_period() method to determine what comes next,
    /// then finds when that period starts regardless of mode.
    fn calculate_next_period(
        current_state: Period,
        config: &Config,
        geo_times: Option<&GeoTimes>,
    ) -> Option<DateTime<Local>> {
        // Static mode has no next period
        if matches!(current_state, Period::Static) {
            return None;
        }

        // Determine what period comes next in the logical sequence
        let next_period = current_state.next_period();

        // Find when that next period starts
        match next_period {
            Period::Sunset => {
                // Next period is Sunset transition - find when it starts
                Self::find_next_sunset_start(config, geo_times)
            }
            Period::Night => {
                // Next period is Night - find when current sunset transition ends
                Self::find_next_night_start(config, geo_times)
            }
            Period::Sunrise => {
                // Next period is Sunrise transition - find when it starts
                Self::find_next_sunrise_start(config, geo_times)
            }
            Period::Day => {
                // Next period is Day - find when current sunrise transition ends
                Self::find_next_day_start(config, geo_times)
            }
            Period::Static => None,
        }
    }

    /// Find when the next Sunset transition starts.
    fn find_next_sunset_start(
        config: &Config,
        geo_times: Option<&GeoTimes>,
    ) -> Option<DateTime<Local>> {
        if config.transition_mode.as_deref() == Some("geo")
            && let Some(times) = geo_times
        {
            // Capture now() once to avoid time drift between calls
            let now = crate::time::source::now();
            let duration = times.duration_until_next_transition(now);
            Some(now + chrono::Duration::from_std(duration).ok()?)
        } else {
            let (sunset_start, _, _, _) = Self::get_transition_windows(config, geo_times);
            Self::find_next_time_occurrence(sunset_start)
        }
    }

    /// Find when Night period starts (sunset transition ends).
    fn find_next_night_start(
        config: &Config,
        geo_times: Option<&GeoTimes>,
    ) -> Option<DateTime<Local>> {
        if config.transition_mode.as_deref() == Some("geo")
            && let Some(times) = geo_times
        {
            // Capture now() once to avoid time drift between calls
            let now = crate::time::source::now();
            times
                .duration_until_transition_end(now)
                .and_then(|duration| chrono::Duration::from_std(duration).ok())
                .map(|duration| now + duration)
        } else {
            let (_, sunset_end, _, _) = Self::get_transition_windows(config, geo_times);
            Self::find_next_time_occurrence(sunset_end)
        }
    }

    /// Find when the next Sunrise transition starts.
    fn find_next_sunrise_start(
        config: &Config,
        geo_times: Option<&GeoTimes>,
    ) -> Option<DateTime<Local>> {
        if config.transition_mode.as_deref() == Some("geo")
            && let Some(times) = geo_times
        {
            // Capture now() once to avoid time drift between calls
            let now = crate::time::source::now();
            let duration = times.duration_until_next_transition(now);
            Some(now + chrono::Duration::from_std(duration).ok()?)
        } else {
            let (_, _, sunrise_start, _) = Self::get_transition_windows(config, geo_times);
            Self::find_next_time_occurrence(sunrise_start)
        }
    }

    /// Find when Day period starts (sunrise transition ends).
    fn find_next_day_start(
        config: &Config,
        geo_times: Option<&GeoTimes>,
    ) -> Option<DateTime<Local>> {
        if config.transition_mode.as_deref() == Some("geo")
            && let Some(times) = geo_times
        {
            // Capture now() once to avoid time drift between calls
            let now = crate::time::source::now();
            times
                .duration_until_transition_end(now)
                .and_then(|duration| chrono::Duration::from_std(duration).ok())
                .map(|duration| now + duration)
        } else {
            let (_, _, _, sunrise_end) = Self::get_transition_windows(config, geo_times);
            Self::find_next_time_occurrence(sunrise_end)
        }
    }

    /// Find the next occurrence of a specific time (today or tomorrow).
    fn find_next_time_occurrence(target_time: NaiveTime) -> Option<DateTime<Local>> {
        let now = crate::time::source::now();
        let today = now.date_naive();
        let tomorrow = today + chrono::Duration::days(1);

        let candidates = vec![today.and_time(target_time), tomorrow.and_time(target_time)];

        candidates
            .into_iter()
            .filter(|dt| *dt > now.naive_local())
            .min()
            .and_then(|naive_dt| Local.from_local_datetime(&naive_dt).single())
    }

    /// Get transition windows from config, matching period module logic.
    fn get_transition_windows(
        config: &Config,
        geo_times: Option<&GeoTimes>,
    ) -> (NaiveTime, NaiveTime, NaiveTime, NaiveTime) {
        // For geo mode, use pre-calculated times
        if config.transition_mode.as_deref() == Some("geo")
            && let Some(times) = geo_times
        {
            return times.as_naive_times_local();
        }

        // For non-geo modes, calculate from config
        let sunset_str = config
            .sunset
            .as_deref()
            .unwrap_or(crate::common::constants::DEFAULT_SUNSET);
        let sunrise_str = config
            .sunrise
            .as_deref()
            .unwrap_or(crate::common::constants::DEFAULT_SUNRISE);

        let sunset = NaiveTime::parse_from_str(sunset_str, "%H:%M:%S")
            .unwrap_or_else(|_| NaiveTime::from_hms_opt(19, 0, 0).unwrap());
        let sunrise = NaiveTime::parse_from_str(sunrise_str, "%H:%M:%S")
            .unwrap_or_else(|_| NaiveTime::from_hms_opt(6, 0, 0).unwrap());

        let transition_duration = Duration::from_secs(
            config
                .transition_duration
                .unwrap_or(crate::common::constants::DEFAULT_TRANSITION_DURATION)
                * 60,
        );

        let mode = config.transition_mode.as_deref().unwrap_or("finish_by");

        match mode {
            "center" => {
                let half = chrono::Duration::from_std(transition_duration / 2).unwrap();
                (
                    sunset - half,  // Sunset start
                    sunset + half,  // Sunset end
                    sunrise - half, // Sunrise start
                    sunrise + half, // Sunrise end
                )
            }
            "start_at" => {
                let full = chrono::Duration::from_std(transition_duration).unwrap();
                (
                    sunset,         // Sunset start
                    sunset + full,  // Sunset end
                    sunrise,        // Sunrise start
                    sunrise + full, // Sunrise end
                )
            }
            _ => {
                // "finish_by" or default
                let full = chrono::Duration::from_std(transition_duration).unwrap();
                (
                    sunset - full,  // Sunset start
                    sunset,         // Sunset end
                    sunrise - full, // Sunrise start
                    sunrise,        // Sunrise end
                )
            }
        }
    }

    /// Update the display state with new RuntimeState.
    ///
    /// This is called during the main loop to keep the DisplayState synchronized
    /// with the actual runtime state.
    pub fn update(&mut self, runtime_state: &crate::core::runtime_state::RuntimeState) {
        // Update all fields with fresh calculations using RuntimeState
        *self = Self::new(runtime_state);
    }

    /// Convert to JSON string for IPC communication.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    /// Convert to pretty JSON string for human-readable output.
    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::core::period::Period;

    fn create_test_config() -> Config {
        Config {
            backend: Some(crate::config::Backend::Auto),
            smoothing: Some(false),
            startup_duration: Some(10.0),
            shutdown_duration: Some(10.0),
            startup_transition: Some(false),
            startup_transition_duration: Some(10.0),
            start_hyprsunset: None,
            adaptive_interval: None,
            latitude: None,
            longitude: None,
            sunset: Some("19:00:00".to_string()),
            sunrise: Some("06:00:00".to_string()),
            night_temp: Some(3300),
            day_temp: Some(6500),
            night_gamma: Some(90.0),
            day_gamma: Some(100.0),
            static_temp: None,
            static_gamma: None,
            transition_duration: Some(30),
            update_interval: Some(60),
            transition_mode: Some("finish_by".to_string()),
        }
    }

    #[test]
    fn test_display_state_creation() {
        let config = create_test_config();
        let current_state = Period::Day;

        let runtime_state = crate::core::runtime_state::RuntimeState::new(
            current_state,
            &config,
            None,
            crate::time::source::now().time(),
        );

        let display_state = DisplayState::new(&runtime_state);

        assert!(!display_state.period.is_transitioning());
        assert_eq!(display_state.current_temp, 6500);
        assert_eq!(display_state.current_gamma, 100.0);
        assert!(display_state.next_period.is_some());
        // Stable period should not have target values
        assert!(display_state.target_temp.is_none());
        assert!(display_state.target_gamma.is_none());
        assert!(display_state.progress.is_none());
    }

    #[test]
    fn test_display_state_transitioning() {
        let config = create_test_config();
        let current_state = Period::Sunset;

        // Use a time within the sunset transition window
        // Config has sunset at 19:00:00 with 30min duration in "finish_by" mode
        // This means transition window is 18:30:00 to 19:00:00
        // Pick a time in the middle: 18:45:00
        let test_time = chrono::NaiveTime::from_hms_opt(18, 45, 0).unwrap();

        // Create RuntimeState following the new architecture
        let runtime_state =
            crate::core::runtime_state::RuntimeState::new(current_state, &config, None, test_time);

        let display_state = DisplayState::new(&runtime_state);

        // Get expected values from RuntimeState for comparison
        let (expected_temp, expected_gamma) = runtime_state.values();

        assert!(display_state.period.is_transitioning());
        assert_eq!(display_state.period, Period::Sunset);
        assert_eq!(display_state.current_temp, expected_temp);
        assert_eq!(display_state.current_gamma, expected_gamma);
        // Transitioning period should have target values
        assert_eq!(display_state.target_temp, Some(3300)); // Target is night temp
        assert_eq!(display_state.target_gamma, Some(90.0)); // Target is night gamma
        assert!(display_state.progress.is_some());
    }

    #[test]
    fn test_display_state_static_mode() {
        let mut config = create_test_config();
        config.transition_mode = Some("static".to_string());
        config.static_temp = Some(5000);
        config.static_gamma = Some(85.0);

        let current_state = Period::Static;

        // Create RuntimeState following the new architecture
        let runtime_state = crate::core::runtime_state::RuntimeState::new(
            current_state,
            &config,
            None,
            crate::time::source::now().time(),
        );

        let display_state = DisplayState::new(&runtime_state);

        assert!(!display_state.period.is_transitioning());
        assert_eq!(display_state.current_temp, 5000);
        assert_eq!(display_state.current_gamma, 85.0);
        // Static mode should not have target values or next period
        assert!(display_state.target_temp.is_none());
        assert!(display_state.target_gamma.is_none());
        assert!(display_state.progress.is_none());
        assert!(display_state.next_period.is_none());
    }

    #[test]
    fn test_display_state_serialization() {
        let config = create_test_config();
        let current_state = Period::Night;

        // Create RuntimeState following the new architecture
        let runtime_state = crate::core::runtime_state::RuntimeState::new(
            current_state,
            &config,
            None,
            crate::time::source::now().time(),
        );

        let display_state = DisplayState::new(&runtime_state);

        // Test JSON serialization
        let json = display_state.to_json();
        assert!(json.is_ok());

        let json_str = json.unwrap();
        assert!(json_str.contains("\"period\":\"night\""));
        assert!(json_str.contains("\"current_temp\":3300"));
        assert!(json_str.contains("\"current_gamma\":90"));
        // Stable period should not have target values or progress in JSON
        assert!(!json_str.contains("\"target_temp\""));
        assert!(!json_str.contains("\"target_gamma\""));
        assert!(!json_str.contains("\"progress\""));
    }
}
