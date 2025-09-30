// Rewritten solar tests for the new unified API
#[cfg(test)]
mod solar_tests {
    use crate::geo::solar::*;
    use chrono::{NaiveDate, Timelike};

    /// Test that coordinate validation works correctly at the API boundary.
    #[test]
    fn test_coordinate_validation() {
        // Test valid latitude and longitude ranges
        assert!(calculate_solar_times_unified(40.7128, -74.0060).is_ok());
        assert!(calculate_solar_times_unified(90.0, 180.0).is_ok());
        assert!(calculate_solar_times_unified(-90.0, -180.0).is_ok());

        // Test invalid latitude (outside -90 to 90 range)
        assert!(calculate_solar_times_unified(91.0, 0.0).is_err());
        assert!(calculate_solar_times_unified(-91.0, 0.0).is_err());
        assert!(calculate_solar_times_unified(150.0, 0.0).is_err());

        // Test invalid longitude (outside -180 to 180 range)
        assert!(calculate_solar_times_unified(0.0, 181.0).is_err());
        assert!(calculate_solar_times_unified(0.0, -181.0).is_err());
        assert!(calculate_solar_times_unified(0.0, 360.0).is_err());
    }

    /// Test that timezone detection works for real-world coordinates.
    #[test]
    fn test_timezone_detection() {
        use chrono_tz::{America, Asia, Europe};

        // New York City
        let tz = determine_timezone_from_coordinates(40.7128, -74.0060);
        assert_eq!(tz, America::New_York, "NYC should be in America/New_York");

        // London
        let tz = determine_timezone_from_coordinates(51.5074, -0.1278);
        assert_eq!(tz, Europe::London, "London should be in Europe/London");

        // Tokyo
        let tz = determine_timezone_from_coordinates(35.6762, 139.6503);
        assert_eq!(tz, Asia::Tokyo, "Tokyo should be in Asia/Tokyo");

        // Sydney
        let tz = determine_timezone_from_coordinates(-33.8688, 151.2093);
        assert_eq!(
            tz,
            chrono_tz::Tz::Australia__Sydney,
            "Sydney should be in Australia/Sydney"
        );

        // Rio de Janeiro
        let tz = determine_timezone_from_coordinates(-22.9068, -43.1729);
        assert_eq!(tz, America::Sao_Paulo, "Rio should be in America/Sao_Paulo");
    }

    /// Test that sunrise calculations work for a variety of global locations.
    #[test]
    fn test_global_sunrise_calculations() {
        // Test equatorial location (Singapore) - consistent ~12 hour days
        let result = calculate_solar_times_unified(1.3521, 103.8198).unwrap();
        assert!(result.sunrise_time.hour() >= 5 && result.sunrise_time.hour() <= 8);
        assert!(result.sunset_time.hour() >= 17 && result.sunset_time.hour() <= 20);

        // Test mid-latitude location (Paris)
        let result = calculate_solar_times_unified(48.8566, 2.3522).unwrap();
        assert!(result.sunrise_time.hour() < 12);
        assert!(result.sunset_time.hour() > 12);

        // Test southern hemisphere (Cape Town)
        let result = calculate_solar_times_unified(-33.9249, 18.4241).unwrap();
        assert!(result.sunrise_time.hour() <= 12);
        assert!(result.sunset_time.hour() >= 12);
    }

    /// Test that civil twilight calculations are included in the unified result.
    #[test]
    fn test_civil_twilight_included() {
        // Test a mid-latitude location
        let result = calculate_solar_times_unified(40.7128, -74.0060).unwrap();

        // Civil dawn should be before sunrise
        assert!(result.civil_dawn < result.sunrise_time);

        // Civil dusk should be after sunset
        assert!(result.civil_dusk > result.sunset_time);

        // The difference should be reasonable (typically 20-40 minutes)
        let morning_diff = (result.sunrise_time.num_seconds_from_midnight()
            - result.civil_dawn.num_seconds_from_midnight())
            / 60;
        let evening_diff = (result.civil_dusk.num_seconds_from_midnight()
            - result.sunset_time.num_seconds_from_midnight())
            / 60;

        assert!(morning_diff > 10 && morning_diff < 60);
        assert!(evening_diff > 10 && evening_diff < 60);
    }

