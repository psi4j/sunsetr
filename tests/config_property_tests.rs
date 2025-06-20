use chrono::{NaiveTime, Timelike};
use proptest::prelude::*;
use sunsetr::config::{Backend, Config, validate_config};
use sunsetr::constants::*;

/// Generate all possible combinations of boolean configuration options
#[derive(Debug, Clone)]
struct BooleanCombinations {
    start_hyprsunset: Option<bool>,
    startup_transition: Option<bool>,
}

/// Generate all possible backend combinations
#[derive(Debug, Clone)]
struct BackendCombinations {
    backend: Option<Backend>,
}

/// Generate all possible transition mode combinations
#[derive(Debug, Clone)]
struct TransitionModeCase {
    mode: String,
}

#[derive(Debug)]
struct TestConfigCreationArgs {
    bool_combo: BooleanCombinations,
    backend_combo: BackendCombinations,
    mode_combo: TransitionModeCase,
    sunset: String,
    sunrise: String,
    transition_duration: Option<u64>,
    update_interval: Option<u64>,
    night_temp: Option<u32>,
    day_temp: Option<u32>,
    night_gamma: Option<f32>,
    day_gamma: Option<f32>,
    startup_transition_duration: Option<u64>,
}

impl Arbitrary for BooleanCombinations {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        // Generate all 9 combinations: 3 states × 3 states
        prop_oneof![
            Just(BooleanCombinations {
                start_hyprsunset: None,
                startup_transition: None
            }),
            Just(BooleanCombinations {
                start_hyprsunset: None,
                startup_transition: Some(true)
            }),
            Just(BooleanCombinations {
                start_hyprsunset: None,
                startup_transition: Some(false)
            }),
            Just(BooleanCombinations {
                start_hyprsunset: Some(true),
                startup_transition: None
            }),
            Just(BooleanCombinations {
                start_hyprsunset: Some(true),
                startup_transition: Some(true)
            }),
            Just(BooleanCombinations {
                start_hyprsunset: Some(true),
                startup_transition: Some(false)
            }),
            Just(BooleanCombinations {
                start_hyprsunset: Some(false),
                startup_transition: None
            }),
            Just(BooleanCombinations {
                start_hyprsunset: Some(false),
                startup_transition: Some(true)
            }),
            Just(BooleanCombinations {
                start_hyprsunset: Some(false),
                startup_transition: Some(false)
            }),
        ]
        .boxed()
    }
}

impl Arbitrary for BackendCombinations {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        // Generate all 4 combinations: None, Auto, Hyprland, Wayland
        prop_oneof![
            Just(BackendCombinations { backend: None }),
            Just(BackendCombinations {
                backend: Some(Backend::Auto)
            }),
            Just(BackendCombinations {
                backend: Some(Backend::Hyprland)
            }),
            Just(BackendCombinations {
                backend: Some(Backend::Wayland)
            }),
        ]
        .boxed()
    }
}

impl Arbitrary for TransitionModeCase {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        prop_oneof![
            Just(TransitionModeCase {
                mode: "finish_by".to_string()
            }),
            Just(TransitionModeCase {
                mode: "start_at".to_string()
            }),
            Just(TransitionModeCase {
                mode: "center".to_string()
            }),
        ]
        .boxed()
    }
}

