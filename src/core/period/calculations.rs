//! Progress calculation functions for period transitions.
//!
//! This module provides pure functions for calculating transition progress
//! during sunset and sunrise periods, maintaining exact compatibility with
//! the current logic embedded in the Period enum.

use chrono::{DateTime, Local, NaiveTime};
use std::time::Duration as StdDuration;

use crate::common::constants::{
    ADAPTIVE_JND_GAMMA, ADAPTIVE_JND_MIREDS, DEFAULT_DAY_GAMMA, DEFAULT_DAY_TEMP,
    DEFAULT_NIGHT_GAMMA, DEFAULT_NIGHT_TEMP, DEFAULT_SUNRISE, DEFAULT_SUNSET,
    DEFAULT_TRANSITION_DURATION,
};
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
    if config.transition_mode.as_deref() == Some("static") {
        return None;
    }

    if config.transition_mode.as_deref() == Some("geo")
        && let Some(times) = geo_times
    {
        return times.get_sunset_progress_if_active(current_time);
    }

    let (sunset_start, sunset_end, _, _) = calculate_transition_windows(config, geo_times);

    if is_time_in_range(current_time, sunset_start, sunset_end) {
        Some(calculate_progress(current_time, sunset_start, sunset_end))
    } else {
        None
    }
}

/// Calculate progress for sunrise transition - maintains exact compatibility with current logic
pub fn calculate_sunrise_progress_for_period(
    current_time: NaiveTime,
    config: &Config,
    geo_times: Option<&GeoTimes>,
) -> Option<f32> {
    if config.transition_mode.as_deref() == Some("static") {
        return None;
    }

    if config.transition_mode.as_deref() == Some("geo")
        && let Some(times) = geo_times
    {
        return times.get_sunrise_progress_if_active(current_time);
    }

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

    if mode == "geo" {
        return geo_times
            .expect("BUG: geo mode without geo_times - this should never happen")
            .as_naive_times_local();
    }

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
            * 60,
    );

    match mode {
        "center" => {
            let sunset_half = chrono::Duration::from_std(transition_duration / 2).unwrap();
            let sunrise_half = chrono::Duration::from_std(transition_duration / 2).unwrap();

            (
                sunset - sunset_half,
                sunset + sunset_half,
                sunrise - sunrise_half,
                sunrise + sunrise_half,
            )
        }
        "start_at" => {
            let full_transition = chrono::Duration::from_std(transition_duration).unwrap();
            (
                sunset,
                sunset + full_transition,
                sunrise,
                sunrise + full_transition,
            )
        }
        "finish_by" => {
            let full_transition = chrono::Duration::from_std(transition_duration).unwrap();
            (
                sunset - full_transition,
                sunset,
                sunrise - full_transition,
                sunrise,
            )
        }
        _ => {
            unreachable!(
                "Invalid transition mode '{}' - config validation should prevent this",
                mode
            )
        }
    }
}

/// Calculate transitioning period progress as a value between 0.0 and 1.0.
///
/// This function calculates linear progress and then applies a smoothstep
/// transformation to create smooth, natural-looking transitions that start
/// and end with zero slope.
///
/// # Arguments
/// * `now` - Current time within the transition window
/// * `start` - When the transition began
/// * `end` - When the transition will complete
///
/// # Returns
/// Progress value transformed by smoothstep, clamped between 0.0 and 1.0
pub fn calculate_progress(now: NaiveTime, start: NaiveTime, end: NaiveTime) -> f32 {
    let today = Local::now().date_naive();

    let start_dt: DateTime<Local> = today
        .and_time(start)
        .and_local_timezone(Local)
        .single()
        .unwrap_or_else(|| {
            today
                .and_time(start)
                .and_local_timezone(Local)
                .earliest()
                .unwrap_or_else(|| Local::now().with_time(start).unwrap())
        });

    let end_dt: DateTime<Local> = if end <= start {
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

    let total_ms = end_dt.timestamp_millis() - start_dt.timestamp_millis();
    let elapsed_ms = now_dt.timestamp_millis() - start_dt.timestamp_millis();

    let linear_progress = if total_ms <= 0 {
        0.0
    } else {
        (elapsed_ms as f32 / total_ms as f32).clamp(0.0, 1.0)
    };

    crate::common::utils::smoothstep(linear_progress)
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
        Ordering::Less => time >= start && time < end,
        Ordering::Greater => time >= start || time < end,
        Ordering::Equal => false,
    }
}

