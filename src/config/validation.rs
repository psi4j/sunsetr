//! Configuration validation functionality.
//!
//! Provides comprehensive validation to prevent impossible or problematic configurations
//! such as overlapping transitions, insufficient time periods, and extreme values.

use anyhow::{Context, Result};
use chrono::{NaiveTime, Timelike};
use std::time::Duration;

use super::Config;
use crate::common::constants::*;

/// Comprehensive configuration validation to prevent impossible or problematic setups
pub fn validate_config(config: &Config) -> Result<()> {
    // First validate fields that apply to ALL modes

    // Validate smooth transition durations (applies to all modes)
    if let Some(startup_duration_secs) = config.startup_duration {
        validate_smooth_transition_duration(startup_duration_secs, "startup_duration")?;
    }

    if let Some(shutdown_duration_secs) = config.shutdown_duration {
        validate_smooth_transition_duration(shutdown_duration_secs, "shutdown_duration")?;
    }

    // Validate legacy startup transition duration (for backward compatibility)
    if let Some(startup_duration_secs) = config.startup_transition_duration {
        validate_smooth_transition_duration(
            startup_duration_secs,
            "startup_transition_duration (deprecated)",
        )?;
    }

    // Validate adaptive interval (1-1000ms) - applies to all modes
    if let Some(interval_ms) = config.adaptive_interval
        && !(MINIMUM_ADAPTIVE_INTERVAL..=MAXIMUM_ADAPTIVE_INTERVAL).contains(&interval_ms)
    {
        anyhow::bail!(
            "adaptive_interval ({} ms) must be between {} and {} milliseconds",
            interval_ms,
            MINIMUM_ADAPTIVE_INTERVAL,
            MAXIMUM_ADAPTIVE_INTERVAL
        );
    }

    // Validate geographic coordinates (applies to geo mode)
    if let Some(lat) = config.latitude
        && !(-90.0..=90.0).contains(&lat)
    {
        anyhow::bail!("latitude must be between -90 and 90 degrees (got {})", lat);
    }

    if let Some(lon) = config.longitude
        && !(-180.0..=180.0).contains(&lon)
    {
        anyhow::bail!(
            "longitude must be between -180 and 180 degrees (got {})",
            lon
        );
    }

    // Get the transition mode
    let mode = config
        .transition_mode
        .as_deref()
        .unwrap_or(DEFAULT_TRANSITION_MODE);

    // Validate static mode configuration
    if mode == "static" {
        // Static mode requires static_temp and static_gamma
        if config.static_temp.is_none() {
            anyhow::bail!("Static mode requires static_temp to be specified");
        }
        if config.static_gamma.is_none() {
            anyhow::bail!("Static mode requires static_gamma to be specified");
        }

        // Validate static temperature range
        if let Some(temp) = config.static_temp
            && !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp)
        {
            anyhow::bail!(
                "static_temp ({}) must be between {} and {} Kelvin",
                temp,
                MINIMUM_TEMP,
                MAXIMUM_TEMP
            );
        }

        // Validate static gamma range
        if let Some(gamma) = config.static_gamma
            && !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma)
        {
            anyhow::bail!(
                "static_gamma ({}%) must be between {}% and {}%",
                gamma,
                MINIMUM_GAMMA,
                MAXIMUM_GAMMA
            );
        }

        // Static mode doesn't need time-based validation, return early
        return Ok(());
    }

    // For time-based modes, sunset and sunrise should be present (defaults were set in apply_defaults_and_validate_fields)
    let sunset_str = config.sunset.as_deref().unwrap_or(DEFAULT_SUNSET);
    let sunrise_str = config.sunrise.as_deref().unwrap_or(DEFAULT_SUNRISE);

    let sunset =
        NaiveTime::parse_from_str(sunset_str, "%H:%M:%S").context("Invalid sunset time format")?;
    let sunrise = NaiveTime::parse_from_str(sunrise_str, "%H:%M:%S")
        .context("Invalid sunrise time format")?;

    let transition_duration_mins = config
        .transition_duration
        .unwrap_or(DEFAULT_TRANSITION_DURATION);
    let update_interval_secs = config.update_interval.unwrap_or(DEFAULT_UPDATE_INTERVAL);

    // Validate transition duration (hard limits)
    if !(MINIMUM_TRANSITION_DURATION..=MAXIMUM_TRANSITION_DURATION)
        .contains(&transition_duration_mins)
    {
        anyhow::bail!(
            "transition_duration ({} minutes) must be between {} and {} minutes",
            transition_duration_mins,
            MINIMUM_TRANSITION_DURATION,
            MAXIMUM_TRANSITION_DURATION
        );
    }

    // 0. Validate basic ranges for temperature and gamma (hard limits)
    if let Some(temp) = config.night_temp
        && !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp)
    {
        anyhow::bail!(
            "night_temp ({}) must be between {} and {} Kelvin",
            temp,
            MINIMUM_TEMP,
            MAXIMUM_TEMP
        );
    }

    if let Some(temp) = config.day_temp
        && !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp)
    {
        anyhow::bail!(
            "day_temp ({}) must be between {} and {} Kelvin",
            temp,
            MINIMUM_TEMP,
            MAXIMUM_TEMP
        );
    }

    if let Some(gamma) = config.night_gamma
        && !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma)
    {
        anyhow::bail!(
            "night_gamma ({}%) must be between {}% and {}%",
            gamma,
            MINIMUM_GAMMA,
            MAXIMUM_GAMMA
        );
    }

    if let Some(gamma) = config.day_gamma
        && !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma)
    {
        anyhow::bail!(
            "day_gamma ({}%) must be between {}% and {}%",
            gamma,
            MINIMUM_GAMMA,
            MAXIMUM_GAMMA
        );
    }

    // 1. Check for identical sunset/sunrise times
    if sunset == sunrise {
        anyhow::bail!(
            "Sunset and sunrise cannot be the same time ({:?}). \
            There must be a distinction between day and night periods.",
            sunset
        );
    }

    // 2. Calculate time periods and check minimums
    let (day_duration_mins, night_duration_mins) = calculate_day_night_durations(sunset, sunrise);

    if day_duration_mins < 60 {
        anyhow::bail!(
            "Day period is too short ({} minutes). \
            Day period must be at least 1 hour. \
            Adjust sunset ({:?}) or sunrise ({:?}) times.",
            day_duration_mins,
            sunset,
            sunrise
        );
    }

    if night_duration_mins < 60 {
        anyhow::bail!(
            "Night period is too short ({} minutes). \
            Night period must be at least 1 hour. \
            Adjust sunset ({:?}) or sunrise ({:?}) times.",
            night_duration_mins,
            sunset,
            sunrise
        );
    }

    // 3. Check that transitions fit within their periods
    validate_transitions_fit_periods(sunset, sunrise, transition_duration_mins, mode)?;

    // 4. Check for transition overlaps
    validate_no_transition_overlaps(sunset, sunrise, transition_duration_mins, mode)?;

    // 5. Validate update interval vs transition duration (must come before range check)
    let transition_duration_secs = transition_duration_mins * 60;
    if update_interval_secs > transition_duration_secs {
        anyhow::bail!(
            "update_interval ({} seconds) is longer than transition_duration ({} seconds). \
            update_interval should be shorter to allow smooth transitions. \
            Reduce update_interval or increase transition_duration.",
            update_interval_secs,
            transition_duration_secs
        );
    }

    // 6. Update interval range check (hard limits)
    if !(MINIMUM_UPDATE_INTERVAL..=MAXIMUM_UPDATE_INTERVAL).contains(&update_interval_secs) {
        anyhow::bail!(
            "update_interval ({} seconds) must be between {} and {} seconds",
            update_interval_secs,
            MINIMUM_UPDATE_INTERVAL,
            MAXIMUM_UPDATE_INTERVAL
        );
    }

    // 7. Check for reasonable transition frequency
    if transition_duration_secs < 300 && update_interval_secs < 30 {
        // This would create very frequent updates
        log_warning!(
            "Very short transition duration ({transition_duration_mins} min) with frequent updates ({update_interval_secs} sec) may stress your graphics system."
        );
    }

    Ok(())
}