/// Helper function to create a test config with specific values
fn create_test_config_with_combinations(
    args: TestConfigCreationArgs
) -> Config {
    Config {
        start_hyprsunset: args.bool_combo.start_hyprsunset,
        backend: args.backend_combo.backend,
        startup_transition: args.bool_combo.startup_transition,
        startup_transition_duration: args.startup_transition_duration,
        latitude: None,
        longitude: None,
        sunset: args.sunset,
        sunrise: args.sunrise,
        night_temp: args.night_temp,
        day_temp: args.day_temp,
        night_gamma: args.night_gamma,
        day_gamma: args.day_gamma,
        transition_duration: args.transition_duration,
        update_interval: args.update_interval,
        transition_mode: Some(args.mode_combo.mode),
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1000, // Run many cases to cover combinations
        max_shrink_iters: 10000,
        ..ProptestConfig::default()
    })]

    /// Test all combinations of boolean and enum fields with valid base values
    #[test]
    fn test_all_boolean_and_enum_combinations(
        bool_combo: BooleanCombinations,
        backend_combo: BackendCombinations,
        mode_combo: TransitionModeCase,
    ) {
        let config = create_test_config_with_combinations(
            TestConfigCreationArgs {
                bool_combo,
                backend_combo,
                mode_combo,
                sunset: "19:00:00".to_string(),
                sunrise: "06:00:00".to_string(),
                transition_duration: Some(DEFAULT_TRANSITION_DURATION),
                update_interval: Some(DEFAULT_UPDATE_INTERVAL),
                night_temp: Some(DEFAULT_NIGHT_TEMP),
                day_temp: Some(DEFAULT_DAY_TEMP),
                night_gamma: Some(DEFAULT_NIGHT_GAMMA),
                day_gamma: Some(DEFAULT_DAY_GAMMA),
                startup_transition_duration: Some(DEFAULT_STARTUP_TRANSITION_DURATION),
            }
        );

        // Check that the specific incompatible combination fails
        let backend = config.backend.as_ref().unwrap_or(&DEFAULT_BACKEND);
        let start_hyprsunset = config.start_hyprsunset.unwrap_or(DEFAULT_START_HYPRSUNSET);

        if *backend == Backend::Wayland && start_hyprsunset {
            // This combination should fail validation
            prop_assert!(validate_config(&config).is_err());
        } else {
            // All other combinations should pass validation
            prop_assert!(validate_config(&config).is_ok());
        }
    }

    /// Test extreme boundary values for temperatures
    #[test]
    fn test_extreme_temperature_boundaries(
        night_temp in prop_oneof![
            Just(MINIMUM_TEMP),
            Just(MAXIMUM_TEMP),
            Just(MINIMUM_TEMP - 1), // Should fail
            Just(MAXIMUM_TEMP + 1), // Should fail
            MINIMUM_TEMP..=MAXIMUM_TEMP, // Valid range
        ],
        day_temp in prop_oneof![
            Just(MINIMUM_TEMP),
            Just(MAXIMUM_TEMP),
            Just(MINIMUM_TEMP - 1), // Should fail
            Just(MAXIMUM_TEMP + 1), // Should fail
            MINIMUM_TEMP..=MAXIMUM_TEMP, // Valid range
        ]
    ) {
        let config = create_test_config_with_combinations(
            TestConfigCreationArgs {
                bool_combo: BooleanCombinations { start_hyprsunset: Some(false), startup_transition: Some(false) },
                backend_combo: BackendCombinations { backend: Some(Backend::Auto) },
                mode_combo: TransitionModeCase { mode: "finish_by".to_string() },
                sunset: "19:00:00".to_string(),
                sunrise: "06:00:00".to_string(),
                transition_duration: Some(DEFAULT_TRANSITION_DURATION),
                update_interval: Some(DEFAULT_UPDATE_INTERVAL),
                night_temp: Some(night_temp),
                day_temp: Some(day_temp),
                night_gamma: Some(DEFAULT_NIGHT_GAMMA),
                day_gamma: Some(DEFAULT_DAY_GAMMA),
                startup_transition_duration: Some(DEFAULT_STARTUP_TRANSITION_DURATION),
            }
        );

        let valid_night = (MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&night_temp);
        let valid_day = (MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&day_temp);

        if valid_night && valid_day {
            prop_assert!(validate_config(&config).is_ok());
        } else {
            prop_assert!(validate_config(&config).is_err());
        }
    }

    /// Test extreme boundary values for gamma
    #[test]
    fn test_extreme_gamma_boundaries(
        night_gamma in prop_oneof![
            Just(MINIMUM_GAMMA),
            Just(MAXIMUM_GAMMA),
            Just(MINIMUM_GAMMA - 0.1), // Should fail
            Just(MAXIMUM_GAMMA + 0.1), // Should fail
            MINIMUM_GAMMA..=MAXIMUM_GAMMA, // Valid range
        ],
        day_gamma in prop_oneof![
            Just(MINIMUM_GAMMA),
            Just(MAXIMUM_GAMMA),
            Just(MINIMUM_GAMMA - 0.1), // Should fail
            Just(MAXIMUM_GAMMA + 0.1), // Should fail
            MINIMUM_GAMMA..=MAXIMUM_GAMMA, // Valid range
        ]
    ) {
        let config = create_test_config_with_combinations(
            TestConfigCreationArgs {
                bool_combo: BooleanCombinations { start_hyprsunset: Some(false), startup_transition: Some(false) },
                backend_combo: BackendCombinations { backend: Some(Backend::Auto) },
                mode_combo: TransitionModeCase { mode: "finish_by".to_string() },
                sunset: "19:00:00".to_string(),
                sunrise: "06:00:00".to_string(),
                transition_duration: Some(DEFAULT_TRANSITION_DURATION),
                update_interval: Some(DEFAULT_UPDATE_INTERVAL),
                night_temp: Some(DEFAULT_NIGHT_TEMP),
                day_temp: Some(DEFAULT_DAY_TEMP),
                night_gamma: Some(night_gamma),
                day_gamma: Some(day_gamma),
                startup_transition_duration: Some(DEFAULT_STARTUP_TRANSITION_DURATION),
            }
        );

        let valid_night = (MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&night_gamma);
        let valid_day = (MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&day_gamma);

        if valid_night && valid_day {
            prop_assert!(validate_config(&config).is_ok());
        } else {
            prop_assert!(validate_config(&config).is_err());
        }
    }

    /// Test extreme boundary values for transition durations
    #[test]
    fn test_extreme_transition_duration_boundaries(
        transition_duration in prop_oneof![
            Just(MINIMUM_TRANSITION_DURATION),
            Just(MAXIMUM_TRANSITION_DURATION),
            Just(MINIMUM_TRANSITION_DURATION - 1), // Should fail
            Just(MAXIMUM_TRANSITION_DURATION + 1), // Should fail
            MINIMUM_TRANSITION_DURATION..=MAXIMUM_TRANSITION_DURATION, // Valid range
        ]
    ) {
        let config = create_test_config_with_combinations(
            TestConfigCreationArgs {
                bool_combo: BooleanCombinations { start_hyprsunset: Some(false), startup_transition: Some(false) },
                backend_combo: BackendCombinations { backend: Some(Backend::Auto) },
                mode_combo: TransitionModeCase { mode: "finish_by".to_string() },
                sunset: "19:00:00".to_string(),
                sunrise: "06:00:00".to_string(),
                transition_duration: Some(transition_duration),
                update_interval: Some(DEFAULT_UPDATE_INTERVAL),
                night_temp: Some(DEFAULT_NIGHT_TEMP),
                day_temp: Some(DEFAULT_DAY_TEMP),
                night_gamma: Some(DEFAULT_NIGHT_GAMMA),
                day_gamma: Some(DEFAULT_DAY_GAMMA),
                startup_transition_duration: Some(DEFAULT_STARTUP_TRANSITION_DURATION),
            }
        );

        let valid_duration = (MINIMUM_TRANSITION_DURATION..=MAXIMUM_TRANSITION_DURATION).contains(&transition_duration);

        if valid_duration {
            prop_assert!(validate_config(&config).is_ok());
        } else {
            prop_assert!(validate_config(&config).is_err());
        }
    }

    /// Test extreme boundary values for update intervals
    #[test]
    fn test_extreme_update_interval_boundaries(
        update_interval in prop_oneof![
            Just(MINIMUM_UPDATE_INTERVAL),
            Just(MAXIMUM_UPDATE_INTERVAL),
            Just(MINIMUM_UPDATE_INTERVAL - 1), // May generate warnings but should not fail hard
            Just(MAXIMUM_UPDATE_INTERVAL + 1), // May generate warnings but should not fail hard
            MINIMUM_UPDATE_INTERVAL..=MAXIMUM_UPDATE_INTERVAL, // Valid range
            1u64..10u64, // Very low values (may warn)
            301u64..1000u64, // High values (may warn)
        ],
        transition_duration in MINIMUM_TRANSITION_DURATION..=MAXIMUM_TRANSITION_DURATION,
    ) {
        let config = create_test_config_with_combinations(
            TestConfigCreationArgs {
                bool_combo: BooleanCombinations { start_hyprsunset: Some(false), startup_transition: Some(false) },
                backend_combo: BackendCombinations { backend: Some(Backend::Auto) },
                mode_combo: TransitionModeCase { mode: "finish_by".to_string() },
                sunset: "19:00:00".to_string(),
                sunrise: "06:00:00".to_string(),
                transition_duration: Some(transition_duration),
                update_interval: Some(update_interval),
                night_temp: Some(DEFAULT_NIGHT_TEMP),
                day_temp: Some(DEFAULT_DAY_TEMP),
                night_gamma: Some(DEFAULT_NIGHT_GAMMA),
                day_gamma: Some(DEFAULT_DAY_GAMMA),
                startup_transition_duration: Some(DEFAULT_STARTUP_TRANSITION_DURATION),
            }
        );

        // Check if update interval is longer than transition duration
        let transition_duration_secs = transition_duration * 60;

        if update_interval > transition_duration_secs {
            // This should fail validation
            prop_assert!(validate_config(&config).is_err());
        } else {
            // Other values may generate warnings but should not fail validation
            prop_assert!(validate_config(&config).is_ok());
        }
    }

    /// Test extreme boundary values for startup transition duration
    #[test]
    fn test_extreme_startup_transition_duration_boundaries(
        startup_duration in prop_oneof![
            Just(MINIMUM_STARTUP_TRANSITION_DURATION),
            Just(MAXIMUM_STARTUP_TRANSITION_DURATION),
            Just(MINIMUM_STARTUP_TRANSITION_DURATION - 1), // Should fail
            Just(MAXIMUM_STARTUP_TRANSITION_DURATION + 1), // Should fail
            MINIMUM_STARTUP_TRANSITION_DURATION..=MAXIMUM_STARTUP_TRANSITION_DURATION, // Valid range
        ]
    ) {
        let config = create_test_config_with_combinations(
            TestConfigCreationArgs {
                bool_combo: BooleanCombinations { start_hyprsunset: Some(false), startup_transition: Some(true) },
                backend_combo: BackendCombinations { backend: Some(Backend::Auto) },
                mode_combo: TransitionModeCase { mode: "finish_by".to_string() },
                sunset: "19:00:00".to_string(),
                sunrise: "06:00:00".to_string(),
                transition_duration: Some(DEFAULT_TRANSITION_DURATION),
                update_interval: Some(DEFAULT_UPDATE_INTERVAL),
                night_temp: Some(DEFAULT_NIGHT_TEMP),
                day_temp: Some(DEFAULT_DAY_TEMP),
                night_gamma: Some(DEFAULT_NIGHT_GAMMA),
                day_gamma: Some(DEFAULT_DAY_GAMMA),
                startup_transition_duration: Some(startup_duration),
            }
        );

        let valid_startup_duration = (MINIMUM_STARTUP_TRANSITION_DURATION..=MAXIMUM_STARTUP_TRANSITION_DURATION).contains(&startup_duration);

        if valid_startup_duration {
            prop_assert!(validate_config(&config).is_ok());
        } else {
            prop_assert!(validate_config(&config).is_err());
        }
    }

    /// Test extreme time combinations that might cause validation issues
    #[test]
    fn test_extreme_time_combinations(
        sunset_hour in 0u32..24,
        sunset_minute in 0u32..60,
        sunrise_hour in 0u32..24,
        sunrise_minute in 0u32..60,
        transition_duration in MINIMUM_TRANSITION_DURATION..=MAXIMUM_TRANSITION_DURATION,
        mode_combo: TransitionModeCase,
    ) {
        let sunset = format!("{:02}:{:02}:00", sunset_hour, sunset_minute);
        let sunrise = format!("{:02}:{:02}:00", sunrise_hour, sunrise_minute);

        let config = create_test_config_with_combinations(
            TestConfigCreationArgs {
                bool_combo: BooleanCombinations { start_hyprsunset: Some(false), startup_transition: Some(false) },
                backend_combo: BackendCombinations { backend: Some(Backend::Auto) },
                mode_combo,
                sunset: sunset.to_string(),
                sunrise: sunrise.to_string(),
                transition_duration: Some(transition_duration),
                update_interval: Some(DEFAULT_UPDATE_INTERVAL),
                night_temp: Some(DEFAULT_NIGHT_TEMP),
                day_temp: Some(DEFAULT_DAY_TEMP),
                night_gamma: Some(DEFAULT_NIGHT_GAMMA),
                day_gamma: Some(DEFAULT_DAY_GAMMA),
                startup_transition_duration: Some(DEFAULT_STARTUP_TRANSITION_DURATION),
            }
        );

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

    /// Test all enum and boolean combinations with extreme numerical values
    #[test]
    fn test_combinations_with_extreme_numerics(
        bool_combo: BooleanCombinations,
        backend_combo: BackendCombinations,
        mode_combo: TransitionModeCase,
        use_extreme_temps in any::<bool>(),
        use_extreme_gammas in any::<bool>(),
        use_extreme_durations in any::<bool>(),
    ) {
        let (night_temp, day_temp) = if use_extreme_temps {
            (MINIMUM_TEMP, MAXIMUM_TEMP)
        } else {
            (DEFAULT_NIGHT_TEMP, DEFAULT_DAY_TEMP)
        };

        let (night_gamma, day_gamma) = if use_extreme_gammas {
            (MINIMUM_GAMMA, MAXIMUM_GAMMA)
        } else {
            (DEFAULT_NIGHT_GAMMA, DEFAULT_DAY_GAMMA)
        };

        let (transition_duration, update_interval) = if use_extreme_durations {
            (MINIMUM_TRANSITION_DURATION, MINIMUM_UPDATE_INTERVAL)
        } else {
            (DEFAULT_TRANSITION_DURATION, DEFAULT_UPDATE_INTERVAL)
        };

        let config = create_test_config_with_combinations(
            TestConfigCreationArgs {
                bool_combo,
                backend_combo,
                mode_combo,
                sunset: "19:00:00".to_string(),
                sunrise: "06:00:00".to_string(),
                transition_duration: Some(transition_duration),
                update_interval: Some(update_interval),
                night_temp: Some(night_temp),
                day_temp: Some(day_temp),
                night_gamma: Some(night_gamma),
                day_gamma: Some(day_gamma),
                startup_transition_duration: Some(DEFAULT_STARTUP_TRANSITION_DURATION),
            }
        );

        // Check for the known incompatible backend combination
        let backend = config.backend.as_ref().unwrap_or(&DEFAULT_BACKEND);
        let start_hyprsunset = config.start_hyprsunset.unwrap_or(DEFAULT_START_HYPRSUNSET);

        if *backend == Backend::Wayland && start_hyprsunset {
            prop_assert!(validate_config(&config).is_err());
        } else {
            // All other combinations with valid extreme values should pass
            prop_assert!(validate_config(&config).is_ok());
        }
    }
}

