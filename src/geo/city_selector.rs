//! Interactive city selection for geographic coordinate determination.
//!
//! This module provides functionality for users to interactively select their
//! city from a database of world cities, organized by region for easy navigation.
//! Uses the `cities` crate for a comprehensive database of 10,000+ cities worldwide.

use crate::logger::Log;
use anyhow::Result;
use crossterm::{
    ExecutableCommand,
    cursor::{Hide, MoveDown, MoveUp, Show},
    event::{self, Event, KeyCode},
    style::Print,
    terminal::{self, Clear, ClearType},
};
use std::io::{Write, stdout};

/// Represents a city with its geographic information
#[derive(Debug, Clone)]
pub struct CityInfo {
    pub name: String,
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// Run interactive city selection with fuzzy search
///
/// This function provides a single-step fuzzy search across all cities worldwide.
///
/// # Returns
/// * `Ok((latitude, longitude, city_name))` - Selected city coordinates and name
/// * `Err(_)` - If selection fails or user cancels
pub fn select_city_interactive() -> Result<(f64, f64, String)> {
    Log::log_block_start("Select the nearest city for more accurate transition times");

    // Get all cities as a single list
    let all_cities = get_all_cities();

    Log::log_indented("Type to search, use ↑/↓ to navigate, Enter to select, Esc to cancel");

    let selected_city = fuzzy_search_city(&all_cities)?;

    // Apply coordinate corrections for known database errors
    let (corrected_lat, corrected_lon) = crate::geo::correct_coordinates(
        &selected_city.name,
        &selected_city.country,
        selected_city.latitude,
        selected_city.longitude,
    );

    Log::log_block_start(&format!(
        "Selected: {}, {}",
        selected_city.name, selected_city.country
    ));
    Log::log_indented(&format!(
        "Coordinates: {:.4}°N, {:.4}°{}",
        corrected_lat,
        corrected_lon.abs(),
        if corrected_lon >= 0.0 { "E" } else { "W" }
    ));

    Ok((
        corrected_lat,
        corrected_lon,
        format!("{}, {}", selected_city.name, selected_city.country),
    ))
}

/// Get all cities as a simple list
fn get_all_cities() -> Vec<CityInfo> {
    let iter = IntoIterator::into_iter(cities::all());
    let mut all_cities: Vec<CityInfo> = iter
        .filter_map(|city| {
            // Skip cities with empty names
            if city.city.trim().is_empty() {
                return None;
            }

            Some(CityInfo {
                name: city.city.to_string(),
                country: city.country.to_string(),
                latitude: city.latitude,
                longitude: city.longitude,
            })
        })
        .collect();

    // Sort cities alphabetically by name
    all_cities.sort_by(|a, b| a.name.cmp(&b.name));

    all_cities
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
pub fn find_cities_near_coordinate(
    target_lat: f64,
    target_lon: f64,
    max_results: usize,
) -> Vec<CityInfo> {
    let iter = IntoIterator::into_iter(cities::all());
    let mut cities_with_distance: Vec<(CityInfo, f64)> = iter
        .map(|city| {
            let city_info = CityInfo {
                name: city.city.to_string(),
                country: city.country.to_string(),
                latitude: city.latitude,
                longitude: city.longitude,
            };
            let distance =
                calculate_distance(target_lat, target_lon, city.latitude, city.longitude);
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

/// Fuzzy search for cities with a fixed-height scrollable list
fn fuzzy_search_city(cities: &[CityInfo]) -> Result<&CityInfo> {
    // Debug: check if we have cities
    if cities.is_empty() {
        return Err(anyhow::anyhow!("No cities available"));
    }

    // Terminal handling for fuzzy search UI

    // Set up terminal
    let mut stdout = stdout();
    stdout.flush()?; // Ensure previous output is displayed
    terminal::enable_raw_mode()?;
    stdout.execute(Hide)?;

    // State for fuzzy search
    let mut search_query = String::new();
    let mut selected_index = 0;
    let mut scroll_offset = 0;
    const VISIBLE_ITEMS: usize = 5;

    // Save terminal state
    let (_initial_col, initial_row) = crossterm::cursor::position()?;
    let _ui_start_row = initial_row + 1; // Start one line below current position

    let result = loop {
        // Filter cities based on search query
        let filtered_cities: Vec<&CityInfo> = if search_query.is_empty() {
            cities.iter().take(100).collect() // Show first 100 when no search
        } else {
            cities
                .iter()
                .filter(|city| {
                    let search_lower = search_query.to_lowercase();
                    city.name.to_lowercase().contains(&search_lower)
                        || city.country.to_lowercase().contains(&search_lower)
                })
                .take(100) // Limit to 100 results for performance
                .collect()
        };

        // Adjust selection if it's out of bounds
        if selected_index >= filtered_cities.len() && !filtered_cities.is_empty() {
            selected_index = filtered_cities.len() - 1;
        }

        // Adjust scroll to keep selection visible
        if selected_index < scroll_offset {
            scroll_offset = selected_index;
        } else if selected_index >= scroll_offset + VISIBLE_ITEMS {
            scroll_offset = selected_index - VISIBLE_ITEMS + 1;
        }

        // Clear from cursor down (like the working dropdown)
        stdout.execute(Clear(ClearType::FromCursorDown))?;

        // Add the pipe-only gap line to maintain logger visual continuity
        stdout.execute(Print("┃\r\n"))?;

        // Draw search box with correct pipe character
        stdout.execute(Print("┃ Search: "))?;
        stdout.execute(Print(&search_query))?;
        if search_query.is_empty() {
            stdout.execute(Print("_"))?;
        }
        stdout.execute(Print("\r\n"))?;

        // Draw city results (always exactly 5 lines)
        for i in 0..VISIBLE_ITEMS {
            if scroll_offset + i < filtered_cities.len() {
                let city = &filtered_cities[scroll_offset + i];
                let is_selected = scroll_offset + i == selected_index;

                let display = format!("{}, {}", city.name, city.country);
                let max_width = 60;
                let display = if display.len() > max_width {
                    format!("{}…", &display[..max_width - 1])
                } else {
                    display
                };

                if is_selected {
                    stdout.execute(Print("┃ ▶ "))?;
                    stdout.execute(Print(&display))?;
                } else {
                    stdout.execute(Print("┃   "))?;
                    stdout.execute(Print(&display))?;
                }
            } else {
                stdout.execute(Print("┃"))?;
            }
            stdout.execute(Print("\r\n"))?;
        }

        // Status line
        stdout.execute(Print("┃ "))?;
        if filtered_cities.is_empty() {
            stdout.execute(Print("No cities found"))?;
        } else {
            stdout.execute(Print(format!(
                "{} of {} cities",
                filtered_cities.len(),
                cities.len()
            )))?;
        }
        stdout.execute(Print("\r\n"))?;

        stdout.flush()?;

        // Move cursor back up to start for next update (like working dropdown)
        // We drew: pipe gap + search line + 5 city lines + status line = 8 lines total
        let lines_drawn = 1 + 1 + VISIBLE_ITEMS + 1; // pipe gap + search + cities + status
        stdout.execute(MoveUp(lines_drawn as u16))?;

        // Handle keyboard input
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc => {
                    break Err(anyhow::anyhow!("City selection cancelled by user"));
                }
                KeyCode::Enter => {
                    if !filtered_cities.is_empty() {
                        break Ok(filtered_cities[selected_index]);
                    }
                }
                KeyCode::Up if selected_index > 0 => {
                    selected_index -= 1;
                }
                KeyCode::Up => {}
                KeyCode::Down => {
                    if selected_index + 1 < filtered_cities.len() {
                        selected_index += 1;
                    }
                }
                KeyCode::Backspace => {
                    search_query.pop();
                    selected_index = 0;
                    scroll_offset = 0;
                }
                KeyCode::Char(c) => {
                    search_query.push(c);
                    selected_index = 0;
                    scroll_offset = 0;
                }
                _ => {}
            }
        }
    };

    // Clean up terminal
    terminal::disable_raw_mode()?;
    stdout.execute(Show)?;

    // Move cursor down past the search UI for next logger output
    let lines_drawn = 1 + 1 + VISIBLE_ITEMS + 1; // pipe gap + search + cities + status
    stdout.execute(MoveDown(lines_drawn as u16))?;
    stdout.flush()?;

    result
}

#[cfg(test)]
mod tests {
    use super::*;

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

