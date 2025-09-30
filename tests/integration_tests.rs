use serial_test::serial;
use std::fs;
use tempfile::tempdir;

use sunsetr::config::{Backend, Config};
use sunsetr::state::period::time_until_next_event;

fn create_test_config_file(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("hypr").join("sunsetr.toml");

    // Create directory structure
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    fs::write(&config_path, content).unwrap();

    (temp_dir, config_path)
}

#[test]
#[serial]
fn test_integration_normal_day_night_cycle() {
    let config_content = r#"
startup_transition = false
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = 3300
day_temp = 6000
night_gamma = 90.0
day_gamma = 100.0
transition_duration = 30
update_interval = 60
transition_mode = "finish_by"
"#;

    let (_temp_dir, config_path) = create_test_config_file(config_content);

    let config = Config::load_from_path(&config_path).unwrap();

    // Test that configuration loads correctly
    assert_eq!(config.sunset, Some("19:00:00".to_string()));
    assert_eq!(config.sunrise, Some("06:00:00".to_string()));
    assert_eq!(config.night_temp, Some(3300));
    assert_eq!(config.day_temp, Some(6000));
    assert_eq!(config.transition_duration, Some(30));
}

#[test]
#[serial]
fn test_integration_extreme_arctic_summer() {
    // Simulate Arctic summer: very short night (22:30 to 02:30 = 4 hours)
    let config_content = r#"
startup_transition = false
sunset = "22:30:00"
sunrise = "02:30:00"
night_temp = 3300
day_temp = 6000
night_gamma = 90.0
day_gamma = 100.0
transition_duration = 60
update_interval = 30
transition_mode = "finish_by"
"#;

    let (_temp_dir, config_path) = create_test_config_file(config_content);
    let config = Config::load_from_path(&config_path).unwrap();

    // This should load successfully despite extreme values
    assert_eq!(config.sunset, Some("22:30:00".to_string()));
    assert_eq!(config.sunrise, Some("02:30:00".to_string()));
}

#[test]
#[serial]
fn test_integration_extreme_arctic_winter() {
    // Simulate Arctic winter: very short day (10:00 to 14:00 = 4 hours)
    let config_content = r#"
startup_transition = false
sunset = "14:00:00"
sunrise = "10:00:00"
night_temp = 2700
day_temp = 5000
night_gamma = 80.0
day_gamma = 100.0
transition_duration = 120
update_interval = 60
transition_mode = "center"
"#;

    let (_temp_dir, config_path) = create_test_config_file(config_content);
    let config = Config::load_from_path(&config_path).unwrap();

    assert_eq!(config.sunset, Some("14:00:00".to_string()));
    assert_eq!(config.sunrise, Some("10:00:00".to_string()));
    assert_eq!(config.transition_mode, Some("center".to_string()));
}

#[test]
#[serial]
fn test_integration_rapid_transitions() {
    // Test very rapid transitions (5 minute transitions, 10 second updates)
    let config_content = r#"
startup_transition = false
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = 3300
day_temp = 6000
night_gamma = 90.0
day_gamma = 100.0
transition_duration = 5
update_interval = 10
transition_mode = "start_at"
"#;

    let (_temp_dir, config_path) = create_test_config_file(config_content);
    let config = Config::load_from_path(&config_path).unwrap();

    assert_eq!(config.transition_duration, Some(5));
    assert_eq!(config.update_interval, Some(10));
}

#[test]
#[serial]
fn test_integration_extreme_temperature_range() {
    // Test extreme but valid temperature range
    let config_content = r#"
startup_transition = false
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = 1000
day_temp = 20000
night_gamma = 50.0
day_gamma = 100.0
transition_duration = 30
update_interval = 60
transition_mode = "finish_by"
"#;

    let (_temp_dir, config_path) = create_test_config_file(config_content);
    let config = Config::load_from_path(&config_path).unwrap();

    assert_eq!(config.night_temp, Some(1000));
    assert_eq!(config.day_temp, Some(20000));
}

#[test]
#[serial]
fn test_integration_midnight_crossing_transitions() {
    // Test transitions that cross midnight
    let config_content = r#"
startup_transition = false
sunset = "23:30:00"
sunrise = "00:30:00"
night_temp = 3300
day_temp = 6000
night_gamma = 90.0
day_gamma = 100.0
transition_duration = 30
update_interval = 60
transition_mode = "center"
"#;

    let (_temp_dir, config_path) = create_test_config_file(config_content);
    let config = Config::load_from_path(&config_path).unwrap();

    // This configuration should load successfully
    assert_eq!(config.sunset, Some("23:30:00".to_string()));
    assert_eq!(config.sunrise, Some("00:30:00".to_string()));
}

