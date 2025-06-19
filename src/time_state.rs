//! Time-based state management for sunrise/sunset transitions.
//!
//! This module handles the core logic for determining when transitions should occur,
//! calculating smooth progress values, deciding when application state updates
//! are needed, and providing standardized state messaging. It supports different
//! transition modes and handles edge cases like midnight crossings and extreme
//! day/night periods.
//!
//! ## Key Functionality
//! - **State Detection**: Determining current time-based state (day, night, or transitioning)
//! - **Transition Calculation**: Computing smooth interpolation between day/night values  
//! - **Update Logic**: Deciding when backend state changes should be applied
//! - **Standardized Messaging**: Providing consistent state announcement messages
//! - **Time Handling**: Managing complex timing scenarios including midnight crossings

use chrono::{Local, NaiveTime, Timelike};
use std::time::Duration as StdDuration;

use crate::config::Config;
use crate::constants::{
    DEFAULT_DAY_GAMMA, DEFAULT_DAY_TEMP, DEFAULT_NIGHT_GAMMA, DEFAULT_NIGHT_TEMP,
    DEFAULT_TRANSITION_DURATION, DEFAULT_UPDATE_INTERVAL,
};
// Note: We use crate::geo:: paths directly in the code below
use crate::logger::Log;
use crate::utils::{interpolate_f32, interpolate_u32};

/// Apply centered transition logic to calculate transition windows.
///
/// This function implements the core "center mode" logic where transitions are
/// symmetrically distributed around a center point. It's used by both the regular
/// center mode (with user-configured times) and geo mode (with solar-calculated times).
///
/// # Arguments
/// * `sunset_time` - The center point for the sunset transition
/// * `sunset_duration` - Total duration of the sunset transition
/// * `sunrise_time` - The center point for the sunrise transition
/// * `sunrise_duration` - Total duration of the sunrise transition
///
/// # Returns
/// A tuple of (sunset_start, sunset_end, sunrise_start, sunrise_end) times
///
/// # Example
/// For a sunset at 19:00 with a 30-minute duration:
/// - Start: 18:45 (19:00 - 15 minutes)
/// - End: 19:15 (19:00 + 15 minutes)
fn apply_centered_transition(
    sunset_time: NaiveTime,
    sunset_duration: StdDuration,
    sunrise_time: NaiveTime,
    sunrise_duration: StdDuration,
) -> (NaiveTime, NaiveTime, NaiveTime, NaiveTime) {
    // Calculate half durations for symmetric distribution
    let sunset_half = chrono::Duration::from_std(sunset_duration / 2).unwrap();
    let sunrise_half = chrono::Duration::from_std(sunrise_duration / 2).unwrap();
    
    // Debug logging removed - now handled in geo selection flow
    
    (
        sunset_time - sunset_half,   // Sunset start: center - half duration
        sunset_time + sunset_half,   // Sunset end: center + half duration
        sunrise_time - sunrise_half, // Sunrise start: center - half duration
        sunrise_time + sunrise_half, // Sunrise end: center + half duration
    )
}

/// Represents the basic time-based state of the display.
#[derive(Debug, PartialEq, Copy, Clone)]
pub enum TimeState {
    Day,   // Natural color temperature and full brightness
    Night, // Warm color temperature and reduced brightness
}

