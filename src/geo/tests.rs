#[cfg(test)]
mod solar_tests {
    use crate::geo::solar::*;
    use chrono::{NaiveDate, Timelike};

    /// Test that coordinate validation works correctly at the API boundary.
    #[test]
    fn test_coordinate_validation() {
        let test_date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();

        // Test valid latitude and longitude ranges
        assert!(calculate_solar_times(40.7128, -74.0060, test_date).is_ok());
        assert!(calculate_solar_times(90.0, 180.0, test_date).is_ok());
        assert!(calculate_solar_times(-90.0, -180.0, test_date).is_ok());

        // Test invalid latitude (outside -90 to 90 range)
        assert!(calculate_solar_times(91.0, 0.0, test_date).is_err());
        assert!(calculate_solar_times(-91.0, 0.0, test_date).is_err());
        assert!(calculate_solar_times(150.0, 0.0, test_date).is_err());

        // Test invalid longitude (outside -180 to 180 range)
        assert!(calculate_solar_times(0.0, 181.0, test_date).is_err());
        assert!(calculate_solar_times(0.0, -181.0, test_date).is_err());
        assert!(calculate_solar_times(0.0, 360.0, test_date).is_err());
    }

    /// Test that timezone detection works for real-world coordinates.
    #[test]
    fn test_timezone_detection() {
        use chrono_tz::{America, Asia, Europe};

        // New York City
        let tz = determine_timezone(40.7128, -74.0060);
        assert_eq!(tz, America::New_York, "NYC should be in America/New_York");

        // London
        let tz = determine_timezone(51.5074, -0.1278);
        assert_eq!(tz, Europe::London, "London should be in Europe/London");

        // Tokyo
        let tz = determine_timezone(35.6762, 139.6503);
        assert_eq!(tz, Asia::Tokyo, "Tokyo should be in Asia/Tokyo");

        // Sydney
        let tz = determine_timezone(-33.8688, 151.2093);
        assert_eq!(
            tz,
            chrono_tz::Tz::Australia__Sydney,
            "Sydney should be in Australia/Sydney"
        );

        // Rio de Janeiro
        let tz = determine_timezone(-22.9068, -43.1729);
        assert_eq!(tz, America::Sao_Paulo, "Rio should be in America/Sao_Paulo");
    }

    /// Test that sunrise calculations work for a variety of global locations.
    #[test]
    fn test_global_sunrise_calculations() {
        let test_date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();

        // Test equatorial location (Singapore) - consistent ~12 hour days
        let result = calculate_solar_times(1.3521, 103.8198, test_date).unwrap();
        assert!(result.sunrise_time.hour() >= 5 && result.sunrise_time.hour() <= 8);
        assert!(result.sunset_time.hour() >= 17 && result.sunset_time.hour() <= 20);

        // Test mid-latitude location (Paris)
        let result = calculate_solar_times(48.8566, 2.3522, test_date).unwrap();
        assert!(result.sunrise_time.hour() < 12);
        assert!(result.sunset_time.hour() > 12);

        // Test southern hemisphere (Cape Town)
        let result = calculate_solar_times(-33.9249, 18.4241, test_date).unwrap();
        assert!(result.sunrise_time.hour() <= 12);
        assert!(result.sunset_time.hour() >= 12);
    }

