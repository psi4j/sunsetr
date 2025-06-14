//! Test module to explore sunrise crate API capabilities

use chrono::{NaiveDate, Datelike};

pub fn explore_sunrise_api() {
    let date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
    let latitude = 40.7128;
    let longitude = -74.0060;
    
    println!("Testing sunrise crate API...");
    
    // Current working function (deprecated)
    let (sunrise_ts, sunset_ts) = sunrise::sunrise_sunset(
        latitude,
        longitude,
        date.year(),
        date.month(),
        date.day(),
    );
    
    println!("Current API - Sunrise timestamp: {}, Sunset timestamp: {}", sunrise_ts, sunset_ts);
    
    // Let's try the new SolarEvent API
    // First, let's see what's available
    println!("Exploring new SolarEvent API...");
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_current_api() {
        explore_sunrise_api();
    }
}