//! Color science implementation for accurate color temperature calculations.
//!
//! This module provides sophisticated colorimetric calculations for converting
//! color temperatures to gamma table adjustments.
//!
//! ## Attribution
//!
//! This is an adaptation of wlsunset's color temperature calculation algorithm.
//! Original C implementation: <https://git.sr.ht/~kennylevinsen/wlsunset>
//!
//! wlsunset uses proper colorimetric calculations with CIE XYZ color space,
//! planckian locus, and illuminant D curves to produce accurate color temperatures.
//! This approach is much more accurate than simple RGB approximations, so we've
//! adopted it with some minor adaptations to extend the bottom end of the range.
//!
//! ## Implementation Details
//!
//! The module performs several color space transformations:
//! 1. **Planckian Locus**: Calculates the theoretical color of a black body at a given temperature
//! 2. **CIE XYZ Color Space**: Uses the standard colorimetric system for device-independent colors
//! 3. **sRGB Conversion**: Transforms to the standard RGB color space used by displays
//! 4. **Gamma Correction**: Applies proper gamma curves for display linearization
//!
//! ## Temperature Range
//!
//! Sunsetr supports color temperatures from 1000K to 20000K:
//! - **1000K-1666K:** Tanner Helland empirical approximation (deep reds)
//! - **1667K-2500K:** Planckian locus (warm incandescent-like colors)
//! - **2500K-4000K:** Smooth transition between planckian and illuminant D
//! - **4000K-20000K:** Illuminant D (daylight simulation)
//!
//! The hybrid approach ensures continuous coverage across the full range
//! while maintaining scientific accuracy where CIE colorimetry is valid.

use anyhow::Result;

/// RGB color representation (0.0 to 1.0 range)
#[derive(Debug, Clone, Copy)]
struct Rgb {
    r: f64,
    g: f64,
    b: f64,
}

/// XYZ color space representation
#[derive(Debug, Clone, Copy)]
struct Xyz {
    x: f64,
    y: f64,
    z: f64,
}

/// Clamp value to 0.0-1.0 range
fn clamp(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

/// Apply sRGB gamma correction
/// Based on: https://en.wikipedia.org/wiki/SRGB
fn srgb_gamma(value: f64, gamma: f64) -> f64 {
    if value <= 0.0031308 {
        12.92 * value
    } else {
        (1.055 * value).powf(1.0 / gamma) - 0.055
    }
}

/// Convert XYZ color space to sRGB using standard transformation matrix
/// Reference: http://www.brucelindbloom.com/index.html?Eqn_RGB_XYZ_Matrix.html
fn xyz_to_srgb(xyz: &Xyz) -> Rgb {
    Rgb {
        r: srgb_gamma(
            clamp(3.2404542 * xyz.x - 1.5371385 * xyz.y - 0.4985314 * xyz.z),
            2.2,
        ),
        g: srgb_gamma(
            clamp(-0.9692660 * xyz.x + 1.8760108 * xyz.y + 0.0415560 * xyz.z),
            2.2,
        ),
        b: srgb_gamma(
            clamp(0.0556434 * xyz.x - 0.2040259 * xyz.y + 1.0572252 * xyz.z),
            2.2,
        ),
    }
}

/// Normalize RGB so the maximum component is 1.0
fn srgb_normalize(rgb: &mut Rgb) {
    let max_component = rgb.r.max(rgb.g.max(rgb.b));
    if max_component > 0.0 {
        rgb.r /= max_component;
        rgb.g /= max_component;
        rgb.b /= max_component;
    }
}

/// Calculate illuminant D chromaticity coordinates
///
/// Illuminant D (daylight locus) describes natural daylight as we perceive it.
/// This is how we expect bright, cold white light sources to look.
/// Valid range: 2500K to 25000K (though we stretch it a bit for transitions)
///
/// Reference: https://en.wikipedia.org/wiki/Standard_illuminant#Illuminant_series_D
fn illuminant_d(temp: i32) -> Result<(f64, f64), &'static str> {
    let temp_f = temp as f64;

    let x = if (2500..=7000).contains(&temp) {
        0.244063 + 0.09911e3 / temp_f + 2.9678e6 / temp_f.powi(2) - 4.6070e9 / temp_f.powi(3)
    } else if temp > 7000 && temp <= 25000 {
        0.237040 + 0.24748e3 / temp_f + 1.9018e6 / temp_f.powi(2) - 2.0064e9 / temp_f.powi(3)
    } else {
        return Err("Temperature out of range for illuminant D");
    };

    let y = (-3.0 * x.powi(2)) + (2.870 * x) - 0.275;

    Ok((x, y))
}

