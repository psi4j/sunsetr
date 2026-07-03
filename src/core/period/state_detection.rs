//! Detect and log the kind of change between two periods.

use crate::core::period::Period;

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
    /// Direct jump between periods that bypasses the natural Day -> Sunset
    /// -> Night -> Sunrise progression.
    StableJump { from: Period, to: Period },
}

/// Classify the change between the two periods, logging it as a side effect.
pub fn should_update_state(current_period: &Period, new_period: &Period) -> StateChange {
    let change = detect_state_change(current_period, new_period);
    log_state_change(&change, new_period);
    change
}

/// Classify the change between the two periods without logging. Use
/// `should_update_state` when you also want the change logged.
pub fn detect_state_change(current_period: &Period, new_period: &Period) -> StateChange {
    match (current_period, new_period) {
        (Period::Static, Period::Static) => StateChange::None,

        (current, new) if std::mem::discriminant(current) == std::mem::discriminant(new) => {
            if current.is_transitioning() {
                StateChange::TransitionProgress
            } else {
                StateChange::None
            }
        }

        (_, Period::Static) => StateChange::TransitionStarted,
        (Period::Static, _) => StateChange::TransitionStarted,

        (Period::Day, Period::Sunset) | (Period::Night, Period::Sunrise) => {
            StateChange::TransitionStarted
        }

        (from @ Period::Sunset, Period::Night) | (from @ Period::Sunrise, Period::Day) => {
            StateChange::TransitionCompleted { from: *from }
        }

        (from @ (Period::Day | Period::Night), to @ (Period::Day | Period::Night)) => {
            StateChange::StableJump {
                from: *from,
                to: *to,
            }
        }

        _ => StateChange::StableJump {
            from: *current_period,
            to: *new_period,
        },
    }
}

fn log_state_change(change: &StateChange, new_period: &Period) {
    match change {
        StateChange::None => {}
        StateChange::TransitionStarted => {
            log_block_start!(
                "Commencing {} {}",
                new_period.display_name(),
                new_period.symbol()
            );
        }
        StateChange::TransitionCompleted { from } => {
            log_decorated!("Transition 100% complete");

            log_block_start!(
                "Completed {} {}",
                from.display_name().to_lowercase(),
                from.symbol()
            );

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
        StateChange::StableJump { to, .. } => match to {
            Period::Day | Period::Night => {
                log_block_start!(
                    "Entering {} mode {}",
                    to.display_name().to_lowercase(),
                    to.symbol()
                );
            }
            _ => {}
        },
    }
}

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