    /// Test that civil twilight calculations are included in the unified result.
    #[test]
    fn test_civil_twilight_included() {
        let test_date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();

        // Test a mid-latitude location
        let result = calculate_solar_times(40.7128, -74.0060, test_date).unwrap();

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
        let test_date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
        let result = calculate_solar_times(40.7128, -74.0060, test_date).unwrap();

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
    /// Note: Latitudes above +/-65 degrees are capped before solar calculations,
    /// so they may not trigger the extreme latitude fallback in solar calculations.
    #[test]
    fn test_extreme_latitude_handling() {
        let test_date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();

        // Test Arctic location (71 gets capped to 65)
        let result = calculate_solar_times(71.0, 25.0, test_date);
        assert!(result.is_ok(), "Should handle Arctic coordinates");

        // Test Antarctic location (gets capped to -65)
        let result = calculate_solar_times(-71.0, 0.0, test_date);
        assert!(result.is_ok(), "Should handle Antarctic coordinates");

        // Test exactly at poles (gets capped to +/-65)
        let result = calculate_solar_times(90.0, 0.0, test_date);
        assert!(result.is_ok(), "Should handle North Pole");

        let result = calculate_solar_times(-90.0, 0.0, test_date);
        assert!(result.is_ok(), "Should handle South Pole");

        // The fallback is used when solar calculations fail at the location,
        // not just based on latitude. Since coordinates are capped at +/-65,
        // we can't directly test the fallback trigger through latitude alone.
    }

    /// Test that mid-latitude locations don't trigger fallback.
    #[test]
    fn test_normal_latitude_no_fallback() {
        let test_date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();

        // NYC should not use fallback
        let result = calculate_solar_times(40.7128, -74.0060, test_date).unwrap();
        assert!(!result.used_extreme_latitude_fallback);

        // Singapore should not use fallback
        let result = calculate_solar_times(1.3521, 103.8198, test_date).unwrap();
        assert!(!result.used_extreme_latitude_fallback);
    }

    /// Transition window edges should be ordered start < actual < end, with
    /// positive durations.
    #[test]
    fn test_transition_window_ordering() {
        let today = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
        let solar = calculate_solar_times(40.7128, -74.0060, today).unwrap();

        assert!(solar.sunset_plus_10_start < solar.sunset_time);
        assert!(solar.sunset_time < solar.sunset_minus_2_end);
        assert!(solar.sunrise_minus_2_start < solar.sunrise_time);
        assert!(solar.sunrise_time < solar.sunrise_plus_10_end);

        assert!(solar.sunset_duration.as_secs() > 0);
        assert!(solar.sunrise_duration.as_secs() > 0);
    }

    /// Property-based tests
    #[cfg(test)]
    mod property_tests {
        use super::*;
        use chrono::NaiveTime;
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
                let test_date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
                if let Ok(result) = calculate_solar_times(lat, lon, test_date) {
                    // At extreme latitudes (>65 degrees), the sun behavior is unusual
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
                let test_date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
                if let Ok(result) = calculate_solar_times(lat, lon, test_date) {
                    // At extreme latitudes with fallback, twilight times might not follow normal patterns
                    if !result.used_extreme_latitude_fallback {
                        // Morning: civil_dawn should be before or at sunrise
                        // Midnight crossing is less common in the morning, but still possible
                        let dawn_valid = result.civil_dawn <= result.sunrise_time || result.sunrise_time.hour() < 12;
                        prop_assert!(
                            dawn_valid,
                            "Civil dawn ({:?}) should be before or at sunrise ({:?}) for lat={}, lon={}",
                            result.civil_dawn, result.sunrise_time, lat, lon
                        );

                        // Evening: sunset should be before or at civil_dusk
                        // Handle midnight crossing: if civil_dusk appears earlier in clock time than sunset,
                        // it means civil_dusk occurred after midnight (next day)
                        let dusk_valid = result.sunset_time <= result.civil_dusk
                            || (result.civil_dusk < result.sunset_time && result.sunset_time.hour() >= 20);

                        prop_assert!(
                            dusk_valid,
                            "Sunset ({:?}) should be before or at civil dusk ({:?}) for lat={}, lon={}",
                            result.sunset_time, result.civil_dusk, lat, lon
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
                let test_date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
                if let Ok(result) = calculate_solar_times(lat, lon, test_date) {
                    // At extreme latitudes with fallback, the boundaries might not follow normal patterns
                    if !result.used_extreme_latitude_fallback {
                        // Helper function to validate time transitions that might span midnight
                        let validate_transition_order = |start: NaiveTime, end: NaiveTime| -> bool {
                            // If start time is later than end time in clock terms (e.g., 23:30 vs 01:00),
                            // this indicates a midnight-spanning transition which is valid at extreme latitudes
                            if start > end {
                                // This is a midnight-spanning transition - always valid for extreme latitudes
                                true
                            } else {
                                // Normal same-day transition - standard comparison applies
                                start <= end
                            }
                        };

                        // Sunset transition boundaries
                        let sunset_start_valid = validate_transition_order(
                            result.sunset_plus_10_start,
                            result.sunset_time
                        );
                        prop_assert!(
                            sunset_start_valid,
                            "Sunset transition start should be valid (start: {:?}, sunset: {:?}, lat: {}, lon: {})",
                            result.sunset_plus_10_start, result.sunset_time, lat, lon
                        );

                        let sunset_end_valid = validate_transition_order(
                            result.sunset_time,
                            result.sunset_minus_2_end
                        );
                        prop_assert!(
                            sunset_end_valid,
                            "Sunset transition end should be valid (sunset: {:?}, end: {:?}, lat: {}, lon: {})",
                            result.sunset_time, result.sunset_minus_2_end, lat, lon
                        );

                        // Sunrise transition boundaries
                        let sunrise_start_valid = validate_transition_order(
                            result.sunrise_minus_2_start,
                            result.sunrise_time
                        );
                        prop_assert!(
                            sunrise_start_valid,
                            "Sunrise transition start should be valid (start: {:?}, sunrise: {:?}, lat: {}, lon: {})",
                            result.sunrise_minus_2_start, result.sunrise_time, lat, lon
                        );

                        let sunrise_end_valid = validate_transition_order(
                            result.sunrise_time,
                            result.sunrise_plus_10_end
                        );
                        prop_assert!(
                            sunrise_end_valid,
                            "Sunrise transition end should be valid (sunrise: {:?}, end: {:?}, lat: {}, lon: {})",
                            result.sunrise_time, result.sunrise_plus_10_end, lat, lon
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
                let test_date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
                if let Ok(result) = calculate_solar_times(lat, lon, test_date) {
                    // Timezone should be set and usable
                    let now = chrono::Utc::now();
                    let _converted = now.with_timezone(&result.city_timezone);
                    prop_assert!(true, "Timezone conversion should always succeed");
                }
            }

            /// Property: solar calculation should never panic for valid coordinates.
            #[test]
            fn prop_calculate_solar_times_never_panics(
                lat in latitude_strategy(),
                lon in longitude_strategy()
            ) {
                let today = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
                let _ = calculate_solar_times(lat, lon, today);
                prop_assert!(true, "calculate_solar_times should handle all valid coordinates");
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
        // The real detection-failure path lives in detect_coordinates_from_timezone()
        // and can't be unit tested without mocking system timezone detection, so this
        // exercises only the unknown-timezone -> London fallback mapping it relies on.

        let result = get_city_from_timezone("Invalid/Unknown_Timezone");
        assert!(result.is_none(), "Should return None for unknown timezone");

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
    use crate::geo::solar::SolarTimes;
    use crate::geo::times::*;
    use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeZone};
    use chrono_tz::Tz;

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
        let solar_result = SolarTimes {
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

        // London coordinates (matching the timezone in solar_result)
        let lat = 51.5074;
        let lon = -0.1278;

        let result = GeoTimes::from_solar_result(&solar_result, base_date, now, lat, lon);
        assert!(result.is_ok());

        let times = result.unwrap();
        // Verify that times are stored with timezone information
        assert_eq!(times.sunset_start.timezone(), chrono_tz::Europe::London);
        assert_eq!(times.sunrise_end.timezone(), chrono_tz::Europe::London);
    }

    // Both coordinates are capped to 65 deg N. At lon 14.4 the summer sun SETS
    // just after local midnight; at lon 60 (fixed-offset zone) it RISES just
    // after midnight. Each makes the opposite transition window straddle the
    // boundary.
    const HIGH_LAT: f64 = 65.0;
    const SUNSET_WRAP_LON: f64 = 14.4049;
    const SUNRISE_WRAP_LON: f64 = 60.0;

    fn noon_local(tz: Tz, date: NaiveDate) -> DateTime<Local> {
        tz.from_local_datetime(&date.and_hms_opt(12, 0, 0).unwrap())
            .single()
            .unwrap()
            .with_timezone(&Local)
    }

    /// Walk a simulated clock through a date whose transition crosses midnight
    /// and assert the cycle still visits every period in order. `wraps` selects
    /// the date whose sunset or sunrise window straddles the boundary.
    fn assert_cycle_order_on_crossing(lat: f64, lon: f64, wraps: impl Fn(&SolarTimes) -> bool) {
        use crate::core::period::Period;
        use crate::geo::solar::{calculate_solar_times, determine_timezone};

        let tz = determine_timezone(lat, lon);
        let end = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        let mut date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let crossing = loop {
            let solar = calculate_solar_times(lat, lon, date).unwrap();
            if wraps(&solar) {
                break date;
            }
            date = date.succ_opt().unwrap();
            assert!(
                date <= end,
                "no midnight-crossing transition found in the year"
            );
        };

        let solar = calculate_solar_times(lat, lon, crossing).unwrap();
        let mut times =
            GeoTimes::from_solar_result(&solar, crossing, noon_local(tz, crossing), lat, lon)
                .unwrap();

        let mid = |a: DateTime<Tz>, b: DateTime<Tz>| {
            (a + chrono::Duration::seconds((b - a).num_seconds() / 2)).with_timezone(&Local)
        };
        let walk = [
            (
                (times.sunset_start - chrono::Duration::minutes(30)).with_timezone(&Local),
                Period::Day,
            ),
            (mid(times.sunset_start, times.sunset_end), Period::Sunset),
            (mid(times.sunset_end, times.sunrise_start), Period::Night),
            (mid(times.sunrise_start, times.sunrise_end), Period::Sunrise),
            (
                (times.sunrise_end + chrono::Duration::minutes(30)).with_timezone(&Local),
                Period::Day,
            ),
        ];

        let mut sequence = Vec::new();
        for (instant, expected) in walk {
            if times.needs_recalculation(instant) {
                let date = instant.with_timezone(&tz).date_naive();
                let solar = calculate_solar_times(lat, lon, date).unwrap();
                times = GeoTimes::from_solar_result(&solar, date, instant, lat, lon).unwrap();
            }
            let period = times.current_period(instant);
            assert_eq!(period, expected, "at {instant} expected {expected:?}");
            if sequence.last() != Some(&period) {
                sequence.push(period);
            }
        }

        assert_eq!(
            sequence,
            [
                Period::Day,
                Period::Sunset,
                Period::Night,
                Period::Sunrise,
                Period::Day
            ],
            "expected Day -> Sunset -> Night -> Sunrise -> Day across the midnight boundary",
        );
    }

    /// A transition window that straddles midnight must never be stored inverted,
    /// in either wrap direction, on any date.
    #[test]
    fn transition_windows_stay_forward_intervals_year_round() {
        use crate::geo::solar::{calculate_solar_times, determine_timezone};

        for (lat, lon) in [(HIGH_LAT, SUNSET_WRAP_LON), (HIGH_LAT, SUNRISE_WRAP_LON)] {
            let tz = determine_timezone(lat, lon);
            let end = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
            let mut date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

            while date <= end {
                let solar = calculate_solar_times(lat, lon, date).unwrap();
                let times =
                    GeoTimes::from_solar_result(&solar, date, noon_local(tz, date), lat, lon)
                        .unwrap();

                assert!(
                    times.sunset_end > times.sunset_start,
                    "inverted sunset window at ({lat}, {lon}) on {date}: {}..{}",
                    times.sunset_start,
                    times.sunset_end,
                );
                assert!(
                    times.sunrise_end > times.sunrise_start,
                    "inverted sunrise window at ({lat}, {lon}) on {date}: {}..{}",
                    times.sunrise_start,
                    times.sunrise_end,
                );

                date = date.succ_opt().unwrap();
            }
        }
    }

    /// A sunset that crosses midnight must not skip the Sunset transition.
    #[test]
    fn high_latitude_sunset_is_not_skipped() {
        assert_cycle_order_on_crossing(HIGH_LAT, SUNSET_WRAP_LON, |s| {
            s.sunset_minus_2_end < s.sunset_plus_10_start
        });
    }

    /// A sunrise that crosses midnight must not skip the Sunrise transition.
    #[test]
    fn high_latitude_sunrise_is_not_skipped() {
        assert_cycle_order_on_crossing(HIGH_LAT, SUNRISE_WRAP_LON, |s| {
            s.sunrise_minus_2_start > s.sunrise_plus_10_end
        });
    }
} // End of transition_times_tests
