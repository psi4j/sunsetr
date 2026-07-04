//! Runtime display state tracking for IPC and external integrations.
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::core::period::{Period, PeriodType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayState {
    pub active_preset: String,
    pub period: Period,
    #[serde(rename = "state")]
    pub period_type: PeriodType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<f32>,
    pub current_temp: u32,
    pub current_gamma: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_temp: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_gamma: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_period: Option<DateTime<Local>>,
}

impl DisplayState {
    pub fn new(runtime_state: &crate::core::runtime_state::RuntimeState) -> Self {
        let current_state = runtime_state.period();
        let config = runtime_state.config();

        let (target_temp, target_gamma) = if current_state.is_transitioning() {
            match current_state {
                Period::Sunset => (Some(config.night_temp), Some(config.night_gamma)),
                Period::Sunrise => (Some(config.day_temp), Some(config.day_gamma)),
                _ => (None, None),
            }
        } else {
            (None, None)
        };

        let next_period = runtime_state.next_period_start();

        let progress = if current_state.is_transitioning() {
            runtime_state.progress()
        } else {
            None
        };

        let active_preset = crate::state::preset::get_active_preset()
            .ok()
            .flatten()
            .unwrap_or_else(|| "default".to_string());

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

    pub fn update(&mut self, runtime_state: &crate::core::runtime_state::RuntimeState) {
        *self = Self::new(runtime_state);
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, TransitionMode};
    use crate::core::period::Period;
    use chrono::TimeZone;

    fn create_test_config() -> Config {
        Config {
            backend: crate::config::Backend::Auto,
            smoothing: false,
            startup_duration: 10.0,
            shutdown_duration: 10.0,
            adaptive_interval: crate::common::constants::DEFAULT_ADAPTIVE_INTERVAL_MS,
            latitude: None,
            longitude: None,
            sunset: Some("19:00:00".to_string()),
            sunrise: Some("06:00:00".to_string()),
            night_temp: 3300,
            day_temp: 6500,
            night_gamma: 90.0,
            day_gamma: 100.0,
            static_temp: None,
            static_gamma: None,
            transition_duration: 30,
            update_interval: crate::config::UpdateInterval::Fixed(60),
            transition_mode: TransitionMode::FinishBy,
        }
    }

    #[test]
    fn test_display_state_creation() {
        let config = create_test_config();
        let current_state = Period::Day;

        let runtime_state = crate::core::runtime_state::RuntimeState::new(
            current_state,
            &config,
            crate::core::schedule::Schedule::from_config(&config, None),
            crate::time::source::now(),
        );

        let display_state = DisplayState::new(&runtime_state);

        assert!(!display_state.period.is_transitioning());
        assert_eq!(display_state.current_temp, 6500);
        assert_eq!(display_state.current_gamma, 100.0);
        assert!(display_state.next_period.is_some());
        assert!(display_state.target_temp.is_none());
        assert!(display_state.target_gamma.is_none());
        assert!(display_state.progress.is_none());
    }

    #[test]
    fn test_display_state_transitioning() {
        let config = create_test_config();
        let current_state = Period::Sunset;

        let test_time = Local
            .from_local_datetime(
                &crate::time::source::now()
                    .date_naive()
                    .and_hms_opt(18, 45, 0)
                    .unwrap(),
            )
            .single()
            .unwrap();

        let runtime_state = crate::core::runtime_state::RuntimeState::new(
            current_state,
            &config,
            crate::core::schedule::Schedule::from_config(&config, None),
            test_time,
        );

        let display_state = DisplayState::new(&runtime_state);

        let (expected_temp, expected_gamma) = runtime_state.values();

        assert!(display_state.period.is_transitioning());
        assert_eq!(display_state.period, Period::Sunset);
        assert_eq!(display_state.current_temp, expected_temp);
        assert_eq!(display_state.current_gamma, expected_gamma);
        assert_eq!(display_state.target_temp, Some(3300));
        assert_eq!(display_state.target_gamma, Some(90.0));
        assert!(display_state.progress.is_some());
    }

    #[test]
    fn test_display_state_static_mode() {
        let mut config = create_test_config();
        config.transition_mode = TransitionMode::Static;
        config.static_temp = Some(5000);
        config.static_gamma = Some(85.0);

        let current_state = Period::Static;

        let runtime_state = crate::core::runtime_state::RuntimeState::new(
            current_state,
            &config,
            crate::core::schedule::Schedule::from_config(&config, None),
            crate::time::source::now(),
        );

        let display_state = DisplayState::new(&runtime_state);

        assert!(!display_state.period.is_transitioning());
        assert_eq!(display_state.current_temp, 5000);
        assert_eq!(display_state.current_gamma, 85.0);
        assert!(display_state.target_temp.is_none());
        assert!(display_state.target_gamma.is_none());
        assert!(display_state.progress.is_none());
        assert!(display_state.next_period.is_none());
    }

    #[test]
    fn test_display_state_serialization() {
        let config = create_test_config();
        let current_state = Period::Night;

        let runtime_state = crate::core::runtime_state::RuntimeState::new(
            current_state,
            &config,
            crate::core::schedule::Schedule::from_config(&config, None),
            crate::time::source::now(),
        );

        let display_state = DisplayState::new(&runtime_state);

        let json = display_state.to_json();
        assert!(json.is_ok());

        let json_str = json.unwrap();
        assert!(json_str.contains("\"period\":\"night\""));
        assert!(json_str.contains("\"current_temp\":3300"));
        assert!(json_str.contains("\"current_gamma\":90"));
        assert!(!json_str.contains("\"target_temp\""));
        assert!(!json_str.contains("\"target_gamma\""));
        assert!(!json_str.contains("\"progress\""));
    }
}
