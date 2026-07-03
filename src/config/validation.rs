//! Reject impossible configurations: out-of-range values, overlapping transitions, and periods
//! too short to stay distinct.

use anyhow::{Context, Result};
use chrono::{NaiveTime, Timelike};
use std::time::Duration;

use super::{RawConfig, TransitionMode};
use crate::common::constants::*;

fn validate_basic_ranges(config: &RawConfig) -> Result<()> {
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

    if let Some(duration_minutes) = config.transition_duration
        && !(MINIMUM_TRANSITION_DURATION_MIN..=MAXIMUM_TRANSITION_DURATION_MIN)
            .contains(&duration_minutes)
    {
        anyhow::bail!(
            "transition_duration ({} minutes) must be between {} and {} minutes",
            duration_minutes,
            MINIMUM_TRANSITION_DURATION_MIN,
            MAXIMUM_TRANSITION_DURATION_MIN
        );
    }

    // Must run before the range check below to match test expectations.
    if let Some(crate::config::UpdateInterval::Fixed(update_interval_secs)) = config.update_interval
    {
        if let Some(transition_duration_mins) = config.transition_duration {
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
        }

        if !(MINIMUM_UPDATE_INTERVAL_SEC..=MAXIMUM_UPDATE_INTERVAL_SEC)
            .contains(&update_interval_secs)
        {
            anyhow::bail!(
                "update_interval ({} seconds) must be between {} and {} seconds",
                update_interval_secs,
                MINIMUM_UPDATE_INTERVAL_SEC,
                MAXIMUM_UPDATE_INTERVAL_SEC
            );
        }
    }

    if let Some(startup_duration_secs) = config.startup_duration {
        validate_smooth_transition_duration(startup_duration_secs, "startup_duration")?;
    }

    if let Some(shutdown_duration_secs) = config.shutdown_duration {
        validate_smooth_transition_duration(shutdown_duration_secs, "shutdown_duration")?;
    }

    if let Some(startup_duration_secs) = config.startup_transition_duration {
        validate_smooth_transition_duration(
            startup_duration_secs,
            "startup_transition_duration (deprecated)",
        )?;
    }

    if let Some(interval_ms) = config.adaptive_interval
        && !(MINIMUM_ADAPTIVE_INTERVAL_MS..=MAXIMUM_ADAPTIVE_INTERVAL_MS).contains(&interval_ms)
    {
        anyhow::bail!(
            "adaptive_interval ({} ms) must be between {} and {} milliseconds",
            interval_ms,
            MINIMUM_ADAPTIVE_INTERVAL_MS,
            MAXIMUM_ADAPTIVE_INTERVAL_MS
        );
    }

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

    Ok(())
}

pub fn validate_config(config: &RawConfig) -> Result<()> {
    let mode = config.transition_mode;

    validate_basic_ranges(config)?;

    if mode == TransitionMode::Static {
        if config.static_temp.is_none() {
            anyhow::bail!("Static mode requires static_temp to be specified");
        }
        if config.static_gamma.is_none() {
            anyhow::bail!("Static mode requires static_gamma to be specified");
        }

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

        return Ok(());
    }

    let sunset_str = config.sunset.as_deref().unwrap_or(DEFAULT_SUNSET);
    let sunrise_str = config.sunrise.as_deref().unwrap_or(DEFAULT_SUNRISE);

    let sunset =
        NaiveTime::parse_from_str(sunset_str, "%H:%M:%S").context("Invalid sunset time format")?;
    let sunrise = NaiveTime::parse_from_str(sunrise_str, "%H:%M:%S")
        .context("Invalid sunrise time format")?;

    let transition_duration_mins = config
        .transition_duration
        .unwrap_or(DEFAULT_TRANSITION_DURATION_MIN);
    let update_interval_secs = match config.update_interval {
        Some(crate::config::UpdateInterval::Fixed(secs)) => Some(secs),
        _ => None,
    };

    if sunset == sunrise {
        anyhow::bail!(
            "Sunset and sunrise cannot be the same time ({:?}). \
            There must be a distinction between day and night periods.",
            sunset
        );
    }

    let (day_duration_secs, night_duration_secs) = calculate_day_night_durations(sunset, sunrise);

    if day_duration_secs < 3600 {
        anyhow::bail!(
            "Day period is too short ({}). \
            Day period must be at least 1 hour. \
            Adjust sunset ({:?}) or sunrise ({:?}) times.",
            format_duration_secs(day_duration_secs.into()),
            sunset,
            sunrise
        );
    }

    if night_duration_secs < 3600 {
        anyhow::bail!(
            "Night period is too short ({}). \
            Night period must be at least 1 hour. \
            Adjust sunset ({:?}) or sunrise ({:?}) times.",
            format_duration_secs(night_duration_secs.into()),
            sunset,
            sunrise
        );
    }

    validate_transitions_fit_periods(sunset, sunrise, transition_duration_mins, mode)?;
    validate_no_transition_overlaps(sunset, sunrise, transition_duration_mins, mode)?;

    let transition_duration_secs = transition_duration_mins * 60;
    if let Some(interval_secs) = update_interval_secs
        && transition_duration_secs < 300
        && interval_secs < 30
    {
        log_warning!(
            "Very short transition duration ({transition_duration_mins} min) with frequent updates ({interval_secs} sec) may stress your graphics system."
        );
    }

    Ok(())
}