/// Calculate day and night durations in minutes
pub(crate) fn calculate_day_night_durations(sunset: NaiveTime, sunrise: NaiveTime) -> (u32, u32) {
    let sunset_mins = sunset.hour() * 60 + sunset.minute();
    let sunrise_mins = sunrise.hour() * 60 + sunrise.minute();

    if sunset_mins > sunrise_mins {
        // Normal case: sunset after sunrise in the same day
        let day_duration = sunset_mins - sunrise_mins;
        let night_duration = (24 * 60) - day_duration;
        (day_duration, night_duration)
    } else {
        // Overnight case: sunset before sunrise (next day)
        let night_duration = sunrise_mins - sunset_mins;
        let day_duration = (24 * 60) - night_duration;
        (day_duration, night_duration)
    }
}

/// Validate that transitions fit within their respective day/night periods
pub(crate) fn validate_transitions_fit_periods(
    sunset: NaiveTime,
    sunrise: NaiveTime,
    transition_duration_mins: u64,
    mode: &str,
) -> Result<()> {
    let (day_duration_mins, night_duration_mins) = calculate_day_night_durations(sunset, sunrise);

    // For "center" mode, transition spans both day and night periods
    // For "finish_by" and "start_at", transition should fit within the target period

    match mode {
        "center" => {
            // Transition spans across sunset/sunrise time, so we need room on both sides
            let half_transition = transition_duration_mins / 2;

            // Check if transition would exceed either period
            if half_transition >= day_duration_mins.into()
                || half_transition >= night_duration_mins.into()
            {
                anyhow::bail!(
                    "transition_duration ({} minutes) is too long for 'center' mode. \
                    With centered transitions, half the duration ({} minutes) must fit in both \
                    day period ({} minutes) and night period ({} minutes). \
                    Reduce transition_duration or adjust sunset/sunrise times.",
                    transition_duration_mins,
                    half_transition,
                    day_duration_mins,
                    night_duration_mins
                );
            }
        }
        "finish_by" | "start_at" => {
            // Transitions should reasonably fit within their periods
            let max_reasonable_ratio = 0.8; // 80% of period
            let max_day_transition = (day_duration_mins as f64 * max_reasonable_ratio) as u64;
            let max_night_transition = (night_duration_mins as f64 * max_reasonable_ratio) as u64;

            if transition_duration_mins > max_day_transition {
                log_warning!(
                    "Transition duration ({transition_duration_mins} min) is quite long compared to day period ({day_duration_mins} min). Consider reducing transition_duration for better experience."
                );
            }

            if transition_duration_mins > max_night_transition {
                log_warning!(
                    "Transition duration ({transition_duration_mins} min) is quite long compared to night period ({night_duration_mins} min). Consider reducing transition_duration for better experience."
                );
            }
        }
        _ => {} // Already validated mode earlier
    }

    Ok(())
}