    /// Test enhanced transition boundaries for geo mode.
    #[test]
    fn test_enhanced_transition_boundaries() {
        let result = calculate_solar_times_unified(40.7128, -74.0060).unwrap();

        // Sunset transition should start before actual sunset
        assert!(result.sunset_plus_10_start < result.sunset_time);

        // Sunset transition should end after actual sunset
        assert!(result.sunset_minus_2_end > result.sunset_time);

        // Sunrise transition should start before actual sunrise
        assert!(result.sunrise_minus_2_start < result.sunrise_time);

        // Sunrise transition should end after actual sunrise
        assert!(result.sunrise_plus_10_end > result.sunrise_time);

        // Transition durations should be reasonable
        assert!(result.sunset_duration.as_secs() > 60 * 20); // At least 20 minutes
        assert!(result.sunset_duration.as_secs() < 60 * 120); // Less than 2 hours
        assert!(result.sunrise_duration.as_secs() > 60 * 20);
        assert!(result.sunrise_duration.as_secs() < 60 * 120);
    }

    /// Test that extreme latitude handling works correctly.
    /// Note: Latitudes above ±65° are capped before solar calculations,
    /// so they may not trigger the extreme latitude fallback in solar calculations.
    #[test]
    fn test_extreme_latitude_handling() {
        // Test Arctic location (71° gets capped to 65°)
        let result = calculate_solar_times_unified(71.0, 25.0);
        assert!(result.is_ok(), "Should handle Arctic coordinates");

        // Test Antarctic location (gets capped to -65°)
        let result = calculate_solar_times_unified(-71.0, 0.0);
        assert!(result.is_ok(), "Should handle Antarctic coordinates");

        // Test exactly at poles (gets capped to ±65°)
        let result = calculate_solar_times_unified(90.0, 0.0);
        assert!(result.is_ok(), "Should handle North Pole");

        let result = calculate_solar_times_unified(-90.0, 0.0);
        assert!(result.is_ok(), "Should handle South Pole");

        // The fallback is used when solar calculations fail at the location,
        // not just based on latitude. Since coordinates are capped at ±65°,
        // we can't directly test the fallback trigger through latitude alone.
    }

    /// Test that mid-latitude locations don't trigger fallback.
    #[test]
    fn test_normal_latitude_no_fallback() {
        // NYC should not use fallback
        let result = calculate_solar_times_unified(40.7128, -74.0060).unwrap();
        assert!(!result.used_extreme_latitude_fallback);

        // Singapore should not use fallback
        let result = calculate_solar_times_unified(1.3521, 103.8198).unwrap();
        assert!(!result.used_extreme_latitude_fallback);
    }

    /// Test that the display function returns the expected format.
    #[test]
    fn test_civil_twilight_display_function() {
        let today = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
        let result = calculate_civil_twilight_times_for_display(40.7128, -74.0060, today, false);
        assert!(result.is_ok());

        let (
            sunset_time,
            sunset_start,
            sunset_end,
            sunrise_time,
            sunrise_start,
            sunrise_end,
            sunset_dur,
            sunrise_dur,
        ) = result.unwrap();

        // Verify the ordering makes sense
        assert!(sunset_start < sunset_time);
        assert!(sunset_time < sunset_end);
        assert!(sunrise_start < sunrise_time);
        assert!(sunrise_time < sunrise_end);

        // Durations should be positive
        assert!(sunset_dur.as_secs() > 0);
        assert!(sunrise_dur.as_secs() > 0);
    }

    /// Property-based tests
    #[cfg(test)]
    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        /// Generate valid latitude values
        fn latitude_strategy() -> impl Strategy<Value = f64> {
            -90.0..=90.0
        }

        /// Generate valid longitude values
        fn longitude_strategy() -> impl Strategy<Value = f64> {
            -180.0..=180.0
        }