#[test]
#[serial]
fn test_integration_config_validation_failures() {
    // Test configurations that should fail validation

    // Test 1: Identical sunset/sunrise times
    let invalid_config = r#"
sunset = "12:00:00"
sunrise = "12:00:00"
"#;

    let (_temp_dir, config_path) = create_test_config_file(invalid_config);
    let result = Config::load_from_path(&config_path);
    assert!(result.is_err());
}

#[test]
#[serial]
fn test_integration_config_validation_extreme_values() {
    // Test configuration with values outside allowed ranges
    let invalid_config = r#"
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = 500
day_temp = 25000
night_gamma = -10.0
day_gamma = 150.0
"#;

    let (_temp_dir, config_path) = create_test_config_file(invalid_config);
    let result = Config::load_from_path(&config_path);
    assert!(result.is_err());
}

#[test]
#[serial]
fn test_integration_smooth_transition_scenarios() {
    // Test smooth transition configurations using new fields
    let config_content = r#"
smoothing = true
startup_duration = 30.5
shutdown_duration = 15.0
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = 3300
day_temp = 6000
night_gamma = 90.0
day_gamma = 100.0
transition_duration = 45
update_interval = 60
transition_mode = "finish_by"
"#;

    let (_temp_dir, config_path) = create_test_config_file(config_content);
    let config = Config::load_from_path(&config_path).unwrap();

    assert_eq!(config.smoothing, Some(true));
    assert_eq!(config.startup_duration, Some(30.5));
    assert_eq!(config.shutdown_duration, Some(15.0));
}

#[test]
#[serial]
fn test_integration_malformed_config_recovery() {
    // Test behavior with malformed TOML
    let malformed_config = r#"
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = "not_a_number"
transition_duration = [1, 2, 3]  # Array instead of number
"#;

    let (_temp_dir, config_path) = create_test_config_file(malformed_config);
    let result = Config::load_from_path(&config_path);
    assert!(result.is_err());
}

#[test]
#[serial]
fn test_integration_backend_configurations() {
    // Test different backend configurations
    let backends = ["auto", "hyprland", "hyprsunset", "wayland"];

    for backend in &backends {
        let config_content = format!(
            r#"
backend = "{}"
transition_mode = "finish_by"
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = 3300
day_temp = 6500
"#,
            backend
        );

        let (_temp_dir, config_path) = create_test_config_file(&config_content);
        let config = Config::load_from_path(&config_path).unwrap();

        // Verify backend was loaded correctly
        match *backend {
            "auto" => assert_eq!(config.backend, Some(Backend::Auto)),
            "hyprland" => assert_eq!(config.backend, Some(Backend::Hyprland)),
            "hyprsunset" => assert_eq!(config.backend, Some(Backend::Hyprsunset)),
            "wayland" => assert_eq!(config.backend, Some(Backend::Wayland)),
            _ => panic!("Unexpected backend"),
        }
    }
}

#[test]
#[serial]
fn test_integration_static_mode_configuration() {
    // Test static mode configuration
    let config_content = r#"
backend = "auto"
transition_mode = "static"
static_temp = 4500
static_gamma = 85.0
# These should be ignored in static mode
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = 3300
day_temp = 6500
latitude = 40.0
longitude = -74.0
"#;

    let (_temp_dir, config_path) = create_test_config_file(config_content);
    let config = Config::load_from_path(&config_path).unwrap();

    assert_eq!(config.transition_mode, Some("static".to_string()));
    assert_eq!(config.static_temp, Some(4500));
    assert_eq!(config.static_gamma, Some(85.0));
}

#[test]
#[serial]
fn test_integration_geo_mode_configuration() {
    // Test geo mode configuration
    let config_content = r#"
backend = "auto"
transition_mode = "geo"
latitude = 51.5074
longitude = -0.1278
night_temp = 3300
day_temp = 6500
night_gamma = 90.0
day_gamma = 100.0
transition_duration = 45
update_interval = 60
# These should be ignored in geo mode
sunset = "19:00:00"
sunrise = "06:00:00"
static_temp = 5000
static_gamma = 95.0
"#;

    let (_temp_dir, config_path) = create_test_config_file(config_content);
    let config = Config::load_from_path(&config_path).unwrap();

    assert_eq!(config.transition_mode, Some("geo".to_string()));
    assert_eq!(config.latitude, Some(51.5074));
    assert_eq!(config.longitude, Some(-0.1278));
    assert!(config.night_temp.is_some());
    assert!(config.day_temp.is_some());
}

