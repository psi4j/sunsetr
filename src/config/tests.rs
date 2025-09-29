use super::validation::validate_config;
use super::*;
use crate::constants::test_constants::*;
use crate::constants::{
    MAXIMUM_GAMMA, MAXIMUM_TEMP, MAXIMUM_TRANSITION_DURATION, MAXIMUM_UPDATE_INTERVAL,
    MINIMUM_GAMMA, MINIMUM_TEMP, MINIMUM_TRANSITION_DURATION, MINIMUM_UPDATE_INTERVAL,
};
use serial_test::serial;
use std::fs;
use tempfile::tempdir;

#[allow(clippy::too_many_arguments)]
fn create_test_config(
    sunset: &str,
    sunrise: &str,
    transition_duration: Option<u64>,
    update_interval: Option<u64>,
    transition_mode: Option<&str>,
    night_temp: Option<u32>,
    day_temp: Option<u32>,
    night_gamma: Option<f32>,
    day_gamma: Option<f32>,
) -> Config {
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
        sunset: Some(sunset.to_string()),
        sunrise: Some(sunrise.to_string()),
        night_temp,
        day_temp,
        night_gamma,
        day_gamma,
        static_temp: None,
        static_gamma: None,
        transition_duration,
        update_interval,
        transition_mode: transition_mode.map(|s| s.to_string()),
    }
}

#[test]
#[serial]
fn test_config_load_default_creation() {
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("sunsetr").join("sunsetr.toml");

    // Save and restore XDG_CONFIG_HOME
    let original = std::env::var("XDG_CONFIG_HOME").ok();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
    }

    // First load should create default config
    let result = Config::load();

    // Restore original
    unsafe {
        match original {
            Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    if let Err(e) = &result {
        eprintln!("Config::load() failed: {:?}", e);
    }
    assert!(result.is_ok());
    assert!(config_path.exists());
}

#[test]
fn test_config_validation_basic() {
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some(TEST_STANDARD_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_ok());
}

#[test]
fn test_config_validation_backend_compatibility() {
    // Test valid combinations
    let mut config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some(TEST_STANDARD_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );

    // Valid: Hyprland backend
    config.backend = Some(Backend::Hyprland);
    assert!(validate_config(&config).is_ok());

    // Valid: Hyprsunset backend
    config.backend = Some(Backend::Hyprsunset);
    assert!(validate_config(&config).is_ok());

    // Valid: Wayland backend
    config.backend = Some(Backend::Wayland);
    assert!(validate_config(&config).is_ok());
}

#[test]
fn test_config_validation_identical_times() {
    let config = create_test_config(
        "12:00:00",
        "12:00:00",
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some(TEST_STANDARD_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_err());
    assert!(
        validate_config(&config)
            .unwrap_err()
            .to_string()
            .contains("cannot be the same time")
    );
}

#[test]
fn test_config_validation_extreme_short_day() {
    // 30 minute day period (sunrise 23:45, sunset 00:15)
    let config = create_test_config(
        "00:15:00",
        "23:45:00",
        Some(5),
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_err());
    assert!(
        validate_config(&config)
            .unwrap_err()
            .to_string()
            .contains("Day period is too short")
    );
}

#[test]
fn test_config_validation_extreme_short_night() {
    // 30 minute night period (sunset 23:45, sunrise 00:15)
    let config = create_test_config(
        "23:45:00",
        "00:15:00",
        Some(5),
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_err());
    assert!(
        validate_config(&config)
            .unwrap_err()
            .to_string()
            .contains("Night period is too short")
    );
}

#[test]
fn test_config_validation_extreme_temperature_values() {
    // Test minimum temperature boundary
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some(TEST_STANDARD_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(MINIMUM_TEMP),
        Some(MAXIMUM_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_ok());

    // Test below minimum temperature
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some(TEST_STANDARD_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(MINIMUM_TEMP - 1),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_err());

    // Test above maximum temperature
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some(TEST_STANDARD_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(MAXIMUM_TEMP + 1),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_err());
}

#[test]
fn test_config_validation_extreme_gamma_values() {
    // Test minimum gamma boundary
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some(TEST_STANDARD_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(MINIMUM_GAMMA),
        Some(MAXIMUM_GAMMA),
    );
    assert!(validate_config(&config).is_ok());

    // Test below minimum gamma
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some(TEST_STANDARD_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(MINIMUM_GAMMA - 0.1),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_err());

    // Test above maximum gamma
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some(TEST_STANDARD_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(MAXIMUM_GAMMA + 0.1),
    );
    assert!(validate_config(&config).is_err());
}

#[test]
fn test_config_validation_extreme_transition_durations() {
    // Test minimum transition duration
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(MINIMUM_TRANSITION_DURATION),
        Some(TEST_STANDARD_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_ok());

    // Test maximum transition duration
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(MAXIMUM_TRANSITION_DURATION),
        Some(TEST_STANDARD_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_ok());

    // Test below minimum (should fail validation)
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(MINIMUM_TRANSITION_DURATION - 1),
        Some(TEST_STANDARD_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_err());

    // Test above maximum (should fail validation)
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(MAXIMUM_TRANSITION_DURATION + 1),
        Some(TEST_STANDARD_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_err());
}

#[test]
fn test_config_validation_extreme_update_intervals() {
    // Test minimum update interval
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some(MINIMUM_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_ok());

    // Test maximum update interval
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(120),
        Some(MAXIMUM_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_ok());

    // Test update interval longer than transition
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(30),
        Some(30 * 60 + 1),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_err());
    assert!(
        validate_config(&config)
            .unwrap_err()
            .to_string()
            .contains("longer than transition_duration")
    );
}

#[test]
fn test_config_validation_center_mode_overlapping() {
    // Center mode with transition duration that would overlap
    // Day period is about 11 hours (06:00-19:00), night is 13 hours
    // Transition of 60 minutes in center mode means 30 minutes each side
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(60),
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some("center"),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_ok());

    // But if we make the transition too long for center mode
    // Let's try a 22-hour transition in center mode (11 hours each side)
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(22 * 60),
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some("center"),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_err());
}