/// Calculate Planckian locus chromaticity coordinates
///
/// Planckian locus (black body locus) describes the color of a black body
/// at a certain temperature directly at its source. This is how we expect
/// dim, warm light sources (like incandescent bulbs) to look.
/// Valid range: 1667K to 25000K
///
/// Reference: https://en.wikipedia.org/wiki/Planckian_locus#Approximation
fn planckian_locus(temp: i32) -> Result<(f64, f64), &'static str> {
    let temp_f = temp as f64;

    let (x, y) = if (1667..=4000).contains(&temp) {
        let x = -0.2661239e9 / temp_f.powi(3) - 0.2343589e6 / temp_f.powi(2)
            + 0.8776956e3 / temp_f
            + 0.179910;

        let y = if temp <= 2222 {
            -1.1064814 * x.powi(3) - 1.34811020 * x.powi(2) + 2.18555832 * x - 0.20219683
        } else {
            -0.9549476 * x.powi(3) - 1.37418593 * x.powi(2) + 2.09137015 * x - 0.16748867
        };

        (x, y)
    } else if temp > 4000 && temp < 25000 {
        let x = -3.0258469e9 / temp_f.powi(3)
            + 2.1070379e6 / temp_f.powi(2)
            + 0.2226347e3 / temp_f
            + 0.240390;

        let y = 3.0817580 * x.powi(3) - 5.87338670 * x.powi(2) + 3.75112997 * x - 0.37001483;

        (x, y)
    } else {
        return Err("Temperature out of range for planckian locus");
    };

    Ok((x, y))
}

