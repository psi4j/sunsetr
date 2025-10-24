//! Progress calculation functions for period transitions.
//!
//! This module provides pure functions for calculating transition progress
//! during sunset and sunrise periods, maintaining exact compatibility with
//! the current logic embedded in the Period enum.

use chrono::{DateTime, Local, NaiveTime};
use std::time::Duration as StdDuration;

use crate::common::constants::{DEFAULT_SUNRISE, DEFAULT_SUNSET, DEFAULT_TRANSITION_DURATION};
use crate::config::Config;
use crate::geo::times::GeoTimes;

/// Calculate progress for sunset transition - maintains exact compatibility with current logic
/// This function assumes we ARE in a sunset period and calculates the progress value
/// that would have been stored in the original Period::Sunset { progress } variant.
pub fn calculate_sunset_progress_for_period(
    current_time: NaiveTime,
    config: &Config,
    geo_times: Option<&GeoTimes>,
) -> Option<f32> {
    // Handle static mode - no transitions
    if config.transition_mode.as_deref() == Some("static") {
        return None;
    }

    // For geo mode, delegate to geo_times
    if config.transition_mode.as_deref() == Some("geo")
        && let Some(times) = geo_times
    {
        // Use existing geo_times method that calculates progress for current time
        return times.get_sunset_progress_if_active(current_time);
    }

    // Traditional calculation - mirrors the logic from get_current_period()
    let (sunset_start, sunset_end, _, _) = calculate_transition_windows(config, geo_times);

    // IMPORTANT: We assume we're in the sunset period - this maintains compatibility
    // with the original logic where progress was only calculated after confirming
    // we're in the transition window
    if is_time_in_range(current_time, sunset_start, sunset_end) {
        Some(calculate_progress(current_time, sunset_start, sunset_end))
    } else {
        // This should not happen if RuntimeState is created correctly,
        // but provide fallback for safety
        None
    }
}

/// Calculate progress for sunrise transition - maintains exact compatibility with current logic
pub fn calculate_sunrise_progress_for_period(
    current_time: NaiveTime,
    config: &Config,
    geo_times: Option<&GeoTimes>,
) -> Option<f32> {
    // Handle static mode - no transitions
    if config.transition_mode.as_deref() == Some("static") {
        return None;
    }

    // For geo mode, delegate to geo_times
    if config.transition_mode.as_deref() == Some("geo")
        && let Some(times) = geo_times
    {
        return times.get_sunrise_progress_if_active(current_time);
    }

    // Traditional calculation
    let (_, _, sunrise_start, sunrise_end) = calculate_transition_windows(config, geo_times);

    if is_time_in_range(current_time, sunrise_start, sunrise_end) {
        Some(calculate_progress(current_time, sunrise_start, sunrise_end))
    } else {
        None
    }
}

/// Calculate transition windows for both `Sunset` and `Sunrise` based on the configured mode.
///
/// This function determines when transition periods should start and end based on four modes:
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
pub fn calculate_transition_windows(
    config: &Config,
    geo_times: Option<&GeoTimes>,
) -> (NaiveTime, NaiveTime, NaiveTime, NaiveTime) {
    let mode = config.transition_mode.as_deref().unwrap_or("finish_by");

    // For geo mode use pre-calculated geo_times
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
            // This should never be reached due to config validation in loading.rs
            unreachable!(
                "Invalid transition mode '{}' - config validation should prevent this",
                mode
            )
        }
    }
}

/// Calculate transitioning period progress as a value between 0.0 and 1.0.
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
pub fn calculate_progress(now: NaiveTime, start: NaiveTime, end: NaiveTime) -> f32 {
    // Convert NaiveTime to DateTime for robust midnight handling
    let today = Local::now().date_naive();

    // Convert times to DateTime, handling potential day boundary crossings
    let start_dt: DateTime<Local> = today
        .and_time(start)
        .and_local_timezone(Local)
        .single()
        .unwrap_or_else(|| {
            // Fallback for DST transitions - use UTC offset from system time
            today
                .and_time(start)
                .and_local_timezone(Local)
                .earliest()
                .unwrap_or_else(|| Local::now().with_time(start).unwrap())
        });

    let end_dt: DateTime<Local> = if end <= start {
        // Midnight crossing: end is tomorrow
        let tomorrow = today + chrono::Duration::days(1);
        tomorrow
            .and_time(end)
            .and_local_timezone(Local)
            .single()
            .unwrap_or_else(|| {
                tomorrow
                    .and_time(end)
                    .and_local_timezone(Local)
                    .earliest()
                    .unwrap_or_else(|| {
                        Local::now().with_time(end).unwrap() + chrono::Duration::days(1)
                    })
            })
    } else {
        // Same day
        today
            .and_time(end)
            .and_local_timezone(Local)
            .single()
            .unwrap_or_else(|| {
                today
                    .and_time(end)
                    .and_local_timezone(Local)
                    .earliest()
                    .unwrap_or_else(|| Local::now().with_time(end).unwrap())
            })
    };

    let now_dt: DateTime<Local> = if end <= start && now < end {
        // Midnight crossing case: current time is past midnight
        let tomorrow = today + chrono::Duration::days(1);
        tomorrow
            .and_time(now)
            .and_local_timezone(Local)
            .single()
            .unwrap_or_else(|| {
                tomorrow
                    .and_time(now)
                    .and_local_timezone(Local)
                    .earliest()
                    .unwrap_or_else(|| {
                        Local::now().with_time(now).unwrap() + chrono::Duration::days(1)
                    })
            })
    } else {
        // Normal case or before midnight crossing
        today
            .and_time(now)
            .and_local_timezone(Local)
            .single()
            .unwrap_or_else(|| {
                today
                    .and_time(now)
                    .and_local_timezone(Local)
                    .earliest()
                    .unwrap_or_else(|| Local::now().with_time(now).unwrap())
            })
    };

    // Calculate using DateTime arithmetic (no more midnight crossing bugs!)
    let total_ms = end_dt.timestamp_millis() - start_dt.timestamp_millis();
    let elapsed_ms = now_dt.timestamp_millis() - start_dt.timestamp_millis();

    let linear_progress = if total_ms <= 0 {
        0.0
    } else {
        (elapsed_ms as f32 / total_ms as f32).clamp(0.0, 1.0)
    };

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
pub fn is_time_in_range(time: NaiveTime, start: NaiveTime, end: NaiveTime) -> bool {
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