#[test]
#[serial]
fn test_integration_smoothing_configuration() {
    // Test new smoothing fields
    let config_content = r#"
backend = "auto"
transition_mode = "finish_by"
smoothing = true
startup_duration = 0.5
shutdown_duration = 1.5
adaptive_interval = 5
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = 3300
day_temp = 6500
"#;

    let (_temp_dir, config_path) = create_test_config_file(config_content);
    let config = Config::load_from_path(&config_path).unwrap();

    assert_eq!(config.smoothing, Some(true));
    assert_eq!(config.startup_duration, Some(0.5));
    assert_eq!(config.shutdown_duration, Some(1.5));
    assert_eq!(config.adaptive_interval, Some(5));
}

#[test]
#[serial]
fn test_integration_legacy_field_migration() {
    // Test that legacy fields are properly migrated to new ones
    let config_content = r#"
backend = "auto"
transition_mode = "finish_by"
startup_transition = true
startup_transition_duration = 2.0
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = 3300
day_temp = 6500
"#;

    let (_temp_dir, config_path) = create_test_config_file(config_content);
    let mut config = Config::load_from_path(&config_path).unwrap();

    // Before migration
    assert_eq!(config.startup_transition, Some(true));
    assert_eq!(config.startup_transition_duration, Some(2.0));

    // After migration
    config.migrate_legacy_fields();
    assert_eq!(config.smoothing, Some(true));
    assert_eq!(config.startup_duration, Some(2.0));
    assert_eq!(config.shutdown_duration, Some(2.0)); // Should match startup_duration
}

#[test]
#[serial]
fn test_integration_extreme_latitude_capping() {
    // Test that extreme latitudes are capped at ±65 degrees
    let config_content = r#"
backend = "auto"
transition_mode = "geo"
latitude = 85.0  # Arctic - will be capped at 65
longitude = 0.0
night_temp = 3300
day_temp = 6500
"#;

    let (_temp_dir, config_path) = create_test_config_file(config_content);
    let config = Config::load_from_path(&config_path).unwrap();

    // Config should load successfully with latitude capped at 65
    assert_eq!(config.latitude, Some(65.0));

    // Test negative extreme latitude
    let config_content = r#"
backend = "auto"
transition_mode = "geo"
latitude = -75.0  # Antarctic - will be capped at -65
longitude = 0.0
night_temp = 3300
day_temp = 6500
"#;

    let (_temp_dir, config_path) = create_test_config_file(config_content);
    let config = Config::load_from_path(&config_path).unwrap();

    // Config should load successfully with latitude capped at -65
    assert_eq!(config.latitude, Some(-65.0));
}

