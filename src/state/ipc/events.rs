//! Event data structures for the IPC system.

use crate::core::period::Period;
use crate::state::display::DisplayState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum IpcEvent {
    /// Emitted whenever the display state changes, for any reason.
    StateApplied {
        #[serde(flatten)]
        state: DisplayState,
    },

    /// Emitted only for automatic time-based period transitions.
    PeriodChanged {
        from_period: Period,
        to_period: Period,
    },

    /// Emitted when the active preset changes, carrying the target values
    /// before any smooth transition completes.
    PresetChanged {
        from_preset: Option<String>,
        to_preset: Option<String>,
        target_period: Period,
        target_temp: u32,
        target_gamma: f64,
    },

    /// Emitted when config values change, carrying the target values before any
    /// smooth transition completes.
    ConfigChanged {
        target_period: Period,
        target_temp: u32,
        target_gamma: f64,
    },
}

impl IpcEvent {
    pub fn state_applied(state: DisplayState) -> Self {
        IpcEvent::StateApplied { state }
    }

    pub fn period_changed(from: Period, to: Period) -> Self {
        IpcEvent::PeriodChanged {
            from_period: from,
            to_period: to,
        }
    }

    pub fn preset_changed(
        from: Option<String>,
        to: Option<String>,
        target_period: Period,
        target_temp: u32,
        target_gamma: f64,
    ) -> Self {
        IpcEvent::PresetChanged {
            from_preset: from,
            to_preset: to,
            target_period,
            target_temp,
            target_gamma,
        }
    }

    pub fn config_changed(target_period: Period, target_temp: u32, target_gamma: f64) -> Self {
        IpcEvent::ConfigChanged {
            target_period,
            target_temp,
            target_gamma,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization() {
        let state = DisplayState {
            active_preset: "evening".to_string(),
            period: Period::Sunset,
            period_type: Period::Sunset.period_type(),
            progress: Some(0.5),
            current_temp: 4500,
            current_gamma: 95.0,
            target_temp: Some(3300),
            target_gamma: Some(90.0),
            next_period: None,
        };

        let event = IpcEvent::state_applied(state);
        let json = serde_json::to_string(&event).unwrap();

        assert!(json.contains("\"event_type\":\"state_applied\""));
        assert!(json.contains("\"period\":\"sunset\""));
        assert!(json.contains("\"current_temp\":4500"));

        let deserialized: IpcEvent = serde_json::from_str(&json).unwrap();
        match deserialized {
            IpcEvent::StateApplied { state } => {
                assert_eq!(state.active_preset, "evening");
                assert_eq!(state.current_temp, 4500);
            }
            _ => panic!("Wrong event type deserialized"),
        }
    }

    #[test]
    fn test_period_changed_serialization() {
        let event = IpcEvent::period_changed(Period::Day, Period::Sunset);
        let json = serde_json::to_string(&event).unwrap();

        assert!(json.contains("\"event_type\":\"period_changed\""));
        assert!(json.contains("\"from_period\":\"day\""));
        assert!(json.contains("\"to_period\":\"sunset\""));
    }

    #[test]
    fn test_preset_changed_serialization() {
        let event = IpcEvent::preset_changed(
            Some("daytime".to_string()),
            Some("evening".to_string()),
            Period::Static,
            3300,
            90.0,
        );
        let json = serde_json::to_string(&event).unwrap();

        assert!(json.contains("\"event_type\":\"preset_changed\""));
        assert!(json.contains("\"from_preset\":\"daytime\""));
        assert!(json.contains("\"to_preset\":\"evening\""));
        assert!(json.contains("\"target_period\":\"static\""));
        assert!(json.contains("\"target_temp\":3300"));
        assert!(json.contains("\"target_gamma\":90"));
    }

    #[test]
    fn test_config_changed_serialization() {
        let event = IpcEvent::config_changed(Period::Night, 3500, 92.5);
        let json = serde_json::to_string(&event).unwrap();

        assert!(json.contains("\"event_type\":\"config_changed\""));
        assert!(json.contains("\"target_period\":\"night\""));
        assert!(json.contains("\"target_temp\":3500"));
        assert!(json.contains("\"target_gamma\":92.5"));
    }
}