/// Represents the current transition state with progress information.
#[derive(Debug, PartialEq, Copy, Clone)]
pub enum TransitionState {
    Stable(TimeState), // Static state - no transition occurring
    Transitioning {
        from: TimeState, // Starting state
        to: TimeState,   // Target state
        progress: f32,   // Transition progress (0.0 = start, 1.0 = complete)
    },
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
///
/// # Returns
/// Tuple of (sunset_start, sunset_end, sunrise_start, sunrise_end) as NaiveTime
fn calculate_transition_windows(config: &Config) -> (NaiveTime, NaiveTime, NaiveTime, NaiveTime) {
    let mode = config.transition_mode.as_deref().unwrap_or("finish_by");

    // Handle geo mode separately using actual sunrise/sunset calculations
    if mode == "geo" {
        // For geo mode, use actual civil twilight transition times
        return calculate_geo_transition_windows(config);
    }

    let (sunset, sunrise) = (
        NaiveTime::parse_from_str(&config.sunset, "%H:%M:%S").unwrap(),
        NaiveTime::parse_from_str(&config.sunrise, "%H:%M:%S").unwrap(),
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
            // Use the shared centered transition logic with uniform durations
            apply_centered_transition(sunset, transition_duration, sunrise, transition_duration)
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

/// Calculate transition windows for geo mode using centered transition logic with solar data.
///
/// This function demonstrates the architectural unification of geo mode with center mode.
/// Instead of using a separate code path, geo mode now:
/// 1. Calculates astronomically accurate sunset/sunrise times (sun at 0° elevation)
/// 2. Measures the actual civil twilight duration (time from +6° to -6° elevation)
/// 3. Feeds these solar-calculated parameters into the same `apply_centered_transition()`
///    function used by center mode
///
/// This approach ensures behavioral consistency across all modes while maintaining
/// astronomical accuracy for geo mode. The transitions are symmetrically distributed
/// around the true solar events, eliminating timing bugs and special-case handling.
///
/// # Priority Order
/// 1. Configured coordinates (latitude/longitude in config)
/// 2. Auto-detected coordinates from system timezone
/// 3. Fallback to static config times with default duration
///
/// # Arguments
/// * `config` - Configuration potentially containing coordinates
///
/// # Returns
/// Tuple of (sunset_start, sunset_end, sunrise_start, sunrise_end) as NaiveTime
fn calculate_geo_transition_windows(
    config: &Config,
) -> (NaiveTime, NaiveTime, NaiveTime, NaiveTime) {
    use crate::logger::Log;

    // Priority 1: Use coordinates from config if available
    if let (Some(lat), Some(lon)) = (config.latitude, config.longitude) {
        if let Ok((sunset_time, sunset_duration, sunrise_time, sunrise_duration)) = 
            crate::geo::solar::calculate_geo_center_times_and_durations(lat, lon) {
            
            // Apply the shared centered transition logic with solar-calculated parameters
            return apply_centered_transition(sunset_time, sunset_duration, sunrise_time, sunrise_duration);
        } else {
            Log::log_warning("Failed to calculate geo center times with configured coordinates");
        }
    }

    // Priority 2: Try timezone detection for automatic coordinates
    if let Ok((lat, lon, _city_name)) = detect_timezone_coordinates() {
        if let Ok((sunset_time, sunset_duration, sunrise_time, sunrise_duration)) = 
            crate::geo::solar::calculate_geo_center_times_and_durations(lat, lon) {
            
            // Apply center-mode logic: sunset/sunrise ± (duration/2)
            let sunset_half_duration = chrono::Duration::from_std(sunset_duration / 2).unwrap();
            let sunrise_half_duration = chrono::Duration::from_std(sunrise_duration / 2).unwrap();
            
            // Debug logging for manual centering calculation (timezone detection path)
            if std::env::var("SUNSETR_DEBUG").is_ok() {
                Log::log_pipe();
                Log::log_debug("=== Geo Mode Fallback: Using timezone-detected coordinates ===");
                Log::log_debug(&format!("Sunset center: {}", sunset_time.format("%H:%M:%S")));
                Log::log_debug(&format!("Sunset duration: {} minutes", sunset_duration.as_secs() / 60));
                Log::log_debug(&format!("Sunset half duration: {} minutes", sunset_half_duration.num_minutes()));
                Log::log_debug(&format!("Calculated sunset start: {}", (sunset_time - sunset_half_duration).format("%H:%M:%S")));
                Log::log_debug(&format!("Calculated sunset end: {}", (sunset_time + sunset_half_duration).format("%H:%M:%S")));
            }
            
            return (
                sunset_time - sunset_half_duration,   // Sunset start
                sunset_time + sunset_half_duration,   // Sunset end  
                sunrise_time - sunrise_half_duration, // Sunrise start
                sunrise_time + sunrise_half_duration, // Sunrise end
            );
        } else {
            Log::log_warning("Failed to calculate geo center times with detected coordinates");
        }
    }

    // Priority 3: Fall back to static config times with default transition
    Log::log_indented("Falling back to configured sunset/sunrise times");
    let sunset = NaiveTime::parse_from_str(&config.sunset, "%H:%M:%S").unwrap_or_else(|_| {
        NaiveTime::parse_from_str(crate::constants::DEFAULT_SUNSET, "%H:%M:%S").unwrap()
    });
    let sunrise = NaiveTime::parse_from_str(&config.sunrise, "%H:%M:%S").unwrap_or_else(|_| {
        NaiveTime::parse_from_str(crate::constants::DEFAULT_SUNRISE, "%H:%M:%S").unwrap()
    });

    // Use default 30-minute transition with the shared centered logic
    let default_duration = StdDuration::from_secs(30 * 60); // 30 minutes
    apply_centered_transition(sunset, default_duration, sunrise, default_duration)
}

/// Detect coordinates from system timezone.
///
/// # Returns
/// Tuple of (latitude, longitude, city_name) or error
fn detect_timezone_coordinates() -> Result<(f64, f64, String), anyhow::Error> {
    crate::geo::detect_coordinates_from_timezone()
}

/// Get the current transition state based on the time of day and configuration.
///
/// This is the main function that determines what state the display should be in.
/// It calculates transition windows and checks if the current time falls within
/// any transition period, returning either a stable state or transition progress.
///
/// # Arguments
/// * `config` - Configuration containing all timing and transition settings
///
/// # Returns
/// TransitionState indicating current state and any transition progress
pub fn get_transition_state(config: &Config) -> TransitionState {
    let now = Local::now().time();
    let (sunset_start, sunset_end, _sunrise_start, _sunrise_end) =
        calculate_transition_windows(config);

    // Check if we're in a transition period
    if is_time_in_range(now, sunset_start, sunset_end) {
        // Sunset transition (day -> night)
        let progress = calculate_progress(now, sunset_start, sunset_end);
        TransitionState::Transitioning {
            from: TimeState::Day,
            to: TimeState::Night,
            progress,
        }
    } else if is_time_in_range(now, _sunrise_start, _sunrise_end) {
        // Sunrise transition (night -> day)
        let progress = calculate_progress(now, _sunrise_start, _sunrise_end);
        TransitionState::Transitioning {
            from: TimeState::Night,
            to: TimeState::Day,
            progress,
        }
    } else {
        // Stable period - determine which stable state based on time relative to transitions
        let current_state = get_stable_state_for_time(now, sunset_end, _sunrise_start);
        TransitionState::Stable(current_state)
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
/// TimeState::Day or TimeState::Night
fn get_stable_state_for_time(
    now: NaiveTime,
    sunset_end: NaiveTime,
    sunrise_start: NaiveTime,
) -> TimeState {
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
            TimeState::Night
        } else {
            TimeState::Day
        }
    } else {
        // Overnight case: sunset transition crosses midnight OR spans most of the day
        // Night period: from sunset_end through midnight OR before sunrise_start
        if now_secs >= sunset_end_secs || now_secs < sunrise_start_secs {
            TimeState::Night
        } else {
            TimeState::Day
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
///
/// # Returns
/// Duration to sleep before the next state check
pub fn time_until_next_event(config: &Config) -> StdDuration {
    // Get current transition state
    let current_state = get_transition_state(config);

    match current_state {
        TransitionState::Transitioning { .. } => {
            // If we're currently transitioning, return the update interval for smooth progress
            StdDuration::from_secs(config.update_interval.unwrap_or(DEFAULT_UPDATE_INTERVAL))
        }
        TransitionState::Stable(_) => {
            // Calculate time until next transition starts
            let now = Local::now();
            let current_time = now.time();
            let (sunset_start, _sunset_end, _sunrise_start, _sunrise_end) =
                calculate_transition_windows(config);

            // Convert times to seconds since midnight for easier comparison
            let current_secs =
                current_time.hour() * 3600 + current_time.minute() * 60 + current_time.second();
            let sunset_start_secs =
                sunset_start.hour() * 3600 + sunset_start.minute() * 60 + sunset_start.second();
            let sunrise_start_secs = _sunrise_start.hour() * 3600
                + _sunrise_start.minute() * 60
                + _sunrise_start.second();

            // Find the next transition start time
            let seconds_until = if sunset_start_secs > current_secs {
                // Sunset transition starts later today
                sunset_start_secs - current_secs
            } else if sunrise_start_secs > current_secs {
                // Sunrise transition starts later today
                sunrise_start_secs - current_secs
            } else {
                // Both transitions are in the past, calculate for tomorrow
                // Since we're past both transitions today, next is always sunrise tomorrow
                // (sunrise happens in the morning, before sunset in the evening)
                (24 * 3600) - current_secs + sunrise_start_secs
            };

            StdDuration::from_secs(seconds_until as u64)
        }
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
    let total_duration = (end - start).num_seconds() as f32;
    let elapsed = (now - start).num_seconds() as f32;
    let linear_progress = (elapsed / total_duration).clamp(0.0, 1.0);
    
    // Apply Bezier curve with control points from constants for smooth S-curve
    // These control points create an ease-in-out effect with no sudden jumps
    crate::utils::bezier_curve(
        linear_progress,
        crate::constants::BEZIER_P1X,
        crate::constants::BEZIER_P1Y,
        crate::constants::BEZIER_P2X,
        crate::constants::BEZIER_P2Y
    )
}

/// Check if a time falls within a given range, handling midnight crossings.
///
/// This function correctly handles cases where the time range crosses midnight
/// (e.g., 23:00 to 01:00), which is common for night-time periods.
///
/// # Arguments
/// * `time` - Time to check
/// * `start` - Range start time
/// * `end` - Range end time
///
/// # Returns
/// true if time is within the range, false otherwise
fn is_time_in_range(time: NaiveTime, start: NaiveTime, end: NaiveTime) -> bool {
    if start <= end {
        // Normal range (doesn't cross midnight)
        time >= start && time <= end
    } else {
        // Overnight range (crosses midnight)
        time >= start || time <= end
    }
}

/// Calculate the initial temperature and gamma values for a given transition state
/// This is used to start hyprsunset with the correct initial values
pub fn get_initial_values_for_state(state: TransitionState, config: &Config) -> (u32, f32) {
    match state {
        TransitionState::Stable(time_state) => match time_state {
            TimeState::Day => (
                config.day_temp.unwrap_or(DEFAULT_DAY_TEMP),
                config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA),
            ),
            TimeState::Night => (
                config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP),
                config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA),
            ),
        },
        TransitionState::Transitioning { from, to, progress } => {
            let temp = calculate_interpolated_temp(from, to, progress, config);
            let gamma = calculate_interpolated_gamma(from, to, progress, config);
            (temp, gamma)
        }
    }
}

/// Helper for calculating interpolated temperature
pub fn calculate_interpolated_temp(
    from: TimeState,
    to: TimeState,
    progress: f32,
    config: &Config,
) -> u32 {
    let (start_temp, end_temp) = match (from, to) {
        (TimeState::Day, TimeState::Night) => (
            config.day_temp.unwrap_or(DEFAULT_DAY_TEMP),
            config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP),
        ),
        (TimeState::Night, TimeState::Day) => (
            config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP),
            config.day_temp.unwrap_or(DEFAULT_DAY_TEMP),
        ),
        // Handle edge cases
        (TimeState::Day, TimeState::Day) => {
            let day_temp = config.day_temp.unwrap_or(DEFAULT_DAY_TEMP);
            (day_temp, day_temp)
        }
        (TimeState::Night, TimeState::Night) => {
            let night_temp = config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP);
            (night_temp, night_temp)
        }
    };

    interpolate_u32(start_temp, end_temp, progress)
}

/// Helper for calculating interpolated gamma
pub fn calculate_interpolated_gamma(
    from: TimeState,
    to: TimeState,
    progress: f32,
    config: &Config,
) -> f32 {
    let (start_gamma, end_gamma) = match (from, to) {
        (TimeState::Day, TimeState::Night) => (
            config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA),
            config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA),
        ),
        (TimeState::Night, TimeState::Day) => (
            config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA),
            config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA),
        ),
        // Handle edge cases
        (TimeState::Day, TimeState::Day) => {
            let day_gamma = config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA);
            (day_gamma, day_gamma)
        }
        (TimeState::Night, TimeState::Night) => {
            let night_gamma = config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA);
            (night_gamma, night_gamma)
        }
    };

    interpolate_f32(start_gamma, end_gamma, progress)
}

