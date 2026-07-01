//! Command-line command handlers for sunsetr.
//!
//! One-shot CLI command implementations, one submodule per command.

pub mod geo;
pub mod get;
pub mod help;
pub mod preset;
pub mod restart;
pub mod set;
pub mod status;
pub mod stop;
pub mod test;

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// A named preset could not be found.
#[derive(Debug)]
pub(crate) struct PresetNotFoundError {
    pub preset_name: String,
    pub available_presets: Vec<String>,
    pub expected_path: PathBuf,
}

impl std::fmt::Display for PresetNotFoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Preset '{}' not found", self.preset_name)
    }
}

impl std::error::Error for PresetNotFoundError {}

/// Resolve the config file path for a target: the active config (preset or default) when
/// `None`, the base config for `Some("default")`, or a named preset otherwise. Errors with
/// the available presets when a named preset is missing.
pub(crate) fn resolve_target_config_path(target: Option<&str>) -> Result<PathBuf> {
    let base_config_path = crate::config::Config::get_config_path()?;
    let config_dir = base_config_path
        .parent()
        .context("Failed to get config directory")?;

    match target {
        None => {
            if let Some(preset_name) = crate::state::preset::get_active_preset()? {
                Ok(config_dir
                    .join("presets")
                    .join(&preset_name)
                    .join("sunsetr.toml"))
            } else {
                Ok(base_config_path)
            }
        }
        Some("default") => Ok(base_config_path),
        Some(preset_name) => {
            preset::validate_preset_name(preset_name)?;

            let preset_path = config_dir
                .join("presets")
                .join(preset_name)
                .join("sunsetr.toml");

            if !preset_path.exists() {
                let available_presets = list_available_presets(config_dir)?;

                return Err(PresetNotFoundError {
                    preset_name: preset_name.to_string(),
                    available_presets,
                    expected_path: preset_path,
                }
                .into());
            }

            Ok(preset_path)
        }
    }
}

/// List all available preset names in the configuration directory.
///
/// Returns a vector with "default" as the first entry, followed by alphabetically
/// sorted preset names found in the presets directory.
/// A preset is considered valid if it contains a sunsetr.toml file.
pub(crate) fn list_available_presets(config_dir: &Path) -> Result<Vec<String>> {
    let presets_dir = config_dir.join("presets");
    let mut available_presets = Vec::new();

    if presets_dir.exists()
        && let Ok(entries) = fs::read_dir(&presets_dir)
    {
        for entry in entries.flatten() {
            if entry.path().is_dir()
                && let Some(name) = entry.file_name().to_str()
                && entry.path().join("sunsetr.toml").exists()
            {
                available_presets.push(name.to_string());
            }
        }
    }

    available_presets.sort();
    available_presets.insert(0, "default".to_string());
    Ok(available_presets)
}

fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();
    let len1 = s1_chars.len();
    let len2 = s2_chars.len();

    if len1 == 0 {
        return len2;
    }
    if len2 == 0 {
        return len1;
    }

    let mut matrix = vec![vec![0; len2 + 1]; len1 + 1];

    (0..=len1).for_each(|i| {
        matrix[i][0] = i;
    });
    (0..=len2).for_each(|j| {
        matrix[0][j] = j;
    });

    for i in 1..=len1 {
        for j in 1..=len2 {
            let cost = if s1_chars[i - 1] == s2_chars[j - 1] {
                0
            } else {
                1
            };
            matrix[i][j] = std::cmp::min(
                std::cmp::min(matrix[i - 1][j] + 1, matrix[i][j - 1] + 1),
                matrix[i - 1][j - 1] + cost,
            );
        }
    }

    matrix[len1][len2]
}

pub(crate) fn find_similar_presets(
    target: &str,
    available: &[String],
    max_count: usize,
) -> Vec<String> {
    if available.is_empty() {
        return Vec::new();
    }

    let target_lower = target.to_lowercase();
    let mut scored_presets: Vec<(String, usize)> = available
        .iter()
        .map(|preset| {
            let preset_lower = preset.to_lowercase();
            let distance = levenshtein_distance(&target_lower, &preset_lower);
            (preset.clone(), distance)
        })
        .collect();

    scored_presets.sort_by_key(|&(_, dist)| dist);

    scored_presets
        .into_iter()
        .take(max_count)
        .map(|(name, _)| name)
        .collect()
}

pub(crate) fn handle_preset_not_found_error(error: &PresetNotFoundError) -> ! {
    log_pipe!();
    log_error!("{} at:", error);
    log_indented!(
        "{}",
        crate::common::utils::private_path(&error.expected_path)
    );

    if error.available_presets.is_empty() {
        log_block_start!("No presets are configured");
        log_indented!("Create a preset directory and config file first:");
        log_indented!("mkdir -p ~/.config/sunsetr/presets/{}", error.preset_name);
        log_indented!(
            "cp ~/.config/sunsetr/sunsetr.toml ~/.config/sunsetr/presets/{}/sunsetr.toml",
            error.preset_name
        );
    } else {
        let similar = find_similar_presets(&error.preset_name, &error.available_presets, 1);

        if let Some(closest) = similar.first() {
            log_block_start!("Did you mean '{}'?", closest);
        }

        log_block_start!("Use `sunsetr preset list` to see all available presets");
    }

    log_end!();
    std::process::exit(1);
}
