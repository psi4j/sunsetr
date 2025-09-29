//! Application coordinator that manages the complete lifecycle of sunsetr.
//!
//! This module handles resource acquisition, initialization, and orchestration
//! of the core application logic. It manages:
//! - Terminal setup with RAII guards
//! - Configuration loading
//! - Backend detection and creation
//! - Lock file management for single-instance enforcement
//! - Signal handler setup
//! - Monitor initialization (D-Bus, config watcher)
//!
//! The `Sunsetr` struct uses a builder pattern to support different startup contexts:
//! - Normal startup: `Sunsetr::new(debug_enabled).run()`
//! - Geo restart: `Sunsetr::new(true).without_lock().with_previous_state(state).run()`
//! - Reload spawn: `Sunsetr::new(debug_enabled).with_reload().run()`
//! - Simulation mode: `Sunsetr::new(debug_enabled).without_lock().without_headers().run()`

use anyhow::{Context, Result};
use fs2::FileExt;

use crate::{
    backend::{create_backend, detect_backend, detect_compositor},
    config::{self, Config},
    core::{Core, CoreParams},
    dbus,
    geo::GeoTransitionTimes,
    signals::setup_signal_handler,
    time_source,
    time_state::TimeState,
    utils::{self, TerminalGuard},
};

/// Builder for configuring and running the sunsetr application.
///
/// This builder provides a flexible way to start sunsetr with different
/// configurations depending on the context (normal startup, geo restart,
/// reload spawn, simulation mode, etc.).
///
/// # Examples
///
/// ```
/// // Normal application startup
/// Sunsetr::new(debug_enabled).run()?;
///
/// // Restart after geo selection without creating a new lock
/// Sunsetr::new(true)
///     .without_lock()
///     .with_previous_state(previous_state)
///     .run()?;
///
/// // Process spawned from reload command
/// Sunsetr::new(debug_enabled)
///     .with_reload()
///     .run()?;
///
/// // Simulation mode
/// Sunsetr::new(debug_enabled)
///     .without_lock()
///     .without_headers()
///     .run()?;
/// ```
pub struct Sunsetr {
    debug_enabled: bool,
    create_lock: bool,
    previous_state: Option<TimeState>,
    show_headers: bool,
    from_reload: bool, // Process spawned from reload command
}

impl Sunsetr {
    /// Create a new runner with defaults matching normal run
    pub fn new(debug_enabled: bool) -> Self {
        Self {
            debug_enabled,
            create_lock: true,
            previous_state: None,
            show_headers: true,
            from_reload: false,
        }
    }

    /// Skip lock file creation (for geo restart)
    pub fn without_lock(mut self) -> Self {
        self.create_lock = false;
        self.show_headers = false; // Geo restarts never show headers
        self
    }

    /// Set previous state for smooth transitions
    pub fn with_previous_state(mut self, state: Option<TimeState>) -> Self {
        self.previous_state = state;
        self
    }

    /// Skip header display (for geo operations)
    pub fn without_headers(mut self) -> Self {
        self.show_headers = false;
        self
    }

    /// Mark this process as spawned from reload command
    pub fn with_reload(mut self) -> Self {
        self.from_reload = true;
        self
    }

