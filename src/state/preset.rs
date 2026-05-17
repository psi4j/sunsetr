//! State management for sunsetr, following XDG Base Directory standards.
//!
//! This module handles persistent state storage (like active presets) in the
//! XDG_STATE_HOME directory, keeping configuration and state properly separated.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config::get_custom_config_dir;

/// Get the state directory for a given configuration directory.
///
/// State is stored in XDG_STATE_HOME/sunsetr/{namespace} where namespace is:
/// - "default" for the default config directory
/// - "custom_<hash>" for custom config directories (via --config)
pub fn get_state_dir(config_dir: Option<&Path>) -> Result<PathBuf> {
    let state_home = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".local/state")
        });

    let state_base = state_home.join("sunsetr");

    let namespace = match config_dir {
        None => "default".to_string(),
        Some(path) => {
            let default_config = dirs::config_dir()
                .context("Could not determine config directory")?
                .join("sunsetr");
            if path == default_config {
                "default".to_string()
            } else {
                get_state_namespace(path)
            }
        }
    };

    Ok(state_base.join(namespace))
}

/// Generate a stable namespace for a custom config directory.
///
/// Uses a SHA256 hash of the canonical path, truncated to 16 chars.
fn get_state_namespace(config_path: &Path) -> String {
    let canonical = config_path
        .canonicalize()
        .unwrap_or_else(|_| config_path.to_path_buf());

    let hash = sha256::digest(canonical.to_string_lossy().as_bytes());
    format!("custom_{}", &hash[..16])
}

/// Get the currently active preset name, if any.
pub fn get_active_preset() -> Result<Option<String>> {
    let identity_valid = check_directory_identity()?;

    if !identity_valid {
        return Ok(None);
    }

    let config_dir = get_custom_config_dir();
    let state_dir = get_state_dir(config_dir.as_deref())?;
    let marker_path = state_dir.join("active_preset");

    if marker_path.exists() {
        match fs::read_to_string(&marker_path) {
            Ok(content) => {
                let preset_name = content.trim().to_string();
                if preset_name.is_empty() {
                    let _ = fs::remove_file(&marker_path);
                    Ok(None)
                } else {
                    let exists = validate_preset_exists(&preset_name)?;

                    if exists {
                        Ok(Some(preset_name))
                    } else {
                        log_warning!(
                            "Active preset '{}' not found, falling back to default config",
                            preset_name
                        );
                        let _ = fs::remove_file(&marker_path);
                        let _ = fs::remove_file(state_dir.join("dir_id"));
                        Ok(None)
                    }
                }
            }
            Err(_) => Ok(None),
        }
    } else {
        Ok(None)
    }
}

/// Clear the active preset marker file.
pub fn clear_active_preset() -> Result<()> {
    let config_dir = get_custom_config_dir();
    let state_dir = get_state_dir(config_dir.as_deref())?;
    let marker_path = state_dir.join("active_preset");

    let _ = fs::remove_file(&marker_path);
    let _ = fs::remove_file(state_dir.join("dir_id"));
    Ok(())
}

/// Write the active preset name to the state file.
///
/// Writes the directory identity before the preset marker so the config
/// watcher never observes an updated `active_preset` alongside a stale or
/// missing `dir_id`.
pub fn set_active_preset(preset_name: &str) -> Result<()> {
    let config_dir = get_custom_config_dir();
    let state_dir = get_state_dir(config_dir.as_deref())?;

    fs::create_dir_all(&state_dir)?;

    write_directory_identity(&state_dir, config_dir.as_deref())?;

    let marker_path = state_dir.join("active_preset");
    fs::write(&marker_path, preset_name)
        .with_context(|| format!("Failed to write preset marker to {}", marker_path.display()))?;

    Ok(())
}

/// Check if a preset exists in the config directory.
fn validate_preset_exists(preset_name: &str) -> Result<bool> {
    let config_path = match crate::config::Config::get_config_path() {
        Ok(path) => path,
        Err(_) => return Ok(false),
    };

    let config_dir = config_path
        .parent()
        .context("Failed to get config directory")?;

    let preset_path = config_dir
        .join("presets")
        .join(preset_name)
        .join("sunsetr.toml");

    Ok(preset_path.exists())
}

/// Write the config directory identity used to detect directory recreation.
///
/// Records the config directory inode (not mtime, which changes whenever a
/// file inside is edited) to `dir_id` in the state directory.
fn write_directory_identity(state_dir: &Path, config_dir: Option<&Path>) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    let config_path = match config_dir {
        Some(path) => path.to_path_buf(),
        None => dirs::config_dir()
            .context("Could not determine config directory")?
            .join("sunsetr"),
    };

    let metadata = fs::metadata(&config_path).with_context(|| {
        format!(
            "Failed to read config directory metadata: {}",
            config_path.display()
        )
    })?;

    let dir_id = format!("{}", metadata.ino());
    fs::write(state_dir.join("dir_id"), dir_id)
        .context("Failed to write directory identity file")?;

    Ok(())
}

/// Check the stored directory identity to detect config directory recreation.
///
/// Returns `false` (after clearing stale state) when the config directory is
/// missing or its inode no longer matches the recorded `dir_id`; an absent
/// `dir_id` is treated as valid.
fn check_directory_identity() -> Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let config_dir = get_custom_config_dir();
    let state_dir = get_state_dir(config_dir.as_deref())?;
    let dir_id_file = state_dir.join("dir_id");

    if !dir_id_file.exists() {
        return Ok(true);
    }

    let config_path = match config_dir {
        Some(ref path) => path.clone(),
        None => dirs::config_dir()
            .context("Could not determine config directory")?
            .join("sunsetr"),
    };

    let stored_id = fs::read_to_string(&dir_id_file)?;

    let metadata = match fs::metadata(&config_path) {
        Ok(meta) => meta,
        Err(_) => {
            let _ = fs::remove_file(state_dir.join("active_preset"));
            let _ = fs::remove_file(&dir_id_file);
            return Ok(false);
        }
    };

    let current_id = format!("{}", metadata.ino());

    if stored_id.trim() != current_id {
        let _ = fs::remove_file(state_dir.join("active_preset"));
        let _ = fs::remove_file(&dir_id_file);
        return Ok(false);
    }

    Ok(true)
}

/// Get the state watch path for the config watcher.
pub fn get_state_watch_path() -> Result<PathBuf> {
    let config_dir = get_custom_config_dir();
    let state_dir = get_state_dir(config_dir.as_deref())?;

    let _ = fs::create_dir_all(&state_dir);

    Ok(state_dir)
}

/// Clean up orphaned state directories that have not been modified in 90 days.
pub fn cleanup_orphaned_state_dirs() -> Result<()> {
    let state_home = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".local/state")
        });

    let sunsetr_state = state_home.join("sunsetr");
    if !sunsetr_state.exists() {
        return Ok(());
    }

    let ninety_days_ago = SystemTime::now()
        .checked_sub(Duration::from_secs(90 * 24 * 60 * 60))
        .unwrap_or(UNIX_EPOCH);

    if let Ok(entries) = fs::read_dir(&sunsetr_state) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            if let Ok(metadata) = entry.metadata()
                && let Ok(modified) = metadata.modified()
                && modified < ninety_days_ago
            {
                let _ = fs::remove_dir_all(&path);
            }
        }
    }

    Ok(())
}