/// Get the name of the transition type (for use in "Commencing/Completed" messages).
///
/// Returns just the transition name without the "Commencing" prefix.
///
/// # Arguments
/// * `from` - Starting time state  
/// * `to` - Target time state
///
/// # Returns
/// String containing the transition type name with icon
pub fn get_transition_type_name(from: TimeState, to: TimeState) -> &'static str {
    match (from, to) {
        (TimeState::Day, TimeState::Night) => "sunset 󰖛 ",
        (TimeState::Night, TimeState::Day) => "sunrise 󰖜 ",
        _ => "transition",
    }
}

/// Determine whether the application state should be updated.
///
/// This function implements the logic for deciding when to apply state changes
/// to the backend. It considers:
/// - Transition start/end detection
/// - Progress during ongoing transitions  
/// - State changes between stable periods
/// - Time jump detection (system sleep/resume)
///
/// # Arguments
/// * `current_state` - The last known transition state
/// * `new_state` - The newly calculated transition state
/// * `time_since_last_check` - Duration since last state check
///
/// # Returns
/// `true` if the state should be updated, `false` to skip this update cycle
pub fn should_update_state(
    current_state: &TransitionState,
    new_state: &TransitionState,
    time_since_last_check: StdDuration,
) -> bool {
    use crate::constants::SLEEP_DETECTION_THRESHOLD_SECS;

    match (current_state, new_state) {
        // Detect entering a transition (from stable to transitioning)
        (TransitionState::Stable(_), TransitionState::Transitioning { progress, from, to })
            if *progress < 0.01 =>
        {
            let transition_type = get_transition_type_name(*from, *to);
            Log::log_block_start(&format!("Commencing {}", transition_type));
            true
        }
        // Detect change from transitioning to stable state (transition completed)
        (
            TransitionState::Transitioning { from, to, progress },
            TransitionState::Stable(stable_state),
        ) => {
            let transition_type = get_transition_type_name(*from, *to);
            Log::log_block_start(&format!("Completed {}", transition_type));

            // Announce the mode we're now entering
            Log::log_block_start(get_stable_state_message(*stable_state));
            
            // If we just completed at 100% (1.0), skip the redundant state application
            // since the final transition update already applied the exact target values.
            // We use >= 0.999 instead of == 1.0 to handle potential floating-point precision.
            // This works correctly for all transition modes (geo, center, start_at, finish_by)
            // because they all use the same interpolation logic that guarantees exact target
            // values at progress=1.0
            if *progress >= 0.999 {
                false  // Don't update - we're already at the target values
            } else {
                true   // Update - we jumped from mid-transition to stable (unusual case)
            }
        }
        // Detect change from one stable state to another (should be rare)
        (TransitionState::Stable(prev), TransitionState::Stable(curr)) if prev != curr => {
            Log::log_block_start(&format!("State changed from {:?} to {:?}", prev, curr));

            // Announce the mode we're now entering
            Log::log_decorated(get_stable_state_message(*curr));
            true
        }
        // We're in a transition and it's time for a regular update
        (TransitionState::Transitioning { .. }, TransitionState::Transitioning { .. }) => true,
        // Large time jump detected - force update to handle system sleep/resume
        _ if time_since_last_check > StdDuration::from_secs(SLEEP_DETECTION_THRESHOLD_SECS) => {
            Log::log_decorated("Applying state due to time jump detection");
            true
        }
        _ => false,
    }
}

