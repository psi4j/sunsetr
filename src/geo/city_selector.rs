//! Interactive city selection for geographic coordinate determination.
//!
//! This module provides functionality for users to interactively select their
//! city from a database of world cities, organized by region for easy navigation.
//! Uses the `cities` crate for a comprehensive database of 10,000+ cities worldwide.

use anyhow::Result;
use crate::logger::Log;
use crate::utils::show_dropdown_menu;
use std::collections::HashMap;

/// Represents a city with its geographic information
#[derive(Debug, Clone)]
pub struct CityInfo {
    pub name: String,
    pub country: String,
    pub region: String,
    pub latitude: f64,
    pub longitude: f64,
    pub population: Option<u64>,
}

/// Run interactive city selection with regional grouping
///
/// This function provides a two-step selection process:
/// 1. Select a region/continent
/// 2. Select a city within that region
///
/// # Returns
/// * `Ok((latitude, longitude, city_name))` - Selected city coordinates and name
/// * `Err(_)` - If selection fails or user cancels
pub fn select_city_interactive() -> Result<(f64, f64, String)> {
    Log::log_block_start("Interactive City Selection");
    Log::log_indented("Choose your region and city for accurate sunrise/sunset times");

    // Get cities grouped by region
    let cities_by_region = get_cities_by_region();
    
    // Step 1: Select region
    let region_options: Vec<(String, String)> = cities_by_region.keys()
        .map(|region| (region.clone(), region.clone()))
        .collect();
    
    Log::log_pipe();
    let selected_region_idx = show_dropdown_menu(
        &region_options,
        Some("Select your region:"),
        Some("City selection cancelled. Use --geo again to retry.")
    )?;
    let selected_region = &region_options[selected_region_idx].1;
    
    // Step 2: Select city within region
    let cities_in_region = &cities_by_region[selected_region];
    let city_options: Vec<(String, &CityInfo)> = cities_in_region.iter()
        .map(|city| {
            let display_name = format!("{}, {}", city.name, city.country);
            (display_name, city)
        })
        .collect();
    
    Log::log_pipe();
    let selected_city_idx = show_dropdown_menu(
        &city_options,
        Some(&format!("Select your city in {}:", selected_region)),
        Some("City selection cancelled. Use --geo again to retry.")
    )?;
    
    let selected_city = city_options[selected_city_idx].1;
    
    Log::log_block_start(&format!("Selected: {}, {}", selected_city.name, selected_city.country));
    Log::log_indented(&format!("Region: {}", selected_city.region));
    Log::log_indented(&format!("Coordinates: {:.4}°N, {:.4}°W", 
        selected_city.latitude, selected_city.longitude.abs()));
    
    Ok((selected_city.latitude, selected_city.longitude, format!("{}, {}", selected_city.name, selected_city.country)))
}

/// Get all cities organized by region/continent
fn get_cities_by_region() -> HashMap<String, Vec<CityInfo>> {
    let mut cities_by_region: HashMap<String, Vec<CityInfo>> = HashMap::new();
    
    // Iterate through all cities from the cities crate
    for city in cities::all() {
        let city_info = CityInfo {
            name: city.city.to_string(),
            country: city.country.to_string(),
            region: region_for_country(city.country),
            latitude: city.latitude,
            longitude: city.longitude,
            population: None, // cities crate doesn't provide population data
        };
        
        cities_by_region
            .entry(city_info.region.clone())
            .or_insert_with(Vec::new)
            .push(city_info);
    }
    
    // Sort cities within each region by name (alphabetical)
    for cities in cities_by_region.values_mut() {
        cities.sort_by(|a, b| a.name.cmp(&b.name));
        
        // Limit to top 100 cities per region to keep menus manageable
        cities.truncate(100);
    }
    
    cities_by_region
}

