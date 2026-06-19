use super::*;
use crate::common::constants::{
    DEFAULT_DAY_GAMMA, DEFAULT_DAY_TEMP, DEFAULT_NIGHT_GAMMA, DEFAULT_NIGHT_TEMP,
    DEFAULT_UPDATE_INTERVAL,
};
use crate::core::period::calculations::{
    calculate_progress, calculate_transition_windows, is_time_in_range,
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
        update_interval: Some(crate::config::UpdateInterval::Fixed(
            DEFAULT_UPDATE_INTERVAL,
        )),
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
    let config = create_test_config("23:30:00", "06:00:00", "start_at", 60);
    let (sunset_start, sunset_end, _, _) = calculate_transition_windows(&config);

    assert_eq!(sunset_start, NaiveTime::from_hms_opt(23, 30, 0).unwrap());
    assert_eq!(sunset_end, NaiveTime::from_hms_opt(0, 30, 0).unwrap());
}

#[test]
fn test_midnight_crossing_sunrise() {
    // Sunrise very early, transitioning period starts before midnight
    let config = create_test_config("20:00:00", "00:30:00", "finish_by", 60);
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
    let end = NaiveTime::from_hms_opt(19, 0, 0).unwrap();

    // Test endpoints (should always be 0.0 and 1.0 regardless of smoothstep curve)
    assert_eq!(
        calculate_progress(NaiveTime::from_hms_opt(18, 0, 0).unwrap(), start, end),
        0.0
    );
    assert_eq!(
        calculate_progress(NaiveTime::from_hms_opt(19, 0, 0).unwrap(), start, end),
        1.0
    );

    // Test monotonic increase - progress should always increase with time
    let progress_15 = calculate_progress(NaiveTime::from_hms_opt(18, 15, 0).unwrap(), start, end);
    let progress_30 = calculate_progress(NaiveTime::from_hms_opt(18, 30, 0).unwrap(), start, end);
    let progress_45 = calculate_progress(NaiveTime::from_hms_opt(18, 45, 0).unwrap(), start, end);

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
    let progress_29 = calculate_progress(NaiveTime::from_hms_opt(18, 29, 0).unwrap(), start, end);
    let progress_31 = calculate_progress(NaiveTime::from_hms_opt(18, 31, 0).unwrap(), start, end);
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
        get_stable_period(
            NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
            sunset_end,
            sunrise_start
        ),
        Period::Day
    );

    // Night time
    assert_eq!(
        get_stable_period(
            NaiveTime::from_hms_opt(22, 0, 0).unwrap(),
            sunset_end,
            sunrise_start
        ),
        Period::Night
    );

    // Early morning night
    assert_eq!(
        get_stable_period(
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
    let (_, sunset_end, sunrise_start, _) = calculate_transition_windows(&config);

    // Should be day most of the time
    assert_eq!(
        get_stable_period(
            NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
            sunset_end,
            sunrise_start
        ),
        Period::Day
    );

    // Should be night for the short period
    assert_eq!(
        get_stable_period(
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
    let (_, sunset_end, sunrise_start, _) = calculate_transition_windows(&config);

    // Should be night most of the time
    assert_eq!(
        get_stable_period(
            NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
            sunset_end,
            sunrise_start
        ),
        Period::Night
    );

    // Should be day for the short period
    assert_eq!(
        get_stable_period(
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
        calculate_transition_windows(&config);

    // Test that we get expected transition windows
    assert_eq!(sunset_start, NaiveTime::from_hms_opt(18, 30, 0).unwrap());
    assert_eq!(sunset_end, NaiveTime::from_hms_opt(19, 0, 0).unwrap());
}

#[cfg(test)]
mod static_tests {
    use super::*;
    use crate::config::{Backend, Config};
    use crate::core::runtime_state::RuntimeState;
    use std::time::Duration as StdDuration;

    // Helper function to create a static mode config for testing
    fn create_static_mode_config(temp: u32, gamma: f64) -> Config {
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
            update_interval: Some(crate::config::UpdateInterval::Fixed(60)),
            transition_mode: Some("static".to_string()),
        }
    }

    fn current_period(config: &Config) -> Period {
        crate::core::schedule::Schedule::from_config(config, None)
            .map_or(Period::Static, |schedule| {
                schedule.current_period(crate::time::source::now())
            })
    }

    #[test]
    fn test_static_mode_state_calculation() {
        let config = create_static_mode_config(4000, 85.0);
        let state = current_period(&config);

        assert_eq!(state, Period::Static);

        // Verify that the state returns correct values from config using RuntimeState
        let runtime_state = RuntimeState::new(
            state,
            &config,
            crate::core::schedule::Schedule::from_config(&config, None),
            crate::time::source::now(),
        );
        assert_eq!(runtime_state.temperature(), 4000);
        assert_eq!(runtime_state.gamma(), 85.0);
    }

    #[test]
    fn test_static_mode_no_time_dependence() {
        let config = create_static_mode_config(5000, 95.0);

        // State should be same regardless of time
        let morning_state = current_period(&config);

        // Mock different time - state should be identical
        let evening_state = current_period(&config);

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

        config.static_gamma = Some(200.0);
        assert!(validate_config(&config).is_ok());

        // Test invalid gamma values
        config.static_gamma = Some(9.9); // Below minimum
        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("static_gamma"));

        config.static_gamma = Some(-10.0);
        let result = validate_config(&config);
        assert!(result.is_err());

        config.static_gamma = Some(200.1);
        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("static_gamma"));
    }

    #[test]
    fn test_static_mode_state_properties() {
        let config = create_static_mode_config(4500, 92.0);
        let state = Period::Static;

        // Test state properties
        assert!(!state.is_stable());
        assert!(!state.is_transitioning());
        let runtime_state = RuntimeState::new(
            state,
            &config,
            crate::core::schedule::Schedule::from_config(&config, None),
            crate::time::source::now(),
        );
        assert_eq!(runtime_state.progress(), None);
        assert_eq!(state.display_name(), "Static");
        assert_eq!(state.symbol(), "󰋙 ");

        // Test that next_period returns itself (no transitions in static mode)
        assert_eq!(state.next_period(), Period::Static);

        // Test that values are retrieved correctly
        let runtime_state = RuntimeState::new(
            state,
            &config,
            crate::core::schedule::Schedule::from_config(&config, None),
            crate::time::source::now(),
        );
        assert_eq!(runtime_state.temperature(), 4500);
        assert_eq!(runtime_state.gamma(), 92.0);
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
        let state = current_period(&config);
        assert_eq!(state, Period::Static);
        let runtime_state = RuntimeState::new(
            state,
            &config,
            crate::core::schedule::Schedule::from_config(&config, None),
            crate::time::source::now(),
        );
        assert_eq!(runtime_state.temperature(), 4000);
        assert_eq!(runtime_state.gamma(), 85.0);
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
            let state = current_period(&config);

            assert_eq!(state, Period::Static);
            // Use RuntimeState to test temperature and gamma calculations
            let runtime_state = RuntimeState::new(
                state,
                &config,
                crate::core::schedule::Schedule::from_config(&config, None),
                crate::time::source::now(),
            );
            assert_eq!(runtime_state.temperature(), temp);
            assert_eq!(runtime_state.gamma(), gamma);
        }
    }
}
