//! Timezone-based coordinate detection for automatic location determination.
//!
//! This module provides functionality to detect user coordinates based on
//! system timezone information as a fallback when no manual coordinates are provided.

use crate::geo::city_selector::find_cities_near_coordinate;
use crate::logger::Log;
use anyhow::{Context, Result};
use chrono_tz::Tz;

/// Detect coordinates based on system timezone
///
/// This function attempts to determine user location by:
/// 1. Getting the system timezone
/// 2. Looking up typical coordinates for that timezone
/// 3. Finding the nearest major city to those coordinates
/// 4. Returning the city's precise coordinates
///
/// # Returns
/// * `Ok((latitude, longitude, city_name))` - Detected coordinates and city
/// * `Err(_)` - If timezone detection fails or no suitable coordinates found
pub fn detect_coordinates_from_timezone() -> Result<(f64, f64, String)> {
    Log::log_block_start("Automatic location detection");
    Log::log_indented("Detecting coordinates from system timezone...");

    // Get system timezone
    let system_tz = get_system_timezone().context("Failed to detect system timezone")?;

    Log::log_indented(&format!("Detected timezone: {}", system_tz));

    // Get approximate coordinates for this timezone
    let (approx_lat, approx_lon) =
        get_timezone_coordinates(&system_tz).context("Failed to find coordinates for timezone")?;

    let lat_dir = if approx_lat >= 0.0 { "N" } else { "S" };
    let lon_dir = if approx_lon >= 0.0 { "E" } else { "W" };
    Log::log_indented(&format!(
        "Timezone center: {:.2}°{}, {:.2}°{}",
        approx_lat.abs(),
        lat_dir,
        approx_lon.abs(),
        lon_dir
    ));

    // Find the nearest major city to these coordinates
    let nearby_cities = find_cities_near_coordinate(approx_lat, approx_lon, 5);

    if let Some(city) = nearby_cities.first() {
        Log::log_indented(&format!(
            "Closest major city: {}, {}",
            city.name, city.country
        ));
        let lat_dir = if city.latitude >= 0.0 { "N" } else { "S" };
        let lon_dir = if city.longitude >= 0.0 { "E" } else { "W" };
        Log::log_indented(&format!(
            "Using coordinates: {:.4}°{}, {:.4}°{}",
            city.latitude.abs(),
            lat_dir,
            city.longitude.abs(),
            lon_dir
        ));

        Ok((
            city.latitude,
            city.longitude,
            format!("{}, {}", city.name, city.country),
        ))
    } else {
        anyhow::bail!("No major cities found near timezone coordinates")
    }
}

/// Get the system timezone
pub fn get_system_timezone() -> Result<Tz> {
    // Try multiple methods to detect system timezone

    // Method 1: Check TZ environment variable
    if let Ok(tz_str) = std::env::var("TZ") {
        if let Ok(tz) = tz_str.parse::<Tz>() {
            return Ok(tz);
        }
    }

    // Method 2: Try to read /etc/timezone (Debian/Ubuntu)
    if let Ok(tz_content) = std::fs::read_to_string("/etc/timezone") {
        let tz_str = tz_content.trim();
        if let Ok(tz) = tz_str.parse::<Tz>() {
            return Ok(tz);
        }
    }

    // Method 3: Try to read /etc/localtime symlink (most Linux distros)
    if let Ok(link_target) = std::fs::read_link("/etc/localtime") {
        if let Some(path_str) = link_target.to_str() {
            // Extract timezone from path like "/usr/share/zoneinfo/America/New_York"
            if let Some(tz_part) = path_str.strip_prefix("/usr/share/zoneinfo/") {
                if let Ok(tz) = tz_part.parse::<Tz>() {
                    return Ok(tz);
                }
            }
        }
    }

    // Method 4: Try timedatectl (systemd systems)
    if let Ok(output) = std::process::Command::new("timedatectl")
        .arg("show")
        .arg("--property=Timezone")
        .arg("--value")
        .output()
    {
        if output.status.success() {
            let tz_string = String::from_utf8_lossy(&output.stdout);
            let tz_str = tz_string.trim();
            if let Ok(tz) = tz_str.parse::<Tz>() {
                return Ok(tz);
            }
        }
    }

    anyhow::bail!("Unable to detect system timezone")
}

