//! File watching module for hot config reloading.
//!
//! This module provides automatic configuration file monitoring and reloading
//! functionality, allowing sunsetr to detect and apply configuration changes
//! in real-time without requiring manual reload signals.

use crate::common::utils::private_path;
use anyhow::{Context, Result};
use notify::{
    Config as NotifyConfig, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use super::Config;
use crate::io::signals::SignalMessage;

const DEBOUNCE_MS: u64 = 0;

/// Configuration file watcher that monitors for changes and triggers reloads.
pub struct ConfigWatcher {
    signal_sender: Sender<SignalMessage>,
    debug_enabled: bool,
    watched_paths: Vec<PathBuf>,
}

impl ConfigWatcher {
    pub fn new(signal_sender: Sender<SignalMessage>, debug_enabled: bool) -> Self {
        Self {
            signal_sender,
            debug_enabled,
            watched_paths: Vec::new(),
        }
    }

    /// Start watching configuration files for changes.
    ///
    /// This spawns a background thread that monitors the configuration files
    /// and sends reload signals when changes are detected.
    pub fn start(mut self) -> Result<()> {
        let paths_to_watch = self.determine_watch_paths()?;

        if paths_to_watch.is_empty() {
            if self.debug_enabled {
                log_pipe!();
                log_debug!("No configuration files found to watch for hot reload");
            }
            return Ok(());
        }

        self.watched_paths = paths_to_watch.clone();

        if self.debug_enabled {
            log_pipe!();
            log_debug!("Starting config file watcher for hot reload:");
            for path in &self.watched_paths {
                let display_path = private_path(path);
                log_indented!("Watching: {}", display_path);
            }
        }

        let (tx, rx) = std::sync::mpsc::channel();

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                            let _ = tx.send(event);
                        }
                        _ => {}
                    }
                }
            },
            NotifyConfig::default(),
        )
        .context("Failed to create file watcher")?;

        let mut watched_dirs = std::collections::HashSet::new();

        for path in &paths_to_watch {
            // Special handling for state files - watch them directly
            // This ensures we get deletion events immediately
            if (path.ends_with("active_preset") || path.ends_with("dir_id")) && path.is_file() {
                watcher
                    .watch(path, RecursiveMode::NonRecursive)
                    .with_context(|| format!("Failed to watch state file: {}", path.display()))?;
            } else if path.is_file() {
                if let Some(parent) = path.parent()
                    && watched_dirs.insert(parent.to_path_buf())
                {
                    watcher
                        .watch(parent, RecursiveMode::NonRecursive)
                        .with_context(|| {
                            format!("Failed to watch directory: {}", parent.display())
                        })?;
                }
            } else if watched_dirs.insert(path.clone()) {
                watcher
                    .watch(path, RecursiveMode::Recursive)
                    .with_context(|| format!("Failed to watch path: {}", path.display()))?;
            }
        }

        let signal_sender = self.signal_sender.clone();
        let debug_enabled = self.debug_enabled;
        let watched_paths = self.watched_paths.clone();

        thread::spawn(move || {
            let _watcher = watcher;
            let mut last_reload_time = std::time::Instant::now();

            // Cache the active preset to avoid repeated filesystem queries that can
            // fail transiently during rapid editor save operations. This cache is
            // invalidated when we actually process a reload (not when debounced).
            let mut cached_active_preset: Option<Option<String>> = None;

            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Config watcher thread started");

            for event in rx {
                let active_preset = cached_active_preset.clone().unwrap_or_else(|| {
                    let preset = crate::state::preset::get_active_preset().ok().flatten();
                    cached_active_preset = Some(preset.clone());
                    preset
                });

                let affects_config = event.paths.iter().any(|event_path| {
                    watched_paths.iter().any(|watched| {
                        if watched.is_file() {
                            event_path == watched
                                || (event_path.parent() == watched.parent()
                                    && event_path
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .zip(watched.file_name().and_then(|w| w.to_str()))
                                        .map(|(event_name, watched_name)| {
                                            event_name == watched_name
                                                || event_name.starts_with(watched_name)
                                                || event_name.ends_with("sunsetr.toml")
                                                || event_name.ends_with("geo.toml")
                                                || event_name == "active_preset"
                                                || event_name == "dir_id"
                                        })
                                        .unwrap_or(false))
                        } else if watched.ends_with("presets") {
                            if let Some(ref preset_name) = active_preset {
                                event_path.starts_with(watched)
                                    && event_path
                                        .components()
                                        .any(|c| c.as_os_str() == preset_name.as_str())
                                    && event_path
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .map(|name| {
                                            name == "sunsetr.toml"
                                                || name == "geo.toml"
                                                || name.ends_with("sunsetr.toml")
                                                || name.ends_with("geo.toml")
                                        })
                                        .unwrap_or(false)
                            } else {
                                false
                            }
                        } else {
                            // Special case: Check if a state namespace directory was deleted
                            // This happens when the entire state directory is removed
                            if watched.ends_with("sunsetr")
                                && event_path.starts_with(watched)
                                && let Some(name) = event_path.file_name().and_then(|n| n.to_str())
                                && (name == "default" || name.starts_with("custom_"))
                            {
                                return true;
                            }

                            event_path.starts_with(watched)
                                && event_path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .map(|name| {
                                        name == "active_preset"
                                            || name == "dir_id"
                                            || (active_preset.is_none()
                                                && (name == "sunsetr.toml" || name == "geo.toml"))
                                    })
                                    .unwrap_or(false)
                        }
                    })
                });

                if !affects_config {
                    #[cfg(debug_assertions)]
                    {
                        if event.paths.iter().any(|p| {
                            p.file_name()
                                .and_then(|n| n.to_str())
                                .map(|n| {
                                    n.contains("sunsetr")
                                        || n.contains("geo")
                                        || n.contains("preset")
                                })
                                .unwrap_or(false)
                        }) {
                            eprintln!("DEBUG: Ignored event for paths: {:?}", event.paths);
                        }
                    }
                    continue;
                }

                let elapsed = last_reload_time.elapsed();
                if elapsed < Duration::from_millis(DEBOUNCE_MS) {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "DEBUG: Ignoring config change event (debounce, {}ms since last reload)",
                        elapsed.as_millis()
                    );
                    continue;
                }

                if debug_enabled {
                    log_pipe!();
                    log_info!("Configuration file change detected");
                    #[cfg(debug_assertions)]
                    {
                        eprintln!("DEBUG: File change event: {:?}", event);
                        for path in &event.paths {
                            eprintln!("  Changed: {}", private_path(path));
                        }
                    }
                }

                match signal_sender.send(SignalMessage::Reload) {
                    Ok(()) => {
                        last_reload_time = std::time::Instant::now();
                        cached_active_preset = None;
                        if debug_enabled {
                            log_indented!("Triggering automatic configuration reload");
                        }
                    }
                    Err(_) => {
                        #[cfg(debug_assertions)]
                        eprintln!("DEBUG: Failed to send reload signal - channel disconnected");
                        break;
                    }
                }
            }

            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Config watcher thread exiting");
        });

        Ok(())
    }

    fn determine_watch_paths(&self) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();

        let config_path = Config::get_config_path()?;
        if config_path.exists() {
            paths.push(config_path.clone());
        }

        if let Some(config_dir) = config_path.parent() {
            let presets_dir = config_dir.join("presets");
            if presets_dir.exists() && presets_dir.is_dir() {
                paths.push(presets_dir);
            }
        }

        if let Ok(state_dir) = crate::state::preset::get_state_watch_path() {
            if let Some(parent) = state_dir.parent() {
                paths.push(parent.to_path_buf());
            }
            paths.push(state_dir.clone());
            let active_preset_path = state_dir.join("active_preset");
            if active_preset_path.exists() {
                paths.push(active_preset_path);
            }
            let dir_id_path = state_dir.join("dir_id");
            if dir_id_path.exists() {
                paths.push(dir_id_path);
            }
        }

        let geo_path = Config::get_geo_path()?;
        if geo_path.exists() {
            paths.push(geo_path);
        }

        Ok(paths)
    }
}

/// Start the configuration file watcher.
///
/// This is called from the main application to enable hot config reloading.
pub fn start_config_watcher(
    signal_sender: Sender<SignalMessage>,
    debug_enabled: bool,
) -> Result<()> {
    let watcher = ConfigWatcher::new(signal_sender, debug_enabled);
    watcher.start()
}
