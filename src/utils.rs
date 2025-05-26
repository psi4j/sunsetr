//! Utility functions shared across the codebase.
//!
//! This module provides common functionality for interpolation, version handling,
//! and other helper operations used throughout the application.

/// Interpolate between two u32 values based on progress (0.0 to 1.0).
/// 
/// This function provides smooth transitions between integer values, commonly
/// used for color temperature transitions during sunrise/sunset.
/// 
/// # Arguments
/// * `start` - Starting value (returned when progress = 0.0)
/// * `end` - Ending value (returned when progress = 1.0)
/// * `progress` - Interpolation progress, automatically clamped to [0.0, 1.0]
/// 
/// # Returns
/// Interpolated value rounded to the nearest integer
/// 
/// # Examples
/// ```
/// use sunsetr::utils::interpolate_u32;
/// assert_eq!(interpolate_u32(1000, 2000, 0.5), 1500);
/// assert_eq!(interpolate_u32(6000, 3000, 0.25), 5250);
/// ```
pub fn interpolate_u32(start: u32, end: u32, progress: f32) -> u32 {
    let start_f = start as f32;
    let end_f = end as f32;
    let result = start_f + (end_f - start_f) * progress.clamp(0.0, 1.0);
    result.round() as u32
}

/// Interpolate between two f32 values based on progress (0.0 to 1.0).
/// 
/// This function provides smooth transitions between floating-point values,
/// commonly used for gamma/brightness transitions during sunrise/sunset.
/// 
/// # Arguments
/// * `start` - Starting value (returned when progress = 0.0)
/// * `end` - Ending value (returned when progress = 1.0)
/// * `progress` - Interpolation progress, automatically clamped to [0.0, 1.0]
/// 
/// # Returns
/// Interpolated floating-point value
/// 
/// # Examples
/// ```
/// use sunsetr::utils::interpolate_f32;
/// assert_eq!(interpolate_f32(90.0, 100.0, 0.5), 95.0);
/// assert_eq!(interpolate_f32(100.0, 90.0, 0.3), 97.0);
/// ```
pub fn interpolate_f32(start: f32, end: f32, progress: f32) -> f32 {
    start + (end - start) * progress.clamp(0.0, 1.0)
}

/// Simple semantic version comparison for version strings.
/// 
/// Compares version strings in the format "vX.Y.Z" or "X.Y.Z" using
/// semantic versioning rules. Handles the optional 'v' prefix automatically.
/// 
/// # Arguments
/// * `version1` - First version string to compare
/// * `version2` - Second version string to compare
/// 
/// # Returns
/// - `Ordering::Less` if version1 < version2
/// - `Ordering::Equal` if version1 == version2  
/// - `Ordering::Greater` if version1 > version2
/// 
/// # Examples
/// ```
/// use std::cmp::Ordering;
/// use sunsetr::utils::compare_versions;
/// assert_eq!(compare_versions("v1.0.0", "v2.0.0"), Ordering::Less);
/// assert_eq!(compare_versions("2.1.0", "v2.0.0"), Ordering::Greater);
/// ```
pub fn compare_versions(version1: &str, version2: &str) -> std::cmp::Ordering {
    let parse_version = |v: &str| -> Vec<u32> {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect()
    };
    
    let v1 = parse_version(version1);
    let v2 = parse_version(version2);
    
    v1.cmp(&v2)
}

/// Extract semantic version string from hyprsunset command output.
/// 
/// Parses hyprsunset output to find version information in various formats.
/// Handles both "vX.Y.Z" and "X.Y.Z" patterns and normalizes to "vX.Y.Z" format.
/// 
/// # Arguments
/// * `output` - Raw output text from hyprsunset command
/// 
/// # Returns
/// - `Some(String)` containing normalized version (e.g., "v2.0.0")
/// - `None` if no valid semantic version found
/// 
/// # Examples
/// ```
/// use sunsetr::utils::extract_version_from_output;
/// assert_eq!(extract_version_from_output("hyprsunset v2.0.0"), Some("v2.0.0".to_string()));
/// assert_eq!(extract_version_from_output("version: 1.5.2"), Some("v1.5.2".to_string()));
/// ```
pub fn extract_version_from_output(output: &str) -> Option<String> {
    for line in output.lines() {
        let line = line.trim();
        // Look for version pattern: vX.Y.Z or X.Y.Z
        if let Some(version) = extract_semver_from_line(line) {
            return Some(version);
        }
    }
    None
}

