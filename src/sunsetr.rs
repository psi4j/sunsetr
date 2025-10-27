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
//! - Simulation mode: `Sunsetr::new(debug_enabled).without_lock().without_headers().run()`

use anyhow::{Context, Result};

use crate::{
    backend::{create_backend, detect_backend},
    common::utils::TerminalGuard,
    config::{self, Config},
    core::period::Period,
    core::{Core, CoreParams},
    geo::times::GeoTimes,
    io::dbus,
    io::signals::setup_signal_handler,
};

/// Builder for configuring and running the sunsetr application.
///
/// This builder provides a flexible way to start sunsetr with different
/// configurations depending on the context (normal startup, geo restart,
/// simulation mode, etc.).
///
/// # Examples
///
/// ```no_run
/// use sunsetr::Sunsetr;
/// use sunsetr::Period;
///
/// # fn main() -> anyhow::Result<()> {
/// // Normal application startup
/// let debug_enabled = false;
/// Sunsetr::new(debug_enabled).run()?;
///
/// // Restart after geo selection without creating a new lock
/// let previous_state = Some(Period::Night);
/// Sunsetr::new(true)
///     .without_lock()
///     .with_previous_state(previous_state)
///     .run()?;
///
/// // Simulation mode
/// Sunsetr::new(debug_enabled)
///     .without_lock()
///     .without_headers()
///     .run()?;
/// # Ok(())
/// # }
/// ```
pub struct Sunsetr {
    debug_enabled: bool,
    create_lock: bool,
    previous_state: Option<Period>,
    show_headers: bool,
    bypass_smoothing: bool, // Skip all smooth transitions for this instance
    background: bool,       // Run in background mode
}

impl Sunsetr {
    /// Create a new runner with defaults matching normal run
    pub fn new(debug_enabled: bool) -> Self {
        Self {
            debug_enabled,
            create_lock: true,
            previous_state: None,
            show_headers: true,
            bypass_smoothing: false,
            background: false,
        }
    }

    /// Skip lock file creation (for geo restart)
    pub fn without_lock(mut self) -> Self {
        self.create_lock = false;
        self.show_headers = false; // Geo restarts never show headers
        self
    }

    /// Set previous state for smooth transitions
    pub fn with_previous_state(mut self, state: Option<Period>) -> Self {
        self.previous_state = state;
        self
    }

    /// Skip header display (for geo operations)
    pub fn without_headers(mut self) -> Self {
        self.show_headers = false;
        self
    }

    /// Skip all smooth transitions for instant behavior (used by --instant flag)
    pub fn bypass_smoothing(mut self) -> Self {
        self.bypass_smoothing = true;
        self
    }

    /// Run in background mode using existing background spawning logic
    pub fn background(mut self) -> Self {
        self.background = true;
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
    ///
    /// If background mode is enabled, spawns a background instance instead.
    pub fn run(self) -> Result<()> {
        // Handle background mode by spawning a background instance
        if self.background {
            // Show headers if they haven't been explicitly disabled
            if self.show_headers {
                log_version!();
            }

            // Check for existing instance first - background mode should respect single instance enforcement
            if let Ok(Some(_)) = crate::io::instance::get_running_instance() {
                // An instance is already running, show the same error as foreground mode
                crate::io::instance::handle_instance_conflict(
                    &crate::io::lock::get_main_lock_path(),
                    self.debug_enabled,
                )?;
            }

            let result = crate::io::instance::spawn_background_instance(self.debug_enabled);
            log_end!();
            return result;
        }

        // Show headers if requested (normal foreground mode)
        if self.show_headers {
            log_version!();
        }

        // Execute the core application logic
        #[cfg(debug_assertions)]
        {
            let log_msg = format!(
                "DEBUG: Process {} startup: debug_enabled={}, create_lock={}, background={}\n",
                std::process::id(),
                self.debug_enabled,
                self.create_lock,
                self.background
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
            // Use the io::instance module for centralized lock management
            match crate::io::instance::ensure_single_instance()? {
                Some((lock, path)) => (Some(lock), Some(path)),
                None => return Ok(()), // Lock not acquired but handled appropriately
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

        // Initialize GeoTimes before backend creation if in geo mode
        // Backends need this to calculate correct initial state values
        let geo_times =
            GeoTimes::from_config(&config).context("Failed to initialize geo transition times")?;

        log_block_start!("Detected backend: {}", backend_type.name());

        // Create RuntimeState as single source of truth (before backend creation)
        // This ensures consistent timing for both simulation and normal operation
        let initial_period = crate::core::period::get_current_period(&config, geo_times.as_ref());
        let runtime_state = crate::core::runtime_state::RuntimeState::new(
            initial_period,
            &config,
            geo_times.as_ref(),
            crate::time::source::now().time(),
        );

        // Extract initial values for backends that need them (e.g., Hyprsunset)
        let (initial_temp, initial_gamma) = runtime_state.values();

        // Create the backend with pre-calculated initial values (optimization for Hyprsunset)
        let backend = create_backend(
            backend_type,
            &config,
            self.debug_enabled,
            geo_times.as_ref(),
            Some((initial_temp, initial_gamma)), // Pass pre-calculated values
        )?;

        // Create lock_info tuple from lock components
        let lock_info = if let (Some(lock_file), Some(lock_path)) = (lock_file, lock_path) {
            log_block_start!("Lock acquired, starting sunsetr...");
            Some((lock_file, lock_path))
        } else {
            // Skip lock creation (geo selection restart case or simulation mode)
            // Only show "Restarting" message if not in simulation mode
            if !crate::time::source::is_simulated() {
                log_block_start!("Restarting sunsetr...");
            }
            None
        };

        // Start IPC server (skip only in simulation mode to avoid socket conflicts)
        let (ipc_notifier, ipc_server) = if crate::time::source::is_simulated() {
            if self.debug_enabled {
                log_debug!("Skipping IPC server - simulation mode detected");
            }
            // Skip IPC only in simulation mode to avoid socket conflicts
            (None, None)
        } else {
            if self.debug_enabled {
                log_debug!("Starting IPC server - not in simulation mode");
            }
            // Create IPC notifier and start server
            let (notifier, state_receiver) = crate::state::ipc::IpcNotifier::new();
            let server = crate::state::ipc::IpcServer::start(
                state_receiver,
                signal_state.running.clone(),
                self.debug_enabled,
            )
            .context("Failed to start IPC server")?;
            if self.debug_enabled {
                log_debug!("IPC server started successfully");
            }
            (Some(notifier), Some(server))
        };

        // RuntimeState already created above (before backend creation)

        // Create previous RuntimeState if we have previous state info
        let initial_previous_runtime_state = self.previous_state.map(|prev_period| {
            crate::core::runtime_state::RuntimeState::new(
                prev_period,
                &config,
                geo_times.as_ref(),
                crate::time::source::now().time(),
            )
        });

        // Create Core with all necessary dependencies
        let core = Core::new(CoreParams {
            backend,
            runtime_state,
            signal_state,
            debug_enabled: self.debug_enabled,
            lock_info,
            initial_previous_runtime_state,
            bypass_smoothing: self.bypass_smoothing,
            ipc_notifier,
        });

        // Execute the core logic
        let result = core.execute();

        // Shutdown IPC server gracefully if it was started
        if let Some(server) = ipc_server
            && let Err(e) = server.shutdown()
        {
            eprintln!("Warning: IPC server shutdown error: {}", e);
        }

        result
    }
}