/// Day and night durations in seconds, in that order.
pub(crate) fn calculate_day_night_durations(sunset: NaiveTime, sunrise: NaiveTime) -> (u32, u32) {
    let sunset_secs = sunset.num_seconds_from_midnight();
    let sunrise_secs = sunrise.num_seconds_from_midnight();

    if sunset_secs > sunrise_secs {
        let day_duration = sunset_secs - sunrise_secs;
        let night_duration = (24 * 3600) - day_duration;
        (day_duration, night_duration)
    } else {
        let night_duration = sunrise_secs - sunset_secs;
        let day_duration = (24 * 3600) - night_duration;
        (day_duration, night_duration)
    }
}

/// Render a duration for validation messages, whole minutes when the seconds
/// component is zero.
fn format_duration_secs(secs: u64) -> String {
    let mins = secs / 60;
    let secs = secs % 60;
    if secs == 0 {
        format!("{mins} minutes")
    } else {
        format!("{mins} minutes {secs} seconds")
    }
}

/// Validate that a center-mode transition fits within both day and night periods.
pub(crate) fn validate_transitions_fit_periods(
    sunset: NaiveTime,
    sunrise: NaiveTime,
    transition_duration_mins: u64,
    mode: TransitionMode,
) -> Result<()> {
    if mode == TransitionMode::Center {
        let (day_duration_secs, night_duration_secs) =
            calculate_day_night_durations(sunset, sunrise);
        let half_transition_secs = transition_duration_mins * 60 / 2;

        if half_transition_secs >= day_duration_secs.into()
            || half_transition_secs >= night_duration_secs.into()
        {
            anyhow::bail!(
                "transition_duration ({} minutes) is too long for 'center' mode. \
                With centered transitions, half the duration ({}) must fit in both \
                day period ({}) and night period ({}). \
                Reduce transition_duration or adjust sunset/sunrise times.",
                transition_duration_mins,
                format_duration_secs(half_transition_secs),
                format_duration_secs(day_duration_secs.into()),
                format_duration_secs(night_duration_secs.into())
            );
        }
    }

    Ok(())
}

