//! Color temperature to RGB conversion (Tanner Helland approximation) for gamma tables.

use anyhow::Result;

/// Calculate RGB using Tanner Helland's algorithm.
///
/// Accurate from 1000K to 20000K. Divides the temperature (Kelvin) by 100 to get
/// "temperature in hundreds", then applies empirical formulas derived from CIE color
/// matching functions. Returns (red, green, blue) factors in 0.0-1.0.
///
/// Reference: https://tannerhelland.com/2012/09/18/convert-temperature-rgb-algorithm-code.html
pub fn temperature_to_rgb(temp: u32) -> (f64, f64, f64) {
    let temp_hundreds = temp as f64 / 100.0;

    let (r, g, b) = if temp_hundreds <= 66.0 {
        let r = 255.0;

        let g = if temp_hundreds <= 1.0 {
            0.0
        } else {
            (99.4708 * temp_hundreds.ln() - 161.11957).clamp(0.0, 255.0)
        };

        let b = if temp_hundreds <= 19.0 {
            0.0
        } else {
            let temp_minus_10 = temp_hundreds - 10.0;
            if temp_minus_10 <= 0.0 {
                0.0
            } else {
                (temp_minus_10.ln() * 138.51773 - 305.0448).clamp(0.0, 255.0)
            }
        };

        (r, g, b)
    } else {
        let r = (329.69873 * (temp_hundreds - 60.0).powf(-0.13320476)).clamp(0.0, 255.0);
        let g = (288.12216 * (temp_hundreds - 60.0).powf(-0.07551485)).clamp(0.0, 255.0);
        let b = 255.0;

        (r, g, b)
    };

    (r / 255.0, g / 255.0, b / 255.0)
}

/// RGB factors rounded to 3 decimal places, for debug-logging display only.
pub fn get_rgb_factors(temperature: u32) -> (f64, f64, f64) {
    let (r, g, b) = temperature_to_rgb(temperature);
    (
        (r * 1000.0).round() / 1000.0,
        (g * 1000.0).round() / 1000.0,
        (b * 1000.0).round() / 1000.0,
    )
}

/// Generate a gamma lookup table for one color channel.
///
/// Applies `output = (input * color_factor)^(1/gamma)`, where `input` is normalized
/// 0.0-1.0, `color_factor` (0.0-1.0) adjusts for color temperature, and `gamma`
/// (typically 0.9-1.0) controls the brightness curve. Output is scaled to 0-65535 for
/// the 16-bit protocol.
pub fn generate_gamma_table(size: usize, color_factor: f64, gamma: f64) -> Vec<u16> {
    let mut table = Vec::with_capacity(size);

    for i in 0..size {
        let val = i as f64 / (size - 1) as f64;

        let output = ((val * color_factor).powf(1.0 / gamma) * 65535.0).clamp(0.0, 65535.0);

        // Convert to u16 only at the final step (kept f64 to minimize rounding error)
        table.push(output as u16);
    }

    table
}

/// Create the full R, G, B gamma tables for the wlr-gamma-control-unstable-v1 protocol.
///
/// Uses f64 precision internally to minimize quantization artifacts in the final u16
/// output. Returns the R, G, B tables concatenated as little-endian u16 bytes.
pub fn create_gamma_tables(
    size: usize,
    temperature: u32,
    gamma_percent: f64,
    debug_enabled: bool,
) -> Result<Vec<u8>> {
    let (red_factor, green_factor, blue_factor) = temperature_to_rgb(temperature);

    let red_table = generate_gamma_table(size, red_factor, gamma_percent);
    let green_table = generate_gamma_table(size, green_factor, gamma_percent);
    let blue_table = generate_gamma_table(size, blue_factor, gamma_percent);

    if debug_enabled {
        let sample_indices = [0, 10, 128, 255];
        let r_samples: Vec<u16> = sample_indices.iter().map(|&idx| red_table[idx]).collect();
        let g_samples: Vec<u16> = sample_indices.iter().map(|&idx| green_table[idx]).collect();
        let b_samples: Vec<u16> = sample_indices.iter().map(|&idx| blue_table[idx]).collect();

        log_decorated!("Sample gamma values:");
        log_indented!("R: {:?}", r_samples);
        log_indented!("G: {:?}", g_samples);
        log_indented!("B: {:?}", b_samples);
    }

    // Protocol order: RED, GREEN, BLUE, each little-endian u16 (wlr-gamma-control)
    let mut gamma_data = Vec::with_capacity(size * 3 * 2);

    for value in red_table {
        gamma_data.extend_from_slice(&value.to_le_bytes());
    }

    for value in green_table {
        gamma_data.extend_from_slice(&value.to_le_bytes());
    }

    for value in blue_table {
        gamma_data.extend_from_slice(&value.to_le_bytes());
    }

    Ok(gamma_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temperature_to_rgb_daylight() {
        let (r, g, b) = temperature_to_rgb(6500);
        // Tanner Helland gives (1.0, ~0.996, ~0.981) at 6500K
        assert!((r - 1.0).abs() < 0.01);
        assert!((g - 1.0).abs() < 0.01);
        assert!((b - 1.0).abs() < 0.03); // blue is slightly lower in the algorithm

        assert!(r >= g && g >= b);
        assert!(b > 0.95);
    }

    #[test]
    fn test_temperature_to_rgb_warm() {
        let (r, g, b) = temperature_to_rgb(3300);
        assert!(r > g);
        assert!(g > b);
        assert!(b < 0.8);
    }

    #[test]
    fn test_temperature_to_rgb_cool() {
        let (r, g, b) = temperature_to_rgb(8000);
        assert!(b > g);
        assert!(r < b);
    }

    #[test]
    fn test_temperature_to_rgb_very_warm() {
        let (r, g, b) = temperature_to_rgb(2000);
        assert!(r > g);
        assert!(g > b);
        assert!(b < 0.1);
    }

    #[test]
    fn test_gamma_table_generation() {
        let table = generate_gamma_table(256, 1.0, 1.0);
        assert_eq!(table.len(), 256);
        assert_eq!(table[0], 0);
        assert_eq!(table[255], 65535);

        for i in 1..table.len() {
            assert!(table[i] >= table[i - 1]);
        }
    }

    #[test]
    fn test_gamma_table_with_color_factor() {
        let full_table = generate_gamma_table(256, 1.0, 1.0);
        let half_table = generate_gamma_table(256, 0.5, 1.0);

        assert!(half_table[255] < full_table[255]);
        assert!(half_table[255] < 40000); // roughly half of 65535
    }

    #[test]
    fn test_create_gamma_tables() {
        let tables = create_gamma_tables(256, 6500, 1.0, false).unwrap();
        assert_eq!(tables.len(), 256 * 3 * 2);
    }

    #[test]
    fn test_precision_warm_temperatures() {
        let (r1, g1, b1) = temperature_to_rgb(2000);
        let (r2, g2, b2) = temperature_to_rgb(2001);

        // f64 precision: 1K apart must not collapse to the same RGB
        assert!(r1 != r2 || g1 != g2 || b1 != b2);
    }
}