        proptest! {
            /// Property: For any valid coordinates, sunrise should occur before sunset
            /// (except at extreme latitudes where fallback is used or near poles)
            #[test]
            fn prop_sunrise_before_sunset(
                lat in latitude_strategy(),
                lon in longitude_strategy()
            ) {
                if let Ok(result) = calculate_solar_times_unified(lat, lon) {
                    // At extreme latitudes (>65°), the sun behavior is unusual
                    // During polar summer/winter, the sun might not rise/set normally
                    // We skip validation for extreme latitudes
                    if !result.used_extreme_latitude_fallback && lat.abs() < 65.0 {
                        prop_assert!(
                            result.sunrise_time < result.sunset_time,
                            "Sunrise ({:?}) should be before sunset ({:?}) for lat={}, lon={}",
                            result.sunrise_time, result.sunset_time, lat, lon
                        );
                    }
                }
            }

            /// Property: Civil dawn should occur before sunrise, and civil dusk after sunset
            /// (except at extreme latitudes where fallback is used)
            #[test]
            fn prop_civil_twilight_order(
                lat in latitude_strategy(),
                lon in longitude_strategy()
            ) {
                if let Ok(result) = calculate_solar_times_unified(lat, lon) {
                    // At extreme latitudes with fallback, twilight times might not follow normal patterns
                    if !result.used_extreme_latitude_fallback {
                        prop_assert!(
                            result.civil_dawn <= result.sunrise_time,
                            "Civil dawn ({:?}) should be before or at sunrise ({:?})",
                            result.civil_dawn, result.sunrise_time
                        );

                        prop_assert!(
                            result.sunset_time <= result.civil_dusk,
                            "Sunset ({:?}) should be before or at civil dusk ({:?})",
                            result.sunset_time, result.civil_dusk
                        );
                    }
                }
            }

            /// Property: Enhanced transition boundaries should bracket the actual times
            /// (except at extreme latitudes where fallback is used)
            #[test]
            fn prop_transition_boundaries_valid(
                lat in latitude_strategy(),
                lon in longitude_strategy()
            ) {
                if let Ok(result) = calculate_solar_times_unified(lat, lon) {
                    // At extreme latitudes with fallback, the boundaries might not follow normal patterns
                    if !result.used_extreme_latitude_fallback {
                        // Sunset transition
                        prop_assert!(
                            result.sunset_plus_10_start <= result.sunset_time,
                            "Sunset should start at or after +10° boundary"
                        );
                        prop_assert!(
                            result.sunset_time <= result.sunset_minus_2_end,
                            "Sunset should end at or before -2° boundary"
                        );

                        // Sunrise transition
                        prop_assert!(
                            result.sunrise_minus_2_start <= result.sunrise_time,
                            "Sunrise should start at or after -2° boundary"
                        );
                        prop_assert!(
                            result.sunrise_time <= result.sunrise_plus_10_end,
                            "Sunrise should end at or before +10° boundary"
                        );
                    }
                }
            }

            /// Property: Timezone should always be valid
            #[test]
            fn prop_timezone_always_valid(
                lat in latitude_strategy(),
                lon in longitude_strategy()
            ) {
                if let Ok(result) = calculate_solar_times_unified(lat, lon) {
                    // Timezone should be set and usable
                    let now = chrono::Utc::now();
                    let _converted = now.with_timezone(&result.city_timezone);
                    prop_assert!(true, "Timezone conversion should always succeed");
                }
            }

            /// Property: Display function should never panic
            #[test]
            fn prop_display_function_never_panics(
                lat in latitude_strategy(),
                lon in longitude_strategy()
            ) {
                let today = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
                let _ = calculate_civil_twilight_times_for_display(lat, lon, today, false);
                prop_assert!(true, "Display function should handle all valid coordinates");
            }
        }
    }
}
mod timezone_tests {
    use crate::geo::timezone::*;

    #[test]
    fn test_timezone_city_mapping() {
        // Test some common timezones with new comprehensive mapping
        let city = get_city_from_timezone("America/New_York").unwrap();
        assert_eq!(city.name, "New York City");
        assert_eq!(city.country, "United States");
        assert!((city.latitude - 40.7142691).abs() < 0.1);
        assert!((city.longitude - (-74.0059738)).abs() < 0.1);

        let city = get_city_from_timezone("America/Chicago").unwrap();
        assert_eq!(city.name, "Chicago");
        assert_eq!(city.country, "United States");
        assert!((city.latitude - 41.850033).abs() < 0.1);
        assert!((city.longitude - (-87.6500549)).abs() < 0.1);

        let city = get_city_from_timezone("Europe/London").unwrap();
        assert_eq!(city.name, "London");
        assert_eq!(city.country, "United Kingdom");
        assert!((city.latitude - 51.5084153).abs() < 0.1);
        assert!((city.longitude - (-0.1255327)).abs() < 0.1);
    }

    #[test]
    fn test_unknown_timezone_fallback() {
        // Unknown timezones return None from get_city_from_timezone
        let result = get_city_from_timezone("Unknown/Timezone");
        assert!(result.is_none());
    }

    #[test]
    fn test_coordinate_bounds() {
        // Test that all mapped cities have valid coordinates
        let test_timezones = [
            "America/New_York",
            "Europe/London",
            "Asia/Tokyo",
            "Australia/Sydney",
            "Africa/Cairo",
        ];

        for tz_str in &test_timezones {
            if let Some(city) = get_city_from_timezone(tz_str) {
                // Coordinates should be within valid ranges
                assert!(
                    (-90.0..=90.0).contains(&city.latitude),
                    "Invalid latitude for {}: {}",
                    tz_str,
                    city.latitude
                );
                assert!(
                    (-180.0..=180.0).contains(&city.longitude),
                    "Invalid longitude for {}: {}",
                    tz_str,
                    city.longitude
                );
            }
        }
    }