/// Reject transition windows that overlap or leave no stable day or night period between them.
pub(crate) fn validate_no_transition_overlaps(
    sunset: NaiveTime,
    sunrise: NaiveTime,
    transition_duration_mins: u64,
    mode: TransitionMode,
) -> Result<()> {
    let transition_duration = Duration::from_secs(transition_duration_mins * 60);

    let (sunset_start, sunset_end, sunrise_start, sunrise_end) = match mode {
        TransitionMode::Center => {
            let half_transition = transition_duration / 2;
            let half_chrono = chrono::Duration::from_std(half_transition).unwrap();
            (
                sunset - half_chrono,
                sunset + half_chrono,
                sunrise - half_chrono,
                sunrise + half_chrono,
            )
        }
        TransitionMode::StartAt => {
            let full_transition = chrono::Duration::from_std(transition_duration).unwrap();
            (
                sunset,
                sunset + full_transition,
                sunrise,
                sunrise + full_transition,
            )
        }
        TransitionMode::FinishBy => {
            let full_transition = chrono::Duration::from_std(transition_duration).unwrap();
            (
                sunset - full_transition,
                sunset,
                sunrise - full_transition,
                sunrise,
            )
        }
        _ => {
            let full_transition = chrono::Duration::from_std(transition_duration).unwrap();
            (
                sunset - full_transition,
                sunset,
                sunrise - full_transition,
                sunrise,
            )
        }
    };

    let sunset_start_secs = sunset_start.num_seconds_from_midnight();
    let sunset_end_secs = sunset_end.num_seconds_from_midnight();
    let sunrise_start_secs = sunrise_start.num_seconds_from_midnight();
    let sunrise_end_secs = sunrise_end.num_seconds_from_midnight();

    let overlap = check_time_ranges_overlap(
        sunset_start_secs,
        sunset_end_secs,
        sunrise_start_secs,
        sunrise_end_secs,
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

    let stable_night_secs = (sunrise_start_secs + 24 * 3600 - sunset_end_secs) % (24 * 3600);
    let stable_day_secs = (sunset_start_secs + 24 * 3600 - sunrise_end_secs) % (24 * 3600);
    if stable_night_secs == 0 || stable_day_secs == 0 {
        let collapsed = if stable_night_secs == 0 {
            "night"
        } else {
            "day"
        };
        anyhow::bail!(
            "Transitions leave no stable {collapsed} period. \
            \nThe sunset and sunrise transitions meet with no {collapsed} between them. \
            \nUse static mode for a constant setting, or reduce transition_duration ({transition_duration_mins} min)."
        );
    }

    Ok(())
}

/// Whether two second-of-day ranges overlap, accounting for ranges that wrap past midnight.
pub(crate) fn check_time_ranges_overlap(
    start1_secs: u32,
    end1_secs: u32,
    start2_secs: u32,
    end2_secs: u32,
) -> bool {
    let normalize_range = |start: u32, end: u32| -> Vec<(u32, u32)> {
        if start <= end {
            vec![(start, end)]
        } else {
            vec![(start, 24 * 3600), (0, end)]
        }
    };

    let range1 = normalize_range(start1_secs, end1_secs);
    let range2 = normalize_range(start2_secs, end2_secs);

    for (r1_start, r1_end) in &range1 {
        for (r2_start, r2_end) in &range2 {
            if r1_start < r2_end && r2_start < r1_end {
                return true;
            }
        }
    }

    false
}

pub(crate) fn validate_smooth_transition_duration(
    duration_seconds: f64,
    field_name: &str,
) -> Result<()> {
    if !(MINIMUM_SMOOTH_TRANSITION_DURATION_SEC..=MAXIMUM_SMOOTH_TRANSITION_DURATION_SEC)
        .contains(&duration_seconds)
    {
        anyhow::bail!(
            "{} ({} seconds) must be between {} and {} seconds",
            field_name,
            duration_seconds,
            MINIMUM_SMOOTH_TRANSITION_DURATION_SEC,
            MAXIMUM_SMOOTH_TRANSITION_DURATION_SEC
        );
    }
    Ok(())
}

/// Maximum safe transition duration in minutes for the given mode and sun times.
pub(crate) fn suggest_max_transition_duration(
    sunset: NaiveTime,
    sunrise: NaiveTime,
    mode: TransitionMode,
) -> u64 {
    let (day_duration_secs, night_duration_secs) = calculate_day_night_durations(sunset, sunrise);
    let min_period = day_duration_secs.min(night_duration_secs) / 60;

    match mode {
        TransitionMode::Center => ((min_period / 2).saturating_sub(1)).into(),
        TransitionMode::FinishBy | TransitionMode::StartAt => {
            ((min_period as f64 * 0.8) as u32).into()
        }
        _ => (min_period.saturating_sub(10)).into(),
    }
}
