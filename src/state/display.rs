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
use crate::core::period::Period;
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

    /// Currently applied temperature in Kelvin
    pub current_temp: u32,

    /// Currently applied gamma as percentage
    pub current_gamma: f32,

    /// Target temperature we're transitioning to
    pub target_temp: u32,

    /// Target gamma we're transitioning to  
    pub target_gamma: f32,

    /// Next scheduled period time
    pub next_period: Option<DateTime<Local>>,

    /// Time remaining until next period starts (seconds)
    pub time_remaining: Option<u64>,
}

impl DisplayState {
    /// Create a new DisplayState from current runtime values.
    ///
    /// # Arguments
    /// * `current_state` - Current Period from period module
    /// * `last_applied_temp` - Temperature value last applied to backend
    /// * `last_applied_gamma` - Gamma value last applied to backend
    /// * `config` - Current configuration
    /// * `geo_times` - Optional geographic transition times
    pub fn new(
        current_state: Period,
        last_applied_temp: u32,
        last_applied_gamma: f32,
        config: &Config,
        geo_times: Option<&GeoTimes>,
    ) -> Self {
        // Calculate target values - what we're transitioning TO, not current interpolated values
        let (target_temp, target_gamma) = match current_state {
            Period::Sunset => {
                // Transitioning to night - use Night state to get target values
                let runtime_state = crate::core::runtime_state::RuntimeState::new(
                    Period::Night,
                    config,
                    geo_times,
                    crate::time::source::now().time(),
                );
                runtime_state.values()
            }
            Period::Sunrise => {
                // Transitioning to day - use Day state to get target values
                let runtime_state = crate::core::runtime_state::RuntimeState::new(
                    Period::Day,
                    config,
                    geo_times,
                    crate::time::source::now().time(),
                );
                runtime_state.values()
            }
            _ => {
                // For stable states, target equals current
                let runtime_state = crate::core::runtime_state::RuntimeState::new(
                    current_state,
                    config,
                    geo_times,
                    crate::time::source::now().time(),
                );
                runtime_state.values()
            }
        };

        // Calculate next period time
        let next_period = Self::calculate_next_period(current_state, config, geo_times);

        // Calculate time remaining until next period starts
        let time_remaining = if let Some(next_time) = next_period {
            let now = crate::time::source::now();
            let duration = next_time - now;
            if duration.num_seconds() > 0 {
                Some(duration.num_seconds() as u64)
            } else {
                None
            }
        } else {
            None
        };

        // Get the active preset name
        let active_preset = Config::get_active_preset()
            .ok()
            .flatten()
            .unwrap_or_else(|| "default".to_string());

        DisplayState {
            active_preset,
            period: current_state,
            current_temp: last_applied_temp,
            current_gamma: last_applied_gamma,
            target_temp,
            target_gamma,
            next_period,
            time_remaining,
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
            let duration = times.duration_until_next_transition(crate::time::source::now());
            Some(crate::time::source::now() + chrono::Duration::from_std(duration).ok()?)
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
            times
                .duration_until_transition_end(crate::time::source::now())
                .and_then(|duration| chrono::Duration::from_std(duration).ok())
                .map(|duration| crate::time::source::now() + duration)
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
            let duration = times.duration_until_next_transition(crate::time::source::now());
            Some(crate::time::source::now() + chrono::Duration::from_std(duration).ok()?)
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
            times
                .duration_until_transition_end(crate::time::source::now())
                .and_then(|duration| chrono::Duration::from_std(duration).ok())
                .map(|duration| crate::time::source::now() + duration)
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

    /// Update the display state with new values.
    ///
    /// This is called during the main loop to keep the DisplayState synchronized
    /// with the actual runtime state.
    pub fn update(
        &mut self,
        current_state: Period,
        last_applied_temp: u32,
        last_applied_gamma: f32,
        config: &Config,
        geo_times: Option<&GeoTimes>,
    ) {
        // Update all fields with fresh calculations
        *self = Self::new(
            current_state,
            last_applied_temp,
            last_applied_gamma,
            config,
            geo_times,
        );
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

        let display_state = DisplayState::new(
            current_state,
            6500,  // last_applied_temp
            100.0, // last_applied_gamma
            &config,
            None,
        );

        assert!(!display_state.period.is_transitioning());
        assert_eq!(display_state.current_temp, 6500);
        assert_eq!(display_state.current_gamma, 100.0);
        assert!(display_state.next_period.is_some());
        assert!(display_state.time_remaining.is_some());
    }

    #[test]
    fn test_display_state_transitioning() {
        let config = create_test_config();
        let current_state = Period::Sunset;

        let display_state = DisplayState::new(
            current_state,
            4900, // last_applied_temp (mid-transition)
            95.0, // last_applied_gamma (mid-transition)
            &config,
            None,
        );

        assert!(display_state.period.is_transitioning());
        assert_eq!(display_state.period, Period::Sunset);
        assert_eq!(display_state.current_temp, 4900);
        assert_eq!(display_state.current_gamma, 95.0);
        assert_eq!(display_state.target_temp, 3300); // Target is night temp
        assert_eq!(display_state.target_gamma, 90.0); // Target is night gamma
    }

    #[test]
    fn test_display_state_static_mode() {
        let mut config = create_test_config();
        config.transition_mode = Some("static".to_string());
        config.static_temp = Some(5000);
        config.static_gamma = Some(85.0);

        let current_state = Period::Static;

        let display_state = DisplayState::new(current_state, 5000, 85.0, &config, None);

        assert!(!display_state.period.is_transitioning());
        assert_eq!(display_state.current_temp, 5000);
        assert_eq!(display_state.current_gamma, 85.0);
        assert!(display_state.next_period.is_none()); // No transitions in static mode
    }

    #[test]
    fn test_display_state_serialization() {
        let config = create_test_config();
        let current_state = Period::Night;

        let display_state = DisplayState::new(current_state, 3300, 90.0, &config, None);

        // Test JSON serialization
        let json = display_state.to_json();
        assert!(json.is_ok());

        let json_str = json.unwrap();
        assert!(json_str.contains("\"period\":\"night\""));
        assert!(json_str.contains("\"current_temp\":3300"));
        assert!(json_str.contains("\"current_gamma\":90"));
    }
}
