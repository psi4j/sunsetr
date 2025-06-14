use chrono::NaiveDate;

fn main() {
    // Test the current sunrise crate API
    let date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
    let latitude = 40.7128;
    let longitude = -74.0060;
    
    // Current working function
    let (sunrise_ts, sunset_ts) = sunrise::sunrise_sunset(
        latitude,
        longitude,
        date.year(),
        date.month(),
        date.day(),
    );
    
    println!("Current API - Sunrise: {}, Sunset: {}", sunrise_ts, sunset_ts);
    
    // Try to explore what other functions might be available
    // This is a test to see what's available in the crate
}