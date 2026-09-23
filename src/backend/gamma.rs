//! Color temperature and brightness to RGB and CTM conversion.

use anyhow::Result;

type Xy = [f64; 2];
type Xyz = [f64; 3];
type Rgb = [f64; 3];
type Mat3x3 = [f64; 9];

const TEMPERATURE_D65: u32 = 6500;
const CHROMATICITY_D65: Xy = [0.31271, 0.32902];

/// For converting brightness to luminance. Typically this would be 3.0,
/// but 2.2 works well and matches the old behavior.
const BRIGHTNESS_POWER: f64 = 2.2;

/// Reference: <http://www.brucelindbloom.com/index.html?Eqn_RGB_XYZ_Matrix.html>
const MATRIX_XYZ_TO_REC709: Mat3x3 = [
    3.2404542, -1.5371385, -0.4985314,
    -0.9692660, 1.8760108, 0.0415560,
    0.0556434, -0.2040259, 1.0572252,
];
const MATRIX_REC709_TO_XYZ: Mat3x3 = [
    0.4124564, 0.3575761, 0.1804375,
    0.2126729, 0.7151522, 0.0721750,
    0.0193339, 0.1191920, 0.9503041,
];

/// Reference: <http://www.brucelindbloom.com/index.html?Eqn_ChromAdapt.html>
const MATRIX_BRADFORD: Mat3x3 = [
    0.8951000, 0.2664000, -0.1614000,
    -0.7502000, 1.7135000, 0.0367000,
    0.0389000, -0.0685000, 1.0296000,
];