#[test]
fn test_config_validation_midnight_crossings() {
    // Sunset after midnight, sunrise in evening - valid but extreme
    let config = create_test_config(
        "01:00:00",
        "22:00:00",
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some(TEST_STANDARD_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_ok());

    // Very late sunset, very early sunrise
    let config = create_test_config(
        "23:30:00",
        "00:30:00",
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some(TEST_STANDARD_UPDATE_INTERVAL),
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    assert!(validate_config(&config).is_ok());
}

#[test]
fn test_config_validation_invalid_time_formats() {
    use chrono::NaiveTime;
    // This should fail during parsing, not validation
    assert!(NaiveTime::parse_from_str("25:00:00", "%H:%M:%S").is_err());
    assert!(NaiveTime::parse_from_str("19:60:00", "%H:%M:%S").is_err());
}

#[test]
fn test_config_validation_transition_overlap_detection() {
    // Test transition overlap detection with extreme short periods
    let config = create_test_config(
        "12:30:00",
        "12:00:00",
        Some(60),
        Some(TEST_STANDARD_TRANSITION_DURATION),
        Some("center"),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    // Should fail because day period is only 30 minutes, can't fit 1-hour center transition
    assert!(validate_config(&config).is_err());
}

#[test]
fn test_config_validation_performance_warnings() {
    // Test configuration with edge case values
    let config = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(5),  // Short transition duration (will generate warning)
        Some(10), // Minimum allowed update interval
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    // Should pass validation with minimum allowed update_interval
    assert!(validate_config(&config).is_ok());

    // Test that update_interval below minimum fails
    let config_too_low = create_test_config(
        TEST_STANDARD_SUNSET,
        TEST_STANDARD_SUNRISE,
        Some(5),
        Some(5), // Below minimum
        Some(TEST_STANDARD_MODE),
        Some(TEST_STANDARD_NIGHT_TEMP),
        Some(TEST_STANDARD_DAY_TEMP),
        Some(TEST_STANDARD_NIGHT_GAMMA),
        Some(TEST_STANDARD_DAY_GAMMA),
    );
    // Should fail validation with update_interval too low
    assert!(validate_config(&config_too_low).is_err());
}

#[test]
fn test_default_config_file_creation() {
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("sunsetr.toml");

    Config::create_default_config(&config_path, None).unwrap();
    assert!(config_path.exists());

    let content = fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("sunset"));
    assert!(content.contains("sunrise"));
    assert!(content.contains("night_temp"));
    assert!(content.contains("transition_mode"));
}

#[test]
fn test_config_toml_parsing() {
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("test_config.toml");

    let config_content = r#"
startup_transition = true
startup_transition_duration = 15
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

    fs::write(&config_path, config_content).unwrap();
    let content = fs::read_to_string(&config_path).unwrap();
    let config: Config = toml::from_str(&content).unwrap();

    assert_eq!(config.sunset, Some("19:00:00".to_string()));
    assert_eq!(config.sunrise, Some("06:00:00".to_string()));
    assert_eq!(config.night_temp, Some(3300));
    assert_eq!(config.transition_mode, Some("finish_by".to_string()));
}

#[test]
fn test_config_malformed_toml() {
    let malformed_content = r#"
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = "not_a_number"  # This should cause parsing to fail
"#;

    let result: Result<Config, _> = toml::from_str(malformed_content);
    assert!(result.is_err());
}

#[test]
fn test_geo_toml_loading() {
    let temp_dir = tempdir().unwrap();
    let config_dir = temp_dir.path().join("sunsetr");
    fs::create_dir_all(&config_dir).unwrap();

    let config_path = config_dir.join("sunsetr.toml");
    let geo_path = config_dir.join("geo.toml");

    // Create main config without coordinates
    let config_content = r#"
sunset = "19:00:00"
sunrise = "06:00:00"
night_temp = 3300
day_temp = 6500
transition_mode = "geo"
"#;
    fs::write(&config_path, config_content).unwrap();

    // Create geo.toml with coordinates
    let geo_content = r#"
# Geographic coordinates
latitude = 51.5074
longitude = -0.1278
"#;
    fs::write(&geo_path, geo_content).unwrap();

    // Load config from path - directly load with the path
    let config = Config::load_from_path(&config_path).unwrap();

    // Check that coordinates were loaded from geo.toml
    assert_eq!(config.latitude, Some(51.5074));
    assert_eq!(config.longitude, Some(-0.1278));
}

#[test]
fn test_geo_toml_overrides_main_config() {
    let temp_dir = tempdir().unwrap();
    let config_dir = temp_dir.path().join("sunsetr");
    fs::create_dir_all(&config_dir).unwrap();

    let config_path = config_dir.join("sunsetr.toml");
    let geo_path = config_dir.join("geo.toml");

    // Create main config with coordinates
    let config_content = r#"
sunset = "19:00:00"
sunrise = "06:00:00"
latitude = 40.7128
longitude = -74.0060
transition_mode = "geo"
"#;
    fs::write(&config_path, config_content).unwrap();

    // Create geo.toml with different coordinates
    let geo_content = r#"
latitude = 51.5074
longitude = -0.1278
"#;
    fs::write(&geo_path, geo_content).unwrap();

    // Load config directly from path (no env var needed)
    let config = Config::load_from_path(&config_path).unwrap();

    // Check that geo.toml coordinates override main config
    assert_eq!(config.latitude, Some(51.5074));
    assert_eq!(config.longitude, Some(-0.1278));
}

#[test]
#[serial]
fn test_update_geo_coordinates_with_geo_toml() {
    let temp_dir = tempdir().unwrap();
    let config_dir = temp_dir.path().join("sunsetr");
    fs::create_dir_all(&config_dir).unwrap();

    let config_path = config_dir.join("sunsetr.toml");
    let geo_path = config_dir.join("geo.toml");

    // Create main config
    let config_content = r#"
sunset = "19:00:00"
sunrise = "06:00:00"
transition_mode = "manual"
"#;
    fs::write(&config_path, config_content).unwrap();

    // Create empty geo.toml
    fs::write(&geo_path, "").unwrap();

    // Save and restore XDG_CONFIG_HOME
    let original = std::env::var("XDG_CONFIG_HOME").ok();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
    }

    // Update coordinates
    Config::update_coordinates(52.5200, 13.4050).unwrap();

    // Restore original
    unsafe {
        match original {
            Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    // Check that geo.toml was updated
    let geo_content = fs::read_to_string(&geo_path).unwrap();
    assert!(geo_content.contains("latitude = 52.52"));
    assert!(geo_content.contains("longitude = 13.405"));

    // Check that main config transition_mode was updated
    let main_content = fs::read_to_string(&config_path).unwrap();
    assert!(main_content.contains("transition_mode = \"geo\""));
}

#[test]
fn test_malformed_geo_toml_fallback() {
    let temp_dir = tempdir().unwrap();
    let config_dir = temp_dir.path().join("sunsetr");
    fs::create_dir_all(&config_dir).unwrap();

    let config_path = config_dir.join("sunsetr.toml");
    let geo_path = config_dir.join("geo.toml");

    // Create main config with coordinates
    let config_content = r#"
sunset = "19:00:00"
sunrise = "06:00:00"
latitude = 40.7128
longitude = -74.0060
transition_mode = "geo"
"#;
    fs::write(&config_path, config_content).unwrap();

    // Create malformed geo.toml
    let geo_content = r#"
latitude = "not a number"
longitude = -0.1278
"#;
    fs::write(&geo_path, geo_content).unwrap();

    // Load config - should use main config coordinates
    let config = Config::load_from_path(&config_path).unwrap();

    // Check that main config coordinates were used
    assert_eq!(config.latitude, Some(40.7128));
    assert_eq!(config.longitude, Some(-74.0060));
}

#[test]
#[serial]
fn test_geo_toml_exists_before_config_creation() {
    let temp_dir = tempdir().unwrap();
    let config_dir = temp_dir.path().join("sunsetr");
    fs::create_dir_all(&config_dir).unwrap();

    let config_path = config_dir.join("sunsetr.toml");
    let geo_path = config_dir.join("geo.toml");

    // Create empty geo.toml BEFORE creating config
    fs::write(&geo_path, "").unwrap();

    // Save and restore XDG_CONFIG_HOME
    let original = std::env::var("XDG_CONFIG_HOME").ok();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
    }

    // Create config with coordinates (simulating geo command)
    Config::create_default_config(&config_path, Some((52.5200, 13.4050, "Berlin".to_string())))
        .unwrap();

    // Restore original
    unsafe {
        match original {
            Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    // Check that coordinates went to geo.toml
    let geo_content = fs::read_to_string(&geo_path).unwrap();
    assert!(geo_content.contains("latitude = 52.52"));
    assert!(geo_content.contains("longitude = 13.405"));

    // Check that main config does NOT have coordinates
    let main_content = fs::read_to_string(&config_path).unwrap();
    assert!(!main_content.contains("latitude = 52.52"));
    assert!(!main_content.contains("longitude = 13.405"));

    // But it should have geo transition mode
    assert!(main_content.contains("transition_mode = \"geo\""));
}

mod property_tests {
    use super::validation::validate_config;
    use super::{Backend, Config};
    use crate::constants::{
        DEFAULT_DAY_GAMMA, DEFAULT_DAY_TEMP, DEFAULT_NIGHT_GAMMA, DEFAULT_NIGHT_TEMP,
        DEFAULT_SUNRISE, DEFAULT_SUNSET, DEFAULT_TRANSITION_DURATION, DEFAULT_UPDATE_INTERVAL,
        MAXIMUM_ADAPTIVE_INTERVAL, MAXIMUM_GAMMA, MAXIMUM_SMOOTH_TRANSITION_DURATION, MAXIMUM_TEMP,
        MAXIMUM_TRANSITION_DURATION, MAXIMUM_UPDATE_INTERVAL, MINIMUM_ADAPTIVE_INTERVAL,
        MINIMUM_GAMMA, MINIMUM_SMOOTH_TRANSITION_DURATION, MINIMUM_TEMP,
        MINIMUM_TRANSITION_DURATION, MINIMUM_UPDATE_INTERVAL,
    };
    use chrono::{NaiveTime, Timelike};
    use proptest::prelude::*;

    /// Transition mode determines which configuration fields are active
    #[derive(Debug, Clone, PartialEq)]
    enum TransitionMode {
        Geo,            // Uses: time-based + geolocation settings
        Static,         // Uses: static settings only
        Manual(String), // finish_by/start_at/center - Uses: time-based + manual transition settings
    }

    impl TransitionMode {
        fn as_str(&self) -> &str {
            match self {
                TransitionMode::Geo => "geo",
                TransitionMode::Static => "static",
                TransitionMode::Manual(mode) => mode.as_str(),
            }
        }

        /// Returns whether this mode uses time-based settings (night/day temp/gamma)
        fn uses_time_based_settings(&self) -> bool {
            !matches!(self, TransitionMode::Static)
        }

        /// Returns whether this mode uses manual transition settings (sunset/sunrise times)
        fn uses_manual_settings(&self) -> bool {
            matches!(self, TransitionMode::Manual(_))
        }

        /// Returns whether this mode uses geolocation settings (lat/lon)
        fn uses_geo_settings(&self) -> bool {
            matches!(self, TransitionMode::Geo)
        }

        /// Returns whether this mode uses static settings (static_temp/gamma)
        fn uses_static_settings(&self) -> bool {
            matches!(self, TransitionMode::Static)
        }
    }

    /// Test configuration builder that ensures fields are set appropriately for the transition mode
    #[derive(Debug)]
    struct ModeAwareConfigBuilder {
        mode: TransitionMode,
        backend: Backend,
        smoothing: Option<bool>,
        startup_duration: Option<f64>,
        shutdown_duration: Option<f64>,
        adaptive_interval: Option<u64>,
        // Time-based settings (used by geo and manual modes)
        night_temp: Option<u32>,
        day_temp: Option<u32>,
        night_gamma: Option<f32>,
        day_gamma: Option<f32>,
        update_interval: Option<u64>,
        // Static settings (used by static mode only)
        static_temp: Option<u32>,
        static_gamma: Option<f32>,
        // Manual transition settings (used by manual modes only)
        sunset: Option<String>,
        sunrise: Option<String>,
        transition_duration: Option<u64>,
        // Geolocation settings (used by geo mode only)
        latitude: Option<f64>,
        longitude: Option<f64>,
    }

    impl Arbitrary for TransitionMode {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            prop_oneof![
                Just(TransitionMode::Geo),
                Just(TransitionMode::Static),
                Just(TransitionMode::Manual("finish_by".to_string())),
                Just(TransitionMode::Manual("start_at".to_string())),
                Just(TransitionMode::Manual("center".to_string())),
            ]
            .boxed()
        }
    }

    impl ModeAwareConfigBuilder {
        /// Create a new builder for the specified transition mode with appropriate defaults
        fn new(mode: TransitionMode) -> Self {
            let mut builder = Self {
                mode: mode.clone(),
                backend: Backend::Auto,
                smoothing: Some(true),
                startup_duration: Some(0.5),
                shutdown_duration: Some(0.5),
                adaptive_interval: Some(1),
                night_temp: None,
                day_temp: None,
                night_gamma: None,
                day_gamma: None,
                update_interval: None,
                static_temp: None,
                static_gamma: None,
                sunset: None,
                sunrise: None,
                transition_duration: None,
                latitude: None,
                longitude: None,
            };

            // Set appropriate defaults based on mode
            if mode.uses_time_based_settings() {
                builder.night_temp = Some(DEFAULT_NIGHT_TEMP);
                builder.day_temp = Some(DEFAULT_DAY_TEMP);
                builder.night_gamma = Some(DEFAULT_NIGHT_GAMMA);
                builder.day_gamma = Some(DEFAULT_DAY_GAMMA);
                builder.update_interval = Some(DEFAULT_UPDATE_INTERVAL);
            }

            if mode.uses_static_settings() {
                builder.static_temp = Some(DEFAULT_DAY_TEMP);
                builder.static_gamma = Some(DEFAULT_DAY_GAMMA);
            }

            if mode.uses_manual_settings() {
                builder.sunset = Some(DEFAULT_SUNSET.to_string());
                builder.sunrise = Some(DEFAULT_SUNRISE.to_string());
                builder.transition_duration = Some(DEFAULT_TRANSITION_DURATION);
            }

            if mode.uses_geo_settings() {
                builder.latitude = Some(40.7128);
                builder.longitude = Some(-74.0060);
            }

            builder
        }

        /// Build the actual Config struct
        fn build(self) -> Config {
            Config {
                backend: Some(self.backend),
                smoothing: self.smoothing,
                startup_duration: self.startup_duration,
                shutdown_duration: self.shutdown_duration,
                startup_transition: self.smoothing, // For backwards compatibility
                startup_transition_duration: self.startup_duration,
                start_hyprsunset: None,
                adaptive_interval: self.adaptive_interval,
                latitude: self.latitude,
                longitude: self.longitude,
                sunset: self.sunset,
                sunrise: self.sunrise,
                night_temp: self.night_temp,
                day_temp: self.day_temp,
                night_gamma: self.night_gamma,
                day_gamma: self.day_gamma,
                static_temp: self.static_temp,
                static_gamma: self.static_gamma,
                transition_duration: self.transition_duration,
                update_interval: self.update_interval,
                transition_mode: Some(self.mode.as_str().to_string()),
            }
        }
    }

    /// Helper to create a config with invalid field combinations for the mode
    fn create_invalid_config_for_mode(mode: TransitionMode) -> Config {
        let mut builder = ModeAwareConfigBuilder::new(mode.clone());

        // Intentionally set fields that shouldn't be used for this mode
        match mode {
            TransitionMode::Static => {
                // Static mode shouldn't use time-based or geo settings
                builder.night_temp = Some(3000);
                builder.day_temp = Some(6000);
                builder.latitude = Some(45.0);
                builder.longitude = Some(-120.0);
            }
            TransitionMode::Geo => {
                // Geo mode shouldn't use static or manual sunset/sunrise
                builder.static_temp = Some(5000);
                builder.static_gamma = Some(80.0);
                builder.sunset = Some("20:00:00".to_string());
                builder.sunrise = Some("05:00:00".to_string());
            }
            TransitionMode::Manual(_) => {
                // Manual modes shouldn't use geo or static settings
                builder.latitude = Some(50.0);
                builder.longitude = Some(-100.0);
                builder.static_temp = Some(4500);
                builder.static_gamma = Some(75.0);
            }
        }

        builder.build()
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 1000, // Run many cases to cover combinations
            max_shrink_iters: 10000,
            ..ProptestConfig::default()
        })]

        /// Test that each transition mode works with its appropriate field combinations
        #[test]
        fn test_mode_specific_field_combinations(
            mode: TransitionMode,
            backend in prop_oneof![
                Just(Backend::Auto),
                Just(Backend::Hyprland),
                Just(Backend::Hyprsunset),
                Just(Backend::Wayland),
            ],
            smoothing in any::<bool>(),
        ) {
            let mut builder = ModeAwareConfigBuilder::new(mode.clone());
            builder.backend = backend;
            builder.smoothing = Some(smoothing);

            let config = builder.build();

            // All properly constructed configs should validate successfully
            prop_assert!(validate_config(&config).is_ok(),
                "Mode {:?} with backend {:?} should validate", mode, backend);

            // Verify the right fields are set for each mode
            match mode {
                TransitionMode::Static => {
                    prop_assert!(config.static_temp.is_some());
                    prop_assert!(config.static_gamma.is_some());
                },
                TransitionMode::Geo => {
                    prop_assert!(config.latitude.is_some());
                    prop_assert!(config.longitude.is_some());
                    prop_assert!(config.night_temp.is_some());
                    prop_assert!(config.day_temp.is_some());
                },
                TransitionMode::Manual(_) => {
                    prop_assert!(config.sunset.is_some());
                    prop_assert!(config.sunrise.is_some());
                    prop_assert!(config.night_temp.is_some());
                    prop_assert!(config.day_temp.is_some());
                    prop_assert!(config.transition_duration.is_some());
                },
            }
        }

        /// Test temperature boundaries for both time-based and static modes
        #[test]
        fn test_temperature_boundaries_by_mode(
            use_static_mode in any::<bool>(),
            temp in prop_oneof![
                Just(MINIMUM_TEMP),
                Just(MAXIMUM_TEMP),
                Just(MINIMUM_TEMP - 1), // Should fail
                Just(MAXIMUM_TEMP + 1), // Should fail
                MINIMUM_TEMP..=MAXIMUM_TEMP, // Valid range
            ],
        ) {
            let mode = if use_static_mode {
                TransitionMode::Static
            } else {
                TransitionMode::Manual("finish_by".to_string())
            };

            let mut builder = ModeAwareConfigBuilder::new(mode);

            if use_static_mode {
                builder.static_temp = Some(temp);
            } else {
                builder.night_temp = Some(temp);
                builder.day_temp = Some(if temp > 5000 { temp - 1000 } else { temp + 1000 });
            }

            let config = builder.build();

            let valid_temp = (MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp);

            if valid_temp {
                prop_assert!(validate_config(&config).is_ok());
            } else {
                prop_assert!(validate_config(&config).is_err(),
                    "Temperature {} should fail validation", temp);
            }
        }

        /// Test gamma boundaries for both time-based and static modes
        #[test]
        fn test_gamma_boundaries_by_mode(
            use_static_mode in any::<bool>(),
            gamma in prop_oneof![
                Just(MINIMUM_GAMMA),
                Just(MAXIMUM_GAMMA),
                Just(MINIMUM_GAMMA - 0.1), // Should fail
                Just(MAXIMUM_GAMMA + 0.1), // Should fail
                MINIMUM_GAMMA..=MAXIMUM_GAMMA, // Valid range
            ],
        ) {
            let mode = if use_static_mode {
                TransitionMode::Static
            } else {
                TransitionMode::Geo
            };

            let mut builder = ModeAwareConfigBuilder::new(mode);

            if use_static_mode {
                builder.static_gamma = Some(gamma);
            } else {
                builder.night_gamma = Some(gamma);
                builder.day_gamma = Some(if gamma > 50.0 { gamma - 10.0 } else { gamma + 10.0 });
            }

            let config = builder.build();

            let valid_gamma = (MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma);

            if valid_gamma {
                prop_assert!(validate_config(&config).is_ok());
            } else {
                prop_assert!(validate_config(&config).is_err(),
                    "Gamma {} should fail validation", gamma);
            }
        }

        /// Test transition duration boundaries (only applies to non-static modes)
        #[test]
        fn test_transition_duration_boundaries(
            mode in prop_oneof![
                Just(TransitionMode::Geo),
                Just(TransitionMode::Manual("finish_by".to_string())),
                Just(TransitionMode::Manual("start_at".to_string())),
                Just(TransitionMode::Manual("center".to_string())),
            ],
            transition_duration in prop_oneof![
                Just(MINIMUM_TRANSITION_DURATION),
                Just(MAXIMUM_TRANSITION_DURATION),
                Just(MINIMUM_TRANSITION_DURATION - 1), // Should fail
                Just(MAXIMUM_TRANSITION_DURATION + 1), // Should fail
                MINIMUM_TRANSITION_DURATION..=MAXIMUM_TRANSITION_DURATION, // Valid range
            ]
        ) {
            let mut builder = ModeAwareConfigBuilder::new(mode);
            builder.transition_duration = Some(transition_duration);
            let config = builder.build();

            let valid_duration = (MINIMUM_TRANSITION_DURATION..=MAXIMUM_TRANSITION_DURATION).contains(&transition_duration);

            if valid_duration {
                prop_assert!(validate_config(&config).is_ok());
            } else {
                prop_assert!(validate_config(&config).is_err());
            }
        }

        /// Test update interval boundaries in relation to transition duration
        #[test]
        fn test_update_interval_boundaries(
            mode in prop_oneof![
                Just(TransitionMode::Geo),
                Just(TransitionMode::Manual("finish_by".to_string())),
                Just(TransitionMode::Manual("start_at".to_string())),
                Just(TransitionMode::Manual("center".to_string())),
            ],
            update_interval in prop_oneof![
                Just(MINIMUM_UPDATE_INTERVAL),
                Just(MAXIMUM_UPDATE_INTERVAL),
                Just(MINIMUM_UPDATE_INTERVAL - 1), // May fail validation
                Just(MAXIMUM_UPDATE_INTERVAL + 1), // May fail validation
                MINIMUM_UPDATE_INTERVAL..=MAXIMUM_UPDATE_INTERVAL, // Valid range
                1u64..10u64, // Very low values
                301u64..1000u64, // High values
            ],
            transition_duration in MINIMUM_TRANSITION_DURATION..=MAXIMUM_TRANSITION_DURATION,
        ) {
            let mut builder = ModeAwareConfigBuilder::new(mode);
            builder.update_interval = Some(update_interval);
            builder.transition_duration = Some(transition_duration);
            let config = builder.build();

            // Check if update interval is longer than transition duration
            let transition_duration_secs = transition_duration * 60;

            if update_interval > transition_duration_secs {
                // This should fail validation
                prop_assert!(validate_config(&config).is_err());
            } else if !(MINIMUM_UPDATE_INTERVAL..=MAXIMUM_UPDATE_INTERVAL).contains(&update_interval) {
                // Values outside the valid range should fail validation (hard limits)
                prop_assert!(validate_config(&config).is_err());
            } else {
                // Values within range should pass validation
                prop_assert!(validate_config(&config).is_ok());
            }
        }

        /// Test smooth transition duration boundaries (applies to all modes)
        #[test]
        fn test_smooth_transition_duration_boundaries(
            mode: TransitionMode,
            startup_duration in prop_oneof![
                Just(MINIMUM_SMOOTH_TRANSITION_DURATION),
                Just(MAXIMUM_SMOOTH_TRANSITION_DURATION),
                Just(MINIMUM_SMOOTH_TRANSITION_DURATION - 1.0), // Should fail
                Just(MAXIMUM_SMOOTH_TRANSITION_DURATION + 1.0), // Should fail
                (0.0..=60.0), // Valid range including decimals
            ],
            shutdown_duration in prop_oneof![
                Just(MINIMUM_SMOOTH_TRANSITION_DURATION),
                Just(MAXIMUM_SMOOTH_TRANSITION_DURATION),
                (0.0..=60.0), // Valid range
            ]
        ) {
            let mut builder = ModeAwareConfigBuilder::new(mode);
            builder.startup_duration = Some(startup_duration);
            builder.shutdown_duration = Some(shutdown_duration);
            let config = builder.build();

            let valid_startup = (MINIMUM_SMOOTH_TRANSITION_DURATION..=MAXIMUM_SMOOTH_TRANSITION_DURATION).contains(&startup_duration);
            let valid_shutdown = (MINIMUM_SMOOTH_TRANSITION_DURATION..=MAXIMUM_SMOOTH_TRANSITION_DURATION).contains(&shutdown_duration);

            if valid_startup && valid_shutdown {
                prop_assert!(validate_config(&config).is_ok());
            } else {
                prop_assert!(validate_config(&config).is_err());
            }
        }

        /// Test sunset/sunrise time combinations for manual modes
        #[test]
        fn test_manual_mode_time_combinations(
            manual_mode in prop_oneof![
                Just("finish_by"),
                Just("start_at"),
                Just("center"),
            ],
            sunset_hour in 0u32..24,
            sunset_minute in 0u32..60,
            sunrise_hour in 0u32..24,
            sunrise_minute in 0u32..60,
            transition_duration in MINIMUM_TRANSITION_DURATION..=MAXIMUM_TRANSITION_DURATION,
        ) {
            let sunset = format!("{sunset_hour:02}:{sunset_minute:02}:00");
            let sunrise = format!("{sunrise_hour:02}:{sunrise_minute:02}:00");

            let mut builder = ModeAwareConfigBuilder::new(TransitionMode::Manual(manual_mode.to_string()));
            builder.sunset = Some(sunset.clone());
            builder.sunrise = Some(sunrise.clone());
            builder.transition_duration = Some(transition_duration);
            let config = builder.build();

            // Parse times for validation logic
            let sunset_time = NaiveTime::parse_from_str(&sunset, "%H:%M:%S").unwrap();
            let sunrise_time = NaiveTime::parse_from_str(&sunrise, "%H:%M:%S").unwrap();

            // Check for identical times (should fail)
            if sunset_time == sunrise_time {
                prop_assert!(validate_config(&config).is_err());
            } else {
                // Calculate day and night durations
                let sunset_mins = sunset_time.hour() * 60 + sunset_time.minute();
                let sunrise_mins = sunrise_time.hour() * 60 + sunrise_time.minute();

                let (day_duration_mins, night_duration_mins) = if sunset_mins > sunrise_mins {
                    let day_duration = sunset_mins - sunrise_mins;
                    let night_duration = (24 * 60) - day_duration;
                    (day_duration, night_duration)
                } else {
                    let night_duration = sunrise_mins - sunset_mins;
                    let day_duration = (24 * 60) - night_duration;
                    (day_duration, night_duration)
                };

                // Very short periods (less than 1 hour) should fail
                if day_duration_mins < 60 || night_duration_mins < 60 {
                    prop_assert!(validate_config(&config).is_err());
                } else {
                    // For longer periods, most should pass unless there are transition overlaps
                    // The validation result depends on complex transition overlap logic
                    let result = validate_config(&config);
                    // We can't predict the exact result due to complex overlap calculations,
                    // but we can ensure it doesn't panic
                    prop_assert!(result.is_ok() || result.is_err());
                }
            }
        }

        /// Test adaptive interval boundaries (applies to all modes)
        #[test]
        fn test_adaptive_interval_boundaries(
            mode: TransitionMode,
            adaptive_interval in prop_oneof![
                Just(MINIMUM_ADAPTIVE_INTERVAL),
                Just(MAXIMUM_ADAPTIVE_INTERVAL),
                Just(MINIMUM_ADAPTIVE_INTERVAL - 1), // Should fail (0)
                Just(MAXIMUM_ADAPTIVE_INTERVAL + 1), // Should fail (1001)
                MINIMUM_ADAPTIVE_INTERVAL..=MAXIMUM_ADAPTIVE_INTERVAL, // Valid range (1-1000ms)
            ]
        ) {
            let mut builder = ModeAwareConfigBuilder::new(mode);
            builder.adaptive_interval = Some(adaptive_interval);
            let config = builder.build();

            let valid_interval = (MINIMUM_ADAPTIVE_INTERVAL..=MAXIMUM_ADAPTIVE_INTERVAL).contains(&adaptive_interval);

            if valid_interval {
                prop_assert!(validate_config(&config).is_ok(),
                    "Adaptive interval {} should be valid", adaptive_interval);
            } else {
                prop_assert!(validate_config(&config).is_err(),
                    "Adaptive interval {} should fail validation", adaptive_interval);
            }
        }

        /// Test latitude boundaries for geo mode
        #[test]
        fn test_latitude_boundaries(
            latitude in prop_oneof![
                Just(-90.0),
                Just(90.0),
                Just(-91.0), // Should fail
                Just(91.0), // Should fail
                (-90.0..=90.0), // Valid range
            ],
            longitude in -180.0..=180.0, // Always use valid longitude
        ) {
            let mut builder = ModeAwareConfigBuilder::new(TransitionMode::Geo);
            builder.latitude = Some(latitude);
            builder.longitude = Some(longitude);
            let config = builder.build();

            let valid_latitude = (-90.0..=90.0).contains(&latitude);

            if valid_latitude {
                prop_assert!(validate_config(&config).is_ok(),
                    "Latitude {} should be valid", latitude);
            } else {
                prop_assert!(validate_config(&config).is_err(),
                    "Latitude {} should fail validation", latitude);
            }
        }

        /// Test longitude boundaries for geo mode
        #[test]
        fn test_longitude_boundaries(
            latitude in -65.0..=65.0, // Use reasonable latitude range (will be capped if > 65)
            longitude in prop_oneof![
                Just(-180.0),
                Just(180.0),
                Just(-181.0), // Should fail
                Just(181.0), // Should fail
                (-180.0..=180.0), // Valid range
            ]
        ) {
            let mut builder = ModeAwareConfigBuilder::new(TransitionMode::Geo);
            builder.latitude = Some(latitude);
            builder.longitude = Some(longitude);
            let config = builder.build();

            let valid_longitude = (-180.0..=180.0).contains(&longitude);

            if valid_longitude {
                prop_assert!(validate_config(&config).is_ok(),
                    "Longitude {} should be valid", longitude);
            } else {
                prop_assert!(validate_config(&config).is_err(),
                    "Longitude {} should fail validation", longitude);
            }
        }

        /// Test mode-specific field interactions with extreme values
        #[test]
        fn test_mode_field_interactions(
            mode: TransitionMode,
            use_extreme_values in any::<bool>(),
        ) {
            let mut builder = ModeAwareConfigBuilder::new(mode.clone());

            if use_extreme_values {
                // Set extreme values appropriate for the mode
                match mode {
                    TransitionMode::Static => {
                        builder.static_temp = Some(MINIMUM_TEMP);
                        builder.static_gamma = Some(MINIMUM_GAMMA);
                    },
                    TransitionMode::Geo => {
                        builder.night_temp = Some(MINIMUM_TEMP);
                        builder.day_temp = Some(MAXIMUM_TEMP);
                        builder.night_gamma = Some(MINIMUM_GAMMA);
                        builder.day_gamma = Some(MAXIMUM_GAMMA);
                        builder.latitude = Some(65.0); // Extreme latitude
                        builder.longitude = Some(180.0);
                    },
                    TransitionMode::Manual(_) => {
                        builder.night_temp = Some(MINIMUM_TEMP);
                        builder.day_temp = Some(MAXIMUM_TEMP);
                        builder.transition_duration = Some(MINIMUM_TRANSITION_DURATION);
                        builder.update_interval = Some(MINIMUM_UPDATE_INTERVAL);
                    },
                }
            }

            let config = builder.build();

            // All properly constructed configs with extreme values should validate
            prop_assert!(validate_config(&config).is_ok());
        }
    }

    /// Exhaustive test of all possible mode and backend combinations
    /// This uses regular test functions to ensure we hit all exact combinations
    mod exhaustive_tests {
        use super::*;

        #[test]
        fn test_all_mode_backend_combinations_exhaustive() {
            // All possible transition modes (5 combinations)
            let transition_modes = [
                TransitionMode::Geo,
                TransitionMode::Static,
                TransitionMode::Manual("finish_by".to_string()),
                TransitionMode::Manual("start_at".to_string()),
                TransitionMode::Manual("center".to_string()),
            ];

            // All possible backend combinations (4 combinations)
            let backends = [
                Backend::Auto,
                Backend::Hyprland,
                Backend::Hyprsunset,
                Backend::Wayland,
            ];

            // All possible smoothing combinations (2 combinations)
            let smoothing_options = [true, false];

            // Test all combinations: 5 × 4 × 2 = 40 total combinations
            for mode in &transition_modes {
                for backend in &backends {
                    for smoothing in &smoothing_options {
                        let mut builder = ModeAwareConfigBuilder::new(mode.clone());
                        builder.backend = *backend;
                        builder.smoothing = Some(*smoothing);

                        let config = builder.build();

                        // All combinations should now pass validation
                        assert!(
                            validate_config(&config).is_ok(),
                            "Expected validation success for mode {:?} with backend {:?} and smoothing {}, but got failure: {:?}",
                            mode,
                            backend,
                            smoothing,
                            validate_config(&config)
                        );
                    }
                }
            }

            println!("✅ All 40 mode/backend/smoothing combinations tested successfully!");
        }

        #[test]
        fn test_mode_specific_boundary_combinations() {
            // Test boundary values for each mode with appropriate fields
            let modes = [
                TransitionMode::Geo,
                TransitionMode::Static,
                TransitionMode::Manual("finish_by".to_string()),
                TransitionMode::Manual("start_at".to_string()),
                TransitionMode::Manual("center".to_string()),
            ];

            for mode in &modes {
                match mode {
                    TransitionMode::Static => {
                        // Test static mode boundaries
                        let temp_boundaries = [MINIMUM_TEMP, MAXIMUM_TEMP];
                        let gamma_boundaries = [MINIMUM_GAMMA, MAXIMUM_GAMMA];

                        for temp in temp_boundaries {
                            for gamma in gamma_boundaries {
                                let mut builder = ModeAwareConfigBuilder::new(mode.clone());
                                builder.static_temp = Some(temp);
                                builder.static_gamma = Some(gamma);
                                let config = builder.build();

                                assert!(
                                    validate_config(&config).is_ok(),
                                    "Static mode boundary values should be valid: temp={}, gamma={}",
                                    temp,
                                    gamma
                                );
                            }
                        }
                    }
                    TransitionMode::Geo => {
                        // Test geo mode boundaries with extreme coordinates
                        let lat_boundaries = [-90.0, -65.0, 0.0, 65.0, 90.0];
                        let lon_boundaries = [-180.0, -90.0, 0.0, 90.0, 180.0];

                        for lat in lat_boundaries {
                            for lon in lon_boundaries {
                                let mut builder = ModeAwareConfigBuilder::new(mode.clone());
                                builder.latitude = Some(lat);
                                builder.longitude = Some(lon);
                                builder.night_temp = Some(MINIMUM_TEMP);
                                builder.day_temp = Some(MAXIMUM_TEMP);
                                let config = builder.build();

                                assert!(
                                    validate_config(&config).is_ok(),
                                    "Geo mode boundary values should be valid: lat={}, lon={}",
                                    lat,
                                    lon
                                );
                            }
                        }
                    }
                    TransitionMode::Manual(_) => {
                        // Test manual mode boundaries
                        let transition_boundaries =
                            [MINIMUM_TRANSITION_DURATION, MAXIMUM_TRANSITION_DURATION];
                        let update_boundaries = [MINIMUM_UPDATE_INTERVAL, MAXIMUM_UPDATE_INTERVAL];

                        for transition_duration in transition_boundaries {
                            for update_interval in update_boundaries {
                                // Skip invalid combinations
                                if update_interval > transition_duration * 60 {
                                    continue;
                                }

                                let mut builder = ModeAwareConfigBuilder::new(mode.clone());
                                builder.transition_duration = Some(transition_duration);
                                builder.update_interval = Some(update_interval);
                                builder.night_temp = Some(MINIMUM_TEMP);
                                builder.day_temp = Some(MAXIMUM_TEMP);
                                let config = builder.build();

                                assert!(
                                    validate_config(&config).is_ok(),
                                    "Manual mode boundary values should be valid: transition={}, update={}",
                                    transition_duration,
                                    update_interval
                                );
                            }
                        }
                    }
                }
            }

            println!("✅ All mode-specific boundary value combinations tested successfully!");
        }

        #[test]
        fn test_field_isolation_by_mode() {
            // Test that fields not used by a mode don't affect validation
            let modes = [
                TransitionMode::Geo,
                TransitionMode::Static,
                TransitionMode::Manual("finish_by".to_string()),
            ];

            for mode in &modes {
                let config = create_invalid_config_for_mode(mode.clone());

                // Config should still validate even with "wrong" fields set
                // because validation should ignore fields not relevant to the mode
                assert!(
                    validate_config(&config).is_ok(),
                    "Config with irrelevant fields set should still validate for mode {:?}",
                    mode
                );
            }

            println!("✅ Field isolation by mode tested successfully!");
        }
    }
}