    /// Execute the application with the configured settings.
    ///
    /// This method handles the complete application lifecycle including:
    /// - Terminal setup
    /// - Configuration loading
    /// - Backend detection and initialization
    /// - Lock file management (if enabled)
    /// - Signal handler setup
    /// - Main application loop
    /// - Graceful shutdown and cleanup
    pub fn run(self) -> Result<()> {
        // Show headers if requested
        if self.show_headers {
            log_version!();
        }

        // Execute the core application logic
        #[cfg(debug_assertions)]
        {
            let log_msg = format!(
                "DEBUG: Process {} startup: debug_enabled={}, create_lock={}\n",
                std::process::id(),
                self.debug_enabled,
                self.create_lock
            );
            let _ = std::fs::write(
                format!("/tmp/sunsetr-debug-{}.log", std::process::id()),
                log_msg,
            );
        }

        // Try to set up terminal features (cursor hiding, echo suppression)
        // This will gracefully handle cases where no terminal is available (e.g., systemd service)
        let _term = TerminalGuard::new().context("failed to initialize terminal features")?;

        // Note: The Hyprsunset backend uses PR_SET_PDEATHSIG for process cleanup

        // Load and validate configuration first (needed for backend detection)
        let config = match Config::load() {
            Ok(config) => config,
            Err(e) => {
                // Use the standalone error format with the full error chain
                log_error_exit!("Configuration failed");
                // Print the error chain in the default format which already looks good
                eprintln!("{:?}", e);
                std::process::exit(1);
            }
        };

        // Detect and validate the backend early (needed for lock file info)
        let backend_type = detect_backend(&config).unwrap_or_else(|_| {
            // Backend detection errors are already logged properly in detect_backend
            // Just exit since the error was already displayed
            std::process::exit(1);
        });

        // Handle lock file BEFORE any debug output from watchers
        let (lock_file, lock_path) = if self.create_lock {
            // Create lock file path
            let runtime_dir =
                std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
            let lock_path = format!("{runtime_dir}/sunsetr.lock");

            // Open lock file without truncating to preserve existing content
            // This prevents a race condition where File::create() would truncate
            // the file before we check if the lock can be acquired.
            // See tests/lock_file_unit_tests.rs and tests/lock_logic_test.rs for details.
            let mut lock_file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false) // Don't truncate existing file
                .open(&lock_path)?;

            // Try to acquire exclusive lock
            match lock_file.try_lock_exclusive() {
                Ok(_) => {
                    // Lock acquired - now safe to truncate and write our info
                    use std::io::{Seek, SeekFrom, Write};

                    // Truncate the file and reset position
                    lock_file.set_len(0)?;
                    lock_file.seek(SeekFrom::Start(0))?;

                    // Write our PID, compositor, and config dir to the lock file for restart functionality
                    let pid = std::process::id();
                    let compositor = detect_compositor().to_string();
                    writeln!(&lock_file, "{pid}")?;
                    writeln!(&lock_file, "{compositor}")?;
                    // Write config directory (empty line if using default)
                    if let Some(ref dir) = config::get_custom_config_dir() {
                        writeln!(&lock_file, "{}", dir.display())?;
                    } else {
                        writeln!(&lock_file)?;
                    }
                    lock_file.flush()?;

                    (Some(lock_file), Some(lock_path))
                }
                Err(_) => {
                    // Handle lock conflict with smart validation
                    match Self::handle_lock_conflict(&lock_path) {
                        Ok(()) => {
                            // Stale lock removed or cross-compositor cleanup completed
                            // Retry lock acquisition without truncating
                            let mut retry_lock_file = std::fs::OpenOptions::new()
                                .write(true)
                                .create(true)
                                .truncate(false)
                                .open(&lock_path)?;
                            match retry_lock_file.try_lock_exclusive() {
                                Ok(_) => {
                                    // Lock acquired - now safe to truncate and write our info
                                    use std::io::{Seek, SeekFrom, Write};

                                    // Truncate the file and reset position
                                    retry_lock_file.set_len(0)?;
                                    retry_lock_file.seek(SeekFrom::Start(0))?;

                                    // Write our PID, compositor, and config dir to the lock file
                                    let pid = std::process::id();
                                    let compositor = detect_compositor().to_string();
                                    writeln!(&retry_lock_file, "{pid}")?;
                                    writeln!(&retry_lock_file, "{compositor}")?;
                                    // Write config directory (empty line if using default)
                                    if let Some(ref dir) = config::get_custom_config_dir() {
                                        writeln!(&retry_lock_file, "{}", dir.display())?;
                                    } else {
                                        writeln!(&retry_lock_file)?;
                                    }
                                    retry_lock_file.flush()?;

                                    (Some(retry_lock_file), Some(lock_path))
                                }
                                Err(e) => {
                                    // Still failed after cleanup attempt
                                    log_error_exit!(
                                        "Failed to acquire lock after cleanup attempt: {}",
                                        e
                                    );
                                    std::process::exit(1);
                                }
                            }
                        }
                        Err(e) => {
                            // Error already logged by handle_lock_conflict
                            return Err(e);
                        }
                    }
                }
            }
        } else {
            (None, None)
        };

        // Set up signal handling
        let signal_state = setup_signal_handler(self.debug_enabled)?;

        // Start D-Bus sleep/resume monitoring (optional - graceful degradation if D-Bus unavailable)
        if let Err(e) =
            dbus::start_sleep_resume_monitor(signal_state.signal_sender.clone(), self.debug_enabled)
        {
            log_pipe!();
            log_warning!("D-Bus sleep/resume monitoring unavailable: {}", e);
            log_indented!(
                "Sleep/resume detection will not work, but sunsetr will continue normally"
            );
            log_indented!("This is normal in environments without systemd or D-Bus");
        }

