//! Event data structures for the IPC system.
//!
//! This module defines the typed events that can be broadcast through the IPC
//! system, providing semantic clarity for different types of state changes.

use crate::core::period::Period;
use crate::state::display::DisplayState;
use serde::{Deserialize, Serialize};

/// All possible IPC events that can be broadcast.
///
/// These events provide semantic information about different types of state
/// changes, enabling IPC clients to respond appropriately to each event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum IpcEvent {
    /// State has been applied to the display.
    ///
    /// This event is emitted whenever the display state changes for any reason:
    /// - Time-based period transitions
    /// - Configuration reloads
    /// - Preset changes
    /// - Manual adjustments
    StateApplied {
        /// The complete display state after the change
        #[serde(flatten)]
        state: DisplayState,
    },

    /// Period has changed due to time-based transition.
    ///
    /// This event is only emitted for automatic time-based transitions
    /// (Day → Sunset → Night → Sunrise → Day cycle).
    PeriodChanged {
        /// The period we're transitioning from
        from_period: Period,
        /// The period we're transitioning to
        to_period: Period,
    },

    /// Preset has been changed.
    ///
    /// This event is emitted when the active preset changes, providing
    /// immediate feedback about the target values even if a smooth
    /// transition will take time to complete.
    PresetChanged {
        /// Previous preset name (None if transitioning from default config)
        from_preset: Option<String>,
        /// New preset name (None if transitioning to default config)
        to_preset: Option<String>,
        /// Target temperature in Kelvin
        target_temp: u32,
        /// Target gamma as percentage
        target_gamma: f32,
    },
}

impl IpcEvent {
    /// Create a StateApplied event from a DisplayState.
    pub fn state_applied(state: DisplayState) -> Self {
        IpcEvent::StateApplied { state }
    }

    /// Create a PeriodChanged event.
    pub fn period_changed(from: Period, to: Period) -> Self {
        IpcEvent::PeriodChanged {
            from_period: from,
            to_period: to,
        }
    }

    /// Create a PresetChanged event.
    pub fn preset_changed(
        from: Option<String>,
        to: Option<String>,
        target_temp: u32,
        target_gamma: f32,
    ) -> Self {
        IpcEvent::PresetChanged {
            from_preset: from,
            to_preset: to,
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
        // Test StateApplied serialization
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

        // Test round-trip
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
            3300,
            90.0,
        );
        let json = serde_json::to_string(&event).unwrap();

        assert!(json.contains("\"event_type\":\"preset_changed\""));
        assert!(json.contains("\"from_preset\":\"daytime\""));
        assert!(json.contains("\"to_preset\":\"evening\""));
        assert!(json.contains("\"target_temp\":3300"));
        assert!(json.contains("\"target_gamma\":90"));
    }
}