fn oetf_srgb(value: f64) -> f64 {
    if value <= 0.0031308 {
        12.92 * value
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn xyz_to_rgb(xyz: Xyz, mat: Mat3x3) -> Rgb {
    [
        mat[0] * xyz[0] + mat[1] * xyz[1] + mat[2] * xyz[2],
        mat[3] * xyz[0] + mat[4] * xyz[1] + mat[5] * xyz[2],
        mat[6] * xyz[0] + mat[7] * xyz[1] + mat[8] * xyz[2],
    ]
}

fn matrix_diag(rgb: Rgb) -> Mat3x3 {
    [
        rgb[0], 0.0, 0.0,
        0.0, rgb[1], 0.0,
        0.0, 0.0, rgb[2],
    ]
}

fn matrix_multiply(a: Mat3x3, b: Mat3x3) -> Mat3x3 {
    [
        a[0] * b[0] + a[1] * b[3] + a[2] * b[6],
        a[0] * b[1] + a[1] * b[4] + a[2] * b[7],
        a[0] * b[2] + a[1] * b[5] + a[2] * b[8],
        a[3] * b[0] + a[4] * b[3] + a[5] * b[6],
        a[3] * b[1] + a[4] * b[4] + a[5] * b[7],
        a[3] * b[2] + a[4] * b[5] + a[5] * b[8],
        a[6] * b[0] + a[7] * b[3] + a[8] * b[6],
        a[6] * b[1] + a[7] * b[4] + a[8] * b[7],
        a[6] * b[2] + a[7] * b[5] + a[8] * b[8],
    ]
}

fn matrix_inverse(mat: Mat3x3) -> Mat3x3 {
    let det = mat[0] * (mat[4] * mat[8] - mat[7] * mat[5])
            - mat[1] * (mat[3] * mat[8] - mat[5] * mat[6])
            + mat[2] * (mat[3] * mat[7] - mat[4] * mat[6]);

    if det.abs() < 0.001 {
        return mat
    }

    let det_inv = 1.0 / det;

    [
        det_inv * (mat[4] * mat[8] - mat[7] * mat[5]),
        det_inv * (mat[2] * mat[7] - mat[1] * mat[8]),
        det_inv * (mat[1] * mat[5] - mat[2] * mat[4]),
        det_inv * (mat[5] * mat[6] - mat[3] * mat[8]),
        det_inv * (mat[0] * mat[8] - mat[2] * mat[6]),
        det_inv * (mat[3] * mat[2] - mat[0] * mat[5]),
        det_inv * (mat[3] * mat[7] - mat[6] * mat[4]),
        det_inv * (mat[6] * mat[1] - mat[0] * mat[7]),
        det_inv * (mat[0] * mat[4] - mat[3] * mat[1]),
    ]
}

fn matrix_scale(mat: Mat3x3, scale: f64) -> Mat3x3 {
    mat.map(|x| x * scale)
}

/// Make all elements non-negative while preserving greyscale after application.
fn matrix_clamp(mat: Mat3x3) -> Mat3x3 {
    let mat_clamped = mat.map(|x| x.max(0.0));
    let mut mat_out = mat;

    for row in 0..3 {
        let i = row * 3;

        let sum_clamped_post = (mat[i] + mat[i+1] + mat[i+2]).max(0.0);
        let sum_clamped_pre = mat_clamped[i] + mat_clamped[i+1] + mat_clamped[i+2];
        let correction = sum_clamped_post / sum_clamped_pre.max(0.001);

        mat_out[i] = mat_clamped[i] * correction;
        mat_out[i+1] = mat_clamped[i+1] * correction;
        mat_out[i+2] = mat_clamped[i+2] * correction;
    }

    mat_out
}

fn matrix_normalize(mat: Mat3x3) -> Mat3x3 {
    let sum_r = mat[0] + mat[1] + mat[2];
    let sum_g = mat[3] + mat[4] + mat[5];
    let sum_b = mat[6] + mat[7] + mat[8];

    let max_inv = 1.0 / sum_r.max(sum_g).max(sum_b);
    mat.map(|x| x * max_inv)
}

fn matrix_apply_oetf(mat: Mat3x3, oetf: fn(f64) -> f64) -> Mat3x3 {
    let mut mat_out = mat;

    for row in 0..3 {
        let i = row * 3;

        let sum = mat[i] + mat[i+1] + mat[i+2];
        let correction = oetf(sum) / sum.max(0.001);

        mat_out[i] = mat[i] * correction;
        mat_out[i+1] = mat[i+1] * correction;
        mat_out[i+2] = mat[i+2] * correction;
    }

    mat_out
}

/// Normalize RGB so the maximum component is 1.0
fn rgb_normalize(rgb: Rgb) -> Rgb {
    let max = rgb[0].max(rgb[1].max(rgb[2]));
    let max_inv = 1.0 / max;
    rgb.map(|x| x * max_inv)
}

fn rgb_scale(rgb: Rgb, scale: f64) -> Rgb {
    rgb.map(|x| x * scale)
}

fn rgb_divide(a: Rgb, b: Rgb) -> Rgb {
    std::array::from_fn(|i| a[i] / b[i])
}

/// Reference: <https://en.wikipedia.org/wiki/Smoothstep>
fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    3.0 * x.powi(2) - 2.0 * x.powi(3)
}

fn kelvin_to_mired(kelvin: f64) -> f64 {
    1000000.0 / kelvin
}

/// Laurent polynomial function going from power of -3 to 3
fn temp_to_chroma_fit_curve(x: f64, c: [f64; 7]) -> f64 {
    (0..6).rev().fold(c[6], |total, i| total * x + c[i]) / (x * x * x)
}

/// Calculate Planckian locus chromaticity coordinates. Valid range: 1000-20000K.
/// Reference: <https://github.com/aeraglyx/locus-pocus>
fn temperature_to_chroma(temp: f64) -> Xy {
    const COEFFS_X: [f64; 7] = [
        4.60243e+08, -1.34958e+06, 1.49958e+03, 2.20742e-02, 1.86755e-05, -7.48912e-10, 1.12218e-14
    ];
    const COEFFS_Y: [f64; 7] = [
        8.19188e-02, -1.32154e+00, 8.63682e+00, -2.95048e+01, 5.67579e+01, -5.42917e+01, 1.98083e+01
    ];

    let chroma_x = temp_to_chroma_fit_curve(temp, COEFFS_X);
    let chroma_y = temp_to_chroma_fit_curve(chroma_x, COEFFS_Y);

    [chroma_x, chroma_y]
}

/// Calculate corrected chromaticity from temperature.
///
/// Offset the locus so it intersects the monitor's whitepoint exactly.
/// That way, when a user sets 6500K on a D65 monitor, they'll get "true white"
/// but more extreme temperatures will blend into the Planckian locus.
fn get_chroma_corrected(temp: u32, temp_at_wp: u32, chroma_at_wp: Xy) -> Xy {
    let temp = temp as f64;
    let temp_at_wp = temp_at_wp as f64;

    let chroma_at_temp = temperature_to_chroma(temp);
    let chroma_at_wp_locus = temperature_to_chroma(temp_at_wp);

    const FALLOFF: f64 = 150.0;
    let mired_diff = (kelvin_to_mired(temp) - kelvin_to_mired(temp_at_wp)).abs();
    let offset_weight = smoothstep(1.0 - mired_diff / FALLOFF);

    [
        chroma_at_temp[0] + offset_weight * (chroma_at_wp[0] - chroma_at_wp_locus[0]),
        chroma_at_temp[1] + offset_weight * (chroma_at_wp[1] - chroma_at_wp_locus[1]),
    ]
}

/// Convert chromaticity coordinates to XYZ.
/// Ignoring overall luminance for performance.
fn chroma_to_xyz(chroma: Xy) -> Xyz {
    let chroma_z = 1.0 - chroma[0] - chroma[1];
    [chroma[0], chroma[1], chroma_z]
}

/// Calculate a matrix that transforms source whitepoint to destination whitepoint
/// in Rec709 gamut using Bradford chromatic adaptation transform.
fn get_adaptation_matrix(src: Xyz, dst: Xyz) -> Mat3x3 {
    let rgb_src = xyz_to_rgb(src, MATRIX_BRADFORD);
    let rgb_dst = xyz_to_rgb(dst, MATRIX_BRADFORD);

    let rgb_ratio = rgb_divide(rgb_dst, rgb_src);
    let diagonal = matrix_diag(rgb_ratio);

    let mat_fwd = matrix_multiply(MATRIX_BRADFORD, MATRIX_REC709_TO_XYZ);
    let mat_inv = matrix_inverse(mat_fwd);

    matrix_multiply(mat_inv, matrix_multiply(diagonal, mat_fwd))
}

/// Calculate CTM for a given color temperature and brightness.
///
/// Based on Bradford chromatic adaptation but clamped to be non-negative, normalized,
/// and corrected with OETF since the CTM is not being applied in linear light.
pub fn state_to_ctm(temp: u32, brightness: f64) -> Mat3x3 {
    let temp_at_wp = TEMPERATURE_D65;
    let chroma_at_wp = CHROMATICITY_D65;

    let chroma = get_chroma_corrected(temp, temp_at_wp, chroma_at_wp);

    let xyz_wp = chroma_to_xyz(chroma_at_wp);
    let xyz = chroma_to_xyz(chroma);

    let mut ctm = get_adaptation_matrix(xyz_wp, xyz);

    // To comply with `hyprland_ctm_control_v1`
    ctm = matrix_clamp(ctm);

    ctm = matrix_normalize(ctm);
    ctm = matrix_scale(ctm, brightness.powf(BRIGHTNESS_POWER));

    // Because CTM is not applied in linear light
    ctm = matrix_apply_oetf(ctm, oetf_srgb);

    ctm
}

/// Calculate RGB values for a given color temperature.
///
/// Accurate from 1000K to 20000K. Approximates Plackian locus XY coordinates, applies
/// a small offset to match the whitepoint exactly and converts XY to normalized RGB.
fn temperature_to_rgb(temp: u32) -> Rgb {
    let temp_at_wp = TEMPERATURE_D65;
    let chroma_at_wp = CHROMATICITY_D65;

    if temp == temp_at_wp {
        return [1.0, 1.0, 1.0];
    }

    let chroma = get_chroma_corrected(temp, temp_at_wp, chroma_at_wp);

    let xyz = chroma_to_xyz(chroma);
    let mut rgb = xyz_to_rgb(xyz, MATRIX_XYZ_TO_REC709);

    rgb = rgb_normalize(rgb);

    rgb
}

/// Calculate RGB values for a given color temperature and brightness.
pub fn state_to_rgb(temp: u32, brightness: f64) -> Rgb {
    let mut rgb = temperature_to_rgb(temp);

    rgb = rgb_scale(rgb, brightness.powf(BRIGHTNESS_POWER));

    // Because RGB is not applied in linear light
    rgb = rgb.map(|x| oetf_srgb(x));

    rgb
}

/// RGB factors rounded to 3 decimal places, for debug-logging display only.
pub fn get_rgb_factors(temperature: u32) -> Rgb {
    let rgb = temperature_to_rgb(temperature);
    rgb.map(|x| (x * 1000.0).round() / 1000.0)
}

/// Generate a gamma lookup table for one color channel.
///
/// Applies `output = input * color_factor`, where `input` is normalized 0.0-1.0
/// and `color_factor` (0.0-1.0) adjusts for color temperature and brightness.
/// Output is scaled to 0-65535 for the 16-bit protocol.
pub fn generate_gamma_table(size: usize, color_factor: f64) -> Vec<u16> {
    let mut table = Vec::with_capacity(size);

    for i in 0..size {
        let val = i as f64 / (size - 1) as f64;

        let output = (val * color_factor * 65535.0).clamp(0.0, 65535.0);

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
    brightness: f64,
    debug_enabled: bool,
) -> Result<Vec<u8>> {
    let [r, g, b] = state_to_rgb(temperature, brightness);

    let red_table = generate_gamma_table(size, r);
    let green_table = generate_gamma_table(size, g);
    let blue_table = generate_gamma_table(size, b);

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
        let [r, g, b] = state_to_rgb(6500, 1.0);
        assert!((r - 1.0).abs() < 0.01);
        assert!((g - 1.0).abs() < 0.01);
        assert!((b - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_temperature_to_rgb_warm() {
        let [r, g, b] = state_to_rgb(3300, 1.0);
        assert!(r > g);
        assert!(g > b);
        assert!(b < 0.8);
    }

    #[test]
    fn test_temperature_to_rgb_cool() {
        let [r, g, b] = state_to_rgb(8000, 1.0);
        assert!(b > g);
        assert!(r < b);
    }

    #[test]
    fn test_temperature_to_rgb_very_warm() {
        let [r, g, b] = state_to_rgb(2000, 1.0);
        assert!(r > g);
        assert!(g > b);
        assert!(b < 0.1);
    }

    #[test]
    fn test_gamma_table_generation() {
        let table = generate_gamma_table(256, 1.0);
        assert_eq!(table.len(), 256);
        assert_eq!(table[0], 0);
        assert_eq!(table[255], 65535);

        for i in 1..table.len() {
            assert!(table[i] >= table[i - 1]);
        }
    }

    #[test]
    fn test_gamma_table_with_color_factor() {
        let full_table = generate_gamma_table(256, 1.0);
        let half_table = generate_gamma_table(256, 0.5);

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
        let [r1, g1, b1] = state_to_rgb(2000, 1.0);
        let [r2, g2, b2] = state_to_rgb(2001, 1.0);

        // f64 precision: 1K apart must not collapse to the same RGB
        assert!(r1 != r2 || g1 != g2 || b1 != b2);
    }
}