#[test]
#[serial]
fn test_integration_default_config_generation() {
    // Test default config generation when no config exists
    let temp_dir = tempdir().unwrap();

    // Save and restore XDG_CONFIG_HOME
    let original = std::env::var("XDG_CONFIG_HOME").ok();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
    }

    let config = Config::load().unwrap();

    // Restore original
    unsafe {
        match original {
            Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    // Should create default config and load it successfully
    assert!(config.sunset.is_some());
    assert!(config.sunrise.is_some());
    assert!(config.night_temp.is_some());
    assert!(config.day_temp.is_some());

    // Check that config file was created
    let config_path = temp_dir.path().join("sunsetr").join("sunsetr.toml");
    assert!(config_path.exists());
}

#[test]
fn test_integration_time_state_calculation_scenarios() {
    // Test time state calculations with various extreme scenarios
    // These don't require file I/O so no serial annotation needed

    use sunsetr::config::Config;

    fn create_config(sunset: &str, sunrise: &str, mode: &str, duration: u64) -> Config {
        Config {
            backend: Some(sunsetr::config::Backend::Auto),
            smoothing: Some(false),
            startup_duration: Some(10.0),
            shutdown_duration: Some(10.0),
            startup_transition: None, // Deprecated field - not needed
            startup_transition_duration: None, // Deprecated field - not needed
            start_hyprsunset: None,
            adaptive_interval: None,
            latitude: None,
            longitude: None,
            sunset: Some(sunset.to_string()),
            sunrise: Some(sunrise.to_string()),
            night_temp: Some(3300),
            day_temp: Some(6000),
            night_gamma: Some(90.0),
            day_gamma: Some(100.0),
            static_temp: None,
            static_gamma: None,
            transition_duration: Some(duration),
            update_interval: Some(60),
            transition_mode: Some(mode.to_string()),
        }
    }

    // Test normal configuration
    let normal_config = create_config("19:00:00", "06:00:00", "finish_by", 30);
    let next_event_duration = time_until_next_event(&normal_config, None);
    assert!(next_event_duration.as_secs() > 0);

    // Test midnight crossing configuration
    let midnight_config = create_config("23:30:00", "00:30:00", "center", 60);
    let next_event_duration = time_until_next_event(&midnight_config, None);
    assert!(next_event_duration.as_secs() > 0);

    // Test extreme short day configuration
    let short_day_config = create_config("02:00:00", "22:00:00", "start_at", 30);
    let next_event_duration = time_until_next_event(&short_day_config, None);
    assert!(next_event_duration.as_secs() > 0);
}

#[test]
#[serial]
fn test_integration_performance_stress_config() {
    // Test configuration that would stress the system
    let stress_config_content = r#"
smoothing = true
startup_duration = 60.0
shutdown_duration = 60.0
adaptive_interval = 1
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = 3300
day_temp = 6000
night_gamma = 90.0
day_gamma = 100.0
transition_duration = 120
update_interval = 10
transition_mode = "center"
"#;

    let (_temp_dir, config_path) = create_test_config_file(stress_config_content);
    let config = Config::load_from_path(&config_path).unwrap();

    // This should load but might generate warnings
    assert_eq!(config.transition_duration, Some(120));
    assert_eq!(config.update_interval, Some(10));
    assert_eq!(config.smoothing, Some(true));
    assert_eq!(config.adaptive_interval, Some(1));
}

#[test]
#[serial]
fn test_integration_config_conflict_detection() {
    // Test that having configs in both locations produces an error
    let temp_dir = tempdir().unwrap();

    // Create config in old location
    let old_config_path = temp_dir.path().join("hypr").join("sunsetr.toml");
    fs::create_dir_all(old_config_path.parent().unwrap()).unwrap();
    fs::write(
        &old_config_path,
        r#"
sunset = "19:00:00"
sunrise = "06:00:00"
"#,
    )
    .unwrap();

    // Create config in new location
    let new_config_path = temp_dir.path().join("sunsetr").join("sunsetr.toml");
    fs::create_dir_all(new_config_path.parent().unwrap()).unwrap();
    fs::write(
        &new_config_path,
        r#"
sunset = "20:00:00"
sunrise = "07:00:00"
"#,
    )
    .unwrap();

    // Save and restore XDG_CONFIG_HOME
    let original = std::env::var("XDG_CONFIG_HOME").ok();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
    }

    let result = Config::load();

    // Restore original
    unsafe {
        match original {
            Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    // Assert the specific error message for testing-support mode
    assert!(
        error_msg.contains("TEST_MODE_CONFLICT"),
        "Error message did not contain TEST_MODE_CONFLICT. Actual: {error_msg}"
    );
    assert!(
        error_msg.contains("sunsetr/sunsetr.toml"),
        "Error message did not contain new path. Actual: {error_msg}"
    );
    assert!(
        error_msg.contains("hypr/sunsetr.toml"),
        "Error message did not contain old path. Actual: {error_msg}"
    );
}

// Property-based testing for configurations
#[cfg(test)]
mod property_tests {
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_config_time_format_parsing(
            hour in 0u32..24,
            minute in 0u32..60,
            second in 0u32..60
        ) {
            use chrono::NaiveTime;
            let time_str = format!("{hour:02}:{minute:02}:{second:02}");
            let result = NaiveTime::parse_from_str(&time_str, "%H:%M:%S");
            prop_assert!(result.is_ok());
        }

        #[test]
        fn test_temperature_interpolation_bounds(
            temp1 in 1000u32..20000,
            temp2 in 1000u32..20000,
            progress in 0.0f32..1.0
        ) {
            use sunsetr::utils::interpolate_u32;
            let result = interpolate_u32(temp1, temp2, progress);
            let min_temp = temp1.min(temp2);
            let max_temp = temp1.max(temp2);
            prop_assert!(result >= min_temp && result <= max_temp);
        }

        #[test]
        fn test_gamma_interpolation_bounds(
            gamma1 in 0.0f32..100.0,
            gamma2 in 0.0f32..100.0,
            progress in 0.0f32..1.0
        ) {
            use sunsetr::utils::interpolate_f32;
            let result = interpolate_f32(gamma1, gamma2, progress);
            let min_gamma = gamma1.min(gamma2);
            let max_gamma = gamma1.max(gamma2);
            prop_assert!(result >= min_gamma && result <= max_gamma);
        }
    }
}
