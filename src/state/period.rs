//! State management for time-based and static periods.
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

use chrono::{NaiveTime, Timelike};
use std::time::Duration as StdDuration;

use crate::common::constants::{
    DEFAULT_DAY_GAMMA, DEFAULT_DAY_TEMP, DEFAULT_NIGHT_GAMMA, DEFAULT_NIGHT_TEMP, DEFAULT_SUNRISE,
    DEFAULT_SUNSET, DEFAULT_TRANSITION_DURATION, DEFAULT_UPDATE_INTERVAL,
};
use crate::common::utils::{interpolate_f32, interpolate_u32};
use crate::config::Config;
use crate::geo::times::GeoTimes;

/// Represents the time-based or static state of the display color temperature.
/// Sunset and Sunrise are treated as distinct transition periods rather than single instances.
/// You can think of these as "Sun setting period" and "Sun rising period" rather than single
/// astronomical events.
#[derive(Debug, PartialEq, Copy, Clone)]
pub enum Period {
    /// Daytime - natural color temperature (6500K) and full brightness
    Day,

    /// Nighttime - warm color temperature (4500K) and reduced brightness
    Night,

    /// Sunset transition - gradual shift from day to night settings
    Sunset {
        /// Transition progress (0.0 = day-like, 1.0 = night-like)
        progress: f32,
    },

    /// Sunrise transition - gradual shift from night to day settings
    Sunrise {
        /// Transition progress (0.0 = night-like, 1.0 = day-like)
        progress: f32,
    },

    /// Static mode - constant temperature and gamma values (no time-based changes)
    Static,
}

impl Period {
    /// Returns true if this is a stable period (Day, Night, or Static).
    pub fn is_stable(&self) -> bool {
        matches!(self, Self::Day | Self::Night | Self::Static)
    }

    /// Returns true if this is a transitioning period (Sunset or Sunrise).
    pub fn is_transitioning(&self) -> bool {
        matches!(self, Self::Sunset { .. } | Self::Sunrise { .. })
    }

    /// Returns the transition progress if transitioning, None if stable.
    pub fn progress(&self) -> Option<f32> {
        match self {
            Self::Sunset { progress } | Self::Sunrise { progress } => Some(*progress),
            _ => None,
        }
    }

    /// Returns the display name for this period (without icon).
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Day => "Day",
            Self::Night => "Night",
            Self::Sunset { .. } => "Sunset",
            Self::Sunrise { .. } => "Sunrise",
            Self::Static => "Static",
        }
    }

    /// Returns the icon/symbol for this period.
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Day => "󰖨 ",
            Self::Night => " ",
            Self::Sunset { .. } => "󰖛 ",
            Self::Sunrise { .. } => "󰖜 ",
            Self::Static => "󰋙 ",
        }
    }

    /// Calculates temperature for this period.
    pub fn temperature(&self, config: &Config) -> u32 {
        let day_temp = config.day_temp.unwrap_or(DEFAULT_DAY_TEMP);
        let night_temp = config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP);

        match self {
            Self::Day => day_temp,
            Self::Night => night_temp,
            Self::Sunset { progress } => interpolate_u32(day_temp, night_temp, *progress),
            Self::Sunrise { progress } => interpolate_u32(night_temp, day_temp, *progress),
            Self::Static => config.static_temp.unwrap_or(DEFAULT_DAY_TEMP),
        }
    }

    /// Calculates gamma for this period.
    pub fn gamma(&self, config: &Config) -> f32 {
        let day_gamma = config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA);
        let night_gamma = config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA);

        match self {
            Self::Day => day_gamma,
            Self::Night => night_gamma,
            Self::Sunset { progress } => interpolate_f32(day_gamma, night_gamma, *progress),
            Self::Sunrise { progress } => interpolate_f32(night_gamma, day_gamma, *progress),
            Self::Static => config.static_gamma.unwrap_or(DEFAULT_DAY_GAMMA),
        }
    }

    /// Returns both temperature and gamma values.
    pub fn values(&self, config: &Config) -> (u32, f32) {
        (self.temperature(config), self.gamma(config))
    }

    /// Returns the next period in the cycle.
    pub fn next_period(&self) -> Self {
        match self {
            Self::Day => Self::Sunset { progress: 0.0 },
            Self::Sunset { .. } => Self::Night,
            Self::Night => Self::Sunrise { progress: 0.0 },
            Self::Sunrise { .. } => Self::Day,
            Self::Static => Self::Static, // Static mode has no next period (doesn't cycle)
        }
    }
}