/// Compute the forward duration in seconds between two `NaiveTime` values,
/// correctly handling midnight crossings.
///
/// Returns the number of seconds from `from` to `to` moving forward in time.
/// If `to <= from`, assumes the interval crosses midnight and adds 24 hours.
fn forward_secs(from: NaiveTime, to: NaiveTime) -> f64 {
    let diff = to.signed_duration_since(from).num_milliseconds() as f64 / 1000.0;
    if diff < 0.0 { diff + 86_400.0 } else { diff }
}

/// Calculate the adaptive update interval for a transition in progress.
///
/// Uses a unified second-order formula to find the largest time step where the
/// perceptual change stays below the JND (just-noticeable difference) threshold
/// for both color temperature (in mireds) and gamma.
///
/// Temperature and gamma changes are perceptually correlated and affect
/// luminance in the same direction during transitions. For correlated signals
/// on the same perceptual channel, their discriminabilities sum linearly:
///
///   `d = delta_mireds/JND_mireds + delta_gamma/JND_gamma <= 1`
///
/// This gives a unified JND fraction `J = 1 / (M/Jm + R/Jg)` which feeds
/// the core equation `a*dt^2 + b*dt = J` where `a = (1/2)*S''(t)`,
/// `b = S'(t)`, and `S(t) = 3t^2 - 2t^3` (smoothstep).
/// The rationalized form `dt = 2J / (b + sqrt(b^2 + 4aJ))`
/// eliminates all singularities and corrects the first-order approximation's systematic
/// bias: shorter intervals during the accelerating phase (t < 0.5) and longer intervals
/// during the decelerating phase (t > 0.5).
///
/// # Arguments
/// * `config` - Configuration for temperature/gamma values
/// * `transition_start` - Start time of the current transition window
/// * `transition_end` - End time of the current transition window
/// * `current_time` - Current time within the transition
///
/// # Returns
/// Optimal update interval in seconds, rounded to nearest u64
pub fn calculate_adaptive_interval(
    config: &Config,
    transition_start: NaiveTime,
    transition_end: NaiveTime,
    current_time: NaiveTime,
) -> u64 {
    let day_temp = config.day_temp.unwrap_or(DEFAULT_DAY_TEMP) as f64;
    let night_temp = config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP) as f64;
    let day_gamma = config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA);
    let night_gamma = config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA);

    let day_mireds = 1_000_000.0 / day_temp;
    let night_mireds = 1_000_000.0 / night_temp;
    let total_mireds = (day_mireds - night_mireds).abs();

    let gamma_range = (day_gamma - night_gamma).abs();

    let total_duration_secs = forward_secs(transition_start, transition_end);
    let elapsed_secs = forward_secs(transition_start, current_time);
    let linear_progress = (elapsed_secs / total_duration_secs).clamp(0.0, 1.0);

    let temp_sensitivity = if total_mireds > 0.1 {
        total_mireds / ADAPTIVE_JND_MIREDS
    } else {
        0.0
    };
    let gamma_sensitivity = if gamma_range > 0.1 {
        gamma_range / ADAPTIVE_JND_GAMMA
    } else {
        0.0
    };
    let combined_sensitivity = temp_sensitivity + gamma_sensitivity;

    if combined_sensitivity < 1.0 {
        return total_duration_secs.round().max(1.0) as u64;
    }

    let jnd_frac = 1.0 / combined_sensitivity;

    let t = linear_progress;
    let a = 3.0 - 6.0 * t;
    let b = 6.0 * t * (1.0 - t);
    let discriminant = b * b + 4.0 * a * jnd_frac;

    let dt = if discriminant <= 0.0 {
        1.0 - t
    } else {
        let denom = b + discriminant.sqrt();
        debug_assert!(
            denom > 0.0,
            "denom must be positive: b={b}, disc={discriminant}"
        );
        (2.0 * jnd_frac / denom).min(1.0 - t)
    };

    let interval_secs = dt * total_duration_secs;
    interval_secs.round().max(1.0) as u64
}
