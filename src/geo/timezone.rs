//! Timezone-based coordinate detection for automatic location determination.
//!
//! This module provides functionality to detect user coordinates based on
//! system timezone information as a fallback when no manual coordinates are provided.
//! It employs multiple detection strategies to determine the system timezone and
//! maps it to approximate geographic coordinates.
//!
//! ## Detection Strategy
//!
//! The module attempts timezone detection in the following order:
//! 1. **TZ environment variable**: Direct timezone specification
//! 2. **/etc/timezone file**: Debian/Ubuntu systems
//! 3. **/etc/localtime symlink**: Most modern Linux distributions
//! 4. **timedatectl command**: systemd-based systems
//!
//! ## Coordinate Mapping
//!
//! Once a timezone is detected, it's mapped to precise coordinates of a
//! representative city within that timezone. The module includes comprehensive
//! mappings for 466 timezones worldwide, generated from authoritative
//! geographic databases (GeoNames and OpenStreetMap).
//!
//! ## Fallback Behavior
//!
//! - Unknown timezones default to UTC (London coordinates)
//! - Failed detection results in an error rather than silent fallback
//! - All mappings provide precise city coordinates and country information

use crate::geo::city_selector::CityInfo;
use anyhow::{Context, Result};
use chrono_tz::Tz;

/// Detect coordinates based on system timezone.
///
/// This function attempts to determine user location by:
/// 1. Getting the system timezone using multiple detection methods
/// 2. Looking up precise coordinates from comprehensive timezone database
/// 3. Returning the mapped city's coordinates and name
///
/// This provides accurate location data when users don't specify
/// coordinates manually, ensuring sunset/sunrise times are precisely
/// calculated for their timezone.
///
/// # Returns
/// * `Ok((latitude, longitude, city_name))` - Detected coordinates and city
/// * `Err(_)` - If timezone detection fails or no suitable coordinates found
///
/// # Errors
/// Returns an error if:
/// - System timezone cannot be detected
///
/// Note: Unknown timezones fall back to UTC (London) coordinates
///
/// # Example
/// ```no_run
/// # use sunsetr::geo::timezone::detect_coordinates_from_timezone;
/// match detect_coordinates_from_timezone() {
///     Ok((lat, lon, city)) => {
///         println!("Detected location: {} at {:.4}°, {:.4}°", city, lat, lon);
///     }
///     Err(e) => {
///         eprintln!("Could not detect location: {}", e);
///     }
/// }
/// ```
pub fn detect_coordinates_from_timezone() -> Result<(f64, f64, String)> {
    log_block_start!("Automatic location detection");
    log_indented!("Detecting coordinates from system timezone...");

    // Get system timezone
    let system_tz = get_system_timezone().context("Failed to detect system timezone")?;

    log_indented!("Detected timezone: {system_tz}");

    // Use comprehensive timezone-to-city mapping (466 timezones covered)
    if let Some(city) = get_city_from_timezone(&system_tz.to_string()) {
        log_indented!("Timezone mapping: {}, {}", city.name, city.country);
        let lat_dir = if city.latitude >= 0.0 { "N" } else { "S" };
        let lon_dir = if city.longitude >= 0.0 { "E" } else { "W" };
        log_indented!(
            "Coordinates: {:.4}°{}, {:.4}°{}",
            city.latitude.abs(),
            lat_dir,
            city.longitude.abs(),
            lon_dir
        );

        return Ok((city.latitude, city.longitude, city.name));
    }

    // Fallback for unmapped timezones - use UTC (London) coordinates
    log_indented!("Unknown timezone '{system_tz}' - using UTC fallback (London)");

    let london_lat = 51.5074f64;
    let london_lon = -0.1278f64;

    log_indented!(
        "Fallback coordinates: {:.4}°N, {:.4}°W",
        london_lat,
        london_lon.abs()
    );

    Ok((london_lat, london_lon, "London, United Kingdom".to_string()))
}

/// Get the system timezone using multiple detection methods.
///
/// This function attempts to detect the system timezone through various
/// platform-specific methods, trying each in order until one succeeds.
///
/// # Detection Methods
///
/// 1. **TZ environment variable**: Checked first as it takes precedence
/// 2. **/etc/timezone**: Common on Debian/Ubuntu systems
/// 3. **/etc/localtime**: Symlink used by most modern Linux distributions
/// 4. **timedatectl**: Command available on systemd-based systems
///
/// # Returns
/// * `Ok(Tz)` - The detected timezone
/// * `Err(_)` - If all detection methods fail
///
/// # Errors
/// Returns an error if:
/// - No detection method succeeds
/// - Detected timezone string cannot be parsed
pub fn get_system_timezone() -> Result<Tz> {
    // Try multiple methods to detect system timezone

    // Method 1: Check TZ environment variable
    if let Ok(tz_str) = std::env::var("TZ")
        && let Ok(tz) = tz_str.parse::<Tz>()
    {
        return Ok(tz);
    }

    // Method 2: Try to read /etc/timezone (Debian/Ubuntu)
    if let Ok(tz_content) = std::fs::read_to_string("/etc/timezone") {
        let tz_str = tz_content.trim();
        if let Ok(tz) = tz_str.parse::<Tz>() {
            return Ok(tz);
        }
    }

    // Method 3: Try to read /etc/localtime symlink (most Linux distros)
    if let Ok(link_target) = std::fs::read_link("/etc/localtime")
        && let Some(path_str) = link_target.to_str()
    {
        // Extract timezone from path like "/usr/share/zoneinfo/America/New_York"
        if let Some(tz_part) = path_str.strip_prefix("/usr/share/zoneinfo/")
            && let Ok(tz) = tz_part.parse::<Tz>()
        {
            return Ok(tz);
        }
    }

    // Method 4: Try timedatectl (systemd systems)
    if let Ok(output) = std::process::Command::new("timedatectl")
        .arg("show")
        .arg("--property=Timezone")
        .arg("--value")
        .output()
        && output.status.success()
    {
        let tz_string = String::from_utf8_lossy(&output.stdout);
        let tz_str = tz_string.trim();
        if let Ok(tz) = tz_str.parse::<Tz>() {
            return Ok(tz);
        }
    }

    anyhow::bail!("Unable to detect system timezone")
}