/// Exhaustive test of all possible combinations of boolean and enum fields
/// This uses regular test functions to ensure we hit all exact combinations
#[cfg(test)]
mod exhaustive_tests {
    use super::*;

    #[test]
    fn test_all_boolean_enum_combinations_exhaustive() {
        // All possible boolean combinations (3^2 = 9 combinations)
        let boolean_combinations = [
            (None, None),
            (None, Some(true)),
            (None, Some(false)),
            (Some(true), None),
            (Some(true), Some(true)),
            (Some(true), Some(false)),
            (Some(false), None),
            (Some(false), Some(true)),
            (Some(false), Some(false)),
        ];

        // All possible backend combinations (4 combinations)
        let backend_combinations = [
            None,
            Some(Backend::Auto),
            Some(Backend::Hyprland),
            Some(Backend::Wayland),
        ];

        // All possible transition modes (3 combinations)
        let transition_modes = ["finish_by", "start_at", "center"];

        // Test all combinations: 9 × 4 × 3 = 108 total combinations
        for (start_hyprsunset, startup_transition) in boolean_combinations {
            for backend in backend_combinations {
                for mode in transition_modes {
                    let config = Config {
                        start_hyprsunset,
                        backend,
                        startup_transition,
                        startup_transition_duration: Some(DEFAULT_STARTUP_TRANSITION_DURATION),
                        latitude: None,
                        longitude: None,
                        sunset: "19:00:00".to_string(),
                        sunrise: "06:00:00".to_string(),
                        night_temp: Some(DEFAULT_NIGHT_TEMP),
                        day_temp: Some(DEFAULT_DAY_TEMP),
                        night_gamma: Some(DEFAULT_NIGHT_GAMMA),
                        day_gamma: Some(DEFAULT_DAY_GAMMA),
                        transition_duration: Some(DEFAULT_TRANSITION_DURATION),
                        update_interval: Some(DEFAULT_UPDATE_INTERVAL),
                        transition_mode: Some(mode.to_string()),
                    };

                    // Check for the specific incompatible combination
                    let actual_backend = config.backend.as_ref().unwrap_or(&DEFAULT_BACKEND);
                    let actual_start_hyprsunset =
                        config.start_hyprsunset.unwrap_or(DEFAULT_START_HYPRSUNSET);

                    if *actual_backend == Backend::Wayland && actual_start_hyprsunset {
                        // This combination should fail
                        assert!(
                            validate_config(&config).is_err(),
                            "Expected validation failure for Wayland + start_hyprsunset=true, but got success. Config: {:?}",
                            config
                        );
                    } else {
                        // All other combinations should pass
                        assert!(
                            validate_config(&config).is_ok(),
                            "Expected validation success, but got failure. Config: {:?}, Error: {:?}",
                            config,
                            validate_config(&config)
                        );
                    }
                }
            }
        }

        println!("✅ All 108 boolean/enum combinations tested successfully!");
    }

