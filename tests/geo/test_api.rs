use chrono::NaiveDate;
use sunrise::{Coordinates, SolarDay, SolarEvent, DawnType};

fn main() {
    let date = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
    let latitude = 40.7128;
    let longitude = -74.0060;
    
    // Test Coordinates::new()
    let coord = Coordinates::new(latitude, longitude);
    println!("Coordinates type: {:?}", std::any::type_name_of_val(&coord));
    
    // Test SolarDay::new()
    let solar_day = SolarDay::new(coord, date);
    println!("SolarDay type: {:?}", std::any::type_name_of_val(&solar_day));
    
    // Test event_time()
    let civil_dawn = solar_day.event_time(SolarEvent::Dawn(DawnType::Civil));
    println!("event_time return type: {:?}", std::any::type_name_of_val(&civil_dawn));
    println!("Civil dawn: {:?}", civil_dawn);
}