/// Extract semantic version from a single line of text using regex.
/// 
/// Internal helper function that uses regex to find and normalize semantic versions.
/// 
/// # Arguments
/// * `line` - Single line of text to search
/// 
/// # Returns
/// - `Some(String)` with normalized version if found
/// - `None` if no semantic version pattern found
fn extract_semver_from_line(line: &str) -> Option<String> {
    use regex::Regex;
    let re = Regex::new(r"v?(\d+\.\d+\.\d+)").ok()?;
    if let Some(captures) = re.captures(line) {
        let full_match = captures.get(0)?.as_str();
        if full_match.starts_with('v') {
            Some(full_match.to_string())
        } else {
            Some(format!("v{}", captures.get(1)?.as_str()))
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn test_interpolate_u32_basic() {
        assert_eq!(interpolate_u32(1000, 2000, 0.0), 1000);
        assert_eq!(interpolate_u32(1000, 2000, 1.0), 2000);
        assert_eq!(interpolate_u32(1000, 2000, 0.5), 1500);
    }

    #[test]
    fn test_interpolate_u32_extreme_values() {
        // Test with extreme temperature values
        assert_eq!(interpolate_u32(1000, 20000, 0.0), 1000);
        assert_eq!(interpolate_u32(1000, 20000, 1.0), 20000);
        assert_eq!(interpolate_u32(1000, 20000, 0.5), 10500);
        
        // Test with same values
        assert_eq!(interpolate_u32(5000, 5000, 0.5), 5000);
        
        // Test with reversed order
        assert_eq!(interpolate_u32(6000, 3000, 0.0), 6000);
        assert_eq!(interpolate_u32(6000, 3000, 1.0), 3000);
        assert_eq!(interpolate_u32(6000, 3000, 0.5), 4500);
    }

    #[test]
    fn test_interpolate_u32_clamping() {
        // Progress values outside 0.0-1.0 should be clamped
        assert_eq!(interpolate_u32(1000, 2000, -0.5), 1000);
        assert_eq!(interpolate_u32(1000, 2000, 1.5), 2000);
        assert_eq!(interpolate_u32(1000, 2000, -100.0), 1000);
        assert_eq!(interpolate_u32(1000, 2000, 100.0), 2000);
    }

    #[test]
    fn test_interpolate_f32_basic() {
        assert_eq!(interpolate_f32(0.0, 100.0, 0.0), 0.0);
        assert_eq!(interpolate_f32(0.0, 100.0, 1.0), 100.0);
        assert_eq!(interpolate_f32(0.0, 100.0, 0.5), 50.0);
    }

    #[test]
    fn test_interpolate_f32_gamma_range() {
        // Test with typical gamma range
        assert_eq!(interpolate_f32(90.0, 100.0, 0.0), 90.0);
        assert_eq!(interpolate_f32(90.0, 100.0, 1.0), 100.0);
        assert_eq!(interpolate_f32(90.0, 100.0, 0.5), 95.0);
        
        // Test precision
        let result = interpolate_f32(90.0, 100.0, 0.3);
        assert!((result - 93.0).abs() < 0.001);
    }

    #[test]
    fn test_interpolate_f32_clamping() {
        assert_eq!(interpolate_f32(0.0, 100.0, -0.5), 0.0);
        assert_eq!(interpolate_f32(0.0, 100.0, 1.5), 100.0);
    }

    #[test]
    fn test_compare_versions_basic() {
        assert_eq!(compare_versions("v1.0.0", "v1.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("v1.0.0", "v2.0.0"), Ordering::Less);
        assert_eq!(compare_versions("v2.0.0", "v1.0.0"), Ordering::Greater);
    }

    #[test]
    fn test_compare_versions_without_v_prefix() {
        assert_eq!(compare_versions("1.0.0", "2.0.0"), Ordering::Less);
        assert_eq!(compare_versions("2.0.0", "1.0.0"), Ordering::Greater);
        assert_eq!(compare_versions("1.5.0", "1.5.0"), Ordering::Equal);
    }

    #[test]
    fn test_compare_versions_mixed_prefix() {
        assert_eq!(compare_versions("v1.0.0", "2.0.0"), Ordering::Less);
        assert_eq!(compare_versions("1.0.0", "v2.0.0"), Ordering::Less);
    }

    #[test]
    fn test_compare_versions_patch_levels() {
        assert_eq!(compare_versions("v1.0.0", "v1.0.1"), Ordering::Less);
        assert_eq!(compare_versions("v1.0.5", "v1.0.1"), Ordering::Greater);
        assert_eq!(compare_versions("v1.2.0", "v1.1.9"), Ordering::Greater);
    }

    #[test]
    fn test_extract_version_from_output_hyprsunset_format() {
        let output = "hyprsunset v2.0.0";
        assert_eq!(extract_version_from_output(output), Some("v2.0.0".to_string()));
        
        let output = "hyprsunset 2.0.0";
        assert_eq!(extract_version_from_output(output), Some("v2.0.0".to_string()));
    }

    #[test]
    fn test_extract_version_from_output_multiline() {
        let output = "hyprsunset - some description\nversion: v1.5.2\nother info";
        assert_eq!(extract_version_from_output(output), Some("v1.5.2".to_string()));
    }

    #[test]
    fn test_extract_version_from_output_no_version() {
        let output = "hyprsunset - no version info here";
        assert_eq!(extract_version_from_output(output), None);
        
        let output = "";
        assert_eq!(extract_version_from_output(output), None);
    }

    #[test]
    fn test_extract_version_from_output_malformed() {
        let output = "version 1.0"; // Missing patch version
        assert_eq!(extract_version_from_output(output), None);
        
        let output = "v1.0.0.0"; // Too many components
        assert_eq!(extract_version_from_output(output), Some("v1.0.0".to_string()));
    }

    // Property-based tests using proptest
    #[cfg(feature = "proptest")]
    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn interpolate_u32_bounds(start in 0u32..20000, end in 0u32..20000, progress in 0.0f32..1.0) {
                let result = interpolate_u32(start, end, progress);
                let min_val = start.min(end);
                let max_val = start.max(end);
                prop_assert!(result >= min_val && result <= max_val);
            }

            #[test]
            fn interpolate_f32_bounds(start in 0.0f32..100.0, end in 0.0f32..100.0, progress in 0.0f32..1.0) {
                let result = interpolate_f32(start, end, progress);
                let min_val = start.min(end);
                let max_val = start.max(end);
                prop_assert!(result >= min_val && result <= max_val);
            }

            #[test]
            fn interpolate_u32_endpoints(start in 0u32..20000, end in 0u32..20000) {
                prop_assert_eq!(interpolate_u32(start, end, 0.0), start);
                prop_assert_eq!(interpolate_u32(start, end, 1.0), end);
            }
        }
    }
} 