/// Calculate RGB using Tanner Helland's algorithm
/// This algorithm is used to extend the range of sunsetr's color science below the minimum of the Planckian
/// locus to match hyprsunset's capabilities, interpolating color temperature betwen 1667K and 1000K.
///
/// Reference: https://tannerhelland.com/2012/09/18/convert-temperature-rgb-algorithm-code.html
fn tanner_helland_rgb(temp: u32) -> (f32, f32, f32) {
    let temp_hundreds = (temp / 100) as f32;

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

/// Calculate white point RGB values for a given color temperature
///
/// This is an adaptation of wlsunset's calc_whitepoint function.
/// It uses a combination of planckian locus (for warm temperatures),
/// illuminant D (for cool temperatures), and Tanner Helland's algorithm
/// (for deep reds).
///
/// The algorithm smoothly transitions between planckian locus and illuminant D
/// in the 2500K-4000K range to provide subjectively pleasant colors and uses
/// Tanner Helland's method to extend the range down from 1667K to 1000K.
pub fn temperature_to_rgb(temp: u32) -> (f32, f32, f32) {
    let temp = temp as i32;

    // D65 standard (6500K) is pure white
    if temp == 6500 {
        return (1.0, 1.0, 1.0);
    }

    // Calculate chromaticity coordinates based on temperature range
    let (wp_x, wp_y) = if temp >= 25000 {
        // Very high temperatures: use illuminant D at 25000K
        illuminant_d(25000).unwrap_or((0.31, 0.33))
    } else if temp >= 4000 {
        // High temperatures (4000K+): use illuminant D
        illuminant_d(temp).unwrap_or((0.31, 0.33))
    } else if temp >= 2500 {
        // Medium temperatures (2500K-4000K): smooth transition between curves
        let (x1, y1) = illuminant_d(temp).unwrap_or((0.31, 0.33));
        let (x2, y2) = planckian_locus(temp).unwrap_or((0.45, 0.41));

        // Cosine interpolation factor for smooth transition
        let factor = (4000.0 - temp as f64) / 1500.0;
        let sine_factor = ((std::f64::consts::PI * factor).cos() + 1.0) / 2.0;

        let wp_x = x1 * sine_factor + x2 * (1.0 - sine_factor);
        let wp_y = y1 * sine_factor + y2 * (1.0 - sine_factor);

        (wp_x, wp_y)
    } else if temp >= 1667 {
        // Low temperatures: use planckian locus (valid range)
        planckian_locus(temp).unwrap_or((0.45, 0.41))
    } else {
        // Very low temperatures (< 1667K): use Tanner Helland algorithm
        // Convert to RGB directly (skip XYZ transformation)
        let (r, g, b) = tanner_helland_rgb(temp as u32);

        // Return directly - Tanner Helland already normalized
        // We need to exit early here since we're skipping XYZ conversion
        return (r, g, b);
    };

    // Convert chromaticity coordinates to XYZ
    let wp_z = 1.0 - wp_x - wp_y;
    let xyz = Xyz {
        x: wp_x,
        y: wp_y,
        z: wp_z,
    };

    // Convert XYZ to sRGB and normalize
    let mut rgb = xyz_to_srgb(&xyz);
    srgb_normalize(&mut rgb);

    // Return as f32 for compatibility with existing code
    (rgb.r as f32, rgb.g as f32, rgb.b as f32)
}

// # End of wlsunset Color Science Implementation

/// Get RGB factors for a given color temperature as a formatted tuple.
/// This is a convenience function for debug logging.
///
/// # Arguments
/// * `temperature` - Color temperature in Kelvin (1000-25000)
///
/// # Returns
/// Tuple of (red_factor, green_factor, blue_factor) rounded to 3 decimal places
pub fn get_rgb_factors(temperature: u32) -> (f32, f32, f32) {
    let (r, g, b) = temperature_to_rgb(temperature);
    // Round to 3 decimal places for cleaner logging
    (
        (r * 1000.0).round() / 1000.0,
        (g * 1000.0).round() / 1000.0,
        (b * 1000.0).round() / 1000.0,
    )
}

/// Generate gamma table for a specific color channel using wlsunset's approach.
///
/// Creates a gamma lookup table (LUT) that maps input values to output values
/// using a power function gamma curve.
///
/// # Arguments
/// * `size` - Size of the gamma table (typically 256 or 1024)
/// * `color_factor` - Color temperature adjustment factor (0.0-1.0)
/// * `gamma` - Gamma curve value (typically 1.0 for linear, 0.9 for 90% brightness)
///
/// # Returns
/// Vector of 16-bit gamma values for this color channel
pub fn generate_gamma_table(size: usize, color_factor: f64, gamma: f64) -> Vec<u16> {
    let mut table = Vec::with_capacity(size);

    for i in 0..size {
        // Calculate normalized input value (0.0 to 1.0)
        let val = i as f64 / (size - 1) as f64;

        // Apply color temperature factor and gamma curve using power function
        // This matches wlsunset's formula: pow(val * color_factor, 1.0 / gamma)
        let output = ((val * color_factor).powf(1.0 / gamma) * 65535.0).clamp(0.0, 65535.0);

        table.push(output as u16);
    }

    table
}

/// Create complete gamma tables for RGB channels.
///
/// Generates the full set of gamma lookup tables needed for the
/// wlr-gamma-control-unstable-v1 protocol.
///
/// # Arguments
/// * `size` - Size of each gamma table (reported by compositor)
/// * `temperature` - Color temperature in Kelvin
/// * `gamma_percent` - Gamma adjustment as percentage (90% = 0.9, 100% = 1.0, 200% = 2.0)
/// * `debug_enabled` - Whether to output debug information
///
/// # Returns
/// Byte vector containing concatenated R, G, B gamma tables
pub fn create_gamma_tables(
    size: usize,
    temperature: u32,
    gamma_percent: f32,
    debug_enabled: bool,
) -> Result<Vec<u8>> {
    // Convert temperature to RGB factors
    let (red_factor, green_factor, blue_factor) = temperature_to_rgb(temperature);

    // Generate individual channel tables using power function gamma curves
    let red_table = generate_gamma_table(size, red_factor as f64, gamma_percent as f64);
    let green_table = generate_gamma_table(size, green_factor as f64, gamma_percent as f64);
    let blue_table = generate_gamma_table(size, blue_factor as f64, gamma_percent as f64);

    // Log some sample values for debugging
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

    // Convert to bytes (little-endian 16-bit values)
    // Using the documented wlr-gamma-control protocol order: RED, GREEN, BLUE
    // This matches wlsunset's layout: r = table, g = table + ramp_size, b = table + 2*ramp_size
    let mut gamma_data = Vec::with_capacity(size * 3 * 2);

    // Red channel
    for value in red_table {
        gamma_data.extend_from_slice(&value.to_le_bytes());
    }

    // Green channel
    for value in green_table {
        gamma_data.extend_from_slice(&value.to_le_bytes());
    }

    // Blue channel
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
        // Daylight should be neutral
        assert!((r - 1.0).abs() < 0.01);
        assert!((g - 1.0).abs() < 0.01);
        assert!((b - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_temperature_to_rgb_warm() {
        let (r, g, b) = temperature_to_rgb(3300);
        // Warm light should be red-heavy, blue-light
        assert!(r > g);
        assert!(g > b);
        assert!(b < 0.8);
    }

    #[test]
    fn test_temperature_to_rgb_cool() {
        let (r, g, b) = temperature_to_rgb(8000);
        // Cool light should be blue-heavy
        assert!(b > g);
        assert!(r < b);
    }

    #[test]
    fn test_gamma_table_generation() {
        let table = generate_gamma_table(256, 1.0, 1.0);
        assert_eq!(table.len(), 256);
        assert_eq!(table[0], 0);
        assert_eq!(table[255], 65535);

        // Should be monotonically increasing
        for i in 1..table.len() {
            assert!(table[i] >= table[i - 1]);
        }
    }

    #[test]
    fn test_create_gamma_tables() {
        let tables = create_gamma_tables(256, 6500, 1.0, false).unwrap();
        // Should contain 3 channels * 256 entries * 2 bytes each
        assert_eq!(tables.len(), 256 * 3 * 2);
    }
}