/// Get approximate coordinates for a timezone
///
/// This function maps common timezones to their approximate center coordinates.
/// For timezones spanning large areas, this picks a representative major city.
fn get_timezone_coordinates(tz: &Tz) -> Result<(f64, f64)> {
    let tz_str = tz.to_string();

    // Major timezone coordinate mappings
    // Format: (latitude, longitude) - using major city coordinates
    let coords = match tz_str.as_str() {
        // North America - Eastern
        "America/New_York" | "America/Detroit" | "America/Louisville" | "America/Montreal"
        | "America/Toronto" => (40.7128, -74.0060), // NYC

        "America/Miami" | "America/Nassau" => (25.7617, -80.1918), // Miami
        "America/Havana" => (23.1136, -82.3666),                   // Havana

        // North America - Central
        "America/Chicago"
        | "America/Indiana/Knox"
        | "America/Indiana/Tell_City"
        | "America/Menominee"
        | "America/North_Dakota/Beulah"
        | "America/North_Dakota/Center"
        | "America/North_Dakota/New_Salem"
        | "America/Winnipeg" => (41.8781, -87.6298), // Chicago

        "America/Mexico_City" | "America/Merida" => (19.4326, -99.1332), // Mexico City
        "America/Guatemala" => (14.6349, -90.5069),                      // Guatemala City

        // North America - Mountain
        "America/Denver" | "America/Boise" | "America/Shiprock" => (39.7392, -104.9903), // Denver
        "America/Phoenix" => (33.4484, -112.0740),                                       // Phoenix
        "America/Edmonton" | "America/Calgary" => (51.0447, -114.0719),                  // Calgary

        // North America - Pacific
        "America/Los_Angeles" | "America/Tijuana" => (34.0522, -118.2437), // LA
        "America/Vancouver" => (49.2827, -123.1207),                       // Vancouver
        "America/Seattle" => (47.6062, -122.3321),                         // Seattle

        // North America - Alaska/Hawaii
        "America/Anchorage" | "America/Juneau" => (61.2181, -149.9003), // Anchorage
        "Pacific/Honolulu" => (21.3099, -157.8581),                     // Honolulu

        // Europe - Western
        "Europe/London" | "Europe/Dublin" => (51.5074, -0.1278), // London
        "Europe/Lisbon" => (38.7223, -9.1393),                   // Lisbon

        // Europe - Central
        "Europe/Berlin" | "Europe/Munich" => (52.5200, 13.4050), // Berlin
        "Europe/Paris" | "Europe/Monaco" => (48.8566, 2.3522),   // Paris
        "Europe/Rome" | "Europe/Vatican" => (41.9028, 12.4964),  // Rome
        "Europe/Madrid" => (40.4168, -3.7038),                   // Madrid
        "Europe/Amsterdam" => (52.3676, 4.9041),                 // Amsterdam
        "Europe/Brussels" => (50.8503, 4.3517),                  // Brussels
        "Europe/Zurich" => (47.3769, 8.5417),                    // Zurich
        "Europe/Vienna" => (48.2082, 16.3738),                   // Vienna
        "Europe/Prague" => (50.0755, 14.4378),                   // Prague
        "Europe/Warsaw" => (52.2297, 21.0122),                   // Warsaw
        "Europe/Stockholm" => (59.3293, 18.0686),                // Stockholm

        // Europe - Eastern
        "Europe/Moscow" => (55.7558, 37.6176),    // Moscow
        "Europe/Kiev" => (50.4501, 30.5234),      // Kiev
        "Europe/Bucharest" => (44.4268, 26.1025), // Bucharest
        "Europe/Athens" => (37.9755, 23.7348),    // Athens
        "Europe/Helsinki" => (60.1699, 24.9384),  // Helsinki

        // Asia - East
        "Asia/Tokyo" | "Asia/Osaka" => (35.6762, 139.6503), // Tokyo
        "Asia/Seoul" => (37.5665, 126.9780),                // Seoul
        "Asia/Shanghai" | "Asia/Beijing" => (39.9042, 116.4074), // Beijing
        "Asia/Hong_Kong" => (22.3193, 114.1694),            // Hong Kong
        "Asia/Taipei" => (25.0330, 121.5654),               // Taipei

        // Asia - Southeast
        "Asia/Singapore" => (1.3521, 103.8198), // Singapore
        "Asia/Bangkok" => (13.7563, 100.5018),  // Bangkok
        "Asia/Jakarta" => (-6.2088, 106.8456),  // Jakarta
        "Asia/Manila" => (14.5995, 120.9842),   // Manila
        "Asia/Kuala_Lumpur" => (3.1390, 101.6869), // Kuala Lumpur
        "Asia/Ho_Chi_Minh" => (10.8231, 106.6297), // Ho Chi Minh

        // Asia - South
        "Asia/Kolkata" | "Asia/Mumbai" => (19.0760, 72.8777), // Mumbai
        "Asia/Delhi" => (28.7041, 77.1025),                   // Delhi
        "Asia/Dhaka" => (23.8103, 90.4125),                   // Dhaka
        "Asia/Karachi" => (24.8607, 67.0011),                 // Karachi
        "Asia/Colombo" => (6.9271, 79.8612),                  // Colombo

        // Asia - Central/West
        "Asia/Tehran" => (35.6892, 51.3890),   // Tehran
        "Asia/Baghdad" => (33.3152, 44.3661),  // Baghdad
        "Asia/Riyadh" => (24.7136, 46.6753),   // Riyadh
        "Asia/Dubai" => (25.2048, 55.2708),    // Dubai
        "Asia/Tashkent" => (41.2995, 69.2401), // Tashkent

        // Australia/Oceania
        "Australia/Sydney" | "Australia/Melbourne" => (-33.8688, 151.2093), // Sydney
        "Australia/Perth" => (-31.9505, 115.8605),                          // Perth
        "Australia/Brisbane" => (-27.4698, 153.0251),                       // Brisbane
        "Australia/Adelaide" => (-34.9285, 138.6007),                       // Adelaide
        "Pacific/Auckland" => (-36.8485, 174.7633),                         // Auckland

        // Africa
        "Africa/Cairo" => (30.0444, 31.2357),         // Cairo
        "Africa/Lagos" => (6.5244, 3.3792),           // Lagos
        "Africa/Johannesburg" => (-26.2041, 28.0473), // Johannesburg
        "Africa/Nairobi" => (-1.2921, 36.8219),       // Nairobi
        "Africa/Casablanca" => (33.5731, -7.5898),    // Casablanca

        // South America
        "America/Sao_Paulo" | "America/Recife" => (-23.5558, -46.6396), // São Paulo
        "America/Argentina/Buenos_Aires" => (-34.6118, -58.3960),       // Buenos Aires
        "America/Lima" => (-12.0464, -77.0428),                         // Lima
        "America/Bogota" => (4.7110, -74.0721),                         // Bogotá
        "America/Santiago" => (-33.4489, -70.6693),                     // Santiago
        "America/Caracas" => (10.4806, -66.9036),                       // Caracas

        // Default fallback - use UTC coordinates (London)
        _ => {
            Log::log_indented(&format!(
                "Unknown timezone '{}', using UTC fallback",
                tz_str
            ));
            (51.5074, -0.1278) // London as fallback
        }
    };

    Ok(coords)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timezone_coordinate_mapping() {
        // Test some common timezones
        let eastern_tz: Tz = "America/New_York".parse().unwrap();
        let coords = get_timezone_coordinates(&eastern_tz).unwrap();
        assert_eq!(coords, (40.7128, -74.0060)); // NYC coordinates

        let central_tz: Tz = "America/Chicago".parse().unwrap();
        let coords = get_timezone_coordinates(&central_tz).unwrap();
        assert_eq!(coords, (41.8781, -87.6298)); // Chicago coordinates

        let london_tz: Tz = "Europe/London".parse().unwrap();
        let coords = get_timezone_coordinates(&london_tz).unwrap();
        assert_eq!(coords, (51.5074, -0.1278)); // London coordinates
    }

    #[test]
    fn test_unknown_timezone_fallback() {
        let unknown_tz: Tz = "UTC".parse().unwrap();
        let coords = get_timezone_coordinates(&unknown_tz).unwrap();
        assert_eq!(coords, (51.5074, -0.1278)); // Should fallback to London
    }

    #[test]
    fn test_coordinate_bounds() {
        let tz: Tz = "America/New_York".parse().unwrap();
        let (lat, lon) = get_timezone_coordinates(&tz).unwrap();

        // Coordinates should be within valid ranges
        assert!((-90.0..=90.0).contains(&lat));
        assert!((-180.0..=180.0).contains(&lon));
    }
}