/// Validate that sunset and sunrise transitions don't overlap
pub(crate) fn validate_no_transition_overlaps(
    sunset: NaiveTime,
    sunrise: NaiveTime,
    transition_duration_mins: u64,
    mode: &str,
) -> Result<()> {
    // Calculate transition windows using the same logic as the main code
    let transition_duration = Duration::from_secs(transition_duration_mins * 60);

    let (sunset_start, sunset_end, sunrise_start, sunrise_end) = match mode {
        "center" => {
            let half_transition = transition_duration / 2;
            let half_chrono = chrono::Duration::from_std(half_transition).unwrap();
            (
                sunset - half_chrono,
                sunset + half_chrono,
                sunrise - half_chrono,
                sunrise + half_chrono,
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
            // Default to "finish_by" mode for any unexpected values
            let full_transition = chrono::Duration::from_std(transition_duration).unwrap();
            (
                sunset - full_transition,
                sunset,
                sunrise - full_transition,
                sunrise,
            )
        }
    };

    // Convert to minutes since midnight for easier comparison
    let sunset_start_mins = sunset_start.hour() * 60 + sunset_start.minute();
    let sunset_end_mins = sunset_end.hour() * 60 + sunset_end.minute();
    let sunrise_start_mins = sunrise_start.hour() * 60 + sunrise_start.minute();
    let sunrise_end_mins = sunrise_end.hour() * 60 + sunrise_end.minute();

    // Check for overlaps - this is complex due to potential midnight crossings
    let overlap = check_time_ranges_overlap(
        sunset_start_mins,
        sunset_end_mins,
        sunrise_start_mins,
        sunrise_end_mins,
    );

    if overlap {
        anyhow::bail!(
            "Transition periods overlap! \
            Sunset transition: {:?} → {:?}, Sunrise transition: {:?} → {:?}. \
            \nThis configuration is impossible because transitions would conflict. \
            \nSolutions: \
            \n  1. Reduce transition_duration from {} to {} minutes or less \
            \n  2. Increase time between sunset ({:?}) and sunrise ({:?}) \
            \n  3. Change transition_mode from '{}' to a different mode",
            sunset_start,
            sunset_end,
            sunrise_start,
            sunrise_end,
            transition_duration_mins,
            suggest_max_transition_duration(sunset, sunrise, mode),
            sunset,
            sunrise,
            mode
        );
    }

    Ok(())
}

/// Check if two time ranges overlap, handling midnight crossings
pub(crate) fn check_time_ranges_overlap(
    start1_mins: u32,
    end1_mins: u32,
    start2_mins: u32,
    end2_mins: u32,
) -> bool {
    // Helper function to normalize ranges that cross midnight
    let normalize_range = |start: u32, end: u32| -> Vec<(u32, u32)> {
        if start <= end {
            vec![(start, end)]
        } else {
            // Range crosses midnight, split into two ranges
            vec![(start, 24 * 60), (0, end)]
        }
    };

    let range1 = normalize_range(start1_mins, end1_mins);
    let range2 = normalize_range(start2_mins, end2_mins);

    // Check if any segment from range1 overlaps with any segment from range2
    for (r1_start, r1_end) in &range1 {
        for (r2_start, r2_end) in &range2 {
            if r1_start < r2_end && r2_start < r1_end {
                return true; // Overlap detected
            }
        }
    }

    false
}

/// Validate smooth transition duration with proper range checking
pub(crate) fn validate_smooth_transition_duration(
    duration_seconds: f64,
    field_name: &str,
) -> Result<()> {
    if !(MINIMUM_SMOOTH_TRANSITION_DURATION..=MAXIMUM_SMOOTH_TRANSITION_DURATION)
        .contains(&duration_seconds)
    {
        anyhow::bail!(
            "{} ({} seconds) must be between {} and {} seconds",
            field_name,
            duration_seconds,
            MINIMUM_SMOOTH_TRANSITION_DURATION,
            MAXIMUM_SMOOTH_TRANSITION_DURATION
        );
    }
    Ok(())
}

/// Suggest a maximum safe transition duration for the given configuration
pub(crate) fn suggest_max_transition_duration(
    sunset: NaiveTime,
    sunrise: NaiveTime,
    mode: &str,
) -> u64 {
    let (day_duration_mins, night_duration_mins) = calculate_day_night_durations(sunset, sunrise);
    let min_period = day_duration_mins.min(night_duration_mins);

    match mode {
        "center" => {
            // For center mode, half the transition goes in each period
            ((min_period / 2).saturating_sub(1)).into()
        }
        "finish_by" | "start_at" => {
            // For these modes, leave some buffer between transitions
            ((min_period as f64 * 0.8) as u32).into()
        }
        _ => (min_period.saturating_sub(10)).into(),
    }
}
