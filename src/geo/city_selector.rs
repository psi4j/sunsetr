//! Interactive city selection for geographic coordinate determination.
//!
//! Fuzzy search over a database of more than 10,000 cities. Users type a
//! partial city or country name to filter, navigate with the arrow keys, and
//! select with Enter (Esc cancels).

use anyhow::Result;
use crossterm::{
    ExecutableCommand,
    cursor::{Hide, MoveUp, Show},
    event::{self, Event, KeyCode},
    style::Print,
    terminal::{self, Clear, ClearType},
};
use std::io::{Write, stdout};

/// A city with the geographic information needed for solar calculations.
#[derive(Debug, Clone)]
pub struct CityInfo {
    pub name: String,
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// Run interactive city selection, returning `(latitude, longitude, name)`.
///
/// Errors if the user cancels with Esc or terminal operations fail.
pub fn select_city_interactive() -> Result<(f64, f64, String)> {
    log_block_start!("Select the nearest city for more accurate transition times");

    let all_cities = get_all_cities();

    log_indented!("Type to search, use ↑/↓ to navigate, Enter to select, Esc to cancel");

    let selected_city = fuzzy_search_city(&all_cities)?;

    log_block_start!(
        "Selected: {}, {}",
        selected_city.name,
        selected_city.country
    );

    Ok((
        selected_city.latitude,
        selected_city.longitude,
        format!("{}, {}", selected_city.name, selected_city.country),
    ))
}

/// Get all cities from the database as a sorted list.
///
/// This function loads all cities from the `cities` crate database,
/// filters out entries with empty names, and sorts them alphabetically.
///
/// # Returns
/// A vector of all valid cities sorted by name
fn get_all_cities() -> Vec<CityInfo> {
    let iter = IntoIterator::into_iter(cities::all());
    let mut all_cities: Vec<CityInfo> = iter
        .filter_map(|city| {
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

    all_cities.sort_by(|a, b| a.name.cmp(&b.name));

    all_cities
}

/// Fuzzy search for cities with a fixed-height scrollable list.
///
/// This function implements the interactive UI for city selection, handling:
/// - Real-time search filtering as the user types
/// - Keyboard navigation with arrow keys
/// - Visual feedback with selection highlighting
/// - Smooth scrolling through results
///
/// # UI Layout
/// ```text
/// ┃
/// ┃ Search: london_
/// ┃ ▶ London, United Kingdom
/// ┃   London, Canada
/// ┃   Londonderry, United Kingdom
/// ┃   New London, United States
/// ┃   East London, South Africa
/// ┃ 23 of 10234 cities
/// ```
///
/// # Keyboard Controls
/// - Type: Filter cities by name or country
/// - ↑/↓: Navigate through results
/// - Enter: Select highlighted city
/// - Esc: Cancel selection
/// - Backspace: Delete last character
///
/// # Arguments
/// * `cities` - Slice of all available cities
///
/// # Returns
/// * `Ok(&CityInfo)` - Reference to the selected city
/// * `Err(_)` - If user cancels or no cities match
///
/// # Errors
/// Returns an error if:
/// - No cities are available
/// - User presses Esc to cancel
/// - Terminal operations fail
fn fuzzy_search_city(cities: &[CityInfo]) -> Result<&CityInfo> {
    if cities.is_empty() {
        return Err(anyhow::anyhow!("No cities available"));
    }

    let mut stdout = stdout();
    stdout.flush()?; // Ensure previous output is displayed
    terminal::enable_raw_mode()?;
    stdout.execute(Hide)?;

    let mut search_query = String::new();
    let mut selected_index = 0;
    let mut scroll_offset = 0;
    const VISIBLE_ITEMS: usize = 5;

    let (_initial_col, initial_row) = crossterm::cursor::position()?;
    let _ui_start_row = initial_row + 1; // Start one line below current position

    let result = loop {
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

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc => {
                    break Err(anyhow::anyhow!("City selection cancelled by user"));
                }
                KeyCode::Enter if !filtered_cities.is_empty() => {
                    break Ok(filtered_cities[selected_index]);
                }
                KeyCode::Up if selected_index > 0 => {
                    selected_index -= 1;
                }
                KeyCode::Up => {}
                KeyCode::Down if selected_index + 1 < filtered_cities.len() => {
                    selected_index += 1;
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

    terminal::disable_raw_mode()?;
    stdout.execute(Show)?;

    // Clear the interactive UI completely - we're already positioned at the top from the last MoveUp
    stdout.execute(Clear(ClearType::FromCursorDown))?;
    stdout.flush()?;

    result
}