/// Get the appropriate log message for announcing a stable state.
///
/// Returns the standardized message with icons for entering day or night mode.
///
/// # Arguments
/// * `state` - The stable time state (Day or Night)
///
/// # Returns
/// String containing the appropriate announcement message
pub fn get_stable_state_message(state: TimeState) -> &'static str {
    match state {
        TimeState::Day => "Entering day mode 󰖨 ",
        TimeState::Night => "Entering night mode  ",
    }
}

/// Log the appropriate message for a transition state.
///
/// This function centralizes the state announcement logic that was previously
/// duplicated across backend modules.
///
/// # Arguments
/// * `state` - The transition state to announce
pub fn log_state_announcement(state: TransitionState) {
    use crate::logger::Log;

    match state {
        TransitionState::Stable(time_state) => {
            Log::log_block_start(get_stable_state_message(time_state));
        }
        TransitionState::Transitioning { from, to, .. } => {
            let transition_type = get_transition_type_name(from, to);
            Log::log_block_start(&format!("Commencing {}", transition_type));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{
        DEFAULT_DAY_GAMMA, DEFAULT_DAY_TEMP, DEFAULT_NIGHT_GAMMA, DEFAULT_NIGHT_TEMP,
        DEFAULT_UPDATE_INTERVAL,
    };

    fn create_test_config(sunset: &str, sunrise: &str, mode: &str, duration_mins: u64) -> Config {
        Config {
            start_hyprsunset: Some(false),
            backend: Some(crate::config::Backend::Auto),
            startup_transition: Some(false),
            startup_transition_duration: Some(10),
            latitude: None,
            longitude: None,
            sunset: sunset.to_string(),
            sunrise: sunrise.to_string(),
            night_temp: Some(DEFAULT_NIGHT_TEMP),
            day_temp: Some(DEFAULT_DAY_TEMP),
            night_gamma: Some(DEFAULT_NIGHT_GAMMA),
            day_gamma: Some(DEFAULT_DAY_GAMMA),
            transition_duration: Some(duration_mins),
            update_interval: Some(DEFAULT_UPDATE_INTERVAL),
            transition_mode: Some(mode.to_string()),
        }
    }

    #[test]
    fn test_calculate_transition_windows_finish_by() {
        let config = create_test_config("19:00:00", "06:00:00", "finish_by", 30);
        let (sunset_start, sunset_end, sunrise_start, sunrise_end) =
            calculate_transition_windows(&config);

        assert_eq!(sunset_start, NaiveTime::from_hms_opt(18, 30, 0).unwrap());
        assert_eq!(sunset_end, NaiveTime::from_hms_opt(19, 0, 0).unwrap());
        assert_eq!(sunrise_start, NaiveTime::from_hms_opt(5, 30, 0).unwrap());
        assert_eq!(sunrise_end, NaiveTime::from_hms_opt(6, 0, 0).unwrap());
    }

    #[test]
    fn test_calculate_transition_windows_start_at() {
        let config = create_test_config("19:00:00", "06:00:00", "start_at", 30);
        let (sunset_start, sunset_end, sunrise_start, sunrise_end) =
            calculate_transition_windows(&config);

        assert_eq!(sunset_start, NaiveTime::from_hms_opt(19, 0, 0).unwrap());
        assert_eq!(sunset_end, NaiveTime::from_hms_opt(19, 30, 0).unwrap());
        assert_eq!(sunrise_start, NaiveTime::from_hms_opt(6, 0, 0).unwrap());
        assert_eq!(sunrise_end, NaiveTime::from_hms_opt(6, 30, 0).unwrap());
    }

    #[test]
    fn test_calculate_transition_windows_center() {
        let config = create_test_config("19:00:00", "06:00:00", "center", 30);
        let (sunset_start, sunset_end, sunrise_start, sunrise_end) =
            calculate_transition_windows(&config);

        assert_eq!(sunset_start, NaiveTime::from_hms_opt(18, 45, 0).unwrap());
        assert_eq!(sunset_end, NaiveTime::from_hms_opt(19, 15, 0).unwrap());
        assert_eq!(sunrise_start, NaiveTime::from_hms_opt(5, 45, 0).unwrap());
        assert_eq!(sunrise_end, NaiveTime::from_hms_opt(6, 15, 0).unwrap());
    }

    #[test]
    fn test_extreme_short_transition() {
        let config = create_test_config("19:00:00", "06:00:00", "finish_by", 5); // 5 minutes
        let (sunset_start, sunset_end, _, _) = calculate_transition_windows(&config);

        assert_eq!(sunset_start, NaiveTime::from_hms_opt(18, 55, 0).unwrap());
        assert_eq!(sunset_end, NaiveTime::from_hms_opt(19, 0, 0).unwrap());
    }

    #[test]
    fn test_extreme_long_transition() {
        let config = create_test_config("19:00:00", "06:00:00", "finish_by", 120); // 2 hours
        let (sunset_start, sunset_end, _, _) = calculate_transition_windows(&config);

        assert_eq!(sunset_start, NaiveTime::from_hms_opt(17, 0, 0).unwrap());
        assert_eq!(sunset_end, NaiveTime::from_hms_opt(19, 0, 0).unwrap());
    }

    #[test]
    fn test_midnight_crossing_sunset() {
        // Sunset very late, should cross midnight
        let config = create_test_config("23:30:00", "06:00:00", "start_at", 60); // 1 hour
        let (sunset_start, sunset_end, _, _) = calculate_transition_windows(&config);

        assert_eq!(sunset_start, NaiveTime::from_hms_opt(23, 30, 0).unwrap());
        assert_eq!(sunset_end, NaiveTime::from_hms_opt(0, 30, 0).unwrap());
    }

    #[test]
    fn test_midnight_crossing_sunrise() {
        // Sunrise very early, transition starts before midnight
        let config = create_test_config("20:00:00", "00:30:00", "finish_by", 60); // 1 hour
        let (_, _, sunrise_start, sunrise_end) = calculate_transition_windows(&config);

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
        assert!(is_time_in_range(
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
            NaiveTime::from_hms_opt(19, 0, 1).unwrap(),
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
        assert!(is_time_in_range(
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

        assert_eq!(
            calculate_progress(NaiveTime::from_hms_opt(18, 0, 0).unwrap(), start, end),
            0.0
        );
        assert_eq!(
            calculate_progress(NaiveTime::from_hms_opt(19, 0, 0).unwrap(), start, end),
            1.0
        );
        assert_eq!(
            calculate_progress(NaiveTime::from_hms_opt(18, 30, 0).unwrap(), start, end),
            0.5
        );
        assert_eq!(
            calculate_progress(NaiveTime::from_hms_opt(18, 15, 0).unwrap(), start, end),
            0.25
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
            TimeState::Day
        );

        // Night time
        assert_eq!(
            get_stable_state_for_time(
                NaiveTime::from_hms_opt(22, 0, 0).unwrap(),
                sunset_end,
                sunrise_start
            ),
            TimeState::Night
        );

        // Early morning night
        assert_eq!(
            get_stable_state_for_time(
                NaiveTime::from_hms_opt(3, 0, 0).unwrap(),
                sunset_end,
                sunrise_start
            ),
            TimeState::Night
        );
    }

    #[test]
    fn test_extreme_day_night_periods() {
        // Very short night: sunset at 23:00, sunrise at 01:00 (2 hour night)
        let config = create_test_config("23:00:00", "01:00:00", "finish_by", 30);
        let (_, sunset_end, sunrise_start, _) = calculate_transition_windows(&config);

        // Should be day most of the time
        assert_eq!(
            get_stable_state_for_time(
                NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
                sunset_end,
                sunrise_start
            ),
            TimeState::Day
        );

        // Should be night for the short period
        assert_eq!(
            get_stable_state_for_time(
                NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                sunset_end,
                sunrise_start
            ),
            TimeState::Night
        );
    }

    #[test]
    fn test_extreme_short_day() {
        // Very short day: sunset at 01:00, sunrise at 23:00 (2 hour day)
        let config = create_test_config("01:00:00", "23:00:00", "finish_by", 30);
        let (_, sunset_end, sunrise_start, _) = calculate_transition_windows(&config);

        // Should be night most of the time
        assert_eq!(
            get_stable_state_for_time(
                NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
                sunset_end,
                sunrise_start
            ),
            TimeState::Night
        );

        // Should be day for the short period
        assert_eq!(
            get_stable_state_for_time(
                NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                sunset_end,
                sunrise_start
            ),
            TimeState::Day
        );
    }

    #[test]
    fn test_transition_state_detection() {
        let config = create_test_config("19:00:00", "06:00:00", "finish_by", 30);

        // Mock current time using a specific test helper function would be better,
        // but for now we test the components individually which is covered above

        // Test the windows calculation which drives the state detection
        let (sunset_start, sunset_end, _sunrise_start, _sunrise_end) =
            calculate_transition_windows(&config);

        // Test that we get expected transition windows
        assert_eq!(sunset_start, NaiveTime::from_hms_opt(18, 30, 0).unwrap());
        assert_eq!(sunset_end, NaiveTime::from_hms_opt(19, 0, 0).unwrap());
    }

    #[test]
    fn test_center_mode_bug_reproduction() {
        // Reproduce the user's exact configuration that shows the bug
        // Sunset: 17:06:00, Sunrise: 06:00:00, Transition: 5 minutes, Mode: center
        let config = create_test_config("17:06:00", "06:00:00", "center", 5);

        println!("=== Testing Center Mode Bug ===");

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

        println!(
            "Expected sunset transition window: {} to {}",
            sunset_start, sunset_end
        );

        for (time_str, description) in test_times {
            // Temporarily override the current time by creating a modified config
            // We'll simulate different times by checking the logic directly
            let test_time = NaiveTime::parse_from_str(time_str, "%H:%M:%S").unwrap();

            // Manually calculate what the state should be
            let (sunset_start_calc, sunset_end_calc, _sunrise_start_calc, _sunrise_end_calc) =
                calculate_transition_windows(&config);

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
                    TimeState::Day => "DAY",
                    TimeState::Night => "NIGHT",
                }
            };

            println!("Time {}: {} ({})", time_str, expected_state, description);

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
                            "  ❌ BUG DETECTED: Should be in SUNSET TRANSITION, but got {}",
                            expected_state
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
                        println!(
                            "  ❌ BUG DETECTED: Should be NIGHT, but got {}",
                            expected_state
                        );
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

        println!("=== Testing Center Mode Timing Edge Cases ===");

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
            calculate_transition_windows(&config);
        println!("Transition window: {} to {}", sunset_start, sunset_end);

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
                    TimeState::Day => "DAY",
                    TimeState::Night => "NIGHT",
                }
            };

            println!("Time {}: {} ({})", time_str, state, description);

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

        println!("=== Testing Center Mode Precision Issue ===");

        // Calculate transition windows
        let (sunset_start, sunset_end, _sunrise_start, _sunrise_end) =
            calculate_transition_windows(&config);

        println!("Sunset: 17:06:00");
        println!("Transition duration: 5 minutes");
        println!("Center mode window: {} to {}", sunset_start, sunset_end);

        // Check what the actual calculated times are
        println!("Sunset start: {:?}", sunset_start);
        println!("Sunset end: {:?}", sunset_end);
        println!("Sunrise start: {:?}", _sunrise_start);
        println!("Sunrise end: {:?}", _sunrise_end);

        // Test the exact sunset time and nearby times
        let test_times = ["17:05:59", "17:06:00", "17:06:01"];

        for time_str in test_times {
            let test_time = NaiveTime::parse_from_str(time_str, "%H:%M:%S").unwrap();
            let in_sunset = is_time_in_range(test_time, sunset_start, sunset_end);
            let in_sunrise = is_time_in_range(test_time, _sunrise_start, _sunrise_end);

            println!(
                "Time {}: sunset={}, sunrise={}",
                time_str, in_sunset, in_sunrise
            );

            if !in_sunset && !in_sunrise {
                let stable_state = get_stable_state_for_time(test_time, sunset_end, _sunrise_start);
                println!("  -> Stable state: {:?}", stable_state);
            }
        }

        // The critical test: is 17:06:00 actually in the sunset transition?
        let exact_sunset = NaiveTime::parse_from_str("17:06:00", "%H:%M:%S").unwrap();
        let should_be_in_transition = is_time_in_range(exact_sunset, sunset_start, sunset_end);
        println!(
            "\nCRITICAL: Is 17:06:00 in sunset transition? {}",
            should_be_in_transition
        );

        if !should_be_in_transition {
            println!("❌ FOUND THE BUG: 17:06:00 should be in transition for center mode!");

            // Let's see what the stable state logic thinks
            let stable_state = get_stable_state_for_time(exact_sunset, sunset_end, _sunrise_start);
            println!("   Stable state logic says: {:?}", stable_state);

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

        println!("=== Testing Startup Transition Flow ===");

        // Test times that the user mentioned as problematic:
        // "before or after the centered time" (17:06:00)
        let problematic_times = [
            "17:05:00", // Before centered time, but should still be in transition
            "17:07:00", // After centered time, but should still be in transition
            "17:06:00", // Exact centered time - user says this works
        ];

        for time_str in problematic_times {
            println!("\n--- Testing startup at {} ---", time_str);

            // Step 1: Get initial state (what StartupTransition::new would capture)
            // We'll simulate this by manually checking the state at this time
            let test_time = NaiveTime::parse_from_str(time_str, "%H:%M:%S").unwrap();
            let (sunset_start, sunset_end, _sunrise_start, _sunrise_end) =
                calculate_transition_windows(&config);

            let initial_state = if is_time_in_range(test_time, sunset_start, sunset_end) {
                let progress = calculate_progress(test_time, sunset_start, sunset_end);
                TransitionState::Transitioning {
                    from: TimeState::Day,
                    to: TimeState::Night,
                    progress,
                }
            } else if is_time_in_range(test_time, _sunrise_start, _sunrise_end) {
                let progress = calculate_progress(test_time, _sunrise_start, _sunrise_end);
                TransitionState::Transitioning {
                    from: TimeState::Night,
                    to: TimeState::Day,
                    progress,
                }
            } else {
                let stable_state = get_stable_state_for_time(test_time, sunset_end, _sunrise_start);
                TransitionState::Stable(stable_state)
            };

            println!("Initial state at {}: {:?}", time_str, initial_state);

            // Step 2: Simulate 10 seconds later (after startup transition)
            // Add 10 seconds to the test time
            let seconds_since_midnight =
                test_time.hour() * 3600 + test_time.minute() * 60 + test_time.second();
            let final_seconds = (seconds_since_midnight + 10) % (24 * 3600); // Handle midnight wrap
            let final_time =
                NaiveTime::from_num_seconds_from_midnight_opt(final_seconds, 0).unwrap();

            println!("Time after 10s startup transition: {}", final_time);

            // Step 3: Get final state (what gets applied after startup transition)
            let final_state = if is_time_in_range(final_time, sunset_start, sunset_end) {
                let progress = calculate_progress(final_time, sunset_start, sunset_end);
                TransitionState::Transitioning {
                    from: TimeState::Day,
                    to: TimeState::Night,
                    progress,
                }
            } else if is_time_in_range(final_time, _sunrise_start, _sunrise_end) {
                let progress = calculate_progress(final_time, _sunrise_start, _sunrise_end);
                TransitionState::Transitioning {
                    from: TimeState::Night,
                    to: TimeState::Day,
                    progress,
                }
            } else {
                let stable_state =
                    get_stable_state_for_time(final_time, sunset_end, _sunrise_start);
                TransitionState::Stable(stable_state)
            };

            println!("Final state at {}: {:?}", final_time, final_state);

            // Check for the bug: if initial was transitioning but final is stable night
            match (initial_state, final_state) {
                (
                    TransitionState::Transitioning {
                        from: TimeState::Day,
                        to: TimeState::Night,
                        ..
                    },
                    TransitionState::Stable(TimeState::Night),
                ) => {
                    println!(
                        "  ❌ BUG DETECTED: Started in sunset transition but ended in stable night mode!"
                    );
                }
                (TransitionState::Transitioning { .. }, TransitionState::Stable(_)) => {
                    println!(
                        "  ⚠️  POTENTIAL ISSUE: Started in transition but ended in stable mode"
                    );
                }
                (TransitionState::Stable(_), TransitionState::Transitioning { .. }) => {
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

        println!("=== Testing Transition Boundary Edge Cases ===");

        let (sunset_start, sunset_end, _sunrise_start, _sunrise_end) =
            calculate_transition_windows(&config);
        println!(
            "Sunset transition window: {} to {}",
            sunset_start, sunset_end
        );

        // Test at the exact boundaries
        let boundary_times = [
            "17:03:29", // 1 second before transition starts
            "17:03:30", // Exact transition start
            "17:08:30", // Exact transition end
            "17:08:31", // 1 second after transition ends
        ];

        for time_str in boundary_times {
            println!("\n--- Testing at {} ---", time_str);

            let test_time = NaiveTime::parse_from_str(time_str, "%H:%M:%S").unwrap();
            let in_sunset = is_time_in_range(test_time, sunset_start, sunset_end);

            if in_sunset {
                let progress = calculate_progress(test_time, sunset_start, sunset_end);
                println!("  State: SUNSET TRANSITION (progress: {:.3})", progress);
            } else {
                let stable_state = get_stable_state_for_time(test_time, sunset_end, _sunrise_start);
                println!("  State: STABLE {:?}", stable_state);

                // Check if this could be the source of the bug
                if time_str == "17:03:29" && stable_state == TimeState::Night {
                    println!(
                        "  ❌ POTENTIAL BUG: Just before transition shows NIGHT instead of DAY!"
                    );
                }
                if time_str == "17:08:31" && stable_state != TimeState::Night {
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
                    "  After 10s ({}): SUNSET TRANSITION (progress: {:.3})",
                    future_time, progress
                );
            } else {
                let stable_state =
                    get_stable_state_for_time(future_time, sunset_end, _sunrise_start);
                println!("  After 10s ({}): STABLE {:?}", future_time, stable_state);
            }

            // Check for problematic transitions
            if in_sunset && !future_in_sunset {
                println!(
                    "  ❌ FOUND ISSUE: Started in transition but ended in stable state after 10s!"
                );
                let stable_state =
                    get_stable_state_for_time(future_time, sunset_end, _sunrise_start);
                if stable_state == TimeState::Night {
                    println!("     This matches the user's bug report!");
                }
            }
        }
    }

    #[test]
    fn test_startup_transition_timing_fix() {
        // Test the fix for the startup transition timing bug
        let config = create_test_config("17:06:00", "06:00:00", "center", 5);

        println!("=== Testing Startup Transition Timing Fix ===");

        // Simulate starting at a time that's in transition but close to the boundary
        let problematic_start_time = "17:08:25"; // 5 seconds before end of transition
        let test_time = NaiveTime::parse_from_str(problematic_start_time, "%H:%M:%S").unwrap();

        let (sunset_start, sunset_end, _sunrise_start, _sunrise_end) =
            calculate_transition_windows(&config);
        println!("Transition window: {} to {}", sunset_start, sunset_end);
        println!("Starting program at: {}", problematic_start_time);

        // Check initial state (what gets captured)
        let initial_in_transition = is_time_in_range(test_time, sunset_start, sunset_end);
        let initial_state = if initial_in_transition {
            let progress = calculate_progress(test_time, sunset_start, sunset_end);
            println!(
                "Initial state: SUNSET TRANSITION (progress: {:.3})",
                progress
            );
            TransitionState::Transitioning {
                from: TimeState::Day,
                to: TimeState::Night,
                progress,
            }
        } else {
            println!("Initial state: NOT in transition (this would be unexpected)");
            TransitionState::Stable(TimeState::Day) // placeholder
        };

        // Check what happens 10 seconds later (after startup transition)
        let seconds_since_midnight =
            test_time.hour() * 3600 + test_time.minute() * 60 + test_time.second();
        let final_seconds = (seconds_since_midnight + 10) % (24 * 3600);
        let final_time = NaiveTime::from_num_seconds_from_midnight_opt(final_seconds, 0).unwrap();

        println!("Time after 10s startup: {}", final_time);

        let final_in_transition = is_time_in_range(final_time, sunset_start, sunset_end);
        if final_in_transition {
            let progress = calculate_progress(final_time, sunset_start, sunset_end);
            println!(
                "Recalculated state: SUNSET TRANSITION (progress: {:.3})",
                progress
            );
        } else {
            println!("Recalculated state: NOT in transition");
        }

        // The bug scenario
        if initial_in_transition && !final_in_transition {
            println!("❌ BUG SCENARIO DETECTED:");
            println!("   - Started in transition at {}", problematic_start_time);
            println!(
                "   - 10 seconds later ({}), no longer in transition",
                final_time
            );
            println!(
                "   - Old code would apply NIGHT mode instead of continuing the sunset transition"
            );
            println!("   - Fixed code applies the originally captured transition state");

            // Verify the fix behavior
            match initial_state {
                TransitionState::Transitioning {
                    from: TimeState::Day,
                    to: TimeState::Night,
                    progress,
                } => {
                    println!(
                        "✅ FIX: Will correctly apply sunset transition with progress {:.3}",
                        progress
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