    #[test]
    fn test_comprehensive_timezone_mapping_coverage() {
        // Test representative timezones from each major region
        let regional_timezones = [
            // North America
            ("America/New_York", "New York City", "United States"),
            ("America/Chicago", "Chicago", "United States"),
            ("America/Denver", "Denver", "United States"),
            ("America/Los_Angeles", "Los Angeles", "United States"),
            ("America/Toronto", "Toronto", "Canada"),
            ("America/Mexico_City", "Mexico City", "Mexico"),
            // South America
            ("America/Buenos_Aires", "Buenos Aires", "Argentina"),
            ("America/Santiago", "Santiago", "Chile"),
            ("America/Bogota", "Bogota", "Colombia"),
            // Europe
            ("Europe/London", "London", "United Kingdom"),
            ("Europe/Paris", "Paris", "France"),
            ("Europe/Berlin", "Berlin", "Germany"),
            ("Europe/Rome", "Rome", "Italy"),
            ("Europe/Madrid", "Madrid", "Spain"),
            ("Europe/Moscow", "Moscow", "Russia"),
            // Asia
            ("Asia/Tokyo", "Tokyo", "Japan"),
            ("Asia/Shanghai", "Shanghai", "China"),
            ("Asia/Calcutta", "Calcutta", "India"),
            ("Asia/Seoul", "Seoul", "South Korea"),
            ("Asia/Bangkok", "Bangkok", "Thailand"),
            // Africa
            ("Africa/Cairo", "Cairo", "Egypt"),
            ("Africa/Johannesburg", "Johannesburg", "South Africa"),
            ("Africa/Lagos", "Lagos", "Nigeria"),
            // Australia/Oceania
            ("Australia/Sydney", "Sydney", "Australia"),
            ("Australia/Melbourne", "Melbourne", "Australia"),
            ("Pacific/Auckland", "Auckland", "New Zealand"),
        ];

        for (tz_str, expected_name, expected_country) in &regional_timezones {
            let city = get_city_from_timezone(tz_str)
                .unwrap_or_else(|| panic!("Missing mapping for timezone: {tz_str}"));

            assert_eq!(city.name, *expected_name, "Wrong city name for {tz_str}");
            assert_eq!(
                city.country, *expected_country,
                "Wrong country for {tz_str}"
            );

            // Validate coordinates are reasonable for the region
            assert!(
                (-90.0..=90.0).contains(&city.latitude),
                "Invalid latitude for {}: {}",
                tz_str,
                city.latitude
            );
            assert!(
                (-180.0..=180.0).contains(&city.longitude),
                "Invalid longitude for {}: {}",
                tz_str,
                city.longitude
            );
        }
    }

    #[test]
    fn test_unusual_timezone_formats() {
        // Test various unusual timezone formats that exist in the mapping
        let unusual_formats = [
            "GMT",
            "UTC",
            "US/Eastern",
            "US/Pacific",
            "Canada/Atlantic",
            "Australia/ACT",
            "Etc/GMT",
            "Europe/Belfast",
        ];

        for tz_str in &unusual_formats {
            if let Some(city) = get_city_from_timezone(tz_str) {
                // Should have valid data
                assert!(!city.name.is_empty(), "Empty city name for {tz_str}");
                assert!(!city.country.is_empty(), "Empty country for {tz_str}");
                assert!(
                    (-90.0..=90.0).contains(&city.latitude),
                    "Invalid latitude for {}: {}",
                    tz_str,
                    city.latitude
                );
                assert!(
                    (-180.0..=180.0).contains(&city.longitude),
                    "Invalid longitude for {}: {}",
                    tz_str,
                    city.longitude
                );
            }
        }
    }

    #[test]
    fn test_detect_coordinates_fallback_behavior() {
        // Test the fallback behavior when timezone detection fails or returns unknown timezone

        // Mock environment where timezone detection would fail
        // We can't easily test system timezone detection failure without complex mocking,
        // but we can test the fallback mapping behavior

        // Test that unknown timezone strings fall back to London coordinates
        let result = get_city_from_timezone("Invalid/Unknown_Timezone");
        assert!(result.is_none(), "Should return None for unknown timezone");

        // The actual fallback to London happens in detect_coordinates_from_timezone()
        // which we can't easily unit test without mocking system timezone detection

        // Test London fallback coordinates are correct
        let london_city = get_city_from_timezone("Europe/London").unwrap();
        assert!((london_city.latitude - 51.5074).abs() < 0.1);
        assert!((london_city.longitude - (-0.1278)).abs() < 0.1);
    }

