//! State change detection and logging for period transitions.
//!
//! This module provides functionality for detecting when the application state
//! should be updated and provides standardized logging for state transitions.

use crate::core::period::Period;

/// Represents the type of state change that occurred.
#[derive(Debug, PartialEq)]
pub enum StateChange {
    /// No change occurred
    None,
    /// Started a new transitioning period from stable period
    TransitionStarted,
    /// Completed a transitioning period and entering stable period
    TransitionCompleted { from: Period },
    /// Progress update during ongoing transitioning period
    TransitionProgress,
    /// Direct jump between stable periods (should not happen in normal operation)
    UnexpectedStableJump { from: Period, to: Period },
}

/// Determine the type of state change and whether the application state should be updated.
///
/// This function detects what type of state change occurred and logs
/// appropriate messages.
///
/// # Arguments
/// * `current_period` - The last known period
/// * `new_period` - The newly calculated period
///
/// # Returns
/// `StateChange` indicating the type of change that occurred
pub fn should_update_state(current_period: &Period, new_period: &Period) -> StateChange {
    // Detect what type of state change occurred
    let change = detect_state_change(current_period, new_period);

    // Log the appropriate message for the change
    log_state_change(&change, new_period);

    // Return the change type directly
    change
}

/// Detect what type of state change occurred between two states.
///
/// This function only detects the change type without logging. Use `should_update_state()`
/// when you want both detection and logging.
pub fn detect_state_change(current_period: &Period, new_period: &Period) -> StateChange {
    match (current_period, new_period) {
        // No change - handle Static mode which never changes
        (Period::Static, Period::Static) => {
            // In static mode, state never changes unless config was reloaded
            // with different static values (handled by config reload logic)
            StateChange::None
        }

        // No change for other modes
        (current, new) if std::mem::discriminant(current) == std::mem::discriminant(new) => {
            // Check if we're in a transitioning period that needs progress updates
            if current.is_transitioning() {
                StateChange::TransitionProgress
            } else {
                StateChange::None
            }
        }

        // Transitions from other modes to static mode
        (_, Period::Static) => StateChange::TransitionStarted,

        // Transitions from static mode to other modes
        (Period::Static, _) => StateChange::TransitionStarted,

        // Normal flow: Time-based stable -> transitioning
        (Period::Day, Period::Sunset) | (Period::Night, Period::Sunrise) => {
            StateChange::TransitionStarted
        }

        // Normal flow: transitioning -> time-based stable
        (from @ Period::Sunset, Period::Night) | (from @ Period::Sunrise, Period::Day) => {
            StateChange::TransitionCompleted { from: *from }
        }

        // Unexpected: Direct stable-to-stable jump
        // This should not happen in normal operation
        (from @ (Period::Day | Period::Night), to @ (Period::Day | Period::Night)) => {
            StateChange::UnexpectedStableJump {
                from: *from,
                to: *to,
            }
        }

        // Any other unexpected transitions
        _ => {
            // This would be transitions like Sunset->Sunrise or vice versa
            // Log as unexpected jump
            StateChange::UnexpectedStableJump {
                from: *current_period,
                to: *new_period,
            }
        }
    }
}

/// Log the appropriate message for a state change.
fn log_state_change(change: &StateChange, new_period: &Period) {
    match change {
        StateChange::None => {
            // No logging needed
        }
        StateChange::TransitionStarted => {
            log_block_start!(
                "Commencing {} {}",
                new_period.display_name(),
                new_period.symbol()
            );
        }
        StateChange::TransitionCompleted { from } => {
            // Log completion
            log_decorated!("Transition 100% complete");

            // Log what we completed
            log_block_start!(
                "Completed {} {}",
                from.display_name().to_lowercase(),
                from.symbol()
            );

            // Announce the new stable state
            match new_period {
                Period::Day => log_block_start!(
                    "Entering {} mode {}",
                    new_period.display_name().to_lowercase(),
                    new_period.symbol()
                ),
                Period::Night => log_block_start!(
                    "Entering {} mode {}",
                    new_period.display_name().to_lowercase(),
                    new_period.symbol()
                ),
                _ => unreachable!("TransitionCompleted should lead to stable state"),
            }
        }
        StateChange::TransitionProgress => {
            // Progress updates are logged elsewhere in the main loop
        }
        StateChange::UnexpectedStableJump { from, to } => {
            log_pipe!();
            log_warning!("Unexpected state jump from {:?} to {:?}", from, to);
            log_indented!("This may indicate a system clock change or time anomaly");

            // Still announce where we ended up
            match to {
                Period::Day | Period::Night => {
                    log_block_start!(
                        "Entering {} mode {}",
                        to.display_name().to_lowercase(),
                        to.symbol()
                    );
                }
                _ => {}
            }
        }
    }
}

/// Log announcement when entering a new period
///
/// This function centralizes the state announcement when entering a new period
///
/// # Arguments
/// * `state` - The transition state to announce
pub fn log_state_announcement(state: Period) {
    match state {
        Period::Day | Period::Night | Period::Static => {
            log_block_start!(
                "Entering {} mode {}",
                state.display_name().to_lowercase(),
                state.symbol()
            );
        }
        Period::Sunset | Period::Sunrise => {
            log_block_start!("Commencing {} {}", state.display_name(), state.symbol());
        }
    }
}
