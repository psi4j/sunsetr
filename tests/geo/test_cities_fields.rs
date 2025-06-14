use cities;

fn main() {
    // Get first city and print its fields
    if let Some(city) = cities::all().into_iter().next() {
        println!("City struct fields:");
        println!("  city: {}", city.city);
        println!("  country: {}", city.country);
        println!("  latitude: {}", city.latitude);
        println!("  longitude: {}", city.longitude);
        
        // Try to access other potential fields
        // This will help us see what's available
        
        // Print the debug representation to see all fields
        println!("\nDebug representation:");
        println!("{:#?}", city);
    }
}