/// Map country names to regions/continents for better organization
fn region_for_country(country: &str) -> String {
    match country {
        // North America
        "United States" | "Canada" | "Mexico" | "Guatemala" | "Belize" | "Honduras" | 
        "El Salvador" | "Nicaragua" | "Costa Rica" | "Panama" => "North America".to_string(),
        
        // South America  
        "Brazil" | "Argentina" | "Chile" | "Peru" | "Colombia" | "Venezuela" | "Ecuador" |
        "Bolivia" | "Paraguay" | "Uruguay" | "Guyana" | "Suriname" | "French Guiana" => "South America".to_string(),
        
        // Europe
        "United Kingdom" | "France" | "Germany" | "Italy" | "Spain" | "Netherlands" | "Belgium" |
        "Switzerland" | "Austria" | "Sweden" | "Norway" | "Denmark" | "Finland" | "Poland" |
        "Czech Republic" | "Hungary" | "Portugal" | "Ireland" | "Greece" | "Romania" | "Bulgaria" |
        "Croatia" | "Slovenia" | "Slovakia" | "Lithuania" | "Latvia" | "Estonia" | "Serbia" |
        "Bosnia and Herzegovina" | "Montenegro" | "North Macedonia" | "Albania" | "Moldova" |
        "Belarus" | "Ukraine" | "Russia" | "Iceland" | "Luxembourg" | "Malta" | "Cyprus" => "Europe".to_string(),
        
        // Asia
        "China" | "India" | "Japan" | "South Korea" | "Indonesia" | "Thailand" | "Vietnam" |
        "Philippines" | "Malaysia" | "Singapore" | "Bangladesh" | "Pakistan" | "Myanmar" |
        "Cambodia" | "Laos" | "Sri Lanka" | "Nepal" | "Bhutan" | "Maldives" | "Mongolia" |
        "Kazakhstan" | "Uzbekistan" | "Turkmenistan" | "Kyrgyzstan" | "Tajikistan" | "Afghanistan" |
        "Iran" | "Iraq" | "Turkey" | "Syria" | "Lebanon" | "Jordan" | "Israel" | "Palestine" |
        "Saudi Arabia" | "United Arab Emirates" | "Qatar" | "Kuwait" | "Bahrain" | "Oman" | "Yemen" => "Asia".to_string(),
        
        // Africa
        "South Africa" | "Nigeria" | "Egypt" | "Kenya" | "Ethiopia" | "Ghana" | "Morocco" |
        "Algeria" | "Tunisia" | "Libya" | "Sudan" | "Tanzania" | "Uganda" | "Zimbabwe" |
        "Zambia" | "Botswana" | "Namibia" | "Angola" | "Mozambique" | "Madagascar" | "Mauritius" |
        "Seychelles" | "Cameroon" | "Ivory Coast" | "Senegal" | "Mali" | "Burkina Faso" | "Niger" |
        "Chad" | "Central African Republic" | "Democratic Republic of the Congo" | "Republic of the Congo" |
        "Gabon" | "Equatorial Guinea" | "Rwanda" | "Burundi" | "Djibouti" | "Somalia" | "Eritrea" => "Africa".to_string(),
        
        // Oceania
        "Australia" | "New Zealand" | "Papua New Guinea" | "Fiji" | "Solomon Islands" |
        "Vanuatu" | "New Caledonia" | "French Polynesia" | "Samoa" | "Tonga" | "Palau" |
        "Micronesia" | "Marshall Islands" | "Kiribati" | "Tuvalu" | "Nauru" => "Oceania".to_string(),
        
        // Default fallback
        _ => "Other".to_string(),
    }
}

/// Format population numbers in a human-readable way
fn format_population(pop: u64) -> String {
    match pop {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1_000_000.0),
        n if n >= 1_000 => format!("{}K", n / 1_000),
        n => n.to_string(),
    }
}

/// Find cities near a given coordinate (for timezone detection)
///
/// This function finds the closest cities to a given coordinate, useful for
/// timezone-based detection where we want to suggest cities near the detected location.
///
/// # Arguments
/// * `target_lat` - Target latitude
/// * `target_lon` - Target longitude  
/// * `max_results` - Maximum number of cities to return
/// 
/// # Returns
/// Vector of closest cities sorted by distance
pub fn find_cities_near_coordinate(target_lat: f64, target_lon: f64, max_results: usize) -> Vec<CityInfo> {
    let mut cities_with_distance: Vec<(CityInfo, f64)> = cities::all()
        .into_iter()
        .map(|city| {
            let city_info = CityInfo {
                name: city.city.to_string(),
                country: city.country.to_string(),
                region: region_for_country(city.country),
                latitude: city.latitude,
                longitude: city.longitude,
                population: None, // cities crate doesn't provide population data
            };
            let distance = calculate_distance(target_lat, target_lon, city.latitude, city.longitude);
            (city_info, distance)
        })
        .collect();
    
    // Sort by distance (closest first)
    cities_with_distance.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    
    // Return top results
    cities_with_distance
        .into_iter()
        .take(max_results)
        .map(|(city, _)| city)
        .collect()
}

/// Calculate approximate distance between two coordinates (simple Euclidean distance)
///
/// This is a simplified distance calculation suitable for finding nearby cities.
/// For more precise geographic calculations, the haversine formula would be better.
fn calculate_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat_diff = lat1 - lat2;
    let lon_diff = lon1 - lon2;
    (lat_diff * lat_diff + lon_diff * lon_diff).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_classification() {
        assert_eq!(region_for_country("United States"), "North America");
        assert_eq!(region_for_country("Brazil"), "South America");
        assert_eq!(region_for_country("France"), "Europe");
        assert_eq!(region_for_country("China"), "Asia");
        assert_eq!(region_for_country("Nigeria"), "Africa");
        assert_eq!(region_for_country("Australia"), "Oceania");
        assert_eq!(region_for_country("Unknown Country"), "Other");
    }

    #[test]
    fn test_population_formatting() {
        assert_eq!(format_population(1_500_000), "1.5M");
        assert_eq!(format_population(500_000), "500K");
        assert_eq!(format_population(1_500), "1K");
        assert_eq!(format_population(500), "500");
    }

    #[test]
    fn test_distance_calculation() {
        // Test distance calculation (should be 0 for same coordinates)
        let distance = calculate_distance(40.7128, -74.0060, 40.7128, -74.0060);
        assert!(distance < 0.001);
        
        // Test that distance is positive for different coordinates
        let distance = calculate_distance(40.7128, -74.0060, 41.8781, -87.6298);
        assert!(distance > 0.0);
    }

    #[test]
    fn test_find_cities_near_coordinate() {
        // Test finding cities near NYC coordinates
        let cities = find_cities_near_coordinate(40.7128, -74.0060, 5);
        assert!(cities.len() <= 5);
        assert!(!cities.is_empty());
    }
}