use std::time::Duration as StdDuration;
use sunsetr::config::{Backend, Config};
use sunsetr::time_state::{TimeState, get_transition_state, time_until_next_event};

// Helper function to create a static mode config for testing
fn create_static_mode_config(temp: u32, gamma: f32) -> Config {
    Config {
        backend: Some(Backend::Auto),
        smoothing: Some(false),
        startup_duration: Some(10.0),
        shutdown_duration: Some(10.0),
        startup_transition: Some(false),
        startup_transition_duration: Some(10.0),
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

    assert_eq!(state, TimeState::Static);

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
    assert_eq!(morning_state, TimeState::Static);
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
    use sunsetr::config::validation::validate_config;

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
    use sunsetr::config::validation::validate_config;

    // Test valid temperature boundaries
    let mut config = create_static_mode_config(1000, 85.0);
    assert!(validate_config(&config).is_ok());

    config.static_temp = Some(20000);
    assert!(validate_config(&config).is_ok());

    // Test invalid temperatures
    config.static_temp = Some(999);
    let result = validate_config(&config);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Static temperature")
    );

    config.static_temp = Some(20001);
    let result = validate_config(&config);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Static temperature")
    );
}

#[test]
fn test_static_mode_gamma_range_validation() {
    use sunsetr::config::validation::validate_config;

    // Test valid gamma boundaries
    let mut config = create_static_mode_config(4000, 10.0); // Minimum valid gamma
    assert!(validate_config(&config).is_ok());

    config.static_gamma = Some(100.0);
    assert!(validate_config(&config).is_ok());

    // Test invalid gamma values
    config.static_gamma = Some(9.9); // Below minimum
    let result = validate_config(&config);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Static gamma"));

    config.static_gamma = Some(-10.0);
    let result = validate_config(&config);
    assert!(result.is_err());

    config.static_gamma = Some(100.1);
    let result = validate_config(&config);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Static gamma"));
}

#[test]
fn test_static_mode_state_properties() {
    let config = create_static_mode_config(4500, 92.0);
    let state = TimeState::Static;

    // Test state properties
    assert!(state.is_stable());
    assert!(!state.is_transitioning());
    assert_eq!(state.progress(), None);
    assert_eq!(state.display_name(), "Static");
    assert_eq!(state.symbol(), "󰋙 ");

    // Test that next_state returns itself (no transitions in static mode)
    assert_eq!(state.next_state(), TimeState::Static);

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
    assert_eq!(state, TimeState::Static);
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

        assert_eq!(state, TimeState::Static);
        assert_eq!(state.temperature(&config), temp);
        assert_eq!(state.gamma(&config), gamma);
    }
}
