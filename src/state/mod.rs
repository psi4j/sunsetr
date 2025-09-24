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

    // Determine namespace based on config directory
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
fn get_state_namespace(config_path: &Path) -> String {
    let canonical = config_path
        .canonicalize()
        .unwrap_or_else(|_| config_path.to_path_buf());

    // Use SHA256 hash truncated to 16 chars for stability and uniqueness
    let hash = sha256::digest(canonical.to_string_lossy().as_bytes());
    format!("custom_{}", &hash[..16])
}

/// Get the currently active preset name, if any.
/// This is a direct migration of the existing get_active_preset() function.
pub fn get_active_preset() -> Result<Option<String>> {
    // Transparently migrate legacy state if needed
    migrate_legacy_state()?;

    // Check directory identity first (detect config dir recreation)
    // Skip this check immediately after migration to avoid issues
    // The migration will have just created the dir_id file
    let identity_valid = check_directory_identity()?;

    if !identity_valid {
        // Directory was recreated, state was already cleaned up
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
                    // Empty file, clean it up
                    let _ = fs::remove_file(&marker_path);
                    Ok(None)
                } else {
                    // Validate the preset still exists
                    let exists = validate_preset_exists(&preset_name)?;

                    if exists {
                        Ok(Some(preset_name))
                    } else {
                        // Stale state - preset no longer exists
                        log_warning!(
                            "Active preset '{}' not found, falling back to default config",
                            preset_name
                        );
                        let _ = fs::remove_file(&marker_path);
                        // Also clean up dir_id file if it exists
                        let _ = fs::remove_file(state_dir.join("dir_id"));
                        Ok(None)
                    }
                }
            }
            Err(_) => {
                // Can't read file, treat as no preset
                Ok(None)
            }
        }
    } else {
        Ok(None)
    }
}

/// Clear the active preset marker file.
/// This is a direct migration of the existing clear_active_preset() function.
pub fn clear_active_preset() -> Result<()> {
    let config_dir = get_custom_config_dir();
    let state_dir = get_state_dir(config_dir.as_deref())?;
    let marker_path = state_dir.join("active_preset");

    // Remove the marker file (ignore errors if file doesn't exist)
    let _ = fs::remove_file(&marker_path);
    // Also clean up dir_id file if it exists
    let _ = fs::remove_file(state_dir.join("dir_id"));
    Ok(())
}

/// Write the active preset name to the state file.
/// This replaces direct file writes in commands/preset.rs.
pub fn set_active_preset(preset_name: &str) -> Result<()> {
    let config_dir = get_custom_config_dir();
    let state_dir = get_state_dir(config_dir.as_deref())?;

    // Create state directory if it doesn't exist
    fs::create_dir_all(&state_dir)?;

    let marker_path = state_dir.join("active_preset");
    fs::write(&marker_path, preset_name)
        .with_context(|| format!("Failed to write preset marker to {}", marker_path.display()))?;

    // Also write directory identity for validation
    write_directory_identity(&state_dir, config_dir.as_deref())?;

    Ok(())
}