        // Start config file watcher for hot reload (optional - graceful degradation if unavailable)
        if let Err(e) =
            config::start_config_watcher(signal_state.signal_sender.clone(), self.debug_enabled)
            && self.debug_enabled
        {
            log_pipe!();
            log_warning!("Config file watching unavailable: {}", e);
            log_indented!("Hot config reload disabled, use SIGUSR2 for manual reload");
        }

        // Log configuration with resolved backend type
        config.log_config(Some(backend_type));

        // Initialize GeoTransitionTimes before backend creation if in geo mode
        // Backends need this to calculate correct initial state values
        let geo_times = GeoTransitionTimes::from_config(&config)
            .context("Failed to initialize geo transition times")?;

        log_block_start!("Detected backend: {}", backend_type.name());

        // Create the backend
        let backend = create_backend(
            backend_type,
            &config,
            self.debug_enabled,
            geo_times.as_ref(),
        )?;

        // Create lock_info tuple from lock components
        let lock_info = if let (Some(lock_file), Some(lock_path)) = (lock_file, lock_path) {
            log_block_start!("Lock acquired, starting sunsetr...");
            Some((lock_file, lock_path))
        } else {
            // Skip lock creation (geo selection restart case or simulation mode)
            // Only show "Restarting" message if not in simulation mode
            if !time_source::is_simulated() {
                log_block_start!("Restarting sunsetr...");
            }
            None
        };

        // Create Core with all necessary dependencies
        let core = Core::new(CoreParams {
            backend,
            config,
            signal_state,
            debug_enabled: self.debug_enabled,
            geo_times,
            lock_info,
            initial_previous_state: self.previous_state,
            from_reload: self.from_reload,
        });

        // Execute the core logic
        core.execute()?;

        Ok(())
    }

    /// Handle lock file conflicts intelligently.
    ///
    /// This function validates and cleans up lock files in the following scenarios:
    /// - Stale lock files (process no longer running)
    /// - Cross-compositor switches (e.g., switching from Hyprland to Sway)
    /// - Providing helpful suggestions when instance is already running
    fn handle_lock_conflict(lock_path: &str) -> Result<()> {
        // Read the lock file to get PID and compositor info
        let lock_content = match std::fs::read_to_string(lock_path) {
            Ok(content) => content,
            Err(_) => {
                // Lock file doesn't exist or can't be read - assume it was cleaned up
                return Ok(());
            }
        };

        let lines: Vec<&str> = lock_content.trim().lines().collect();

        // Lock file format: PID (line 1), compositor (line 2), config_dir (line 3, optional)
        if lines.len() < 2 || lines.len() > 3 {
            // Invalid lock file format
            log_warning!("Lock file format invalid, removing");
            let _ = std::fs::remove_file(lock_path);
            return Ok(());
        }

        let pid = match lines[0].parse::<u32>() {
            Ok(pid) => pid,
            Err(_) => {
                log_warning!("Lock file contains invalid PID, removing stale lock");
                let _ = std::fs::remove_file(lock_path);
                return Ok(());
            }
        };

        let existing_compositor = lines[1].to_string();

        // Check if the process is actually running
        if !utils::is_process_running(pid) {
            log_warning!("Removing stale lock file (process {pid} no longer running)");
            let _ = std::fs::remove_file(lock_path);
            return Ok(());
        }

        // Process is running - check if this is a cross-compositor switch scenario
        let current_compositor = detect_compositor().to_string();

        if existing_compositor != current_compositor {
            // Cross-compositor switch detected - force cleanup
            log_pipe!();
            log_warning!(
                "Cross-compositor switch detected: {existing_compositor} → {current_compositor}"
            );
            log_indented!("Terminating existing sunsetr process (PID: {pid})");

            if utils::kill_process(pid) {
                // Wait for process to fully exit
                std::thread::sleep(std::time::Duration::from_millis(500));

                // Clean up lock file
                let _ = std::fs::remove_file(lock_path);

                log_indented!("Cross-compositor cleanup completed");
                return Ok(());
            } else {
                log_pipe!();
                log_error!("Failed to terminate existing process");
                log_indented!("Cannot force cleanup - existing process could not be terminated");
                log_end!();
                std::process::exit(1)
            }
        }

        // Same compositor - respect single instance enforcement
        log_pipe!();
        log_error!("sunsetr is already running (PID: {pid})");
        log_block_start!("Did you mean to:");
        log_indented!("• Reload configuration: sunsetr reload");
        log_indented!("• Test new values: sunsetr test <temp> <gamma>");
        log_indented!("• Switch to a preset: sunsetr preset <preset>");
        log_indented!("• Switch geolocation: sunsetr geo");
        log_block_start!("Cannot start - another sunsetr instance is running");
        log_end!();
        std::process::exit(1)
    }
}