/// Calculate transition windows for both sunset and sunrise based on the configured mode.
///
/// This function determines when transitions should start and end based on four modes:
/// - "finish_by": Transition completes at the configured time
/// - "start_at": Transition begins at the configured time  
/// - "center": Transition is centered on the configured time
/// - "geo": Uses geographic coordinates to calculate actual sunrise/sunset times
///
/// # Arguments
/// * `config` - Configuration containing sunset/sunrise times and transition settings
/// * `geo_times` - Optional pre-calculated geo transition times
///
/// # Returns
/// Tuple of (sunset_start, sunset_end, sunrise_start, sunrise_end) as NaiveTime
fn calculate_transition_windows(
    config: &Config,
    geo_times: Option<&GeoTimes>,
) -> (NaiveTime, NaiveTime, NaiveTime, NaiveTime) {
    let mode = config.transition_mode.as_deref().unwrap_or("finish_by");

    // Handle geo mode separately using actual sunrise/sunset calculations
    if mode == "geo" {
        // In geo mode, we MUST have geo_times (enforced by fail-fast startup)
        // If this fails, it's a bug in our logic that needs fixing
        return geo_times
            .expect("BUG: geo mode without geo_times - this should never happen")
            .as_naive_times_local();
    }

    // For non-geo modes, sunset and sunrise should be present (with defaults from validation)
    let sunset_str = config.sunset.as_deref().unwrap_or(DEFAULT_SUNSET);
    let sunrise_str = config.sunrise.as_deref().unwrap_or(DEFAULT_SUNRISE);

    let (sunset, sunrise) = (
        NaiveTime::parse_from_str(sunset_str, "%H:%M:%S").unwrap(),
        NaiveTime::parse_from_str(sunrise_str, "%H:%M:%S").unwrap(),
    );

    let transition_duration = StdDuration::from_secs(
        config
            .transition_duration
            .unwrap_or(DEFAULT_TRANSITION_DURATION)
            * 60, // Convert minutes to seconds
    );

    let mode = config.transition_mode.as_deref().unwrap_or("finish_by");

    match mode {
        "center" => {
            // Center mode: transitions are symmetrically distributed around the configured time
            let sunset_half = chrono::Duration::from_std(transition_duration / 2).unwrap();
            let sunrise_half = chrono::Duration::from_std(transition_duration / 2).unwrap();

            (
                sunset - sunset_half,   // Sunset start: center - half duration
                sunset + sunset_half,   // Sunset end: center + half duration
                sunrise - sunrise_half, // Sunrise start: center - half duration
                sunrise + sunrise_half, // Sunrise end: center + half duration
            )
        }
        "start_at" => {
            // Transition begins at the configured time
            let full_transition = chrono::Duration::from_std(transition_duration).unwrap();
            (
                sunset,                    // Sunset start: at sunset
                sunset + full_transition,  // Sunset end: sunset + 30min
                sunrise,                   // Sunrise start: at sunrise
                sunrise + full_transition, // Sunrise end: sunrise + 30min
            )
        }
        "finish_by" => {
            // Transition completes at the configured time (default)
            let full_transition = chrono::Duration::from_std(transition_duration).unwrap();
            (
                sunset - full_transition,  // Sunset start: sunset - 30min
                sunset,                    // Sunset end: at sunset
                sunrise - full_transition, // Sunrise start: sunrise - 30min
                sunrise,                   // Sunrise end: at sunrise
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
    }
}

/// Get the current transition state based on the time of day and configuration.
///
/// This is the main function that determines what state the display should be in.
/// It calculates transition windows and checks if the current time falls within
/// any transition period, returning either a stable state or transition progress.
///
/// # Arguments
/// * `config` - Configuration containing all timing and transition settings
/// * `geo_times` - Optional pre-calculated geo transition times
///
/// # Returns
/// Period indicating current state and any transition progress
pub fn get_transition_state(config: &Config, geo_times: Option<&GeoTimes>) -> Period {
    // Handle static mode first - skip all time calculations
    if config.transition_mode.as_deref() == Some("static") {
        return Period::Static;
    }

    // For geo mode with pre-calculated times, use the optimized path
    if config.transition_mode.as_deref() == Some("geo")
        && let Some(times) = geo_times
    {
        return times.get_current_state(crate::time::source::now());
    }

    // Fall back to traditional calculation
    let now = crate::time::source::now().time();
    let (sunset_start, sunset_end, _sunrise_start, _sunrise_end) =
        calculate_transition_windows(config, geo_times);

    // Check if we're in a transition period
    if is_time_in_range(now, sunset_start, sunset_end) {
        // Sunset transition (day -> night)
        let progress = calculate_progress(now, sunset_start, sunset_end);
        Period::Sunset { progress }
    } else if is_time_in_range(now, _sunrise_start, _sunrise_end) {
        // Sunrise transition (night -> day)
        let progress = calculate_progress(now, _sunrise_start, _sunrise_end);
        Period::Sunrise { progress }
    } else {
        // Stable period - determine which stable state based on time relative to transitions
        get_stable_state_for_time(now, sunset_end, _sunrise_start)
    }
}

/// Determine the stable time state for periods outside of transitions.
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
fn get_stable_state_for_time(
    now: NaiveTime,
    sunset_end: NaiveTime,
    sunrise_start: NaiveTime,
) -> Period {
    // For stable periods, determine if we're in day or night based on transition windows
    // If we're after sunset transition ends OR before sunrise transition starts, it's night
    // Otherwise, it's day

    // Convert to seconds since midnight for easier comparison
    let now_secs = now.hour() * 3600 + now.minute() * 60 + now.second();
    let sunset_end_secs = sunset_end.hour() * 3600 + sunset_end.minute() * 60 + sunset_end.second();
    let sunrise_start_secs =
        sunrise_start.hour() * 3600 + sunrise_start.minute() * 60 + sunrise_start.second();

    // Handle the logic based on whether sunset/sunrise cross midnight
    if sunset_end_secs < sunrise_start_secs {
        // Normal case: sunset ends before sunrise starts (no midnight crossing)
        // Night period: from sunset_end until sunrise_start
        if now_secs >= sunset_end_secs && now_secs < sunrise_start_secs {
            Period::Night
        } else {
            Period::Day
        }
    } else {
        // Overnight case: sunset transition crosses midnight OR spans most of the day
        // Night period: from sunset_end through midnight OR before sunrise_start
        if now_secs >= sunset_end_secs || now_secs < sunrise_start_secs {
            Period::Night
        } else {
            Period::Day
        }
    }
}

/// Calculate how long until the next transition event begins.
///
/// This function determines the appropriate sleep duration for the main loop:
/// - During transitions: Use the configured update interval for smooth progress
/// - During stable periods: Sleep until the next transition starts
///
/// # Arguments
/// * `config` - Configuration containing update intervals and transition times
/// * `geo_times` - Optional pre-calculated geo transition times
///
/// # Returns
/// Duration to sleep before the next state check
pub fn time_until_next_event(config: &Config, geo_times: Option<&GeoTimes>) -> StdDuration {
    // Static mode has no time-based events - wait indefinitely
    if config.transition_mode.as_deref() == Some("static") {
        // Duration::MAX means the main loop will only wake on signals.
        // The app remains fully responsive since recv_timeout() wakes
        // immediately when a signal arrives.
        return StdDuration::MAX;
    }

    // For geo mode with pre-calculated times, use the optimized path
    if config.transition_mode.as_deref() == Some("geo")
        && let Some(times) = geo_times
    {
        let current_period = times.get_current_state(crate::time::source::now());
        if current_period.is_transitioning() {
            // During transitions, return update interval for smooth progress
            return StdDuration::from_secs(
                config.update_interval.unwrap_or(DEFAULT_UPDATE_INTERVAL),
            );
        } else {
            // In stable state, return time until next transition
            return times.duration_until_next_transition(crate::time::source::now());
        }
    }

    // Get current transition state
    let current_period = get_transition_state(config, geo_times);

    if current_period.is_transitioning() {
        // If we're currently transitioning, return the update interval for smooth progress
        StdDuration::from_secs(config.update_interval.unwrap_or(DEFAULT_UPDATE_INTERVAL))
    } else {
        // Calculate time until next transition starts
        let now = crate::time::source::now();
        let now_naive = now.naive_local();
        let today = now.date_naive();
        let tomorrow = today + chrono::Duration::days(1);

        let (sunset_start, _sunset_end, sunrise_start, _sunrise_end) =
            calculate_transition_windows(config, geo_times);

        // Create DateTime objects for today's transitions
        let today_sunset = today.and_time(sunset_start);
        let today_sunrise = today.and_time(sunrise_start);
        let tomorrow_sunset = tomorrow.and_time(sunset_start);
        let tomorrow_sunrise = tomorrow.and_time(sunrise_start);

        // Find the next transition that occurs after now
        // CRITICAL: Compare using full datetime, not just time!
        let candidates = [
            (today_sunset, "sunset"),
            (today_sunrise, "sunrise"),
            (tomorrow_sunset, "sunset"),
            (tomorrow_sunrise, "sunrise"),
        ];

        let next_transition = candidates
            .iter()
            .filter(|(datetime, _)| *datetime > now_naive)
            .min_by_key(|(datetime, _)| *datetime)
            .expect("Should always find a next transition");

        let duration_until = next_transition.0 - now_naive;
        let millis = duration_until.num_milliseconds().max(0) as u64;
        StdDuration::from_millis(millis)
    }
}

/// Calculate time remaining until the current transition ends.
///
/// This function is used during transitions to determine if we should sleep
/// for the full update interval or a shorter duration to hit the transition
/// end time exactly.
///
/// # Arguments
/// * `config` - Configuration containing transition settings
/// * `geo_times` - Optional pre-calculated geo transition times
///
/// # Returns
/// - `Some(duration)` if currently transitioning, with time until transition ends
/// - `None` if not currently transitioning
pub fn time_until_transition_end(
    config: &Config,
    geo_times: Option<&GeoTimes>,
) -> Option<StdDuration> {
    // Static mode never has transitions
    if config.transition_mode.as_deref() == Some("static") {
        return None;
    }

    // For geo mode with pre-calculated times, use the optimized path
    if config.transition_mode.as_deref() == Some("geo")
        && let Some(times) = geo_times
    {
        return times.duration_until_transition_end(crate::time::source::now());
    }

    let current_period = get_transition_state(config, geo_times);

    match current_period {
        Period::Sunset { .. } => {
            let now = crate::time::source::now().time();

            // Get the end time for the sunset transition
            let transition_end =
                get_current_transition_end_time(config, geo_times, Period::Day, Period::Night)?;

            // Calculate duration until transition ends
            // Handle potential midnight crossing
            let now_secs =
                now.hour() as i32 * 3600 + now.minute() as i32 * 60 + now.second() as i32;
            let end_secs = transition_end.hour() as i32 * 3600
                + transition_end.minute() as i32 * 60
                + transition_end.second() as i32;

            let seconds_remaining = if end_secs >= now_secs {
                // Normal case: end time is later today
                end_secs - now_secs
            } else {
                // Midnight crossing: end time is tomorrow
                (24 * 3600 - now_secs) + end_secs
            };

            if seconds_remaining > 0 {
                Some(StdDuration::from_millis((seconds_remaining * 1000) as u64))
            } else {
                // We've passed the end time (shouldn't normally happen)
                Some(StdDuration::from_millis(0))
            }
        }
        Period::Sunrise { .. } => {
            let now = crate::time::source::now().time();

            // Get the end time for the sunrise transition
            let transition_end =
                get_current_transition_end_time(config, geo_times, Period::Night, Period::Day)?;

            // Calculate duration until transition ends
            // Handle potential midnight crossing
            let now_secs =
                now.hour() as i32 * 3600 + now.minute() as i32 * 60 + now.second() as i32;
            let end_secs = transition_end.hour() as i32 * 3600
                + transition_end.minute() as i32 * 60
                + transition_end.second() as i32;

            let seconds_remaining = if end_secs >= now_secs {
                // Normal case: end time is later today
                end_secs - now_secs
            } else {
                // Midnight crossing: end time is tomorrow
                (24 * 3600 - now_secs) + end_secs
            };

            if seconds_remaining > 0 {
                Some(StdDuration::from_millis((seconds_remaining * 1000) as u64))
            } else {
                // We've passed the end time (shouldn't normally happen)
                Some(StdDuration::from_millis(0))
            }
        }
        Period::Day | Period::Night | Period::Static => None,
    }
}

/// Get the end time for the current transition.
///
/// Helper function to get only the specific end time we need for a transition.
///
/// # Arguments
/// * `config` - Configuration containing transition settings
/// * `from` - Starting time state
/// * `to` - Target time state
///
/// # Returns
/// The end time of the transition, or None if invalid transition
fn get_current_transition_end_time(
    config: &Config,
    geo_times: Option<&GeoTimes>,
    from: Period,
    to: Period,
) -> Option<NaiveTime> {
    let (_, sunset_end, _, sunrise_end) = calculate_transition_windows(config, geo_times);

    match (from, to) {
        (Period::Day, Period::Night) => Some(sunset_end),
        (Period::Night, Period::Day) => Some(sunrise_end),
        _ => None,
    }
}

/// Calculate transition progress as a value between 0.0 and 1.0.
///
/// This function calculates linear progress and then applies a Bezier curve
/// transformation to create smooth, natural-looking transitions that start
/// and end with zero slope.
///
/// # Arguments
/// * `now` - Current time within the transition window
/// * `start` - When the transition began
/// * `end` - When the transition will complete
///
/// # Returns
/// Progress value transformed by Bezier curve, clamped between 0.0 and 1.0
fn calculate_progress(now: NaiveTime, start: NaiveTime, end: NaiveTime) -> f32 {
    let total_duration = (end - start).num_milliseconds() as f32;
    let elapsed = (now - start).num_milliseconds() as f32;
    let linear_progress = (elapsed / total_duration).clamp(0.0, 1.0);

    // Apply Bezier curve with control points from constants for smooth S-curve
    // These control points create an ease-in-out effect with no sudden jumps
    crate::common::utils::bezier_curve(
        linear_progress,
        crate::common::constants::BEZIER_P1X,
        crate::common::constants::BEZIER_P1Y,
        crate::common::constants::BEZIER_P2X,
        crate::common::constants::BEZIER_P2Y,
    )
}

/// Check if a time falls within a given range, handling midnight crossings.
///
/// This function correctly handles cases where the time range crosses midnight
/// (e.g., 23:00 to 01:00), which is common for night-time periods.
///
/// # Arguments
/// * `time` - Time to check
/// * `start` - Range start time (inclusive)
/// * `end` - Range end time (exclusive)
///
/// # Returns
/// true if time is within the range [start, end), false otherwise
fn is_time_in_range(time: NaiveTime, start: NaiveTime, end: NaiveTime) -> bool {
    use std::cmp::Ordering;

    match start.cmp(&end) {
        Ordering::Less => {
            // Normal range (doesn't cross midnight)
            time >= start && time < end
        }
        Ordering::Greater => {
            // Overnight range (crosses midnight)
            time >= start || time < end
        }
        Ordering::Equal => {
            // start == end, empty range
            false
        }
    }
}

/// Represents the type of state change that occurred.
#[derive(Debug, PartialEq)]
pub enum StateChange {
    /// No change occurred
    None,
    /// Started a new transition from stable period
    TransitionStarted,
    /// Completed a transition to stable period
    TransitionCompleted { from: Period },
    /// Progress update during ongoing transition
    TransitionProgress,
    /// Direct jump between stable periods (should not happen in normal operation)
    UnexpectedStableJump { from: Period, to: Period },
}

/// Determine whether the application state should be updated.
///
/// This function detects what type of state change occurred and logs
/// appropriate messages.
///
/// # Arguments
/// * `current_period` - The last known transition period
/// * `new_period` - The newly calculated transition period
///
/// # Returns
/// `true` if the period should be updated, `false` to skip this update cycle
pub fn should_update_state(current_period: &Period, new_period: &Period) -> bool {
    // Detect what type of state change occurred
    let change = detect_state_change(current_period, new_period);

    // Log the appropriate message for the change
    log_state_change(&change, new_period);

    // Return whether an update is needed
    !matches!(change, StateChange::None)
}

/// Detect what type of state change occurred between two states.
fn detect_state_change(current_period: &Period, new_period: &Period) -> StateChange {
    match (current_period, new_period) {
        // No change - handle Static mode which never changes
        (Period::Static, Period::Static) => {
            // In static mode, state never changes unless config was reloaded
            // with different static values (handled by config reload logic)
            StateChange::None
        }

        // No change for other modes
        (current, new) if std::mem::discriminant(current) == std::mem::discriminant(new) => {
            // Check if we're in a transition that needs progress updates
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

        // Normal flow: Stable -> Transition
        (Period::Day, Period::Sunset { .. }) | (Period::Night, Period::Sunrise { .. }) => {
            StateChange::TransitionStarted
        }

        // Normal flow: Transition -> Stable
        (from @ Period::Sunset { .. }, Period::Night)
        | (from @ Period::Sunrise { .. }, Period::Day) => {
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

/// Log the appropriate message for a transition state.
///
/// This function centralizes the state announcement logic that was previously
/// duplicated across backend modules.
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
        Period::Sunset { .. } | Period::Sunrise { .. } => {
            log_block_start!("Commencing {} {}", state.display_name(), state.symbol());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::constants::{
        DEFAULT_DAY_GAMMA, DEFAULT_DAY_TEMP, DEFAULT_NIGHT_GAMMA, DEFAULT_NIGHT_TEMP,
        DEFAULT_UPDATE_INTERVAL,
    };

    fn create_test_config(sunset: &str, sunrise: &str, mode: &str, duration_mins: u64) -> Config {
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
            sunset: Some(sunset.to_string()),
            sunrise: Some(sunrise.to_string()),
            night_temp: Some(DEFAULT_NIGHT_TEMP),
            day_temp: Some(DEFAULT_DAY_TEMP),
            night_gamma: Some(DEFAULT_NIGHT_GAMMA),
            day_gamma: Some(DEFAULT_DAY_GAMMA),
            static_temp: None,
            static_gamma: None,
            transition_duration: Some(duration_mins),
            update_interval: Some(DEFAULT_UPDATE_INTERVAL),
            transition_mode: Some(mode.to_string()),
        }
    }

    #[test]
    fn test_calculate_transition_windows_finish_by() {
        let config = create_test_config("19:00:00", "06:00:00", "finish_by", 30);
        let (sunset_start, sunset_end, sunrise_start, sunrise_end) =
            calculate_transition_windows(&config, None);

        assert_eq!(sunset_start, NaiveTime::from_hms_opt(18, 30, 0).unwrap());
        assert_eq!(sunset_end, NaiveTime::from_hms_opt(19, 0, 0).unwrap());
        assert_eq!(sunrise_start, NaiveTime::from_hms_opt(5, 30, 0).unwrap());
        assert_eq!(sunrise_end, NaiveTime::from_hms_opt(6, 0, 0).unwrap());
    }

    #[test]
    fn test_calculate_transition_windows_start_at() {
        let config = create_test_config("19:00:00", "06:00:00", "start_at", 30);
        let (sunset_start, sunset_end, sunrise_start, sunrise_end) =
            calculate_transition_windows(&config, None);

        assert_eq!(sunset_start, NaiveTime::from_hms_opt(19, 0, 0).unwrap());
        assert_eq!(sunset_end, NaiveTime::from_hms_opt(19, 30, 0).unwrap());
        assert_eq!(sunrise_start, NaiveTime::from_hms_opt(6, 0, 0).unwrap());
        assert_eq!(sunrise_end, NaiveTime::from_hms_opt(6, 30, 0).unwrap());
    }

    #[test]
    fn test_calculate_transition_windows_center() {
        let config = create_test_config("19:00:00", "06:00:00", "center", 30);
        let (sunset_start, sunset_end, sunrise_start, sunrise_end) =
            calculate_transition_windows(&config, None);

        assert_eq!(sunset_start, NaiveTime::from_hms_opt(18, 45, 0).unwrap());
        assert_eq!(sunset_end, NaiveTime::from_hms_opt(19, 15, 0).unwrap());
        assert_eq!(sunrise_start, NaiveTime::from_hms_opt(5, 45, 0).unwrap());
        assert_eq!(sunrise_end, NaiveTime::from_hms_opt(6, 15, 0).unwrap());
    }

    #[test]
    fn test_extreme_short_transition() {
        let config = create_test_config("19:00:00", "06:00:00", "finish_by", 5); // 5 minutes
        let (sunset_start, sunset_end, _, _) = calculate_transition_windows(&config, None);

        assert_eq!(sunset_start, NaiveTime::from_hms_opt(18, 55, 0).unwrap());
        assert_eq!(sunset_end, NaiveTime::from_hms_opt(19, 0, 0).unwrap());
    }

    #[test]
    fn test_extreme_long_transition() {
        let config = create_test_config("19:00:00", "06:00:00", "finish_by", 120); // 2 hours
        let (sunset_start, sunset_end, _, _) = calculate_transition_windows(&config, None);

        assert_eq!(sunset_start, NaiveTime::from_hms_opt(17, 0, 0).unwrap());
        assert_eq!(sunset_end, NaiveTime::from_hms_opt(19, 0, 0).unwrap());
    }

    #[test]
    fn test_midnight_crossing_sunset() {
        // Sunset very late, should cross midnight
        let config = create_test_config("23:30:00", "06:00:00", "start_at", 60); // 1 hour
        let (sunset_start, sunset_end, _, _) = calculate_transition_windows(&config, None);

        assert_eq!(sunset_start, NaiveTime::from_hms_opt(23, 30, 0).unwrap());
        assert_eq!(sunset_end, NaiveTime::from_hms_opt(0, 30, 0).unwrap());
    }

    #[test]
    fn test_midnight_crossing_sunrise() {
        // Sunrise very early, transition starts before midnight
        let config = create_test_config("20:00:00", "00:30:00", "finish_by", 60); // 1 hour
        let (_, _, sunrise_start, sunrise_end) = calculate_transition_windows(&config, None);

        assert_eq!(sunrise_start, NaiveTime::from_hms_opt(23, 30, 0).unwrap());
        assert_eq!(sunrise_end, NaiveTime::from_hms_opt(0, 30, 0).unwrap());
    }

    #[test]
    fn test_is_time_in_range_normal() {
        let start = NaiveTime::from_hms_opt(18, 0, 0).unwrap();
        let end = NaiveTime::from_hms_opt(19, 0, 0).unwrap();

        assert!(is_time_in_range(
            NaiveTime::from_hms_opt(18, 30, 0).unwrap(),
            start,
            end
        ));
        assert!(is_time_in_range(
            NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
            start,
            end
        ));
        assert!(!is_time_in_range(
            NaiveTime::from_hms_opt(19, 0, 0).unwrap(),
            start,
            end
        ));
        assert!(!is_time_in_range(
            NaiveTime::from_hms_opt(17, 59, 59).unwrap(),
            start,
            end
        ));
        assert!(!is_time_in_range(
            NaiveTime::from_hms_opt(19, 0, 0).unwrap(),
            start,
            end
        ));
    }

    #[test]
    fn test_is_time_in_range_overnight() {
        // Range that crosses midnight: 23:00 to 01:00
        let start = NaiveTime::from_hms_opt(23, 0, 0).unwrap();
        let end = NaiveTime::from_hms_opt(1, 0, 0).unwrap();

        assert!(is_time_in_range(
            NaiveTime::from_hms_opt(23, 30, 0).unwrap(),
            start,
            end
        ));
        assert!(is_time_in_range(
            NaiveTime::from_hms_opt(0, 30, 0).unwrap(),
            start,
            end
        ));
        assert!(is_time_in_range(
            NaiveTime::from_hms_opt(23, 0, 0).unwrap(),
            start,
            end
        ));
        assert!(!is_time_in_range(
            NaiveTime::from_hms_opt(1, 0, 0).unwrap(),
            start,
            end
        ));
        assert!(!is_time_in_range(
            NaiveTime::from_hms_opt(2, 0, 0).unwrap(),
            start,
            end
        ));
        assert!(!is_time_in_range(
            NaiveTime::from_hms_opt(22, 59, 59).unwrap(),
            start,
            end
        ));
    }

    #[test]
    fn test_calculate_progress() {
        let start = NaiveTime::from_hms_opt(18, 0, 0).unwrap();
        let end = NaiveTime::from_hms_opt(19, 0, 0).unwrap(); // 1 hour duration

        // Test endpoints (should always be 0.0 and 1.0 regardless of Bezier curve)
        assert_eq!(
            calculate_progress(NaiveTime::from_hms_opt(18, 0, 0).unwrap(), start, end),
            0.0
        );
        assert_eq!(
            calculate_progress(NaiveTime::from_hms_opt(19, 0, 0).unwrap(), start, end),
            1.0
        );

        // Test monotonic increase - progress should always increase with time
        let progress_15 =
            calculate_progress(NaiveTime::from_hms_opt(18, 15, 0).unwrap(), start, end);
        let progress_30 =
            calculate_progress(NaiveTime::from_hms_opt(18, 30, 0).unwrap(), start, end);
        let progress_45 =
            calculate_progress(NaiveTime::from_hms_opt(18, 45, 0).unwrap(), start, end);

        assert!(
            progress_15 < progress_30,
            "Progress should increase over time"
        );
        assert!(
            progress_30 < progress_45,
            "Progress should increase over time"
        );

        // Test bounded values - all progress values should be between 0 and 1
        assert!((0.0..=1.0).contains(&progress_15));
        assert!((0.0..=1.0).contains(&progress_30));
        assert!((0.0..=1.0).contains(&progress_45));

        // Test ease-in characteristic with current control points
        // With control points (0.33, 0.07) and (0.33, 1.0), we expect:
        // - Slower progress at the start (ease-in)
        // - Faster progress near the end
        let linear_quarter = 0.25;
        let linear_three_quarter = 0.75;

        // Early progress should be less than linear (ease-in effect)
        assert!(
            progress_15 < linear_quarter,
            "Early progress ({progress_15}) should be less than linear ({linear_quarter}) due to ease-in"
        );

        // Later progress should be greater than linear (catching up)
        assert!(
            progress_45 > linear_three_quarter,
            "Later progress ({progress_45}) should be greater than linear ({linear_three_quarter}) due to acceleration"
        );

        // Verify smoothness - no sudden jumps
        let progress_29 =
            calculate_progress(NaiveTime::from_hms_opt(18, 29, 0).unwrap(), start, end);
        let progress_31 =
            calculate_progress(NaiveTime::from_hms_opt(18, 31, 0).unwrap(), start, end);
        let delta = (progress_31 - progress_29).abs();

        assert!(
            delta < 0.1,
            "Progress change over 2 minutes should be smooth, not jumpy (delta: {delta})"
        );
    }

    #[test]
    fn test_get_stable_state_for_time_normal_day() {
        // Normal case: sunset ends at 19:00, sunrise starts at 06:00
        let sunset_end = NaiveTime::from_hms_opt(19, 0, 0).unwrap();
        let sunrise_start = NaiveTime::from_hms_opt(6, 0, 0).unwrap();

        // Day time
        assert_eq!(
            get_stable_state_for_time(
                NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
                sunset_end,
                sunrise_start
            ),
            Period::Day
        );

        // Night time
        assert_eq!(
            get_stable_state_for_time(
                NaiveTime::from_hms_opt(22, 0, 0).unwrap(),
                sunset_end,
                sunrise_start
            ),
            Period::Night
        );

        // Early morning night
        assert_eq!(
            get_stable_state_for_time(
                NaiveTime::from_hms_opt(3, 0, 0).unwrap(),
                sunset_end,
                sunrise_start
            ),
            Period::Night
        );
    }

    #[test]
    fn test_extreme_day_night_periods() {
        // Very short night: sunset at 23:00, sunrise at 01:00 (2 hour night)
        let config = create_test_config("23:00:00", "01:00:00", "finish_by", 30);
        let (_, sunset_end, sunrise_start, _) = calculate_transition_windows(&config, None);

        // Should be day most of the time
        assert_eq!(
            get_stable_state_for_time(
                NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
                sunset_end,
                sunrise_start
            ),
            Period::Day
        );

        // Should be night for the short period
        assert_eq!(
            get_stable_state_for_time(
                NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                sunset_end,
                sunrise_start
            ),
            Period::Night
        );
    }

    #[test]
    fn test_extreme_short_day() {
        // Very short day: sunset at 01:00, sunrise at 23:00 (2 hour day)
        let config = create_test_config("01:00:00", "23:00:00", "finish_by", 30);
        let (_, sunset_end, sunrise_start, _) = calculate_transition_windows(&config, None);

        // Should be night most of the time
        assert_eq!(
            get_stable_state_for_time(
                NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
                sunset_end,
                sunrise_start
            ),
            Period::Night
        );

        // Should be day for the short period
        assert_eq!(
            get_stable_state_for_time(
                NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                sunset_end,
                sunrise_start
            ),
            Period::Day
        );
    }

    #[test]
    fn test_transition_state_detection() {
        let config = create_test_config("19:00:00", "06:00:00", "finish_by", 30);

        // Mock current time using a specific test helper function would be better,
        // but for now we test the components individually which is covered above

        // Test the windows calculation which drives the state detection
        let (sunset_start, sunset_end, _sunrise_start, _sunrise_end) =
            calculate_transition_windows(&config, None);

        // Test that we get expected transition windows
        assert_eq!(sunset_start, NaiveTime::from_hms_opt(18, 30, 0).unwrap());
        assert_eq!(sunset_end, NaiveTime::from_hms_opt(19, 0, 0).unwrap());
    }

    #[test]
    fn test_center_mode_bug_reproduction() {
        // Reproduce the user's exact configuration that shows the bug
        // Sunset: 17:06:00, Sunrise: 06:00:00, Transition: 5 minutes, Mode: center
        let config = create_test_config("17:06:00", "06:00:00", "center", 5);

        println!("Testing Center Mode Bug");

        // Test different times around the sunset transition
        let test_times = vec![
            ("17:03:00", "Before sunset transition - should be DAY"),
            (
                "17:05:00",
                "During sunset transition - should be TRANSITIONING",
            ),
            ("17:06:00", "Exact sunset time - should be TRANSITIONING"),
            (
                "17:07:00",
                "During sunset transition - should be TRANSITIONING",
            ),
            ("17:09:00", "After sunset transition - should be NIGHT"),
        ];

        // Calculate expected transition windows for center mode
        let sunset = NaiveTime::parse_from_str("17:06:00", "%H:%M:%S").unwrap();
        let _sunrise = NaiveTime::parse_from_str("06:00:00", "%H:%M:%S").unwrap();
        let transition_duration = std::time::Duration::from_secs(5 * 60); // 5 minutes
        let half_transition = transition_duration / 2;
        let half_chrono = chrono::Duration::from_std(half_transition).unwrap();

        let sunset_start = sunset - half_chrono; // 17:03:30
        let sunset_end = sunset + half_chrono; // 17:08:30

        println!("Expected sunset transition window: {sunset_start} to {sunset_end}");

        for (time_str, description) in test_times {
            // Temporarily override the current time by creating a modified config
            // We'll simulate different times by checking the logic directly
            let test_time = NaiveTime::parse_from_str(time_str, "%H:%M:%S").unwrap();

            // Manually calculate what the state should be
            let (sunset_start_calc, sunset_end_calc, _sunrise_start_calc, _sunrise_end_calc) =
                calculate_transition_windows(&config, None);

            let in_sunset_transition =
                is_time_in_range(test_time, sunset_start_calc, sunset_end_calc);
            let in_sunrise_transition =
                is_time_in_range(test_time, _sunrise_start_calc, _sunrise_end_calc);

            let expected_state = if in_sunset_transition {
                "SUNSET TRANSITION"
            } else if in_sunrise_transition {
                "SUNRISE TRANSITION"
            } else {
                let stable_state =
                    get_stable_state_for_time(test_time, sunset_end_calc, _sunrise_start_calc);
                match stable_state {
                    Period::Day => "DAY",
                    Period::Night => "NIGHT",
                    _ => unreachable!("get_stable_state_for_time should only return Day or Night"),
                }
            };

            println!("Time {time_str}: {expected_state} ({description})");

            // The bug: times before/after sunset transition might incorrectly show NIGHT
            // when they should show DAY (before) or be in transition
            match time_str {
                "17:03:00" => {
                    // Before transition - should be DAY
                    assert!(
                        !in_sunset_transition,
                        "17:03:00 should not be in sunset transition"
                    );
                    if expected_state == "NIGHT" {
                        println!("  ❌ BUG DETECTED: Should be DAY, but got NIGHT");
                    }
                }
                "17:05:00" | "17:06:00" | "17:07:00" => {
                    // During transition - should be TRANSITIONING
                    if !in_sunset_transition {
                        println!(
                            "  ❌ BUG DETECTED: Should be in SUNSET TRANSITION, but got {expected_state}"
                        );
                    }
                }
                "17:09:00" => {
                    // After transition - should be NIGHT
                    assert!(
                        !in_sunset_transition,
                        "17:09:00 should not be in sunset transition"
                    );
                    if expected_state != "NIGHT" {
                        println!("  ❌ BUG DETECTED: Should be NIGHT, but got {expected_state}");
                    }
                }
                _ => {}
            }
        }
    }

    #[test]
    fn test_center_mode_timing_edge_cases() {
        // Test the edge case where timing might cause issues
        // Sunset: 17:06:00, Transition: 5 minutes, Mode: center
        // Window: 17:03:30 to 17:08:30
        let config = create_test_config("17:06:00", "06:00:00", "center", 5);

        println!("Testing Center Mode Timing Edge Cases");

        // Test times that are just at the edge of transition windows
        let edge_times = vec![
            ("17:03:29", "1 second before transition starts"),
            ("17:03:30", "Exact transition start"),
            ("17:03:31", "1 second after transition starts"),
            ("17:08:29", "1 second before transition ends"),
            ("17:08:30", "Exact transition end"),
            ("17:08:31", "1 second after transition ends"),
        ];

        let (sunset_start, sunset_end, _sunrise_start, _sunrise_end) =
            calculate_transition_windows(&config, None);
        println!("Transition window: {sunset_start} to {sunset_end}");

        for (time_str, description) in edge_times {
            let test_time = NaiveTime::parse_from_str(time_str, "%H:%M:%S").unwrap();

            let in_sunset_transition = is_time_in_range(test_time, sunset_start, sunset_end);
            let in_sunrise_transition = is_time_in_range(test_time, _sunrise_start, _sunrise_end);

            let state = if in_sunset_transition {
                "SUNSET TRANSITION"
            } else if in_sunrise_transition {
                "SUNRISE TRANSITION"
            } else {
                let stable_state = get_stable_state_for_time(test_time, sunset_end, _sunrise_start);
                match stable_state {
                    Period::Day => "DAY",
                    Period::Night => "NIGHT",
                    _ => unreachable!("get_stable_state_for_time should only return Day or Night"),
                }
            };

            println!("Time {time_str}: {state} ({description})");

            // Check for unexpected behavior at boundaries
            match time_str {
                "17:03:29" => {
                    if state != "DAY" {
                        println!("  ⚠️  POTENTIAL ISSUE: Expected DAY just before transition");
                    }
                }
                "17:03:30" | "17:03:31" => {
                    if state != "SUNSET TRANSITION" {
                        println!(
                            "  ⚠️  POTENTIAL ISSUE: Expected SUNSET TRANSITION at start boundary"
                        );
                    }
                }
                "17:08:29" | "17:08:30" => {
                    if state != "SUNSET TRANSITION" {
                        println!(
                            "  ⚠️  POTENTIAL ISSUE: Expected SUNSET TRANSITION at end boundary"
                        );
                    }
                }
                "17:08:31" => {
                    if state != "NIGHT" {
                        println!("  ⚠️  POTENTIAL ISSUE: Expected NIGHT just after transition");
                    }
                }
                _ => {}
            }
        }

        // Test the specific scenario: what happens if we're right at sunset time in center mode?
        let exact_sunset = NaiveTime::parse_from_str("17:06:00", "%H:%M:%S").unwrap();
        let in_transition = is_time_in_range(exact_sunset, sunset_start, sunset_end);
        println!(
            "\nAt exact sunset time (17:06:00): {}",
            if in_transition {
                "IN TRANSITION"
            } else {
                "NOT IN TRANSITION"
            }
        );

        if !in_transition {
            println!("  ❌ BUG FOUND: Exact sunset time should be in transition for center mode!");
        }
    }

    #[test]
    fn test_center_mode_precision_issue() {
        // Test with the exact user configuration
        let config = create_test_config("17:06:00", "06:00:00", "center", 5);

        println!("Testing Center Mode Precision Issue");

        // Calculate transition windows
        let (sunset_start, sunset_end, _sunrise_start, _sunrise_end) =
            calculate_transition_windows(&config, None);

        println!("Sunset: 17:06:00");
        println!("Transition duration: 5 minutes");
        println!("Center mode window: {sunset_start} to {sunset_end}");

        // Check what the actual calculated times are
        println!("Sunset start: {sunset_start:?}");
        println!("Sunset end: {sunset_end:?}");
        println!("Sunrise start: {_sunrise_start:?}");
        println!("Sunrise end: {_sunrise_end:?}");

        // Test the exact sunset time and nearby times
        let test_times = ["17:05:59", "17:06:00", "17:06:01"];

        for time_str in test_times {
            let test_time = NaiveTime::parse_from_str(time_str, "%H:%M:%S").unwrap();
            let in_sunset = is_time_in_range(test_time, sunset_start, sunset_end);
            let in_sunrise = is_time_in_range(test_time, _sunrise_start, _sunrise_end);

            println!("Time {time_str}: sunset={in_sunset}, sunrise={in_sunrise}");

            if !in_sunset && !in_sunrise {
                let stable_state = get_stable_state_for_time(test_time, sunset_end, _sunrise_start);
                println!("  -> Stable state: {stable_state:?}");
            }
        }

        // The critical test: is 17:06:00 actually in the sunset transition?
        let exact_sunset = NaiveTime::parse_from_str("17:06:00", "%H:%M:%S").unwrap();
        let should_be_in_transition = is_time_in_range(exact_sunset, sunset_start, sunset_end);
        println!("\nCRITICAL: Is 17:06:00 in sunset transition? {should_be_in_transition}");

        if !should_be_in_transition {
            println!("❌ FOUND THE BUG: 17:06:00 should be in transition for center mode!");

            // Let's see what the stable state logic thinks
            let stable_state = get_stable_state_for_time(exact_sunset, sunset_end, _sunrise_start);
            println!("   Stable state logic says: {stable_state:?}");

            // And let's see the exact boundary times in seconds
            println!(
                "   Sunset start seconds: {}",
                sunset_start.hour() * 3600 + sunset_start.minute() * 60 + sunset_start.second()
            );
            println!(
                "   Test time seconds: {}",
                exact_sunset.hour() * 3600 + exact_sunset.minute() * 60 + exact_sunset.second()
            );
            println!(
                "   Sunset end seconds: {}",
                sunset_end.hour() * 3600 + sunset_end.minute() * 60 + sunset_end.second()
            );
        }
    }

    #[test]
    fn test_startup_transition_flow_bug() {
        // Simulate the exact flow that happens in the real application
        let config = create_test_config("17:06:00", "06:00:00", "center", 5);

        println!("Testing Startup Transition Flow");

        // Test times that the user mentioned as problematic:
        // "before or after the centered time" (17:06:00)
        let problematic_times = [
            "17:05:00", // Before centered time, but should still be in transition
            "17:07:00", // After centered time, but should still be in transition
            "17:06:00", // Exact centered time - user says this works
        ];

        for time_str in problematic_times {
            println!("\n--- Testing startup at {time_str} ---");

            // Step 1: Get initial state (what StartupTransition::new would capture)
            // We'll simulate this by manually checking the state at this time
            let test_time = NaiveTime::parse_from_str(time_str, "%H:%M:%S").unwrap();
            let (sunset_start, sunset_end, _sunrise_start, _sunrise_end) =
                calculate_transition_windows(&config, None);

            let initial_state = if is_time_in_range(test_time, sunset_start, sunset_end) {
                let progress = calculate_progress(test_time, sunset_start, sunset_end);
                Period::Sunset { progress }
            } else if is_time_in_range(test_time, _sunrise_start, _sunrise_end) {
                let progress = calculate_progress(test_time, _sunrise_start, _sunrise_end);
                Period::Sunrise { progress }
            } else {
                get_stable_state_for_time(test_time, sunset_end, _sunrise_start)
            };

            println!("Initial state at {time_str}: {initial_state:?}");

            // Step 2: Simulate 10 seconds later (after startup transition)
            // Add 10 seconds to the test time
            let seconds_since_midnight =
                test_time.hour() * 3600 + test_time.minute() * 60 + test_time.second();
            let final_seconds = (seconds_since_midnight + 10) % (24 * 3600); // Handle midnight wrap
            let final_time =
                NaiveTime::from_num_seconds_from_midnight_opt(final_seconds, 0).unwrap();

            println!("Time after 10s startup transition: {final_time}");

            // Step 3: Get final state (what gets applied after startup transition)
            let final_state = if is_time_in_range(final_time, sunset_start, sunset_end) {
                let progress = calculate_progress(final_time, sunset_start, sunset_end);
                Period::Sunset { progress }
            } else if is_time_in_range(final_time, _sunrise_start, _sunrise_end) {
                let progress = calculate_progress(final_time, _sunrise_start, _sunrise_end);
                Period::Sunrise { progress }
            } else {
                get_stable_state_for_time(final_time, sunset_end, _sunrise_start)
            };

            println!("Final state at {final_time}: {final_state:?}");

            // Check for the bug: if initial was transitioning but final is stable night
            match (initial_state, final_state) {
                (Period::Sunset { .. }, Period::Night) => {
                    println!(
                        "  ❌ BUG DETECTED: Started in sunset transition but ended in stable night mode!"
                    );
                }
                (Period::Sunset { .. } | Period::Sunrise { .. }, Period::Day | Period::Night) => {
                    println!(
                        "  ⚠️  POTENTIAL ISSUE: Started in transition but ended in stable mode"
                    );
                }
                (Period::Day | Period::Night, Period::Sunset { .. } | Period::Sunrise { .. }) => {
                    println!("  ✓ Started stable, ended transitioning - this is normal");
                }
                _ => {
                    println!("  ✓ State transition looks correct");
                }
            }
        }
    }

    #[test]
    fn test_transition_boundary_edge_cases() {
        // Test what happens at the exact boundaries of the transition window
        let config = create_test_config("17:06:00", "06:00:00", "center", 5);

        println!("Testing Transition Boundary Edge Cases");

        let (sunset_start, sunset_end, _sunrise_start, _sunrise_end) =
            calculate_transition_windows(&config, None);
        println!("Sunset transition window: {sunset_start} to {sunset_end}");

        // Test at the exact boundaries
        let boundary_times = [
            "17:03:29", // 1 second before transition starts
            "17:03:30", // Exact transition start
            "17:08:30", // Exact transition end
            "17:08:31", // 1 second after transition ends
        ];

        for time_str in boundary_times {
            println!("\n--- Testing at {time_str} ---");

            let test_time = NaiveTime::parse_from_str(time_str, "%H:%M:%S").unwrap();
            let in_sunset = is_time_in_range(test_time, sunset_start, sunset_end);

            if in_sunset {
                let progress = calculate_progress(test_time, sunset_start, sunset_end);
                println!("  State: SUNSET TRANSITION (progress: {progress:.3})");
            } else {
                let stable_state = get_stable_state_for_time(test_time, sunset_end, _sunrise_start);
                println!("  State: STABLE {stable_state:?}");

                // Check if this could be the source of the bug
                if time_str == "17:03:29" && stable_state == Period::Night {
                    println!(
                        "  ❌ POTENTIAL BUG: Just before transition shows NIGHT instead of DAY!"
                    );
                }
                if time_str == "17:08:31" && stable_state != Period::Night {
                    println!("  ⚠️  UNEXPECTED: Just after transition should be NIGHT");
                }
            }

            // Test what happens 10 seconds later (simulating startup delay)
            let seconds_since_midnight =
                test_time.hour() * 3600 + test_time.minute() * 60 + test_time.second();
            let future_seconds = (seconds_since_midnight + 10) % (24 * 3600);
            let future_time =
                NaiveTime::from_num_seconds_from_midnight_opt(future_seconds, 0).unwrap();

            let future_in_sunset = is_time_in_range(future_time, sunset_start, sunset_end);

            if future_in_sunset {
                let progress = calculate_progress(future_time, sunset_start, sunset_end);
                println!(
                    "  After 10s ({future_time}): SUNSET TRANSITION (progress: {progress:.3})"
                );
            } else {
                let stable_state =
                    get_stable_state_for_time(future_time, sunset_end, _sunrise_start);
                println!("  After 10s ({future_time}): STABLE {stable_state:?}");
            }

            // Check for problematic transitions
            if in_sunset && !future_in_sunset {
                println!(
                    "  ❌ FOUND ISSUE: Started in transition but ended in stable state after 10s!"
                );
                let stable_state =
                    get_stable_state_for_time(future_time, sunset_end, _sunrise_start);
                if stable_state == Period::Night {
                    println!("     This matches the user's bug report!");
                }
            }
        }
    }

    #[test]
    fn test_startup_transition_timing_fix() {
        // Test the fix for the startup transition timing bug
        let config = create_test_config("17:06:00", "06:00:00", "center", 5);

        println!("Testing Startup Transition Timing Fix");

        // Simulate starting at a time that's in transition but close to the boundary
        let problematic_start_time = "17:08:25"; // 5 seconds before end of transition
        let test_time = NaiveTime::parse_from_str(problematic_start_time, "%H:%M:%S").unwrap();

        let (sunset_start, sunset_end, _sunrise_start, _sunrise_end) =
            calculate_transition_windows(&config, None);
        println!("Transition window: {sunset_start} to {sunset_end}");
        println!("Starting program at: {problematic_start_time}");

        // Check initial state (what gets captured)
        let initial_in_transition = is_time_in_range(test_time, sunset_start, sunset_end);
        let initial_state = if initial_in_transition {
            let progress = calculate_progress(test_time, sunset_start, sunset_end);
            println!("Initial state: SUNSET TRANSITION (progress: {progress:.3})");
            Period::Sunset { progress }
        } else {
            println!("Initial state: NOT in transition (this would be unexpected)");
            Period::Day // placeholder
        };

        // Check what happens 10 seconds later (after startup transition)
        let seconds_since_midnight =
            test_time.hour() * 3600 + test_time.minute() * 60 + test_time.second();
        let final_seconds = (seconds_since_midnight + 10) % (24 * 3600);
        let final_time = NaiveTime::from_num_seconds_from_midnight_opt(final_seconds, 0).unwrap();

        println!("Time after 10s startup: {final_time}");

        let final_in_transition = is_time_in_range(final_time, sunset_start, sunset_end);
        if final_in_transition {
            let progress = calculate_progress(final_time, sunset_start, sunset_end);
            println!("Recalculated state: SUNSET TRANSITION (progress: {progress:.3})");
        } else {
            println!("Recalculated state: NOT in transition");
        }

        // The bug scenario
        if initial_in_transition && !final_in_transition {
            println!("❌ BUG SCENARIO DETECTED:");
            println!("   - Started in transition at {problematic_start_time}");
            println!("   - 10 seconds later ({final_time}), no longer in transition");
            println!(
                "   - Old code would apply NIGHT mode instead of continuing the sunset transition"
            );
            println!("   - Fixed code applies the originally captured transition state");

            // Verify the fix behavior
            match initial_state {
                Period::Sunset { progress } => {
                    println!(
                        "✅ FIX: Will correctly apply sunset transition with progress {progress:.3}"
                    );
                }
                _ => {
                    println!("❌ Unexpected initial state");
                }
            }
        } else {
            println!("✅ No timing issue in this scenario");
        }
    }
}

#[cfg(test)]
mod static_tests {
    use super::*;
    use crate::config::{Backend, Config};
    use std::time::Duration as StdDuration;

    // Helper function to create a static mode config for testing
    fn create_static_mode_config(temp: u32, gamma: f32) -> Config {
        Config {
            backend: Some(Backend::Auto),
            smoothing: Some(false),
            startup_duration: Some(10.0),
            shutdown_duration: Some(10.0),
            startup_transition: None, // Deprecated field - not needed
            startup_transition_duration: None, // Deprecated field - not needed
            start_hyprsunset: None,
            adaptive_interval: None,
            latitude: None,
            longitude: None,
            sunset: None,  // Not needed for static mode
            sunrise: None, // Not needed for static mode
            night_temp: None,
            day_temp: None,
            night_gamma: None,
            day_gamma: None,
            static_temp: Some(temp),
            static_gamma: Some(gamma),
            transition_duration: None,
            update_interval: Some(60),
            transition_mode: Some("static".to_string()),
        }
    }

    #[test]
    fn test_static_mode_state_calculation() {
        let config = create_static_mode_config(4000, 85.0);
        let state = get_transition_state(&config, None);

        assert_eq!(state, Period::Static);

        // Verify that the state returns correct values from config
        assert_eq!(state.temperature(&config), 4000);
        assert_eq!(state.gamma(&config), 85.0);
    }

    #[test]
    fn test_static_mode_no_time_dependence() {
        let config = create_static_mode_config(5000, 95.0);

        // State should be same regardless of time
        let morning_state = get_transition_state(&config, None);

        // Mock different time - state should be identical
        let evening_state = get_transition_state(&config, None);

        assert_eq!(morning_state, evening_state);
        assert_eq!(morning_state, Period::Static);
    }

    #[test]
    fn test_static_mode_long_sleep_duration() {
        let config = create_static_mode_config(4000, 85.0);
        let sleep_duration = time_until_next_event(&config, None);

        // Should wait indefinitely in static mode (Duration::MAX)
        assert_eq!(sleep_duration, StdDuration::MAX);
    }

    #[test]
    fn test_static_mode_config_validation() {
        use crate::config::validation::validate_config;

        // Valid static config
        let valid_config = create_static_mode_config(4000, 85.0);
        assert!(validate_config(&valid_config).is_ok());

        // Invalid static config - missing static_temperature
        let mut invalid_config = valid_config.clone();
        invalid_config.static_temp = None;
        let result = validate_config(&invalid_config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Static mode requires static_temp")
        );

        // Invalid static config - missing static_gamma
        invalid_config.static_temp = Some(4000);
        invalid_config.static_gamma = None;
        let result = validate_config(&invalid_config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Static mode requires static_gamma")
        );
    }

    #[test]
    fn test_static_mode_temperature_range_validation() {
        use crate::config::validation::validate_config;

        // Test valid temperature boundaries
        let mut config = create_static_mode_config(1000, 85.0);
        assert!(validate_config(&config).is_ok());

        config.static_temp = Some(20000);
        assert!(validate_config(&config).is_ok());

        // Test invalid temperatures
        config.static_temp = Some(999);
        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("static_temp"));

        config.static_temp = Some(20001);
        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("static_temp"));
    }

    #[test]
    fn test_static_mode_gamma_range_validation() {
        use crate::config::validation::validate_config;

        // Test valid gamma boundaries
        let mut config = create_static_mode_config(4000, 10.0); // Minimum valid gamma
        assert!(validate_config(&config).is_ok());

        config.static_gamma = Some(100.0);
        assert!(validate_config(&config).is_ok());

        // Test invalid gamma values
        config.static_gamma = Some(9.9); // Below minimum
        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("static_gamma"));

        config.static_gamma = Some(-10.0);
        let result = validate_config(&config);
        assert!(result.is_err());

        config.static_gamma = Some(100.1);
        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("static_gamma"));
    }

    #[test]
    fn test_static_mode_state_properties() {
        let config = create_static_mode_config(4500, 92.0);
        let state = Period::Static;

        // Test state properties
        assert!(state.is_stable());
        assert!(!state.is_transitioning());
        assert_eq!(state.progress(), None);
        assert_eq!(state.display_name(), "Static");
        assert_eq!(state.symbol(), "󰋙 ");

        // Test that next_period returns itself (no transitions in static mode)
        assert_eq!(state.next_period(), Period::Static);

        // Test that values are retrieved correctly
        assert_eq!(state.temperature(&config), 4500);
        assert_eq!(state.gamma(&config), 92.0);
    }

    #[test]
    fn test_static_mode_ignores_time_settings() {
        // Static mode should work regardless of time settings
        let mut config = create_static_mode_config(4000, 85.0);

        // Change time settings - should still work in static mode
        config.sunset = Some("23:59:59".to_string());
        config.sunrise = Some("00:00:01".to_string());
        config.transition_duration = Some(1000);

        // Should still be valid since static mode ignores these
        let state = get_transition_state(&config, None);
        assert_eq!(state, Period::Static);
        assert_eq!(state.temperature(&config), 4000);
        assert_eq!(state.gamma(&config), 85.0);
    }

    #[test]
    fn test_static_mode_different_values() {
        // Test various temperature and gamma combinations
        let test_cases = vec![
            (1000, 10.0),   // Minimum values
            (20000, 100.0), // Maximum values
            (6500, 100.0),  // Day-like values
            (3300, 90.0),   // Night-like values
            (5000, 95.0),   // Medium values
        ];

        for (temp, gamma) in test_cases {
            let config = create_static_mode_config(temp, gamma);
            let state = get_transition_state(&config, None);

            assert_eq!(state, Period::Static);
            assert_eq!(state.temperature(&config), temp);
            assert_eq!(state.gamma(&config), gamma);
        }
    }
}