/// Check if a preset exists in the config directory.
fn validate_preset_exists(preset_name: &str) -> Result<bool> {
    // Try to get the config path - if it fails, the preset can't exist
    let config_path = match crate::config::Config::get_config_path() {
        Ok(path) => path,
        Err(_) => return Ok(false), // Config doesn't exist, so preset doesn't exist
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

/// Write directory identity information for validation.
fn write_directory_identity(state_dir: &Path, config_dir: Option<&Path>) -> Result<()> {
    // Get the config directory path
    let config_path = match config_dir {
        Some(path) => path.to_path_buf(),
        None => dirs::config_dir()
            .context("Could not determine config directory")?
            .join("sunsetr"),
    };

    // Write directory identity (inode only - mtime changes when files are edited)
    if let Ok(metadata) = fs::metadata(&config_path) {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::MetadataExt;
            // Only use inode, not mtime, because mtime changes when files inside are edited
            let dir_id = format!("{}", metadata.ino());
            let _ = fs::write(state_dir.join("dir_id"), dir_id);
        }
    }

    Ok(())
}

/// Check directory identity to detect config directory recreation.
fn check_directory_identity() -> Result<bool> {
    let config_dir = get_custom_config_dir();
    let state_dir = get_state_dir(config_dir.as_deref())?;
    let dir_id_file = state_dir.join("dir_id");

    if !dir_id_file.exists() {
        return Ok(true); // No identity file, assume valid
    }

    // Get the config directory path
    let config_path = match config_dir {
        Some(ref path) => path.clone(),
        None => dirs::config_dir()
            .context("Could not determine config directory")?
            .join("sunsetr"),
    };

    let stored_id = fs::read_to_string(&dir_id_file)?;

    // Try to get metadata - if the config directory doesn't exist yet, clean up state
    let metadata = match fs::metadata(&config_path) {
        Ok(meta) => meta,
        Err(_) => {
            // Config directory doesn't exist, clean up stale state
            let _ = fs::remove_file(state_dir.join("active_preset"));
            let _ = fs::remove_file(&dir_id_file);
            return Ok(false);
        }
    };

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        // Only check inode, not mtime, because mtime changes when files inside are edited
        let current_id = format!("{}", metadata.ino());

        if stored_id.trim() != current_id {
            // Directory was recreated (different inode) - clean up state
            let _ = fs::remove_file(state_dir.join("active_preset"));
            let _ = fs::remove_file(&dir_id_file);
            return Ok(false);
        }
    }

    Ok(true)
}

/// Migrate legacy state from config directory to XDG_STATE_HOME.
fn migrate_legacy_state() -> Result<()> {
    // Try to get the config directory - if it fails (e.g., config doesn't exist yet),
    // just return Ok since there's nothing to migrate
    let config_path = match crate::config::Config::get_config_path() {
        Ok(path) => path,
        Err(_) => return Ok(()), // Config doesn't exist yet, nothing to migrate
    };

    let config_dir = config_path
        .parent()
        .context("Failed to get config directory")?;
    let legacy_file = config_dir.join(".active_preset");

    if legacy_file.exists() {
        // Read the legacy state
        if let Ok(preset_name) = fs::read_to_string(&legacy_file) {
            let preset_name = preset_name.trim();
            if !preset_name.is_empty() {
                // Directly write to new location without validation
                // (we're just migrating existing state, not applying a new preset)
                let config_dir = get_custom_config_dir();
                let state_dir = get_state_dir(config_dir.as_deref())?;

                // Create state directory if it doesn't exist
                if fs::create_dir_all(&state_dir).is_err() {
                    return Ok(());
                }

                // Write the preset name directly
                let marker_path = state_dir.join("active_preset");

                // Remove legacy file FIRST before capturing directory identity
                // This is important: deleting the file changes the directory mtime
                let _ = fs::remove_file(&legacy_file);

                if fs::write(&marker_path, preset_name).is_ok() {
                    // Now write directory identity after the directory has stabilized
                    let _ = write_directory_identity(&state_dir, config_dir.as_deref());
                }
            }
        } else {
            // Empty preset name, just remove the legacy file
            let _ = fs::remove_file(&legacy_file);
        }
    }

    Ok(())
}

/// Get the state watch path for the config watcher.
pub fn get_state_watch_path() -> Result<PathBuf> {
    let config_dir = get_custom_config_dir();
    let state_dir = get_state_dir(config_dir.as_deref())?;

    // Create state directory if it doesn't exist (for watching)
    let _ = fs::create_dir_all(&state_dir);

    Ok(state_dir)
}

/// Clean up orphaned state directories that haven't been accessed in 90 days.
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

    // Iterate through all state subdirectories
    if let Ok(entries) = fs::read_dir(&sunsetr_state) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            // Check directory modification time
            if let Ok(metadata) = entry.metadata()
                && let Ok(modified) = metadata.modified()
                && modified < ninety_days_ago
            {
                // Directory hasn't been touched in 90 days, remove it
                let _ = fs::remove_dir_all(&path);
            }
        }
    }

    Ok(())
}