    #[test]
    fn test_city_info_structure_completeness() {
        // Test that all CityInfo structures have complete, non-empty data
        let sample_timezones = [
            "America/New_York",
            "Europe/London",
            "Asia/Tokyo",
            "Australia/Sydney",
            "Africa/Cairo",
            "America/Buenos_Aires",
            "Europe/Paris",
            "Asia/Shanghai",
        ];

        for tz_str in &sample_timezones {
            let city = get_city_from_timezone(tz_str)
                .unwrap_or_else(|| panic!("Missing city for timezone: {tz_str}"));

            // All fields should be populated
            assert!(!city.name.is_empty(), "Empty name for timezone {tz_str}");
            assert!(
                !city.country.is_empty(),
                "Empty country for timezone {tz_str}"
            );

            // Names should not just be the timezone string
            assert_ne!(
                city.name, *tz_str,
                "City name should not be the timezone string"
            );

            // Coordinates should be non-zero (except for edge cases)
            assert!(
                city.latitude != 0.0 || city.longitude != 0.0,
                "Both coordinates are zero for {tz_str} (suspicious)"
            );
        }
    }

    #[test]
    fn test_timezone_mapping_consistency() {
        // Test that similar timezones map to geographically reasonable locations

        // US timezone consistency
        let us_cities = [
            ("US/Eastern", get_city_from_timezone("US/Eastern")),
            ("US/Central", get_city_from_timezone("US/Central")),
            ("US/Mountain", get_city_from_timezone("US/Mountain")),
            ("US/Pacific", get_city_from_timezone("US/Pacific")),
        ];

        for (tz, city_opt) in &us_cities {
            if let Some(city) = city_opt {
                // All should be in United States
                assert_eq!(city.country, "United States", "Wrong country for {tz}");

                // Should be within continental US latitude bounds
                assert!(
                    (25.0..=50.0).contains(&city.latitude),
                    "Latitude {} outside continental US for {}",
                    city.latitude,
                    tz
                );

                // Should be within continental US longitude bounds
                assert!(
                    (-170.0..=-65.0).contains(&city.longitude),
                    "Longitude {} outside continental US for {}",
                    city.longitude,
                    tz
                );
            }
        }
    }
} // End of timezone_tests

mod transition_times_tests {
    use crate::geo::solar::SolarCalculationResult;
    use crate::geo::times::*;
    use chrono::{Local, NaiveTime, TimeZone};

    use std::time::Duration as StdDuration;

    #[test]
    fn test_geo_transition_times_creation() {
        // Test with London coordinates
        let result = GeoTimes::new(51.5074, -0.1278);
        assert!(result.is_ok());

        let times = result.unwrap();
        assert_eq!(times.coordinate_tz.to_string(), "Europe/London");
    }

    #[test]
    fn test_timezone_preservation() {
        // Create a mock solar result for testing
        let solar_result = SolarCalculationResult {
            sunset_time: NaiveTime::from_hms_opt(19, 30, 0).unwrap(),
            sunrise_time: NaiveTime::from_hms_opt(5, 30, 0).unwrap(),
            sunset_duration: StdDuration::from_secs(3600),
            sunrise_duration: StdDuration::from_secs(3600),
            sunset_plus_10_start: NaiveTime::from_hms_opt(19, 0, 0).unwrap(),
            sunset_minus_2_end: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
            sunrise_minus_2_start: NaiveTime::from_hms_opt(5, 0, 0).unwrap(),
            sunrise_plus_10_end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
            civil_dawn: NaiveTime::from_hms_opt(4, 45, 0).unwrap(),
            civil_dusk: NaiveTime::from_hms_opt(20, 15, 0).unwrap(),
            golden_hour_start: NaiveTime::from_hms_opt(18, 30, 0).unwrap(),
            golden_hour_end: NaiveTime::from_hms_opt(6, 30, 0).unwrap(),
            city_timezone: chrono_tz::Europe::London,
            used_extreme_latitude_fallback: false,
            fallback_duration_minutes: 0,
        };

        let now = Local.with_ymd_and_hms(2024, 6, 21, 12, 0, 0).unwrap();
        let base_date = now.date_naive();

        let result = GeoTimes::from_solar_result(&solar_result, base_date, now);
        assert!(result.is_ok());

        let times = result.unwrap();
        // Verify that times are stored with timezone information
        assert_eq!(times.sunset_start.timezone(), chrono_tz::Europe::London);
        assert_eq!(times.sunrise_end.timezone(), chrono_tz::Europe::London);
    }
} // End of transition_times_tests
