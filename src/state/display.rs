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
use crate::geo::times::GeoTimes;
use crate::state::period::{Period, time_until_transition_end};

/// Runtime display state that changes during transitions.
///
/// This struct captures all dynamic runtime values that external applications
/// might need to react to sunsetr's state changes. It's designed to be
/// serializable for IPC communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct DisplayState {
    /// Current time-based state
    pub time_state: PeriodSerializable,

    /// Whether currently transitioning between states
    pub is_transitioning: bool,

    /// Progress through current transition (0.0 to 100.0)
    pub transition_progress: f32,

    /// Currently applied temperature in Kelvin
    pub current_temp: u32,

    /// Currently applied gamma as percentage
    pub current_gamma: f32,

    /// Target temperature we're transitioning to
    pub target_temp: u32,

    /// Target gamma we're transitioning to  
    pub target_gamma: f32,

    /// Next scheduled transition time
    pub next_transition: Option<DateTime<Local>>,

    /// Time remaining in current transition (seconds)
    pub transition_remaining: Option<u64>,
}

/// Serializable version of Period for IPC communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeriodSerializable {
    Day,
    Night,
    Sunrise,
    Sunset,
    Static,
}

impl From<Period> for PeriodSerializable {
    fn from(state: Period) -> Self {
        match state {
            Period::Day => PeriodSerializable::Day,
            Period::Night => PeriodSerializable::Night,
            Period::Sunrise { .. } => PeriodSerializable::Sunrise,
            Period::Sunset { .. } => PeriodSerializable::Sunset,
            Period::Static => PeriodSerializable::Static,
        }
    }
}

#[allow(dead_code)]
impl DisplayState {
    /// Create a new DisplayState from current runtime values.
    ///
    /// # Arguments
    /// * `current_state` - Current Period from time_state module
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
        // Determine if we're transitioning
        let is_transitioning = current_state.is_transitioning();

        // Get transition progress if applicable (convert to percentage)
        let transition_progress = current_state.progress().unwrap_or(0.0) * 100.0;

        // Calculate target values based on current state
        let (target_temp, target_gamma) = current_state.values(config);

        // Calculate next transition time
        let next_transition = Self::calculate_next_transition(current_state, config, geo_times);

        // Calculate time remaining in current transition
        let transition_remaining = if is_transitioning {
            time_until_transition_end(config, geo_times).map(|d| d.as_secs())
        } else {
            None
        };

        DisplayState {
            time_state: current_state.into(),
            is_transitioning,
            transition_progress,
            current_temp: last_applied_temp,
            current_gamma: last_applied_gamma,
            target_temp,
            target_gamma,
            next_transition,
            transition_remaining,
        }
    }

    /// Calculate the next scheduled transition time.
    ///
    /// This determines when the next sunrise or sunset transition will begin
    /// based on the current configuration mode (geo, static, or time-based).
    fn calculate_next_transition(
        current_state: Period,
        config: &Config,
        geo_times: Option<&GeoTimes>,
    ) -> Option<DateTime<Local>> {
        // Static mode has no transitions
        if matches!(current_state, Period::Static) {
            return None;
        }

        // For geo mode with pre-calculated times, use the optimized path
        if config.transition_mode.as_deref() == Some("geo")
            && let Some(times) = geo_times
        {
            // Get duration until next transition and convert to DateTime
            let duration = times.duration_until_next_transition(crate::time::source::now());
            let next_time =
                crate::time::source::now() + chrono::Duration::from_std(duration).ok()?;
            return Some(next_time);
        }

        // For non-geo modes, calculate from config times
        let now = crate::time::source::now();
        let today = now.date_naive();
        let tomorrow = today + chrono::Duration::days(1);

        // Get transition windows
        let (sunset_start, _, sunrise_start, _) = Self::get_transition_windows(config, geo_times);

        // Create DateTime objects for today's and tomorrow's transitions
        let candidates = vec![
            today.and_time(sunset_start),
            today.and_time(sunrise_start),
            tomorrow.and_time(sunset_start),
            tomorrow.and_time(sunrise_start),
        ];

        // Find the next future transition
        candidates
            .into_iter()
            .filter(|dt| *dt > now.naive_local())
            .min()
            .and_then(|naive_dt| {
                // Convert NaiveDateTime to DateTime<Local>
                Local.from_local_datetime(&naive_dt).single()
            })
    }

    /// Get transition windows from config, matching time_state module logic.
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
    use crate::state::period::Period;

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

        assert!(!display_state.is_transitioning);
        assert_eq!(display_state.current_temp, 6500);
        assert_eq!(display_state.current_gamma, 100.0);
        assert_eq!(display_state.transition_progress, 0.0);
        assert!(display_state.next_transition.is_some());
        assert!(display_state.transition_remaining.is_none());
    }

    #[test]
    fn test_display_state_transitioning() {
        let config = create_test_config();
        let current_state = Period::Sunset { progress: 0.5 };

        let display_state = DisplayState::new(
            current_state,
            4900, // last_applied_temp (mid-transition)
            95.0, // last_applied_gamma (mid-transition)
            &config,
            None,
        );

        assert!(display_state.is_transitioning);
        assert_eq!(display_state.transition_progress, 50.0);
        assert_eq!(display_state.current_temp, 4900);
        assert_eq!(display_state.current_gamma, 95.0);
        assert_eq!(display_state.target_temp, 4900); // Interpolated value at 50%
        assert_eq!(display_state.target_gamma, 95.0); // Interpolated value at 50%
    }

    #[test]
    fn test_display_state_static_mode() {
        let mut config = create_test_config();
        config.transition_mode = Some("static".to_string());
        config.static_temp = Some(5000);
        config.static_gamma = Some(85.0);

        let current_state = Period::Static;

        let display_state = DisplayState::new(current_state, 5000, 85.0, &config, None);

        assert!(!display_state.is_transitioning);
        assert_eq!(display_state.current_temp, 5000);
        assert_eq!(display_state.current_gamma, 85.0);
        assert!(display_state.next_transition.is_none()); // No transitions in static mode
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
        assert!(json_str.contains("\"time_state\":\"night\""));
        assert!(json_str.contains("\"current_temp\":3300"));
        assert!(json_str.contains("\"current_gamma\":90"));
    }
}