/// Get city information directly from timezone string
/// This provides accurate city data eliminating the need for coordinate approximation and distance calculations
pub(crate) fn get_city_from_timezone(tz_str: &str) -> Option<CityInfo> {
    match tz_str {
        "Africa/Abidjan" => Some(CityInfo {
            name: "Abidjan".to_string(),
            country: "Ivory Coast".to_string(),
            latitude: 5.35444,
            longitude: -4.00167,
        }),
        "Africa/Accra" => Some(CityInfo {
            name: "Accra".to_string(),
            country: "Ghana".to_string(),
            latitude: 5.55,
            longitude: -0.2166667,
        }),
        "Africa/Addis_Ababa" => Some(CityInfo {
            name: "Addis Ababa".to_string(),
            country: "Ethiopia".to_string(),
            latitude: 9.0333333,
            longitude: 38.7000008,
        }),
        "Africa/Algiers" => Some(CityInfo {
            name: "Algiers".to_string(),
            country: "Algeria".to_string(),
            latitude: 36.7630556,
            longitude: 3.0505557,
        }),
        "Africa/Asmara" => Some(CityInfo {
            name: "Asmara".to_string(),
            country: "Eritrea".to_string(),
            latitude: 15.3333333,
            longitude: 38.9333344,
        }),
        "Africa/Asmera" => Some(CityInfo {
            name: "Asmera".to_string(),
            country: "Eritrea".to_string(),
            latitude: 15.33805,
            longitude: 38.93184,
        }),
        "Africa/Bamako" => Some(CityInfo {
            name: "Bamako".to_string(),
            country: "Mali".to_string(),
            latitude: 12.60915,
            longitude: -7.97522,
        }),
        "Africa/Bangui" => Some(CityInfo {
            name: "Bangui".to_string(),
            country: "Central African Republic".to_string(),
            latitude: 4.3666667,
            longitude: 18.583334,
        }),
        "Africa/Banjul" => Some(CityInfo {
            name: "Banjul".to_string(),
            country: "Gambia, The".to_string(),
            latitude: 13.4564084,
            longitude: -16.5812874,
        }),
        "Africa/Bissau" => Some(CityInfo {
            name: "Bissau".to_string(),
            country: "Guinea-Bissau".to_string(),
            latitude: 11.86357,
            longitude: -15.59767,
        }),
        "Africa/Blantyre" => Some(CityInfo {
            name: "Blantyre".to_string(),
            country: "Malawi".to_string(),
            latitude: -15.7833333,
            longitude: 35.0,
        }),
        "Africa/Brazzaville" => Some(CityInfo {
            name: "Brazzaville".to_string(),
            country: "Congo Republic".to_string(),
            latitude: -4.26613,
            longitude: 15.28318,
        }),
        "Africa/Bujumbura" => Some(CityInfo {
            name: "Bujumbura".to_string(),
            country: "Burundi".to_string(),
            latitude: -3.38193,
            longitude: 29.36142,
        }),
        "Africa/Cairo" => Some(CityInfo {
            name: "Cairo".to_string(),
            country: "Egypt".to_string(),
            latitude: 30.06263,
            longitude: 31.24967,
        }),
        "Africa/Casablanca" => Some(CityInfo {
            name: "Casablanca".to_string(),
            country: "Morocco".to_string(),
            latitude: 33.59,
            longitude: -7.6100001,
        }),
        "Africa/Ceuta" => Some(CityInfo {
            name: "Ceuta".to_string(),
            country: "Spain".to_string(),
            latitude: 35.8893282,
            longitude: -5.3197861,
        }),
        "Africa/Conakry" => Some(CityInfo {
            name: "Conakry".to_string(),
            country: "Guinea".to_string(),
            latitude: 9.53795,
            longitude: -13.67729,
        }),
        "Africa/Dakar" => Some(CityInfo {
            name: "Dakar".to_string(),
            country: "Senegal".to_string(),
            latitude: 14.6951119,
            longitude: -17.4438858,
        }),
        "Africa/Dar_es_Salaam" => Some(CityInfo {
            name: "Dar es Salaam".to_string(),
            country: "Tanzania".to_string(),
            latitude: -6.8,
            longitude: 39.2833328,
        }),
        "Africa/Djibouti" => Some(CityInfo {
            name: "Djibouti".to_string(),
            country: "Djibouti".to_string(),
            latitude: 11.595,
            longitude: 43.148056,
        }),
        "Africa/Douala" => Some(CityInfo {
            name: "Douala".to_string(),
            country: "Cameroon".to_string(),
            latitude: 4.0502778,
            longitude: 9.6999998,
        }),
        "Africa/El_Aaiun" => Some(CityInfo {
            name: "El Aaiun".to_string(),
            country: "Western Sahara".to_string(),
            latitude: 27.1418,
            longitude: -13.18797,
        }),
        "Africa/Freetown" => Some(CityInfo {
            name: "Freetown".to_string(),
            country: "Sierra Leone".to_string(),
            latitude: 8.6161901,
            longitude: -13.1988056,
        }),
        "Africa/Gaborone" => Some(CityInfo {
            name: "Gaborone".to_string(),
            country: "Botswana".to_string(),
            latitude: -24.65451,
            longitude: 25.90859,
        }),
        "Africa/Harare" => Some(CityInfo {
            name: "Harare".to_string(),
            country: "Zimbabwe".to_string(),
            latitude: -17.8177778,
            longitude: 31.0447216,
        }),
        "Africa/Johannesburg" => Some(CityInfo {
            name: "Johannesburg".to_string(),
            country: "South Africa".to_string(),
            latitude: -26.2,
            longitude: 28.083334,
        }),
        "Africa/Juba" => Some(CityInfo {
            name: "Juba".to_string(),
            country: "South Sudan".to_string(),
            latitude: 4.8606914,
            longitude: 31.5771892,
        }),
        "Africa/Kampala" => Some(CityInfo {
            name: "Kampala".to_string(),
            country: "Uganda".to_string(),
            latitude: 0.3155556,
            longitude: 32.5655556,
        }),
        "Africa/Khartoum" => Some(CityInfo {
            name: "Khartoum".to_string(),
            country: "Sudan".to_string(),
            latitude: 15.55177,
            longitude: 32.53241,
        }),
        "Africa/Kigali" => Some(CityInfo {
            name: "Kigali".to_string(),
            country: "Rwanda".to_string(),
            latitude: -1.94995,
            longitude: 30.05885,
        }),
        "Africa/Kinshasa" => Some(CityInfo {
            name: "Kinshasa".to_string(),
            country: "DR Congo".to_string(),
            latitude: -4.32758,
            longitude: 15.31357,
        }),
        "Africa/Lagos" => Some(CityInfo {
            name: "Lagos".to_string(),
            country: "Nigeria".to_string(),
            latitude: 6.4530556,
            longitude: 3.3958333,
        }),
        "Africa/Libreville" => Some(CityInfo {
            name: "Libreville".to_string(),
            country: "Gabon".to_string(),
            latitude: 0.3833333,
            longitude: 9.4499998,
        }),
        "Africa/Lome" => Some(CityInfo {
            name: "Lome".to_string(),
            country: "Togo".to_string(),
            latitude: 6.1319444,
            longitude: 1.2227778,
        }),
        "Africa/Luanda" => Some(CityInfo {
            name: "Luanda".to_string(),
            country: "Angola".to_string(),
            latitude: -8.8383333,
            longitude: 13.2344446,
        }),
        "Africa/Lubumbashi" => Some(CityInfo {
            name: "Lubumbashi".to_string(),
            country: "DR Congo".to_string(),
            latitude: -11.66089,
            longitude: 27.47938,
        }),
        "Africa/Lusaka" => Some(CityInfo {
            name: "Lusaka".to_string(),
            country: "Zambia".to_string(),
            latitude: -15.4166667,
            longitude: 28.2833328,
        }),
        "Africa/Malabo" => Some(CityInfo {
            name: "Malabo".to_string(),
            country: "Equatorial Guinea".to_string(),
            latitude: 3.75,
            longitude: 8.7833328,
        }),
        "Africa/Maputo" => Some(CityInfo {
            name: "Maputo".to_string(),
            country: "Mozambique".to_string(),
            latitude: -25.9652778,
            longitude: 32.5891685,
        }),
        "Africa/Maseru" => Some(CityInfo {
            name: "Maseru".to_string(),
            country: "Lesotho".to_string(),
            latitude: -29.3368097,
            longitude: 27.4758717,
        }),
        "Africa/Mbabane" => Some(CityInfo {
            name: "Mbabane".to_string(),
            country: "Eswatini".to_string(),
            latitude: -26.3166667,
            longitude: 31.1333332,
        }),
        "Africa/Mogadishu" => Some(CityInfo {
            name: "Mogadishu".to_string(),
            country: "Somalia".to_string(),
            latitude: 2.0666667,
            longitude: 45.3666649,
        }),
        "Africa/Monrovia" => Some(CityInfo {
            name: "Monrovia".to_string(),
            country: "Liberia".to_string(),
            latitude: 6.3105556,
            longitude: -10.8047218,
        }),
        "Africa/Nairobi" => Some(CityInfo {
            name: "Nairobi".to_string(),
            country: "Kenya".to_string(),
            latitude: -1.2833333,
            longitude: 36.8166656,
        }),
        "Africa/Ndjamena" => Some(CityInfo {
            name: "Ndjamena".to_string(),
            country: "Chad".to_string(),
            latitude: 12.10672,
            longitude: 15.0444,
        }),
        "Africa/Niamey" => Some(CityInfo {
            name: "Niamey".to_string(),
            country: "Niger".to_string(),
            latitude: 13.5166667,
            longitude: 2.1166668,
        }),
        "Africa/Nouakchott" => Some(CityInfo {
            name: "Nouakchott".to_string(),
            country: "Mauritania".to_string(),
            latitude: 18.1194444,
            longitude: -16.040556,
        }),
        "Africa/Ouagadougou" => Some(CityInfo {
            name: "Ouagadougou".to_string(),
            country: "Burkina Faso".to_string(),
            latitude: 12.36566,
            longitude: -1.53388,
        }),
        "Africa/Porto-Novo" => Some(CityInfo {
            name: "Porto-Novo".to_string(),
            country: "Benin".to_string(),
            latitude: 6.4833333,
            longitude: 2.6166668,
        }),
        "Africa/Sao_Tome" => Some(CityInfo {
            name: "Sao Tome".to_string(),
            country: "Sao Tome and Principe".to_string(),
            latitude: 0.3333333,
            longitude: 6.7333331,
        }),
        "Africa/Timbuktu" => Some(CityInfo {
            name: "Timbuktu".to_string(),
            country: "Mali".to_string(),
            latitude: 16.730556,
            longitude: -3.0075,
        }),
        "Africa/Tripoli" => Some(CityInfo {
            name: "Tripoli".to_string(),
            country: "Libya".to_string(),
            latitude: 32.8925,
            longitude: 13.1800003,
        }),
        "Africa/Tunis" => Some(CityInfo {
            name: "Tunis".to_string(),
            country: "Tunisia".to_string(),
            latitude: 36.8027778,
            longitude: 10.1797218,
        }),
        "Africa/Windhoek" => Some(CityInfo {
            name: "Windhoek".to_string(),
            country: "Namibia".to_string(),
            latitude: -22.5637353,
            longitude: 16.9798244,
        }),
        "America/Adak" => Some(CityInfo {
            name: "Adak".to_string(),
            country: "United States".to_string(),
            latitude: 51.87395,
            longitude: -176.63402,
        }),
        "America/Anchorage" => Some(CityInfo {
            name: "Anchorage".to_string(),
            country: "United States".to_string(),
            latitude: 61.2180556,
            longitude: -149.9002838,
        }),
        "America/Anguilla" => Some(CityInfo {
            name: "Anguilla".to_string(),
            country: "United Kingdom".to_string(),
            latitude: 18.3895845,
            longitude: -63.5298092,
        }),
        "America/Antigua" => Some(CityInfo {
            name: "Saint John's".to_string(),
            country: "Antigua and Barbuda".to_string(),
            latitude: 17.121389,
            longitude: -61.843611,
        }),
        "America/Araguaina" => Some(CityInfo {
            name: "Araguaina".to_string(),
            country: "Brazil".to_string(),
            latitude: -7.19111,
            longitude: -48.20722,
        }),
        "America/Argentina/Buenos_Aires" => Some(CityInfo {
            name: "Buenos Aires".to_string(),
            country: "Argentina".to_string(),
            latitude: -34.5761256,
            longitude: -58.4088135,
        }),
        "America/Argentina/Catamarca" => Some(CityInfo {
            name: "Catamarca".to_string(),
            country: "Argentina".to_string(),
            latitude: -28.469581,
            longitude: -65.7795441,
        }),
        "America/Argentina/ComodRivadavia" => Some(CityInfo {
            name: "Comodoro Rivadavia".to_string(),
            country: "Argentina".to_string(),
            latitude: -45.785278,
            longitude: -67.465556,
        }),
        "America/Argentina/Cordoba" => Some(CityInfo {
            name: "Cordoba".to_string(),
            country: "Argentina".to_string(),
            latitude: -31.4,
            longitude: -64.1833344,
        }),
        "America/Argentina/Jujuy" => Some(CityInfo {
            name: "Jujuy".to_string(),
            country: "Argentina".to_string(),
            latitude: -24.194444,
            longitude: -65.299444,
        }),
        "America/Argentina/La_Rioja" => Some(CityInfo {
            name: "La Rioja".to_string(),
            country: "Argentina".to_string(),
            latitude: -29.4333333,
            longitude: -66.8499985,
        }),
        "America/Argentina/Mendoza" => Some(CityInfo {
            name: "Mendoza".to_string(),
            country: "Argentina".to_string(),
            latitude: -32.8833333,
            longitude: -68.8166656,
        }),
        "America/Argentina/Rio_Gallegos" => Some(CityInfo {
            name: "Rio Gallegos".to_string(),
            country: "Argentina".to_string(),
            latitude: -51.6333333,
            longitude: -69.2166672,
        }),
        "America/Argentina/Salta" => Some(CityInfo {
            name: "Salta".to_string(),
            country: "Argentina".to_string(),
            latitude: -24.7833333,
            longitude: -65.4166641,
        }),
        "America/Argentina/San_Juan" => Some(CityInfo {
            name: "San Juan".to_string(),
            country: "Argentina".to_string(),
            latitude: -31.5375,
            longitude: -68.5363922,
        }),
        "America/Argentina/San_Luis" => Some(CityInfo {
            name: "San Luis".to_string(),
            country: "Argentina".to_string(),
            latitude: -33.3,
            longitude: -66.3499985,
        }),
        "America/Argentina/Tucuman" => Some(CityInfo {
            name: "Tucuman".to_string(),
            country: "Argentina".to_string(),
            latitude: -26.808278,
            longitude: -65.217499,
        }),
        "America/Argentina/Ushuaia" => Some(CityInfo {
            name: "Ushuaia".to_string(),
            country: "Argentina".to_string(),
            latitude: -54.8,
            longitude: -68.3000031,
        }),
        "America/Aruba" => Some(CityInfo {
            name: "Aruba".to_string(),
            country: "Aruba".to_string(),
            latitude: 12.52398,
            longitude: -70.02703,
        }),
        "America/Asuncion" => Some(CityInfo {
            name: "Asuncion".to_string(),
            country: "Paraguay".to_string(),
            latitude: -25.2666667,
            longitude: -57.6666679,
        }),
        "America/Atikokan" => Some(CityInfo {
            name: "Atikokan".to_string(),
            country: "Canada".to_string(),
            latitude: 48.7739,
            longitude: -91.6386,
        }),
        "America/Atka" => Some(CityInfo {
            name: "Atka".to_string(),
            country: "United States".to_string(),
            latitude: 52.19611,
            longitude: -174.20056,
        }),
        "America/Bahia" => Some(CityInfo {
            name: "Bahia".to_string(),
            country: "Brazil".to_string(),
            latitude: -12.908611,
            longitude: -38.3225,
        }),
        "America/Bahia_Banderas" => Some(CityInfo {
            name: "Bahia Banderas".to_string(),
            country: "Mexico".to_string(),
            latitude: 20.80426,
            longitude: -105.30913,
        }),
        "America/Barbados" => Some(CityInfo {
            name: "Barbados".to_string(),
            country: "Barbados".to_string(),
            latitude: 13.16453,
            longitude: -59.55165,
        }),
        "America/Belem" => Some(CityInfo {
            name: "Belem".to_string(),
            country: "Brazil".to_string(),
            latitude: -1.45583,
            longitude: -48.50444,
        }),
        "America/Belize" => Some(CityInfo {
            name: "Belize City".to_string(),
            country: "Belize".to_string(),
            latitude: 17.49518,
            longitude: -88.19756,
        }),
        "America/Blanc-Sablon" => Some(CityInfo {
            name: "Blanc-Sablon".to_string(),
            country: "Canada".to_string(),
            latitude: 51.44361,
            longitude: -57.18528,
        }),
        "America/Boa_Vista" => Some(CityInfo {
            name: "Boa Vista".to_string(),
            country: "Brazil".to_string(),
            latitude: 2.81972,
            longitude: -60.67333,
        }),
        "America/Bogota" => Some(CityInfo {
            name: "Bogota".to_string(),
            country: "Colombia".to_string(),
            latitude: 4.6,
            longitude: -74.0833359,
        }),
        "America/Boise" => Some(CityInfo {
            name: "Boise".to_string(),
            country: "United States".to_string(),
            latitude: 43.6135,
            longitude: -116.20345,
        }),
        "America/Buenos_Aires" => Some(CityInfo {
            name: "Buenos Aires".to_string(),
            country: "Argentina".to_string(),
            latitude: -34.5761256,
            longitude: -58.4088135,
        }),
        "America/Cambridge_Bay" => Some(CityInfo {
            name: "Cambridge Bay".to_string(),
            country: "Canada".to_string(),
            latitude: 69.10806,
            longitude: -105.13833,
        }),
        "America/Campo_Grande" => Some(CityInfo {
            name: "Campo Grande".to_string(),
            country: "Brazil".to_string(),
            latitude: -20.469444,
            longitude: -54.670278,
        }),
        "America/Cuiaba" => Some(CityInfo {
            name: "Cuiaba".to_string(),
            country: "Brazil".to_string(),
            latitude: -15.65,
            longitude: -56.1175,
        }),
        "America/Cancun" => Some(CityInfo {
            name: "Cancun".to_string(),
            country: "Mexico".to_string(),
            latitude: 21.1742876,
            longitude: -86.8465576,
        }),
        "America/Caracas" => Some(CityInfo {
            name: "Caracas".to_string(),
            country: "Venezuela".to_string(),
            latitude: 10.5,
            longitude: -66.9166641,
        }),
        "America/Catamarca" => Some(CityInfo {
            name: "Catamarca".to_string(),
            country: "Argentina".to_string(),
            latitude: -28.469581,
            longitude: -65.7795441,
        }),
        "America/Cayenne" => Some(CityInfo {
            name: "Cayenne".to_string(),
            country: "French Guiana".to_string(),
            latitude: 4.9381,
            longitude: -52.33455,
        }),
        "America/Cayman" => Some(CityInfo {
            name: "Cayman Islands".to_string(),
            country: "Cayman Islands".to_string(),
            latitude: 19.50000,
            longitude: -80.66667,
        }),
        "America/Chicago" => Some(CityInfo {
            name: "Chicago".to_string(),
            country: "United States".to_string(),
            latitude: 41.850033,
            longitude: -87.6500549,
        }),
        "America/Chihuahua" => Some(CityInfo {
            name: "Chihuahua".to_string(),
            country: "Mexico".to_string(),
            latitude: 28.6333333,
            longitude: -106.0833359,
        }),
        "America/Ciudad_Juarez" => Some(CityInfo {
            name: "Ciudad Juarez".to_string(),
            country: "Mexico".to_string(),
            latitude: 31.7333333,
            longitude: -106.4833298,
        }),
        "America/Coral_Harbour" => Some(CityInfo {
            name: "Coral Harbour".to_string(),
            country: "Bahamas".to_string(),
            latitude: 24.98167,
            longitude: -77.46528,
        }),
        "America/Cordoba" => Some(CityInfo {
            name: "Cordoba".to_string(),
            country: "Argentina".to_string(),
            latitude: -31.4,
            longitude: -64.1833344,
        }),
        "America/Costa_Rica" => Some(CityInfo {
            name: "Costa Rica".to_string(),
            country: "Costa Rica".to_string(),
            latitude: 10.0,
            longitude: -84.0,
        }),
        "America/Coyhaique" => Some(CityInfo {
            name: "Coyhaique".to_string(),
            country: "Chile".to_string(),
            latitude: -45.5775941,
            longitude: -72.0723453,
        }),
        "America/Creston" => Some(CityInfo {
            name: "Creston".to_string(),
            country: "United States".to_string(),
            latitude: 41.02138,
            longitude: -94.36329,
        }),
        "America/Curacao" => Some(CityInfo {
            name: "Curacao".to_string(),
            country: "Curaçao".to_string(),
            latitude: 12.12246,
            longitude: -68.88641,
        }),
        "America/Danmarkshavn" => Some(CityInfo {
            name: "Danmarkshavn".to_string(),
            country: "Greenland".to_string(),
            latitude: 76.76667,
            longitude: -18.66667,
        }),
        "America/Dawson" => Some(CityInfo {
            name: "Dawson".to_string(),
            country: "United States".to_string(),
            latitude: 37.16727,
            longitude: -87.69251,
        }),
        "America/Dawson_Creek" => Some(CityInfo {
            name: "Dawson Creek".to_string(),
            country: "United States".to_string(),
            latitude: 34.55481,
            longitude: -84.25242,
        }),
        "America/Denver" => Some(CityInfo {
            name: "Denver".to_string(),
            country: "United States".to_string(),
            latitude: 39.7391536,
            longitude: -104.9847031,
        }),
        "America/Detroit" => Some(CityInfo {
            name: "Detroit".to_string(),
            country: "United States".to_string(),
            latitude: 42.331427,
            longitude: -83.0457535,
        }),
        "America/Dominica" => Some(CityInfo {
            name: "Dominica".to_string(),
            country: "Dominica".to_string(),
            latitude: 15.5,
            longitude: -61.33333,
        }),
        "America/Edmonton" => Some(CityInfo {
            name: "Edmonton".to_string(),
            country: "Canada".to_string(),
            latitude: 53.5501359,
            longitude: -113.4687119,
        }),
        "America/Eirunepe" => Some(CityInfo {
            name: "Eirunepe".to_string(),
            country: "Brazil".to_string(),
            latitude: -6.63953,
            longitude: -69.8798,
        }),
        "America/El_Salvador" => Some(CityInfo {
            name: "San Salvador".to_string(),
            country: "El Salvador".to_string(),
            latitude: 13.681111,
            longitude: -89.2225
        }),
        "America/Ensenada" => Some(CityInfo {
            name: "Ensenada".to_string(),
            country: "Mexico".to_string(),
            latitude: 31.8666667,
            longitude: -116.6166687,
        }),
        "America/Fort_Nelson" => Some(CityInfo {
            name: "Fort Nelson".to_string(),
            country: "United States".to_string(),
            latitude: 37.69983,
            longitude: -85.61779,
        }),
        "America/Fort_Wayne" => Some(CityInfo {
            name: "Fort Wayne".to_string(),
            country: "United States".to_string(),
            latitude: 41.1306041,
            longitude: -85.1288605,
        }),
        "America/Fortaleza" => Some(CityInfo {
            name: "Fortaleza".to_string(),
            country: "Brazil".to_string(),
            latitude: -3.775833,
            longitude: -38.532222,
        }),
        "America/Glace_Bay" => Some(CityInfo {
            name: "Glace Bay".to_string(),
            country: "Canada".to_string(),
            latitude: 46.19695,
            longitude: -59.9569817,
        }),
        "America/Godthab" => Some(CityInfo {
            name: "Godthab".to_string(),
            country: "Greenland".to_string(),
            latitude: 64.18347,
            longitude: -51.72157,
        }),
        "America/Goose_Bay" => Some(CityInfo {
            name: "Goose Bay".to_string(),
            country: "United States".to_string(),
            latitude: 29.7355,
            longitude: -94.97743,
        }),
        "America/Grand_Turk" => Some(CityInfo {
            name: "Grand Turk".to_string(),
            country: "Turks and Caicos Islands".to_string(),
            latitude: 21.46861,
            longitude: -71.13917,
        }),
        "America/Grenada" => Some(CityInfo {
            name: "Grenada".to_string(),
            country: "Grenada".to_string(),
            latitude: 12.11667,
            longitude: -61.66667,
        }),
        "America/Guadeloupe" => Some(CityInfo {
            name: "Guadeloupe".to_string(),
            country: "Guadeloupe".to_string(),
            latitude: 16.273,
            longitude: -61.50507,
        }),
        "America/Guatemala" => Some(CityInfo {
            name: "Guatemala City".to_string(),
            country: "Guatemala".to_string(),
            latitude: 14.641278,
            longitude: -90.513333,
        }),
        "America/Guayaquil" => Some(CityInfo {
            name: "Guayaquil".to_string(),
            country: "Ecuador".to_string(),
            latitude: -2.1666667,
            longitude: -79.9000015,
        }),
        "America/Guyana" => Some(CityInfo {
            name: "Guyana".to_string(),
            country: "Cuba".to_string(),
            latitude: 21.16667,
            longitude: -77.80000,
        }),
        "America/Halifax" => Some(CityInfo {
            name: "Halifax".to_string(),
            country: "Canada".to_string(),
            latitude: 44.6519862,
            longitude: -63.5968475,
        }),
        "America/Havana" => Some(CityInfo {
            name: "Havana".to_string(),
            country: "Cuba".to_string(),
            latitude: 23.1319444,
            longitude: -82.3641663,
        }),
        "America/Hermosillo" => Some(CityInfo {
            name: "Hermosillo".to_string(),
            country: "Mexico".to_string(),
            latitude: 29.0666667,
            longitude: -110.9666672,
        }),
        "America/Indiana/Indianapolis" => Some(CityInfo {
            name: "Indianapolis".to_string(),
            country: "United States".to_string(),
            latitude: 39.7683765,
            longitude: -86.1580429,
        }),
        "America/Indiana/Knox" => Some(CityInfo {
            name: "Knox".to_string(),
            country: "United States".to_string(),
            latitude: 35.96064,
            longitude: -83.92074,
        }),
        "America/Indiana/Marengo" => Some(CityInfo {
            name: "Marengo".to_string(),
            country: "United States".to_string(),
            latitude: 32.24761,
            longitude: -87.78952,
        }),
        "America/Indiana/Petersburg" => Some(CityInfo {
            name: "Petersburg".to_string(),
            country: "United States".to_string(),
            latitude: 38.492181,
            longitude: -87.278616,
        }),
        "America/Indiana/Tell_City" => Some(CityInfo {
            name: "Tell City".to_string(),
            country: "United States".to_string(),
            latitude: 37.95144,
            longitude: -86.76777,
        }),
        "America/Indiana/Vevay" => Some(CityInfo {
            name: "Vevay".to_string(),
            country: "United States".to_string(),
            latitude: 38.74784,
            longitude: -85.06717,
        }),
        "America/Indiana/Vincennes" => Some(CityInfo {
            name: "Vincennes".to_string(),
            country: "United States".to_string(),
            latitude: 38.67727,
            longitude: -87.52863,
        }),
        "America/Indiana/Winamac" => Some(CityInfo {
            name: "Winamac".to_string(),
            country: "United States".to_string(),
            latitude: 41.05143,
            longitude: -86.60306,
        }),
        "America/Indianapolis" => Some(CityInfo {
            name: "Indianapolis".to_string(),
            country: "United States".to_string(),
            latitude: 39.7683765,
            longitude: -86.1580429,
        }),
        "America/Inuvik" => Some(CityInfo {
            name: "Inuvik".to_string(),
            country: "Canada".to_string(),
            latitude: 68.30417,
            longitude: -133.48278,
        }),
        "America/Iqaluit" => Some(CityInfo {
            name: "Iqaluit".to_string(),
            country: "Canada".to_string(),
            latitude: 63.74697,
            longitude: -68.51727,
        }),
        "America/Jamaica" => Some(CityInfo {
            name: "Jamaica".to_string(),
            country: "Jamaica".to_string(),
            latitude: 18.16667,
            longitude: -77.25,
        }),
        "America/Jujuy" => Some(CityInfo {
            name: "Jujuy".to_string(),
            country: "Argentina".to_string(),
            latitude: -24.194444,
            longitude: -65.299444,
        }),
        "America/Juneau" => Some(CityInfo {
            name: "Juneau".to_string(),
            country: "United States".to_string(),
            latitude: 58.30194,
            longitude: -134.41972,
        }),
        "America/Kentucky/Louisville" => Some(CityInfo {
            name: "Louisville".to_string(),
            country: "United States".to_string(),
            latitude: 38.2542376,
            longitude: -85.759407,
        }),
        "America/Kentucky/Monticello" => Some(CityInfo {
            name: "Monticello".to_string(),
            country: "United States".to_string(),
            latitude: 45.30552,
            longitude: -93.79414,
        }),
        "America/Knox_IN" => Some(CityInfo {
            name: "Knox IN".to_string(),
            country: "United States".to_string(),
            latitude: 38.67727,
            longitude: -87.52863,
        }),
        "America/Kralendijk" => Some(CityInfo {
            name: "Kralendijk".to_string(),
            country: "Bonaire, Sint Eustatius, and Saba".to_string(),
            latitude: 12.15,
            longitude: -68.26667,
        }),
        "America/La_Paz" => Some(CityInfo {
            name: "La Paz".to_string(),
            country: "Bolivia".to_string(),
            latitude: -16.5,
            longitude: -68.1500015,
        }),
        "America/Lima" => Some(CityInfo {
            name: "Lima".to_string(),
            country: "Peru".to_string(),
            latitude: -12.05,
            longitude: -77.0500031,
        }),
        "America/Lower_Princes" => Some(CityInfo {
            name: "Lower Prince's Quarter".to_string(),
            country: "Sint Maartin".to_string(),
            latitude: 18.040833,
            longitude: -63.109444,
        }),
        "America/Los_Angeles" => Some(CityInfo {
            name: "Los Angeles".to_string(),
            country: "United States".to_string(),
            latitude: 34.0522342,
            longitude: -118.2436829,
        }),
        "America/Louisville" => Some(CityInfo {
            name: "Louisville".to_string(),
            country: "United States".to_string(),
            latitude: 38.2542376,
            longitude: -85.759407,
        }),
        "America/Maceio" => Some(CityInfo {
            name: "Maceio".to_string(),
            country: "Brazil".to_string(),
            latitude: -9.66583,
            longitude: -35.73528,
        }),
        "America/Managua" => Some(CityInfo {
            name: "Managua".to_string(),
            country: "Nicaragua".to_string(),
            latitude: 12.0976469,
            longitude: -86.3408617,
        }),
        "America/Manaus" => Some(CityInfo {
            name: "Manaus".to_string(),
            country: "Brazil".to_string(),
            latitude: -3.10194,
            longitude: -60.025,
        }),
        "America/Marigot" => Some(CityInfo {
            name: "Marigot".to_string(),
            country: "Saint Martin".to_string(),
            latitude: 18.0686,
            longitude: -63.0825,
        }),
        "America/Martinique" => Some(CityInfo {
            name: "Martinique".to_string(),
            country: "Martinique".to_string(),
            latitude: 14.60365,
            longitude: -61.07418,
        }),
        "America/Matamoros" => Some(CityInfo {
            name: "Matamoros".to_string(),
            country: "Mexico".to_string(),
            latitude: 25.8833333,
            longitude: -97.5,
        }),
        "America/Mazatlan" => Some(CityInfo {
            name: "Mazatlan".to_string(),
            country: "Mexico".to_string(),
            latitude: 23.2166667,
            longitude: -106.4166641,
        }),
        "America/Mendoza" => Some(CityInfo {
            name: "Mendoza".to_string(),
            country: "Argentina".to_string(),
            latitude: -32.8833333,
            longitude: -68.8166656,
        }),
        "America/Menominee" => Some(CityInfo {
            name: "Menominee".to_string(),
            country: "United States".to_string(),
            latitude: 45.00417,
            longitude: -88.71000,
        }),
        "America/Merida" => Some(CityInfo {
            name: "Merida".to_string(),
            country: "Mexico".to_string(),
            latitude: 20.9666667,
            longitude: -89.6166687,
        }),
        "America/Metlakatla" => Some(CityInfo {
            name: "Metlakatla".to_string(),
            country: "United States".to_string(),
            latitude: 55.12905,
            longitude: -131.57698,
        }),
        "America/Mexico_City" => Some(CityInfo {
            name: "Mexico City".to_string(),
            country: "Mexico".to_string(),
            latitude: 19.4341667,
            longitude: -99.1386108,
        }),
        "America/Miquelon" => Some(CityInfo {
            name: "Miquelon".to_string(),
            country: "St Pierre and Miquelon".to_string(),
            latitude: 47.0966373,
            longitude: -56.3879987,
        }),
        "America/Moncton" => Some(CityInfo {
            name: "Moncton".to_string(),
            country: "Canada".to_string(),
            latitude: 46.1159434,
            longitude: -64.8018646,
        }),
        "America/Monterrey" => Some(CityInfo {
            name: "Monterrey".to_string(),
            country: "Mexico".to_string(),
            latitude: 25.6666667,
            longitude: -100.3166656,
        }),
        "America/Montevideo" => Some(CityInfo {
            name: "Montevideo".to_string(),
            country: "Uruguay".to_string(),
            latitude: -34.8580556,
            longitude: -56.1708336,
        }),
        "America/Montreal" => Some(CityInfo {
            name: "Montreal".to_string(),
            country: "Canada".to_string(),
            latitude: 45.5167792,
            longitude: -73.6491776,
        }),
        "America/Montserrat" => Some(CityInfo {
            name: "Montserrat".to_string(),
            country: "Montserrat".to_string(),
            latitude: 16.75,
            longitude: -62.2,
        }),
        "America/Nassau" => Some(CityInfo {
            name: "Nassau".to_string(),
            country: "Bahamas".to_string(),
            latitude: 25.05806,
            longitude: -77.34306,
        }),
        "America/New_York" => Some(CityInfo {
            name: "New York City".to_string(),
            country: "United States".to_string(),
            latitude: 40.7142691,
            longitude: -74.0059738,
        }),
        "America/Nipigon" => Some(CityInfo {
            name: "Nipigon".to_string(),
            country: "Canada".to_string(),
            latitude: 49.00000,
            longitude: -88.33333,
        }),
        "America/Nome" => Some(CityInfo {
            name: "Nome".to_string(),
            country: "United States".to_string(),
            latitude: 25.05806,
            longitude: -165.445278,
        }),
        "America/North_Dakota/Beulah" => Some(CityInfo {
            name: "Beulah".to_string(),
            country: "United States".to_string(),
            latitude: 47.26334,
            longitude: -101.77795,
        }),
        "America/North_Dakota/Center" => Some(CityInfo {
            name: "Center".to_string(),
            country: "United States".to_string(),
            latitude: 39.76838,
            longitude: -86.15804,
        }),
        "America/North_Dakota/New_Salem" => Some(CityInfo {
            name: "New Salem".to_string(),
            country: "United States".to_string(),
            latitude: 44.9429,
            longitude: -123.0351,
        }),
        "America/Ojinaga" => Some(CityInfo {
            name: "Ojinaga".to_string(),
            country: "Mexico".to_string(),
            latitude: 29.56689,
            longitude: -104.54487,
        }),
        "America/Panama" => Some(CityInfo {
            name: "Panama".to_string(),
            country: "Panama".to_string(),
            latitude: 8.9936,
            longitude: -79.51973,
        }),
        "America/Pangnirtung" => Some(CityInfo {
            name: "Pangnirtung".to_string(),
            country: "Canada".to_string(),
            latitude: 66.145,
            longitude: -65.71361,
        }),
        "America/Paramaribo" => Some(CityInfo {
            name: "Paramaribo".to_string(),
            country: "Suriname".to_string(),
            latitude: 5.8333333,
            longitude: -55.1666679,
        }),
        "America/Phoenix" => Some(CityInfo {
            name: "Phoenix".to_string(),
            country: "United States".to_string(),
            latitude: 33.4483771,
            longitude: -112.0740356,
        }),
        "America/Port-au-Prince" => Some(CityInfo {
            name: "Port-au-Prince".to_string(),
            country: "Haiti".to_string(),
            latitude: 18.5391667,
            longitude: -72.3349991,
        }),
        "America/Port_of_Spain" => Some(CityInfo {
            name: "Port of Spain".to_string(),
            country: "Trinidad and Tobago".to_string(),
            latitude: 10.66668,
            longitude: -61.51889,
        }),
        "America/Porto_Acre" => Some(CityInfo {
            name: "Porto Acre".to_string(),
            country: "Brazil".to_string(),
            latitude: -9.65038,
            longitude: -67.77733,
        }),
        "America/Porto_Velho" => Some(CityInfo {
            name: "Porto Velho".to_string(),
            country: "Brazil".to_string(),
            latitude: -8.76194,
            longitude: -63.90389,
        }),
        "America/Rainy_River" => Some(CityInfo {
            name: "Rainy River".to_string(),
            country: "Canada".to_string(),
            latitude: 48.71639,
            longitude: -94.56694,
        }),
        "America/Rankin_Inlet" => Some(CityInfo {
            name: "Rankin Inlet".to_string(),
            country: "Canada".to_string(),
            latitude: 62.81027,
            longitude: -92.11332,
        }),
        "America/Regina" => Some(CityInfo {
            name: "Regina".to_string(),
            country: "Canada".to_string(),
            latitude: 50.4500801,
            longitude: -104.6177979,
        }),
        "America/Resolute" => Some(CityInfo {
            name: "Resolute".to_string(),
            country: "Canada".to_string(),
            latitude: 74.69722,
            longitude: -94.83056,
        }),
        "America/Rosario" => Some(CityInfo {
            name: "Rosario".to_string(),
            country: "Argentina".to_string(),
            latitude: -32.9511111,
            longitude: -60.6663895,
        }),
        "America/Santarem" => Some(CityInfo {
            name: "Santarem".to_string(),
            country: "Brazil".to_string(),
            latitude: -2.44306,
            longitude: -54.70833,
        }),
        "America/Santiago" => Some(CityInfo {
            name: "Santiago".to_string(),
            country: "Chile".to_string(),
            latitude: -33.45694,
            longitude: -70.64827,
        }),
        "America/Santo_Domingo" => Some(CityInfo {
            name: "Santo Domingo".to_string(),
            country: "Dominican Republic".to_string(),
            latitude: 18.4337802,
            longitude: -69.6709078,
        }),
        "America/Scoresbysund" => Some(CityInfo {
            name: "Scoresbysund".to_string(),
            country: "Greenland".to_string(),
            latitude: 70.48456,
            longitude: -21.96221,
        }),
        "America/Shiprock" => Some(CityInfo {
            name: "Shiprock".to_string(),
            country: "United States".to_string(),
            latitude: 36.78555,
            longitude: -108.68703,
        }),
        "America/Sitka" => Some(CityInfo {
            name: "Sitka".to_string(),
            country: "United States".to_string(),
            latitude: 57.05315,
            longitude: -135.33088,
        }),
        "America/St_Barthelemy" => Some(CityInfo {
            name: "St Barthelemy".to_string(),
            country: "France".to_string(),
            latitude: 17.9112312,
            longitude: -62.892528,
        }),
        "America/St_Johns" => Some(CityInfo {
            name: "St. John's".to_string(),
            country: "Canada".to_string(),
            latitude: 47.4821589,
            longitude: -53.1592775,
        }),
        "America/St_Kitts" => Some(CityInfo {
            name: "St Kitts".to_string(),
            country: "St Kitts and Nevis".to_string(),
            latitude: 17.2955,
            longitude: -62.72499,
        }),
        "America/St_Lucia" => Some(CityInfo {
            name: "St Lucia".to_string(),
            country: "Saint Lucia".to_string(),
            latitude: 13.88333,
            longitude: -60.96667,
        }),
        "America/St_Thomas" => Some(CityInfo {
            name: "St Thomas".to_string(),
            country: "U.S. Virgin Islands".to_string(),
            latitude: 18.3419,
            longitude: -64.9307,
        }),
        "America/St_Vincent" => Some(CityInfo {
            name: "St Vincent".to_string(),
            country: "St Vincent and Grenadines".to_string(),
            latitude: 13.15527,
            longitude: -61.22742,
        }),
        "America/Swift_Current" => Some(CityInfo {
            name: "Swift Current".to_string(),
            country: "Canada".to_string(),
            latitude: 50.28333,
            longitude: -107.80111,
        }),
        "America/Tegucigalpa" => Some(CityInfo {
            name: "Tegucigalpa".to_string(),
            country: "Honduras".to_string(),
            latitude: 14.1,
            longitude: -87.2166672,
        }),
        "America/Thule" => Some(CityInfo {
            name: "Thule".to_string(),
            country: "Greenland".to_string(),
            latitude: 76.56250,
            longitude: -68.78333,
        }),
        "America/Thunder_Bay" => Some(CityInfo {
            name: "Thunder Bay".to_string(),
            country: "Canada".to_string(),
            latitude: 48.4000957,
            longitude: -89.3168259,
        }),
        "America/Tijuana" => Some(CityInfo {
            name: "Tijuana".to_string(),
            country: "Mexico".to_string(),
            latitude: 32.5333333,
            longitude: -117.0166702,
        }),
        "America/Toronto" => Some(CityInfo {
            name: "Toronto".to_string(),
            country: "Canada".to_string(),
            latitude: 43.7001138,
            longitude: -79.4163055,
        }),
        "America/Tortola" => Some(CityInfo {
            name: "Tortola".to_string(),
            country: "British Virgin Islands".to_string(),
            latitude: 18.43662,
            longitude: -64.61849,
        }),
        "America/Vancouver" => Some(CityInfo {
            name: "Vancouver".to_string(),
            country: "Canada".to_string(),
            latitude: 49.2496574,
            longitude: -123.119339,
        }),
        "America/Whitehorse" => Some(CityInfo {
            name: "Whitehorse".to_string(),
            country: "Canada".to_string(),
            latitude: 60.7161148,
            longitude: -135.0537415,
        }),
        "America/Winnipeg" => Some(CityInfo {
            name: "Winnipeg".to_string(),
            country: "Canada".to_string(),
            latitude: 49.8843986,
            longitude: -97.1470413,
        }),
        "America/Yakutat" => Some(CityInfo {
            name: "Yakutat".to_string(),
            country: "United States".to_string(),
            latitude: 59.66667,
            longitude: -139.13333,
        }),
        "America/Yellowknife" => Some(CityInfo {
            name: "Yellowknife".to_string(),
            country: "Canada".to_string(),
            latitude: 62.45411,
            longitude: -114.37248,
        }),
        "Antarctica/Rothera" => Some(CityInfo {
            name: "Rothera".to_string(),
            country: "Antarctica".to_string(),
            latitude: -67.56934,
            longitude: -68.12697,
        }),
        "Arctic/Longyearbyen" => Some(CityInfo {
            name: "Longyearbyen".to_string(),
            country: "Svalbard and Jan Mayen".to_string(),
            latitude: 78.22334,
            longitude: 15.64689,
        }),
        "Asia/Aden" => Some(CityInfo {
            name: "Aden".to_string(),
            country: "Yemen".to_string(),
            latitude: 13.8080948,
            longitude: 43.6280473,
        }),
        "Asia/Almaty" => Some(CityInfo {
            name: "Almaty".to_string(),
            country: "Kazakhstan".to_string(),
            latitude: 43.25,
            longitude: 76.9499969,
        }),
        "Asia/Amman" => Some(CityInfo {
            name: "Amman".to_string(),
            country: "Jordan".to_string(),
            latitude: 31.95,
            longitude: 35.9333344,
        }),
        "Asia/Anadyr" => Some(CityInfo {
            name: "Anadyr".to_string(),
            country: "Russia".to_string(),
            latitude: 64.73424,
            longitude: 177.5103,
        }),
        "Asia/Aqtobe" => Some(CityInfo {
            name: "Aqtobe".to_string(),
            country: "Kazakhstan".to_string(),
            latitude: 50.2796868,
            longitude: 57.2071838,
        }),
        "Asia/Ashgabat" => Some(CityInfo {
            name: "Ashgabat".to_string(),
            country: "Turkmenistan".to_string(),
            latitude: 37.95,
            longitude: 58.38333,
        }),
        "Asia/Atyrau" => Some(CityInfo {
            name: "Atyrau".to_string(),
            country: "Kazakhstan".to_string(),
            latitude: 47.1166667,
            longitude: 51.8833351,
        }),
        "Asia/Baghdad" => Some(CityInfo {
            name: "Baghdad".to_string(),
            country: "Iraq".to_string(),
            latitude: 33.3386111,
            longitude: 44.3938904,
        }),
        "Asia/Baku" => Some(CityInfo {
            name: "Baku".to_string(),
            country: "Azerbaijan".to_string(),
            latitude: 40.3952778,
            longitude: 49.8822212,
        }),
        "Asia/Bangkok" => Some(CityInfo {
            name: "Bangkok".to_string(),
            country: "Thailand".to_string(),
            latitude: 13.75,
            longitude: 100.5166702,
        }),
        "Asia/Barnaul" => Some(CityInfo {
            name: "Barnaul".to_string(),
            country: "Russia".to_string(),
            latitude: 53.36,
            longitude: 83.7600021,
        }),
        "Asia/Beirut" => Some(CityInfo {
            name: "Beirut".to_string(),
            country: "Lebanon".to_string(),
            latitude: 33.8719444,
            longitude: 35.5097237,
        }),
        "Asia/Bishkek" => Some(CityInfo {
            name: "Bishkek".to_string(),
            country: "Kyrgyzstan".to_string(),
            latitude: 42.87,
            longitude: 74.59,
        }),
        "Asia/Brunei" => Some(CityInfo {
            name: "Brunei".to_string(),
            country: "Brunei".to_string(),
            latitude: 4.9061412,
            longitude: 114.9380903,
        }),
        "Asia/Calcutta" => Some(CityInfo {
            name: "Calcutta".to_string(),
            country: "India".to_string(),
            latitude: 22.5697222,
            longitude: 88.3697205,
        }),
        "Asia/Chita" => Some(CityInfo {
            name: "Chita".to_string(),
            country: "Russia".to_string(),
            latitude: 52.0333333,
            longitude: 113.5500031,
        }),
        "Asia/Choibalsan" => Some(CityInfo {
            name: "Choibalsan".to_string(),
            country: "Mongolia".to_string(),
            latitude: 48.1357,
            longitude: 114.646,
        }),
        "Asia/Chongqing" => Some(CityInfo {
            name: "Chongqing".to_string(),
            country: "China".to_string(),
            latitude: 29.5627778,
            longitude: 106.5527802,
        }),
        "Asia/Chungking" => Some(CityInfo {
            name: "Chungking".to_string(),
            country: "China".to_string(),
            latitude: 29.56026,
            longitude: 106.55771,
        }),
        "Asia/Colombo" => Some(CityInfo {
            name: "Colombo".to_string(),
            country: "Sri Lanka".to_string(),
            latitude: 6.9319444,
            longitude: 79.8477783,
        }),
        "Asia/Dacca" => Some(CityInfo {
            name: "Dhaka".to_string(),
            country: "Bangladesh".to_string(),
            latitude: 23.7230556,
            longitude: 90.4086075,
        }),
        "Asia/Damascus" => Some(CityInfo {
            name: "Damascus".to_string(),
            country: "Syria".to_string(),
            latitude: 33.5,
            longitude: 36.2999992,
        }),
        "Asia/Dhaka" => Some(CityInfo {
            name: "Dhaka".to_string(),
            country: "Bangladesh".to_string(),
            latitude: 23.7230556,
            longitude: 90.4086075,
        }),
        "Asia/Dili" => Some(CityInfo {
            name: "Dili".to_string(),
            country: "Timor-Leste".to_string(),
            latitude: -8.5563293,
            longitude: 125.528727,
        }),
        "Asia/Dushanbe" => Some(CityInfo {
            name: "Dushanbe".to_string(),
            country: "Tajikistan".to_string(),
            latitude: 38.56,
            longitude: 68.7738876,
        }),
        "Asia/Famagusta" => Some(CityInfo {
            name: "Famagusta".to_string(),
            country: "Cyprus".to_string(),
            latitude: 35.12489,
            longitude: 33.94135,
        }),
        "Asia/Gaza" => Some(CityInfo {
            name: "Gaza".to_string(),
            country: "Palesitne".to_string(),
            latitude: 31.5017125,
            longitude: 34.4565448,
        }),
        "Asia/Harbin" => Some(CityInfo {
            name: "Harbin".to_string(),
            country: "China".to_string(),
            latitude: 45.75,
            longitude: 126.6500015,
        }),
        "Asia/Ho_Chi_Minh" => Some(CityInfo {
            name: "Ho Chi Minh".to_string(),
            country: "Vietnam".to_string(),
            latitude: 10.82302,
            longitude: 106.62965,
        }),
        "Asia/Hong_Kong" => Some(CityInfo {
            name: "Hong Kong".to_string(),
            country: "China".to_string(),
            latitude: 22.3193,
            longitude: 114.1694,
        }),
        "Asia/Hovd" => Some(CityInfo {
            name: "Hovd".to_string(),
            country: "Mongolia".to_string(),
            latitude: 48.0055556,
            longitude: 91.6419449,
        }),
        "Asia/Irkutsk" => Some(CityInfo {
            name: "Irkutsk".to_string(),
            country: "Russia".to_string(),
            latitude: 52.2977778,
            longitude: 104.2963867,
        }),
        "Asia/Istanbul" => Some(CityInfo {
            name: "Istanbul".to_string(),
            country: "Turkey".to_string(),
            latitude: 41.01384,
            longitude: 28.94966,
        }),
        "Asia/Jakarta" => Some(CityInfo {
            name: "Jakarta".to_string(),
            country: "Indonesia".to_string(),
            latitude: -6.1744444,
            longitude: 106.8294449,
        }),
        "Asia/Jayapura" => Some(CityInfo {
            name: "Jayapura".to_string(),
            country: "Indonesia".to_string(),
            latitude: -2.5333333,
            longitude: 140.6999969,
        }),
        "Asia/Jerusalem" => Some(CityInfo {
            name: "Jerusalem".to_string(),
            country: "Israel".to_string(),
            latitude: 31.7666667,
            longitude: 35.2333336,
        }),
        "Asia/Kabul" => Some(CityInfo {
            name: "Kabul".to_string(),
            country: "Afghanistan".to_string(),
            latitude: 34.5166667,
            longitude: 69.1833344,
        }),
        "Asia/Karachi" => Some(CityInfo {
            name: "Karachi".to_string(),
            country: "Pakistan".to_string(),
            latitude: 24.8666667,
            longitude: 67.0500031,
        }),
        "Asia/Kashgar" => Some(CityInfo {
            name: "Kashgar".to_string(),
            country: "China".to_string(),
            latitude: 39.46718,
            longitude: 75.98675,
        }),
        "Asia/Kathmandu" => Some(CityInfo {
            name: "Kathmandu".to_string(),
            country: "Nepal".to_string(),
            latitude: 27.70169,
            longitude: 85.3206,
        }),
        "Asia/Katmandu" => Some(CityInfo {
            name: "Katmandu".to_string(),
            country: "Nepal".to_string(),
            latitude: 27.70169,
            longitude: 85.3206,
        }),
        "Asia/Khandyga" => Some(CityInfo {
            name: "Khandyga".to_string(),
            country: "Russia".to_string(),
            latitude: 62.65302,
            longitude: 135.56649,
        }),
        "Asia/Kolkata" => Some(CityInfo {
            name: "Kolkata".to_string(),
            country: "India".to_string(),
            latitude: 22.5697222,
            longitude: 88.3697205,
        }),
        "Asia/Krasnoyarsk" => Some(CityInfo {
            name: "Krasnoyarsk".to_string(),
            country: "Russia".to_string(),
            latitude: 56.0097222,
            longitude: 92.7916641,
        }),
        "Asia/Kuala_Lumpur" => Some(CityInfo {
            name: "Kuala Lumpur".to_string(),
            country: "Malaysia".to_string(),
            latitude: 3.1666667,
            longitude: 101.6999969,
        }),
        "Asia/Kuching" => Some(CityInfo {
            name: "Kuching".to_string(),
            country: "Malaysia".to_string(),
            latitude: 1.55,
            longitude: 110.3333359,
        }),
        "Asia/Kuwait" => Some(CityInfo {
            name: "Kuwait".to_string(),
            country: "Kuwait".to_string(),
            latitude: 29.3697222,
            longitude: 47.9783325,
        }),
        "Asia/Magadan" => Some(CityInfo {
            name: "Magadan".to_string(),
            country: "Russia".to_string(),
            latitude: 59.5638,
            longitude: 150.80347,
        }),
        "Asia/Makassar" => Some(CityInfo {
            name: "Makassar".to_string(),
            country: "Indonesia".to_string(),
            latitude: -5.1463889,
            longitude: 119.4386139,
        }),
        "Asia/Manila" => Some(CityInfo {
            name: "Manila".to_string(),
            country: "Philippines".to_string(),
            latitude: 14.6041667,
            longitude: 120.9822235,
        }),
        "Asia/Muscat" => Some(CityInfo {
            name: "Muscat".to_string(),
            country: "Oman".to_string(),
            latitude: 23.6133333,
            longitude: 58.5933342,
        }),
        "Asia/Nicosia" => Some(CityInfo {
            name: "Nicosia".to_string(),
            country: "Cyprus".to_string(),
            latitude: 35.1666667,
            longitude: 33.3666649,
        }),
        "Asia/Novokuznetsk" => Some(CityInfo {
            name: "Novokuznetsk".to_string(),
            country: "Russia".to_string(),
            latitude: 53.75,
            longitude: 87.0999985,
        }),
        "Asia/Novosibirsk" => Some(CityInfo {
            name: "Novosibirsk".to_string(),
            country: "Russia".to_string(),
            latitude: 55.0411111,
            longitude: 82.9344406,
        }),
        "Asia/Omsk" => Some(CityInfo {
            name: "Omsk".to_string(),
            country: "Russia".to_string(),
            latitude: 55.0,
            longitude: 73.4000015,
        }),
        "Asia/Oral" => Some(CityInfo {
            name: "Oral".to_string(),
            country: "Kazakhstan".to_string(),
            latitude: 51.2333333,
            longitude: 51.3666649,
        }),
        "Asia/Phnom_Penh" => Some(CityInfo {
            name: "Phnom Penh".to_string(),
            country: "Cambodia".to_string(),
            latitude: 11.55,
            longitude: 104.9166641,
        }),
        "Asia/Pontianak" => Some(CityInfo {
            name: "Pontianak".to_string(),
            country: "Indonesia".to_string(),
            latitude: -0.0333333,
            longitude: 109.3333359,
        }),
        "Asia/Pyongyang" => Some(CityInfo {
            name: "Pyongyang".to_string(),
            country: "North Korea".to_string(),
            latitude: 39.03385,
            longitude: 125.75432,
        }),
        "Asia/Qatar" => Some(CityInfo {
            name: "Doha".to_string(),
            country: "Qatar".to_string(),
            latitude: 25.2839176,
            longitude: 51.3471854,
        }),
        "Asia/Qostanay" => Some(CityInfo {
            name: "Qostanay".to_string(),
            country: "Kazakhstan".to_string(),
            latitude: 53.1666667,
            longitude: 63.5833321,
        }),
        "Asia/Rangoon" => Some(CityInfo {
            name: "Rangoon".to_string(),
            country: "Myanmar".to_string(),
            latitude: 16.8052778,
            longitude: 96.1561127,
        }),
        "Asia/Riyadh" => Some(CityInfo {
            name: "Riyadh".to_string(),
            country: "Saudi Arabia".to_string(),
            latitude: 24.68773,
            longitude: 46.72185,
        }),
        "Asia/Samarkand" => Some(CityInfo {
            name: "Samarkand".to_string(),
            country: "Uzbekistan".to_string(),
            latitude: 39.6402225,
            longitude: 66.6382423,
        }),
        "Asia/Seoul" => Some(CityInfo {
            name: "Seoul".to_string(),
            country: "South Korea".to_string(),
            latitude: 37.566,
            longitude: 126.9784,
        }),
        "Asia/Shanghai" => Some(CityInfo {
            name: "Shanghai".to_string(),
            country: "China".to_string(),
            latitude: 31.2222222,
            longitude: 121.4580536,
        }),
        "Asia/Singapore" => Some(CityInfo {
            name: "Singapore".to_string(),
            country: "Singapore".to_string(),
            latitude: 1.3143379,
            longitude: 103.6794398,
        }),
        "Asia/Srednekolymsk" => Some(CityInfo {
            name: "Srednekolymsk".to_string(),
            country: "Russia".to_string(),
            latitude: 67.47979,
            longitude: 153.73512,
        }),
        "Asia/Taipei" => Some(CityInfo {
            name: "Taipei".to_string(),
            country: "Taiwan".to_string(),
            latitude: 25.0855388,
            longitude: 121.4791002,
        }),
        "Asia/Tashkent" => Some(CityInfo {
            name: "Tashkent".to_string(),
            country: "Uzbekistan".to_string(),
            latitude: 41.26465,
            longitude: 69.21627,
        }),
        "Asia/Tbilisi" => Some(CityInfo {
            name: "Tbilisi".to_string(),
            country: "Georgia".to_string(),
            latitude: 41.725,
            longitude: 44.7908325,
        }),
        "Asia/Tehran" => Some(CityInfo {
            name: "Tehran".to_string(),
            country: "Iran".to_string(),
            latitude: 35.6719444,
            longitude: 51.4244461,
        }),
        "Asia/Tel_Aviv" => Some(CityInfo {
            name: "Tel Aviv-Yafo".to_string(),
            country: "Israel".to_string(),
            latitude: 32.0677778,
            longitude: 34.7647209,
        }),
        "Asia/Thimbu" => Some(CityInfo {
            name: "Thimbu".to_string(),
            country: "Bhutan".to_string(),
            latitude: 27.46609,
            longitude: 89.64191,
        }),
        "Asia/Thimphu" => Some(CityInfo {
            name: "Thimphu".to_string(),
            country: "Bhutan".to_string(),
            latitude: 27.4833333,
            longitude: 89.5999985,
        }),
        "Asia/Tokyo" => Some(CityInfo {
            name: "Tokyo".to_string(),
            country: "Japan".to_string(),
            latitude: 35.6895266,
            longitude: 139.6916809,
        }),
        "Asia/Tomsk" => Some(CityInfo {
            name: "Tomsk".to_string(),
            country: "Russia".to_string(),
            latitude: 56.5,
            longitude: 84.9666672,
        }),
        "Asia/Ujung_Pandang" => Some(CityInfo {
            name: "Ujung Pandang".to_string(),
            country: "Indonesia".to_string(),
            latitude: -5.14056,
            longitude: 119.4125,
        }),
        "Asia/Ulaanbaatar" => Some(CityInfo {
            name: "Ulaanbaatar".to_string(),
            country: "Mongolia".to_string(),
            latitude: 47.9166667,
            longitude: 106.9166641,
        }),
        "Asia/Ulan_Bator" => Some(CityInfo {
            name: "Ulan Bator".to_string(),
            country: "Mongolia".to_string(),
            latitude: 47.90771,
            longitude: 106.88324,
        }),
        "Asia/Urumqi" => Some(CityInfo {
            name: "Urumqi".to_string(),
            country: "China".to_string(),
            latitude: 43.8,
            longitude: 87.5833359,
        }),
        "Asia/Ust-Nera" => Some(CityInfo {
            name: "Ust-Nera".to_string(),
            country: "Russia".to_string(),
            latitude: 64.54933,
            longitude: 143.11083,
        }),
        "Asia/Vientiane" => Some(CityInfo {
            name: "Vientiane".to_string(),
            country: "Laos".to_string(),
            latitude: 17.96667,
            longitude: 102.6,
        }),
        "Asia/Vladivostok" => Some(CityInfo {
            name: "Vladivostok".to_string(),
            country: "Russia".to_string(),
            latitude: 43.1056202,
            longitude: 131.8735352,
        }),
        "Asia/Yakutsk" => Some(CityInfo {
            name: "Yakutsk".to_string(),
            country: "Russia".to_string(),
            latitude: 62.0338889,
            longitude: 129.7330627,
        }),
        "Asia/Yangon" => Some(CityInfo {
            name: "Yangon".to_string(),
            country: "Myanmar".to_string(),
            latitude: 16.80528,
            longitude: 96.15611,
        }),
        "Asia/Yekaterinburg" => Some(CityInfo {
            name: "Yekaterinburg".to_string(),
            country: "Russia".to_string(),
            latitude: 56.8575,
            longitude: 60.6124992,
        }),
        "Asia/Yerevan" => Some(CityInfo {
            name: "Yerevan".to_string(),
            country: "Armenia".to_string(),
            latitude: 40.18111,
            longitude: 44.51361,
        }),
        "Atlantic/Azores" => Some(CityInfo {
            name: "Azores".to_string(),
            country: "Portugal".to_string(),
            latitude: 37.80847,
            longitude: -25.47466,
        }),
        "Atlantic/Bermuda" => Some(CityInfo {
            name: "Bermuda".to_string(),
            country: "Bermuda".to_string(),
            latitude: 32.33022,
            longitude: -64.74003,
        }),
        "Atlantic/Cape_Verde" => Some(CityInfo {
            name: "Cape Verde".to_string(),
            country: "Cabo Verde".to_string(),
            latitude: 16.0,
            longitude: -24.0,
        }),
        "Atlantic/Jan_Mayen" => Some(CityInfo {
            name: "Jan Mayen".to_string(),
            country: "Svalbard and Jan Mayen".to_string(),
            latitude: 71.08333,
            longitude: -8.16667,
        }),
        "Atlantic/Madeira" => Some(CityInfo {
            name: "Madeira".to_string(),
            country: "Portugal".to_string(),
            latitude: 32.66568,
            longitude: -16.92547,
        }),
        "Atlantic/Reykjavik" => Some(CityInfo {
            name: "Reykjavik".to_string(),
            country: "Iceland".to_string(),
            latitude: 64.15,
            longitude: -21.9500008,
        }),
        "Atlantic/St_Helena" => Some(CityInfo {
            name: "St Helena".to_string(),
            country: "Saint Helena".to_string(),
            latitude: -15.95,
            longitude: -5.7,
        }),
        "Atlantic/Stanley" => Some(CityInfo {
            name: "Stanley".to_string(),
            country: "Falkland Islands".to_string(),
            latitude: -51.69382,
            longitude: -57.85701,
        }),
        "Australia/ACT" => Some(CityInfo {
            name: "ACT".to_string(),
            country: "Australia".to_string(),
            latitude: -35.28346,
            longitude: 149.12807,
        }),
        "Australia/Adelaide" => Some(CityInfo {
            name: "Adelaide".to_string(),
            country: "Australia".to_string(),
            latitude: -34.9333333,
            longitude: 138.6000061,
        }),
        "Australia/Brisbane" => Some(CityInfo {
            name: "Brisbane".to_string(),
            country: "Australia".to_string(),
            latitude: -27.4679357,
            longitude: 153.0280914,
        }),
        "Australia/Broken_Hill" => Some(CityInfo {
            name: "Broken Hill".to_string(),
            country: "Australia".to_string(),
            latitude: -31.95,
            longitude: 141.4333344,
        }),
        "Australia/Canberra" => Some(CityInfo {
            name: "Canberra".to_string(),
            country: "Australia".to_string(),
            latitude: -35.2834625,
            longitude: 149.128067,
        }),
        "Australia/Currie" => Some(CityInfo {
            name: "Currie".to_string(),
            country: "Australia".to_string(),
            latitude: -25.2,
            longitude: 130.5,
        }),
        "Australia/Darwin" => Some(CityInfo {
            name: "Darwin".to_string(),
            country: "Australia".to_string(),
            latitude: -12.4611337,
            longitude: 130.8418427,
        }),
        "Australia/Eucla" => Some(CityInfo {
            name: "Eucla".to_string(),
            country: "Australia".to_string(),
            latitude: -31.70674,
            longitude: 128.87718,
        }),
        "Australia/Hobart" => Some(CityInfo {
            name: "Hobart".to_string(),
            country: "Australia".to_string(),
            latitude: -42.9166667,
            longitude: 147.3333282,
        }),
        "Australia/Lindeman" => Some(CityInfo {
            name: "Lindeman".to_string(),
            country: "Australia".to_string(),
            latitude: -20.47371,
            longitude: 149.08414,
        }),
        "Australia/Lord_Howe" => Some(CityInfo {
            name: "Lord Howe".to_string(),
            country: "Australia".to_string(),
            latitude: -31.55281,
            longitude: 159.08579,
        }),
        "Australia/Melbourne" => Some(CityInfo {
            name: "Melbourne".to_string(),
            country: "Australia".to_string(),
            latitude: -37.8139966,
            longitude: 144.9633179,
        }),
        "Australia/NSW" => Some(CityInfo {
            name: "NSW".to_string(),
            country: "Australia".to_string(),
            latitude: -33.86785,
            longitude: 151.20732,
        }),
        "Australia/North" => Some(CityInfo {
            name: "North".to_string(),
            country: "Australia".to_string(),
            latitude: -16.56884,
            longitude: 142.4846,
        }),
        "Australia/Perth" => Some(CityInfo {
            name: "Perth".to_string(),
            country: "Australia".to_string(),
            latitude: -31.9333333,
            longitude: 115.8333359,
        }),
        "Australia/Queensland" => Some(CityInfo {
            name: "Queensland".to_string(),
            country: "Australia".to_string(),
            latitude: -27.46794,
            longitude: 153.02809,
        }),
        "Australia/South" => Some(CityInfo {
            name: "South".to_string(),
            country: "Australia".to_string(),
            latitude: -33.86785,
            longitude: 151.20732,
        }),
        "Australia/Sydney" => Some(CityInfo {
            name: "Sydney".to_string(),
            country: "Australia".to_string(),
            latitude: -33.86785,
            longitude: 151.2073212,
        }),
        "Australia/Tasmania" => Some(CityInfo {
            name: "Tasmania".to_string(),
            country: "Australia".to_string(),
            latitude: -42.0,
            longitude: 147.0,
        }),
        "Australia/Victoria" => Some(CityInfo {
            name: "Victoria".to_string(),
            country: "Australia".to_string(),
            latitude: -37.814,
            longitude: 144.96332,
        }),
        "Australia/West" => Some(CityInfo {
            name: "West".to_string(),
            country: "Australia".to_string(),
            latitude: -31.95224,
            longitude: 115.8614,
        }),
        "Australia/Yancowinna" => Some(CityInfo {
            name: "Yancowinna".to_string(),
            country: "Australia".to_string(),
            latitude: -32.37502,
            longitude: 142.16786,
        }),
        "Canada/Newfoundland" => Some(CityInfo {
            name: "Newfoundland".to_string(),
            country: "Canada".to_string(),
            latitude: 52.0,
            longitude: -56.0,
        }),
        "Canada/Saskatchewan" => Some(CityInfo {
            name: "Saskatchewan".to_string(),
            country: "Canada".to_string(),
            latitude: 54.0001,
            longitude: -106.00099,
        }),
        "Canada/Yukon" => Some(CityInfo {
            name: "Yukon".to_string(),
            country: "Canada".to_string(),
            latitude: 62.99962,
            longitude: -135.00404,
        }),
        "Cuba" => Some(CityInfo {
            name: "Cuba".to_string(),
            country: "Cuba".to_string(),
            latitude: 22.0,
            longitude: -79.5,
        }),
        "Egypt" => Some(CityInfo {
            name: "Egypt".to_string(),
            country: "Egypt".to_string(),
            latitude: 27.0,
            longitude: 30.0,
        }),
        "Eire" => Some(CityInfo {
            name: "Eire".to_string(),
            country: "Ireland".to_string(),
            latitude: 53.0,
            longitude: -8.0,
        }),
        "Etc/GMT" => Some(CityInfo {
            name: "GMT".to_string(),
            country: "United Kingdom".to_string(),
            latitude: 51.47785,
            longitude: 0.01176,
        }),
        "Etc/Greenwich" => Some(CityInfo {
            name: "Greenwich".to_string(),
            country: "United Kingdom".to_string(),
            latitude: 51.47785,
            longitude: -0.01176,
        }),
        "Etc/UCT" => Some(CityInfo {
            name: "UCT".to_string(),
            country: "Russia".to_string(),
            latitude: 63.56904,
            longitude: 53.69141,
        }),
        "Europe/Amsterdam" => Some(CityInfo {
            name: "Amsterdam".to_string(),
            country: "Netherlands".to_string(),
            latitude: 52.373084,
            longitude: 4.8999023,
        }),
        "Europe/Andorra" => Some(CityInfo {
            name: "Andorra".to_string(),
            country: "Andorra".to_string(),
            latitude: 42.506097,
            longitude: 1.5200651,
        }),
        "Europe/Astrakhan" => Some(CityInfo {
            name: "Astrakhan".to_string(),
            country: "Russia".to_string(),
            latitude: 46.34968,
            longitude: 48.04076,
        }),
        "Europe/Athens" => Some(CityInfo {
            name: "Athens".to_string(),
            country: "Greece".to_string(),
            latitude: 37.9833333,
            longitude: 23.7333336,
        }),
        "Europe/Belfast" => Some(CityInfo {
            name: "Belfast".to_string(),
            country: "United Kingdom".to_string(),
            latitude: 54.5833333,
            longitude: -5.9333334,
        }),
        "Europe/Belgrade" => Some(CityInfo {
            name: "Belgrade".to_string(),
            country: "Serbia".to_string(),
            latitude: 44.8148699,
            longitude: 20.0928283,
        }),
        "Europe/Berlin" => Some(CityInfo {
            name: "Berlin".to_string(),
            country: "Germany".to_string(),
            latitude: 52.5166667,
            longitude: 13.3999996,
        }),
        "Europe/Bratislava" => Some(CityInfo {
            name: "Bratislava".to_string(),
            country: "Slovakia".to_string(),
            latitude: 48.14816,
            longitude: 17.10674,
        }),
        "Europe/Brussels" => Some(CityInfo {
            name: "Brussels".to_string(),
            country: "Belgium".to_string(),
            latitude: 50.8465975,
            longitude: 4.3527746,
        }),
        "Europe/Bucharest" => Some(CityInfo {
            name: "Bucharest".to_string(),
            country: "Romania".to_string(),
            latitude: 44.4333333,
            longitude: 26.1000004,
        }),
        "Europe/Budapest" => Some(CityInfo {
            name: "Budapest".to_string(),
            country: "Hungary".to_string(),
            latitude: 47.5,
            longitude: 19.083334,
        }),
        "Europe/Busingen" => Some(CityInfo {
            name: "Busingen".to_string(),
            country: "Germany".to_string(),
            latitude: 47.69638,
            longitude: 8.68759,
        }),
        "Europe/Chisinau" => Some(CityInfo {
            name: "Chisinau".to_string(),
            country: "Moldova".to_string(),
            latitude: 47.0055556,
            longitude: 28.8575001,
        }),
        "Europe/Copenhagen" => Some(CityInfo {
            name: "Copenhagen".to_string(),
            country: "Denmark".to_string(),
            latitude: 55.6776812,
            longitude: 12.5709343,
        }),
        "Europe/Dublin" => Some(CityInfo {
            name: "Dublin".to_string(),
            country: "Ireland".to_string(),
            latitude: 53.3330556,
            longitude: -6.248889,
        }),
        "Europe/Gibraltar" => Some(CityInfo {
            name: "Gibraltar".to_string(),
            country: "Gibraltar".to_string(),
            latitude: 36.13333,
            longitude: -5.35,
        }),
        "Europe/Guernsey" => Some(CityInfo {
            name: "Guernsey".to_string(),
            country: "Guernsey".to_string(),
            latitude: 49.45474,
            longitude: -2.57629,
        }),
        "Europe/Helsinki" => Some(CityInfo {
            name: "Helsinki".to_string(),
            country: "Finland".to_string(),
            latitude: 60.1755556,
            longitude: 24.934166,
        }),
        "Europe/Isle_of_Man" => Some(CityInfo {
            name: "Isle of Man".to_string(),
            country: "United Kingdom".to_string(),
            latitude: 53.45,
            longitude: -2.23333,
        }),
        "Europe/Istanbul" => Some(CityInfo {
            name: "Istanbul".to_string(),
            country: "Turkey".to_string(),
            latitude: 41.013843,
            longitude: 28.9496613,
        }),
        "Europe/Jersey" => Some(CityInfo {
            name: "Jersey".to_string(),
            country: "Jersey".to_string(),
            latitude: 49.21667,
            longitude: -2.11667,
        }),
        "Europe/Kaliningrad" => Some(CityInfo {
            name: "Kaliningrad".to_string(),
            country: "Russia".to_string(),
            latitude: 54.71,
            longitude: 20.5,
        }),
        "Europe/Kiev" => Some(CityInfo {
            name: "Kiev".to_string(),
            country: "Ukraine".to_string(),
            latitude: 50.4333333,
            longitude: 30.5166664,
        }),
        "Europe/Kirov" => Some(CityInfo {
            name: "Kirov".to_string(),
            country: "Russia".to_string(),
            latitude: 58.5969444,
            longitude: 49.6583328,
        }),
        "Europe/Kyiv" => Some(CityInfo {
            name: "Kyiv".to_string(),
            country: "Ukraine".to_string(),
            latitude: 50.45466,
            longitude: 30.5238,
        }),
        "Europe/Lisbon" => Some(CityInfo {
            name: "Lisbon".to_string(),
            country: "Portugal".to_string(),
            latitude: 38.7166667,
            longitude: -9.1333332,
        }),
        "Europe/Ljubljana" => Some(CityInfo {
            name: "Ljubljana".to_string(),
            country: "Slovenia".to_string(),
            latitude: 46.0552778,
            longitude: 14.5144444,
        }),
        "Europe/London" => Some(CityInfo {
            name: "London".to_string(),
            country: "United Kingdom".to_string(),
            latitude: 51.5084153,
            longitude: -0.1255327,
        }),
        "Europe/Luxembourg" => Some(CityInfo {
            name: "Luxembourg".to_string(),
            country: "Luxembourg".to_string(),
            latitude: 49.6116667,
            longitude: 6.1300001,
        }),
        "Europe/Madrid" => Some(CityInfo {
            name: "Madrid".to_string(),
            country: "Spain".to_string(),
            latitude: 40.4165021,
            longitude: -3.7025642,
        }),
        "Europe/Malta" => Some(CityInfo {
            name: "Malta".to_string(),
            country: "Malta".to_string(),
            latitude: 35.8866745,
            longitude: 14.4539425,
        }),
        "Europe/Mariehamn" => Some(CityInfo {
            name: "Mariehamn".to_string(),
            country: "Finland".to_string(),
            latitude: 60.1,
            longitude: 19.9500008,
        }),
        "Europe/Minsk" => Some(CityInfo {
            name: "Minsk".to_string(),
            country: "Belarus".to_string(),
            latitude: 53.9,
            longitude: 27.5666676,
        }),
        "Europe/Monaco" => Some(CityInfo {
            name: "Monaco".to_string(),
            country: "Monaco".to_string(),
            latitude: 43.7378496,
            longitude: 7.4052529,
        }),
        "Europe/Moscow" => Some(CityInfo {
            name: "Moscow".to_string(),
            country: "Russia".to_string(),
            latitude: 55.7522222,
            longitude: 37.6155548,
        }),
        "Europe/Nicosia" => Some(CityInfo {
            name: "Nicosia".to_string(),
            country: "Italy".to_string(),
            latitude: 37.74747,
            longitude: 14.39218,
        }),
        "Europe/Oslo" => Some(CityInfo {
            name: "Oslo".to_string(),
            country: "Norway".to_string(),
            latitude: 59.911491,
            longitude: 10.757933,
        }),
        "Europe/Paris" => Some(CityInfo {
            name: "Paris".to_string(),
            country: "France".to_string(),
            latitude: 48.85341,
            longitude: 2.3487999,
        }),
        "Europe/Podgorica" => Some(CityInfo {
            name: "Podgorica".to_string(),
            country: "Montenegro".to_string(),
            latitude: 42.44111,
            longitude: 19.26361,
        }),
        "Europe/Prague" => Some(CityInfo {
            name: "Prague".to_string(),
            country: "Czechia".to_string(),
            latitude: 48.3328073,
            longitude: 15.7648158,
        }),
        "Europe/Riga" => Some(CityInfo {
            name: "Riga".to_string(),
            country: "Latvia".to_string(),
            latitude: 56.95,
            longitude: 24.1000004,
        }),
        "Europe/Rome" => Some(CityInfo {
            name: "Rome".to_string(),
            country: "Italy".to_string(),
            latitude: 41.9,
            longitude: 12.4833336,
        }),
        "Europe/Samara" => Some(CityInfo {
            name: "Samara".to_string(),
            country: "Russia".to_string(),
            latitude: 53.2,
            longitude: 50.1500015,
        }),
        "Europe/San_Marino" => Some(CityInfo {
            name: "San Marino".to_string(),
            country: "San Marino".to_string(),
            latitude: 43.9346,
            longitude: 12.4473,
        }),
        "Europe/Sarajevo" => Some(CityInfo {
            name: "Sarajevo".to_string(),
            country: "Bosnia and Herzegovina".to_string(),
            latitude: 43.85,
            longitude: 18.3833332,
        }),
        "Europe/Saratov" => Some(CityInfo {
            name: "Saratov".to_string(),
            country: "Russia".to_string(),
            latitude: 51.5666667,
            longitude: 46.0333328,
        }),
        "Europe/Simferopol" => Some(CityInfo {
            name: "Simferopol".to_string(),
            country: "Ukraine".to_string(),
            latitude: 44.95719,
            longitude: 34.11079,
        }),
        "Europe/Skopje" => Some(CityInfo {
            name: "Skopje".to_string(),
            country: "Macedonia".to_string(),
            latitude: 42.0,
            longitude: 21.4333324,
        }),
        "Europe/Sofia" => Some(CityInfo {
            name: "Sofia".to_string(),
            country: "Bulgaria".to_string(),
            latitude: 42.6975135,
            longitude: 23.3241463,
        }),
        "Europe/Stockholm" => Some(CityInfo {
            name: "Stockholm".to_string(),
            country: "Sweden".to_string(),
            latitude: 59.3325765,
            longitude: 18.0649033,
        }),
        "Europe/Tallinn" => Some(CityInfo {
            name: "Tallinn".to_string(),
            country: "Estonia".to_string(),
            latitude: 59.4369583,
            longitude: 24.7535267,
        }),
        "Europe/Tirane" => Some(CityInfo {
            name: "Tirane".to_string(),
            country: "Albania".to_string(),
            latitude: 41.3275,
            longitude: 19.81889,
        }),
        "Europe/Tiraspol" => Some(CityInfo {
            name: "Tiraspol".to_string(),
            country: "Moldova".to_string(),
            latitude: 46.8402778,
            longitude: 29.6433334,
        }),
        "Europe/Ulyanovsk" => Some(CityInfo {
            name: "Ulyanovsk".to_string(),
            country: "Russia".to_string(),
            latitude: 54.32824,
            longitude: 48.38657,
        }),
        "Europe/Uzhgorod" => Some(CityInfo {
            name: "Uzhgorod".to_string(),
            country: "Ukraine".to_string(),
            latitude: 48.6242,
            longitude: 22.2947,
        }),
        "Europe/Vaduz" => Some(CityInfo {
            name: "Vaduz".to_string(),
            country: "Liechtenstein".to_string(),
            latitude: 47.1415115,
            longitude: 9.5215416,
        }),
        "Europe/Vatican" => Some(CityInfo {
            name: "Vatican".to_string(),
            country: "Vatican City".to_string(),
            latitude: 41.9038124,
            longitude: 12.441757,
        }),
        "Europe/Vienna" => Some(CityInfo {
            name: "Vienna".to_string(),
            country: "Austria".to_string(),
            latitude: 48.2202331,
            longitude: 16.3796424,
        }),
        "Europe/Vilnius" => Some(CityInfo {
            name: "Vilnius".to_string(),
            country: "Lithuania".to_string(),
            latitude: 54.6833333,
            longitude: 25.3166676,
        }),
        "Europe/Volgograd" => Some(CityInfo {
            name: "Volgograd".to_string(),
            country: "Russia".to_string(),
            latitude: 48.8047222,
            longitude: 44.5858345,
        }),
        "Europe/Warsaw" => Some(CityInfo {
            name: "Warsaw".to_string(),
            country: "Poland".to_string(),
            latitude: 52.25,
            longitude: 21.0,
        }),
        "Europe/Zagreb" => Some(CityInfo {
            name: "Zagreb".to_string(),
            country: "Croatia".to_string(),
            latitude: 45.8,
            longitude: 16.0,
        }),
        "Europe/Zaporozhye" => Some(CityInfo {
            name: "Zaporozhye".to_string(),
            country: "Ukraine".to_string(),
            latitude: 47.85167,
            longitude: 35.11714,
        }),
        "Europe/Zurich" => Some(CityInfo {
            name: "Zurich".to_string(),
            country: "Switzerland".to_string(),
            latitude: 47.3666667,
            longitude: 8.5500002,
        }),
        "GMT" => Some(CityInfo {
            name: "GMT".to_string(),
            country: "United States".to_string(),
            latitude: 65.40456,
            longitude: -161.28205,
        }),
        "Greenwich" => Some(CityInfo {
            name: "Greenwich".to_string(),
            country: "United Kingdom".to_string(),
            latitude: 51.47785,
            longitude: -0.01176,
        }),
        "Hongkong" => Some(CityInfo {
            name: "Hongkong".to_string(),
            country: "Hong Kong".to_string(),
            latitude: 22.27832,
            longitude: 114.17469,
        }),
        "Iceland" => Some(CityInfo {
            name: "Iceland".to_string(),
            country: "Iceland".to_string(),
            latitude: 65.0,
            longitude: -18.0,
        }),
        "Indian/Antananarivo" => Some(CityInfo {
            name: "Antananarivo".to_string(),
            country: "Madagascar".to_string(),
            latitude: -18.9166667,
            longitude: 47.5166664,
        }),
        "Indian/Cocos" => Some(CityInfo {
            name: "Cocos".to_string(),
            country: "Cocos (Keeling) Islands".to_string(),
            latitude: -12.0,
            longitude: 96.83333,
        }),
        "Indian/Kerguelen" => Some(CityInfo {
            name: "Kerguelen".to_string(),
            country: "French Southern Territories".to_string(),
            latitude: -49.25,
            longitude: 69.16667,
        }),
        "Indian/Mauritius" => Some(CityInfo {
            name: "Mauritius".to_string(),
            country: "Mauritius".to_string(),
            latitude: -20.3,
            longitude: 57.58333,
        }),
        "Indian/Mayotte" => Some(CityInfo {
            name: "Mayotte".to_string(),
            country: "Mayotte".to_string(),
            latitude: -12.83333,
            longitude: 45.16667,
        }),
        "Indian/Reunion" => Some(CityInfo {
            name: "Reunion".to_string(),
            country: "Réunion".to_string(),
            latitude: -21.1,
            longitude: 55.6,
        }),
        "Iran" => Some(CityInfo {
            name: "Iran".to_string(),
            country: "Iran".to_string(),
            latitude: 32.0,
            longitude: 53.0,
        }),
        "Israel" => Some(CityInfo {
            name: "Israel".to_string(),
            country: "Israel".to_string(),
            latitude: 31.5,
            longitude: 34.75,
        }),
        "Jamaica" => Some(CityInfo {
            name: "Jamaica".to_string(),
            country: "Jamaica".to_string(),
            latitude: 18.16667,
            longitude: -77.25,
        }),
        "Japan" => Some(CityInfo {
            name: "Japan".to_string(),
            country: "Japan".to_string(),
            latitude: 35.68536,
            longitude: 139.75309,
        }),
        "Kwajalein" => Some(CityInfo {
            name: "Kwajalein".to_string(),
            country: "Marshall Islands".to_string(),
            latitude: 9.182,
            longitude: 167.308,
        }),
        "Libya" => Some(CityInfo {
            name: "Libya".to_string(),
            country: "Libya".to_string(),
            latitude: 28.0,
            longitude: 17.0,
        }),
        "MST" => Some(CityInfo {
            name: "MST".to_string(),
            country: "The Netherlands".to_string(),
            latitude: 50.84833,
            longitude: 5.68889,
        }),
        "NZ-CHAT" => Some(CityInfo {
            name: "NZ-CHAT".to_string(),
            country: "New Zealand".to_string(),
            latitude: -41.21667,
            longitude: 174.91667,
        }),
        "Navajo" => Some(CityInfo {
            name: "Navajo".to_string(),
            country: "United States".to_string(),
            latitude: 35.39944,
            longitude: -110.32139,
        }),
        "Pacific/Apia" => Some(CityInfo {
            name: "Apia".to_string(),
            country: "Samoa".to_string(),
            latitude: -13.8333333,
            longitude: -171.7333374,
        }),
        "Pacific/Auckland" => Some(CityInfo {
            name: "Auckland".to_string(),
            country: "New Zealand".to_string(),
            latitude: -36.99282,
            longitude: 174.87986,
        }),
        "Pacific/Bougainville" => Some(CityInfo {
            name: "Bougainville".to_string(),
            country: "French Polynesia".to_string(),
            latitude: -17.58333,
            longitude: -149.3,
        }),
        "Pacific/Chatham" => Some(CityInfo {
            name: "Chatham".to_string(),
            country: "New Zealand".to_string(),
            latitude: -44.0,
            longitude: -176.5,
        }),
        "Pacific/Chuuk" => Some(CityInfo {
            name: "Chuuk".to_string(),
            country: "Micronesia".to_string(),
            latitude: 7.44077,
            longitude: 151.85431,
        }),
        "Pacific/Easter" => Some(CityInfo {
            name: "Easter".to_string(),
            country: "Chile".to_string(),
            latitude: -27.164722,
            longitude: -109.421667,
        }),
        "Pacific/Efate" => Some(CityInfo {
            name: "Efate".to_string(),
            country: "Vanuatu".to_string(),
            latitude: -17.67899,
            longitude: 168.39415,
        }),
        "Pacific/Enderbury" => Some(CityInfo {
            name: "Enderbury".to_string(),
            country: "Kiribati".to_string(),
            latitude: -3.13333,
            longitude: -171.08333,
        }),
        "Pacific/Fakaofo" => Some(CityInfo {
            name: "Fakaofo".to_string(),
            country: "Tokelau".to_string(),
            latitude: -9.376,
            longitude: -171.233,
        }),
        "Pacific/Fiji" => Some(CityInfo {
            name: "Fiji".to_string(),
            country: "Fiji".to_string(),
            latitude: 18.0443939,
            longitude: 178.5559735,
        }),
        "Pacific/Funafuti" => Some(CityInfo {
            name: "Funafuti".to_string(),
            country: "Tuvalu".to_string(),
            latitude: -8.52425,
            longitude: 179.19417,
        }),
        "Pacific/Gambier" => Some(CityInfo {
            name: "Gambier".to_string(),
            country: "French Polynesia".to_string(),
            latitude: -21.83281,
            longitude: -138.89049,
        }),
        "Pacific/Guadalcanal" => Some(CityInfo {
            name: "Guadalcanal".to_string(),
            country: "Solomon Islands".to_string(),
            latitude: -9.69523,
            longitude: 159.71734,
        }),
        "Pacific/Guam" => Some(CityInfo {
            name: "Guam".to_string(),
            country: "Guam".to_string(),
            latitude: 13.485,
            longitude: 144.7975,
        }),
        "Pacific/Honolulu" => Some(CityInfo {
            name: "Honolulu".to_string(),
            country: "United States".to_string(),
            latitude: 21.3069444,
            longitude: -157.8583374,
        }),
        "Pacific/Johnston" => Some(CityInfo {
            name: "Johnston".to_string(),
            country: "Johnston".to_string(),
            latitude: 16.736944,
            longitude: -169.523889,
        }),
        "Pacific/Kiritimati" => Some(CityInfo {
            name: "Kiritimati".to_string(),
            country: "Kiribati".to_string(),
            latitude: 1.94,
            longitude: -157.474,
        }),
        "Pacific/Kosrae" => Some(CityInfo {
            name: "Kosrae".to_string(),
            country: "Micronesia".to_string(),
            latitude: 5.32479,
            longitude: 163.00781,
        }),
        "Pacific/Kwajalein" => Some(CityInfo {
            name: "Kwajalein".to_string(),
            country: "Marshall Islands".to_string(),
            latitude: 9.182,
            longitude: 167.308,
        }),
        "Pacific/Majuro" => Some(CityInfo {
            name: "Majuro".to_string(),
            country: "Marshall Islands".to_string(),
            latitude: 7.1,
            longitude: 171.3833313,
        }),
        "Pacific/Marquesas" => Some(CityInfo {
            name: "Marquesas".to_string(),
            country: "French Polynesia".to_string(),
            latitude: -9.31899,
            longitude: -139.61426,
        }),
        "Pacific/Midway" => Some(CityInfo {
            name: "Midway".to_string(),
            country: "Midway Islands".to_string(),
            latitude: 28.2020739,
            longitude: -177.3805387,
        }),
        "Pacific/Nauru" => Some(CityInfo {
            name: "Nauru".to_string(),
            country: "Nauru".to_string(),
            latitude: -0.547458,
            longitude: 166.9191,
        }),
        "Pacific/Niue" => Some(CityInfo {
            name: "Niue".to_string(),
            country: "Niue".to_string(),
            latitude: -19.078889,
            longitude: -169.925833,
        }),
        "Pacific/Norfolk" => Some(CityInfo {
            name: "Norfolk".to_string(),
            country: "Norfolk Island".to_string(),
            latitude: -29.0,
            longitude: 168.0,
        }),
        "Pacific/Noumea" => Some(CityInfo {
            name: "Noumea".to_string(),
            country: "New Caledonia".to_string(),
            latitude: -22.27407,
            longitude: 166.44884,
        }),
        "Pacific/Pohnpei" => Some(CityInfo {
            name: "Pohnpei".to_string(),
            country: "Micronesia".to_string(),
            latitude: 6.96735,
            longitude: 158.21333,
        }),
        "Pacific/Ponape" => Some(CityInfo {
            name: "Ponape".to_string(),
            country: "Micronesia".to_string(),
            latitude: 6.96735,
            longitude: 158.21333,
        }),
        "Pacific/Port_Moresby" => Some(CityInfo {
            name: "Port Moresby".to_string(),
            country: "Papua New Guinea".to_string(),
            latitude: -9.4647222,
            longitude: 147.1925049,
        }),
        "Pacific/Rarotonga" => Some(CityInfo {
            name: "Rarotonga".to_string(),
            country: "Cook Islands".to_string(),
            latitude: -21.23274,
            longitude: -159.77245,
        }),
        "Pacific/Saipan" => Some(CityInfo {
            name: "Saipan".to_string(),
            country: "Northern Mariana Islands".to_string(),
            latitude: 15.21233,
            longitude: 145.7545,
        }),
        "Pacific/Samoa" => Some(CityInfo {
            name: "Samoa".to_string(),
            country: "French Polynesia".to_string(),
            latitude: -16.77068,
            longitude: -151.42283,
        }),
        "Pacific/Tahiti" => Some(CityInfo {
            name: "Tahiti".to_string(),
            country: "French Polynesia".to_string(),
            latitude: -17.5347,
            longitude: -149.56843,
        }),
        "Pacific/Tarawa" => Some(CityInfo {
            name: "Tarawa".to_string(),
            country: "Kiribati".to_string(),
            latitude: -0.869217,
            longitude: 169.5406342,
        }),
        "Pacific/Tongatapu" => Some(CityInfo {
            name: "Tongatapu".to_string(),
            country: "Tonga".to_string(),
            latitude: -21.17735,
            longitude: -175.1172,
        }),
        "Pacific/Wake" => Some(CityInfo {
            name: "Wake".to_string(),
            country: "Wake Island".to_string(),
            latitude: 19.2825,
            longitude: 166.636667,
        }),
        "Pacific/Yap" => Some(CityInfo {
            name: "Yap".to_string(),
            country: "Micronesia".to_string(),
            latitude: 9.5015731,
            longitude: 138.0857159,
        }),
        "Poland" => Some(CityInfo {
            name: "Poland".to_string(),
            country: "Poland".to_string(),
            latitude: 52.0,
            longitude: 20.0,
        }),
        "Portugal" => Some(CityInfo {
            name: "Portugal".to_string(),
            country: "Portugal".to_string(),
            latitude: 39.6945,
            longitude: -8.13057,
        }),
        "Singapore" => Some(CityInfo {
            name: "Singapore".to_string(),
            country: "Singapore".to_string(),
            latitude: 1.28967,
            longitude: 103.85007,
        }),
        "Turkey" => Some(CityInfo {
            name: "Turkey".to_string(),
            country: "Turkey".to_string(),
            latitude: 39.0,
            longitude: 35.0,
        }),
        "UCT" => Some(CityInfo {
            name: "UCT".to_string(),
            country: "Russia".to_string(),
            latitude: 63.56904,
            longitude: 53.69141,
        }),
        "US/Alaska" => Some(CityInfo {
            name: "Alaska".to_string(),
            country: "United States".to_string(),
            latitude: 61.21806,
            longitude: -149.90028,
        }),
        "US/Arizona" => Some(CityInfo {
            name: "Arizona".to_string(),
            country: "United States".to_string(),
            latitude: 34.5003,
            longitude: -111.50098,
        }),
        "US/East-Indiana" => Some(CityInfo {
            name: "East-Indiana".to_string(),
            country: "United States".to_string(),
            latitude: 38.30308,
            longitude: -85.68222,
        }),
        "US/Hawaii" => Some(CityInfo {
            name: "Hawaii".to_string(),
            country: "United States".to_string(),
            latitude: 20.78785,
            longitude: -156.38612,
        }),
        "US/Indiana-Starke" => Some(CityInfo {
            name: "Indiana-Starke".to_string(),
            country: "United States".to_string(),
            latitude: 41.28093,
            longitude: -86.64765,
        }),
        "US/Michigan" => Some(CityInfo {
            name: "Michigan".to_string(),
            country: "United States".to_string(),
            latitude: 44.25029,
            longitude: -85.50033,
        }),
        "US/Mountain" => Some(CityInfo {
            name: "Mountain".to_string(),
            country: "United States".to_string(),
            latitude: 40.55633,
            longitude: -111.12645,
        }),
        "US/Samoa" => Some(CityInfo {
            name: "Samoa".to_string(),
            country: "Samoa".to_string(),
            latitude: -13.8,
            longitude: -172.13333,
        }),
        _ => None,
    }
}
