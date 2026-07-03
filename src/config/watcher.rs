//! Watch the config files and send a reload signal when they change.

use crate::common::utils::private_path;
use anyhow::{Context, Result};
use notify::{
    Config as NotifyConfig, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use super::Config;
use crate::io::signals::SignalMessage;

/// Editor saves (e.g. Neovim) are not atomic from the watcher's view, so a
/// reload can read the file mid-write and fail spuriously. Retry a few times
/// before reporting. A partial write settles within a few milliseconds, while
/// a genuinely bad config keeps failing every attempt.
const RELOAD_ATTEMPTS: u32 = 3;
const RELOAD_RETRY_DELAY: Duration = Duration::from_millis(50);

pub struct ConfigWatcher {
    signal_sender: Sender<SignalMessage>,
    interrupt: Arc<AtomicBool>,
    debug_enabled: bool,
    watched_paths: Vec<PathBuf>,
}

impl ConfigWatcher {
    pub fn new(
        signal_sender: Sender<SignalMessage>,
        interrupt: Arc<AtomicBool>,
        debug_enabled: bool,
    ) -> Self {
        Self {
            signal_sender,
            interrupt,
            debug_enabled,
            watched_paths: Vec::new(),
        }
    }

    /// Spawn a background thread that watches the config files and sends a reload signal on change.
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
            // Watch state files directly (not their parent dir) so deletions arrive immediately.
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
        let interrupt = self.interrupt.clone();
        let debug_enabled = self.debug_enabled;
        let watched_paths = self.watched_paths.clone();

        thread::spawn(move || {
            let _watcher = watcher;

            // Cache the active preset to avoid repeated filesystem queries that can
            // fail transiently during rapid editor save operations. This cache is
            // invalidated when we actually process a reload.
            let mut cached_active_preset: Option<Option<String>> = None;

            // One editor save emits a burst of filesystem events, so a failed
            // reload would otherwise log the same error repeatedly. Log a given
            // failure once until it clears (successful reload) or changes.
            let mut last_reload_error: Option<String> = None;

            // One editor save emits a burst of events that all resolve to the
            // same preset and contents. Deduplicate by (active preset, config
            // value) so redundant events do not re-interrupt transitions or
            // repeat reload logging.
            let mut last_sent: Option<(Option<String>, Config)> = None;

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
                            // A whole state namespace directory (default/custom_*) was deleted.
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

                let mut load_result = Config::load();
                for _ in 1..RELOAD_ATTEMPTS {
                    if load_result.is_ok() {
                        break;
                    }
                    thread::sleep(RELOAD_RETRY_DELAY);
                    load_result = Config::load();
                }

                let new_config = match load_result {
                    Ok(config) => config,
                    Err(e) => {
                        let rendered = format!("{e:#}");
                        if last_reload_error.as_deref() != Some(rendered.as_str()) {
                            log_pipe!();
                            crate::common::error::log_error_chain("Failed to reload config", &e);
                            log_indented!("Continuing with previous configuration");
                            last_reload_error = Some(rendered);
                        }
                        continue;
                    }
                };

                let current_preset = crate::state::preset::get_active_preset().ok().flatten();
                if let Some((last_preset, last_config)) = last_sent.as_ref()
                    && last_preset == &current_preset
                    && last_config == &new_config
                {
                    last_reload_error = None;
                    continue;
                }

                // Set interrupt flag directly so smooth transitions can
                // detect the interruption immediately without waiting for the
                // main loop to process the channel message
                interrupt.store(true, Ordering::SeqCst);

                match signal_sender.send(SignalMessage::Reload(Box::new(new_config.clone()))) {
                    Ok(()) => {
                        last_sent = Some((current_preset, new_config));
                        cached_active_preset = None;
                        last_reload_error = None;
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

pub fn start_config_watcher(
    signal_sender: Sender<SignalMessage>,
    interrupt: Arc<AtomicBool>,
    debug_enabled: bool,
) -> Result<()> {
    let watcher = ConfigWatcher::new(signal_sender, interrupt, debug_enabled);
    watcher.start()
}
