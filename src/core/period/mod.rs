//! State calculation for time-based and static periods.
//!
//! This module handles the core logic for determining when transitions should or should
//! not occur, calculating smooth interpolation values for transition periods, deciding when
//! application state updates are needed, and providing standardized state messaging. It
//! supports different transition modes and handles edge cases like midnight crossings and
//! extreme day/night periods.
//!
//! ## Key Functionality
//! - **Period Detection**: Determining current time-based or static period
//! - **Transition Calculation**: Computing smooth interpolation between day/night values  
//! - **Update Logic**: Deciding when backend state changes should be applied
//! - **Standardized Messaging**: Providing consistent period announcement messages
//! - **Time Handling**: Managing complex timing scenarios including midnight crossings

pub mod calculations;
pub mod state_detection;

#[cfg(test)]
mod tests;

pub use state_detection::{StateChange, log_state_announcement, should_update_state};

use chrono::{NaiveTime, Timelike};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration as StdDuration;

use crate::config::Config;
use crate::geo::times::GeoTimes;

/// Represents the time-based or static state of the application used for color temperature and
/// gamma interpolation. `Sunset` and `Sunrise` are treated as distinct transition periods rather than
/// single-instance astronomical events (Think "period during which the Sun rises or sets").
#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Period {
    Day,
    Night,
    Sunset,
    Sunrise,
    Static,
}

impl fmt::Display for Period {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Period::Day => write!(f, "Day"),
            Period::Night => write!(f, "Night"),
            Period::Sunset => write!(f, "Sunset"),
            Period::Sunrise => write!(f, "Sunrise"),
            Period::Static => write!(f, "Static"),
        }
    }
}

impl Period {
    /// Returns true if this is a stable period (Day or Night).
    pub fn is_stable(&self) -> bool {
        matches!(self, Self::Day | Self::Night)
    }

    /// Returns true if this is a transitioning period (Sunset or Sunrise).
    pub fn is_transitioning(&self) -> bool {
        matches!(self, Self::Sunset | Self::Sunrise)
    }

    /// Returns true if this period changes based on time of day
    pub fn is_time_based(&self) -> bool {
        matches!(self, Self::Day | Self::Night | Self::Sunset | Self::Sunrise)
    }

    /// Returns true if this period is static (no time-based changes)
    pub fn is_static(&self) -> bool {
        matches!(self, Self::Static)
    }

    /// Returns the period type for presentation purposes
    pub fn period_type(&self) -> PeriodType {
        match self {
            Self::Day | Self::Night => PeriodType::Stable,
            Self::Sunset | Self::Sunrise => PeriodType::Transitioning,
            Self::Static => PeriodType::Static,
        }
    }

    /// Returns the display name for this period (without icon).
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Day => "Day",
            Self::Night => "Night",
            Self::Sunset => "Sunset",
            Self::Sunrise => "Sunrise",
            Self::Static => "Static",
        }
    }

    /// Returns the icon/symbol for this period.
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Day => "󰖨 ",
            Self::Night => " ",
            Self::Sunset => "󰖛 ",
            Self::Sunrise => "󰖜 ",
            Self::Static => "󰋙 ",
        }
    }

    /// Returns the next period in the cycle.
    pub fn next_period(&self) -> Self {
        match self {
            Self::Day => Self::Sunset,
            Self::Sunset => Self::Night,
            Self::Night => Self::Sunrise,
            Self::Sunrise => Self::Day,
            Self::Static => Self::Static,
        }
    }
}

/// Determine the active stable period
///
/// This function handles the logic for determining whether we're in day or night
/// mode when not actively transitioning. It must handle edge cases like:
/// - Normal day/night cycles
/// - Midnight crossings
/// - Extreme schedules (very short days or nights)
///
/// # Arguments
/// * `now` - Current time to evaluate
/// * `sunset_end` - When sunset transition completes (night mode begins)
/// * `sunrise_start` - When sunrise transition begins (night mode ends)
///
/// # Returns
/// Period::Day or Period::Night
pub(crate) fn get_stable_period(
    now: NaiveTime,
    sunset_end: NaiveTime,
    sunrise_start: NaiveTime,
) -> Period {
    let now_secs = now.hour() * 3600 + now.minute() * 60 + now.second();
    let sunset_end_secs = sunset_end.hour() * 3600 + sunset_end.minute() * 60 + sunset_end.second();
    let sunrise_start_secs =
        sunrise_start.hour() * 3600 + sunrise_start.minute() * 60 + sunrise_start.second();

    if sunset_end_secs < sunrise_start_secs {
        if now_secs >= sunset_end_secs && now_secs < sunrise_start_secs {
            Period::Night
        } else {
            Period::Day
        }
    } else if now_secs >= sunset_end_secs || now_secs < sunrise_start_secs {
        Period::Night
    } else {
        Period::Day
    }
}

/// Sleep duration for the main loop: the update-interval tick while
/// transitioning, the time until the next transition while stable, and
/// `Duration::MAX` in static mode.
pub fn time_until_next_event(config: &Config, geo_times: Option<&GeoTimes>) -> StdDuration {
    let Some(schedule) = crate::core::schedule::Schedule::from_config(config, geo_times.cloned())
    else {
        return StdDuration::MAX;
    };
    let now = crate::time::source::now();
    let period = schedule.current_period(now);
    schedule.time_until_next_event(config, period, now)
}

/// Period type enum for presentation layer categorization
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeriodType {
    /// Stable time-based periods - Day, Night
    Stable,

    /// Transitioning time-based periods - Sunset, Sunrise
    Transitioning,

    /// Static periods - Static (no time-based changes)
    Static,
}

impl fmt::Display for PeriodType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PeriodType::Stable => write!(f, "stable"),
            PeriodType::Transitioning => write!(f, "transitioning"),
            PeriodType::Static => write!(f, "static"),
        }
    }
}
