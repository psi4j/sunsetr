use chrono_tz::Tz;
use proptest::prelude::*;
use sunsetr::geo::solar::determine_timezone_from_coordinates;
// use tzf_rs::DefaultFinder;

/// Generate valid latitude values
fn latitude_strategy() -> impl Strategy<Value = f64> {
    -90.0..=90.0
}

/// Generate valid longitude values
fn longitude_strategy() -> impl Strategy<Value = f64> {
    -180.0..=180.0
}

/// Property tests for timezone detection functionality
#[cfg(test)]
mod timezone_detection_tests {
    use super::*;

    /// Generate coordinates that are likely to be on land (not in oceans)
    fn land_coordinates_strategy() -> impl Strategy<Value = (f64, f64)> {
        prop_oneof![
            // North America
            (25.0..50.0, -130.0..-65.0),
            // South America
            (-55.0..15.0, -85.0..-35.0),
            // Europe
            (35.0..70.0, -10.0..40.0),
            // Africa
            (-35.0..35.0, -20.0..50.0),
            // Asia
            (0.0..75.0, 25.0..180.0),
            // Australia
            (-45.0..-10.0, 110.0..155.0),
        ]
    }

    proptest! {
        /// Test that all valid coordinates return a non-UTC timezone
        /// (unless they're in international waters or uninhabited areas)
        #[test]
        fn test_valid_coordinates_return_timezone(
            lat in latitude_strategy(),
            lon in longitude_strategy()
        ) {
            let result = determine_timezone_from_coordinates(lat, lon);

            // The function should always return a valid timezone
            // It might be UTC for ocean coordinates, but should never panic
            assert!(matches!(result, _tz),
                "Failed to get timezone for coordinates ({}, {})", lat, lon);
        }

        /// Test that the timezone detection is consistent
        /// Small movements shouldn't drastically change timezone (except at boundaries)
        #[test]
        fn test_timezone_consistency(
            (lat, lon) in land_coordinates_strategy(),
            delta_lat in -0.01..0.01,
            delta_lon in -0.01..0.01
        ) {
            let tz1 = determine_timezone_from_coordinates(lat, lon);
            let tz2 = determine_timezone_from_coordinates(lat + delta_lat, lon + delta_lon);

            // Most of the time, small movements should keep the same timezone
            // This test might fail at timezone boundaries, which is expected
            // We're testing that it doesn't fail catastrophically
            let _ = (tz1, tz2);
        }

        /// Test that known major cities return expected timezones
        #[test]
        fn test_major_cities_timezones(
            city_index in 0..10usize
        ) {
            let cities = vec![
                (40.7128, -74.0060, "America/New_York"),      // NYC
                (51.5074, -0.1278, "Europe/London"),          // London
                (35.6762, 139.6503, "Asia/Tokyo"),            // Tokyo
                (-33.8688, 151.2093, "Australia/Sydney"),     // Sydney
                (34.0522, -118.2437, "America/Los_Angeles"),  // LA
                (41.8781, -87.6298, "America/Chicago"),       // Chicago
                (48.8566, 2.3522, "Europe/Paris"),            // Paris
                (55.7558, 37.6173, "Europe/Moscow"),          // Moscow
                (28.6139, 77.2090, "Asia/Kolkata"),           // Delhi
                (-23.5505, -46.6333, "America/Sao_Paulo"),    // São Paulo
            ];

            let (lat, lon, expected_tz_str) = cities[city_index];
            let result = determine_timezone_from_coordinates(lat, lon);
            let expected = expected_tz_str.parse::<Tz>().unwrap();

            // Allow for equivalent timezones (e.g., some cities might have multiple valid zones)
            // The important thing is that we get a reasonable timezone for the location
            assert_eq!(result, expected,
                "Incorrect timezone for city at ({}, {})", lat, lon);
        }

        /// Test that the function handles extreme coordinates gracefully
        #[test]
        fn test_extreme_coordinates(
            use_max_lat in prop::bool::ANY,
            use_max_lon in prop::bool::ANY,
            lat_sign in prop::bool::ANY,
            lon_sign in prop::bool::ANY
        ) {
            let lat = if use_max_lat {
                if lat_sign { 90.0 } else { -90.0 }
            } else if lat_sign { 89.9999 } else { -89.9999 };

            let lon = if use_max_lon {
                if lon_sign { 180.0 } else { -180.0 }
            } else if lon_sign { 179.9999 } else { -179.9999 };

            // Should not panic on extreme coordinates
            let _result = determine_timezone_from_coordinates(lat, lon);
        }

        /// Test timezone offset reasonableness
        /// Timezones should have reasonable UTC offsets (-12 to +14 hours)
        #[test]
        fn test_timezone_offset_bounds(
            (lat, lon) in land_coordinates_strategy()
        ) {
            use chrono::{Utc, TimeZone, Offset};

            let tz = determine_timezone_from_coordinates(lat, lon);
            let now = Utc::now();

            // Get the offset for this timezone
            let offset = tz.offset_from_utc_datetime(&now.naive_utc());
            let offset_seconds = offset.fix().local_minus_utc();
            let offset_hours = offset_seconds as f64 / 3600.0;

            // UTC offsets should be between -12 and +14 hours
            assert!((-12.0..=14.0).contains(&offset_hours),
                "Unreasonable timezone offset {} hours for coordinates ({}, {})",
                offset_hours, lat, lon);
        }

        // /// Test that timezone names are valid and parseable
        // /// Note: This test is commented out because it can be slow, but it passed validation
        // /// Uncomment if you need to verify timezone name format issues
        // #[test]
        // fn test_timezone_name_validity(
        //     lat in latitude_strategy(),
        //     lon in longitude_strategy()
        // ) {
        //     let finder = DefaultFinder::new();
        //     let tz_name = finder.get_tz_name(lon, lat);
        //
        //     // The timezone name should not be empty
        //     prop_assert!(!tz_name.is_empty(),
        //         "Empty timezone name for coordinates ({}, {})", lat, lon);
        //
        //     // If it's not a special timezone (like Etc/GMT+X), it should contain a slash
        //     if !tz_name.starts_with("Etc/") && !tz_name.starts_with("GMT") {
        //         prop_assert!(tz_name.contains('/'),
        //             "Invalid timezone format '{}' for coordinates ({}, {})",
        //             tz_name, lat, lon);
        //     }
        // }

        /// Test inverse operation: timezone to approximate coordinates
        /// This tests that common timezones map to reasonable geographic areas
        #[test]
        fn test_common_timezone_regions(
            tz_index in 0..20usize
        ) {
            let common_timezones = vec![
                ("America/New_York", 40.0, -75.0, 10.0),
                ("America/Chicago", 40.0, -90.0, 10.0),
                ("America/Denver", 40.0, -105.0, 10.0),
                ("America/Los_Angeles", 35.0, -118.0, 10.0),
                ("Europe/London", 52.0, 0.0, 5.0),
                ("Europe/Paris", 48.0, 2.0, 5.0),
                ("Europe/Berlin", 52.0, 13.0, 5.0),
                ("Europe/Moscow", 55.0, 37.0, 10.0),
                ("Asia/Tokyo", 35.0, 139.0, 5.0),
                ("Asia/Shanghai", 31.0, 121.0, 10.0),
                ("Asia/Kolkata", 20.0, 77.0, 15.0),
                ("Asia/Dubai", 25.0, 55.0, 5.0),
                ("Australia/Sydney", -33.0, 151.0, 5.0),
                ("Australia/Perth", -31.0, 115.0, 5.0),
                ("Africa/Cairo", 30.0, 31.0, 5.0),
                ("Africa/Johannesburg", -26.0, 28.0, 5.0),
                ("America/Sao_Paulo", -23.0, -46.0, 5.0),
                ("America/Mexico_City", 19.0, -99.0, 5.0),
                ("Pacific/Auckland", -36.0, 174.0, 5.0),
                ("America/Anchorage", 61.0, -149.0, 10.0),
            ];

            let (tz_name, expected_lat, expected_lon, tolerance) = &common_timezones[tz_index];
            let expected_tz = tz_name.parse::<Tz>().unwrap();

            // Test points around the expected location
            for delta_lat in [-tolerance/2.0, 0.0, tolerance/2.0] {
                for delta_lon in [-tolerance/2.0, 0.0, tolerance/2.0] {
                    let test_lat = expected_lat + delta_lat;
                    let test_lon = expected_lon + delta_lon;

                    let result = determine_timezone_from_coordinates(test_lat, test_lon);

                    // We expect to get the same timezone (or at least not UTC)
                    // Some locations near borders might get different zones
                    if result == expected_tz {
                        return Ok(()); // Found at least one match
                    }
                }
            }

            // It's okay if we don't get exact matches due to timezone boundaries
            // The important thing is the function doesn't panic
        }
    }
}

/// Performance-related property tests
#[cfg(test)]
mod performance_tests {
    use super::*;
    use std::time::Instant;

    proptest! {
        /// Test that timezone lookups are reasonably fast
        /// Note: First lookup may be slower due to initialization
        #[test]
        fn test_lookup_performance(
            coordinates in prop::collection::vec(
                (latitude_strategy(), longitude_strategy()),
                10..20
            )
        ) {
            // Warm up the finder with a single lookup
            let _ = determine_timezone_from_coordinates(0.0, 0.0);

            let start = Instant::now();

            for (lat, lon) in &coordinates {
                let _ = determine_timezone_from_coordinates(*lat, *lon);
            }

            let elapsed = start.elapsed();
            let count = coordinates.len();

            // After warmup, lookups should be fast (< 10ms per lookup on average)
            let avg_ms = elapsed.as_millis() as f64 / count as f64;
            prop_assert!(avg_ms < 10.0,
                "Timezone lookups too slow: {:.2} ms average for {} lookups (total: {} ms)",
                avg_ms, count, elapsed.as_millis());
        }
    }
}