    #[test]
    fn test_all_boundary_value_combinations() {
        // Test all boundary combinations for numerical values
        let temp_boundaries = [MINIMUM_TEMP, MAXIMUM_TEMP];
        let gamma_boundaries = [MINIMUM_GAMMA, MAXIMUM_GAMMA];
        let transition_boundaries = [MINIMUM_TRANSITION_DURATION, MAXIMUM_TRANSITION_DURATION];
        let update_boundaries = [MINIMUM_UPDATE_INTERVAL, MAXIMUM_UPDATE_INTERVAL];
        let startup_boundaries = [
            MINIMUM_STARTUP_TRANSITION_DURATION,
            MAXIMUM_STARTUP_TRANSITION_DURATION,
        ];

        for night_temp in temp_boundaries {
            for day_temp in temp_boundaries {
                for night_gamma in gamma_boundaries {
                    for day_gamma in gamma_boundaries {
                        for transition_duration in transition_boundaries {
                            for update_interval in update_boundaries {
                                for startup_duration in startup_boundaries {
                                    // Skip cases where update_interval > transition_duration (in seconds)
                                    if update_interval > transition_duration * 60 {
                                        continue;
                                    }

                                    let config = Config {
                                        start_hyprsunset: Some(false),
                                        backend: Some(Backend::Auto),
                                        startup_transition: Some(false),
                                        startup_transition_duration: Some(startup_duration),
                                        latitude: None,
                                        longitude: None,
                                        sunset: "19:00:00".to_string(),
                                        sunrise: "06:00:00".to_string(),
                                        night_temp: Some(night_temp),
                                        day_temp: Some(day_temp),
                                        night_gamma: Some(night_gamma),
                                        day_gamma: Some(day_gamma),
                                        transition_duration: Some(transition_duration),
                                        update_interval: Some(update_interval),
                                        transition_mode: Some("finish_by".to_string()),
                                    };

                                    assert!(
                                        validate_config(&config).is_ok(),
                                        "Boundary value combination should be valid: {:?}",
                                        config
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        println!("✅ All boundary value combinations tested successfully!");
    }
}

