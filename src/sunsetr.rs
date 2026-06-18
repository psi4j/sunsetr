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
//! - Simulation: `Sunsetr::new(debug_enabled).without_lock().without_headers().run()`

use anyhow::{Context, Result};

use crate::{
    backend::{create_backend, detect_backend},
    common::utils::TerminalGuard,
    config::{self, Config},
    core::{Core, CoreParams},
    geo::times::GeoTimes,
    io::dbus,
    io::signals::setup_signal_handler,
};

/// Builder for configuring and running the sunsetr application.
///
/// This builder provides a flexible way to start sunsetr with different
/// configurations depending on the context (normal startup, simulation,
/// restart, background spawn).
pub struct Sunsetr {
    debug_enabled: bool,
    create_lock: bool,
    show_headers: bool,
    bypass_smoothing: bool,
    background: bool,
}

impl Sunsetr {
    /// Create a new runner with defaults matching normal run
    pub fn new(debug_enabled: bool) -> Self {
        Self {
            debug_enabled,
            create_lock: true,
            show_headers: true,
            bypass_smoothing: false,
            background: false,
        }
    }

    /// Run without acquiring the single-instance lock.
    pub fn without_lock(mut self) -> Self {
        self.create_lock = false;
        self
    }

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
    pub fn background(mut self, background: bool) -> Self {
        self.background = background;
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
        if self.background {
            if self.show_headers {
                log_version!();
            }

            if let Ok(Some(_)) = crate::io::instance::get_running_instance() {
                crate::io::instance::handle_instance_conflict(
                    &crate::io::lock::get_main_lock_path(),
                    self.debug_enabled,
                )?;
            }

            let result = crate::io::instance::spawn_background_instance(self.debug_enabled);
            log_end!();
            return result;
        }

        let _ = crate::state::preset::cleanup_orphaned_state_dirs();

        if self.show_headers {
            log_version!();
        }

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

        let _term = TerminalGuard::new().context("failed to initialize terminal features")?;
        let config = Config::load()?;
        let backend_type = detect_backend(&config)?;

        let (lock_file, lock_path) = if self.create_lock {
            match crate::io::instance::ensure_single_instance()? {
                Some((lock, path)) => (Some(lock), Some(path)),
                None => return Ok(()),
            }
        } else {
            (None, None)
        };

        let signal_state = setup_signal_handler(self.debug_enabled)?;

        if let Err(e) = dbus::start_sleep_resume_monitor(
            signal_state.signal_sender.clone(),
            signal_state.interrupt.clone(),
            self.debug_enabled,
        ) {
            log_pipe!();
            log_warning!("D-Bus sleep/resume monitoring unavailable: {}", e);
            log_indented!(
                "Sleep/resume detection will not work, but sunsetr will continue normally"
            );
            log_indented!("This is normal in environments without systemd or D-Bus");
        }

        if let Err(e) = config::start_config_watcher(
            signal_state.signal_sender.clone(),
            signal_state.interrupt.clone(),
            self.debug_enabled,
        ) && self.debug_enabled
        {
            log_pipe!();
            log_warning!("Config file watching unavailable: {}", e);
            log_indented!("Hot config reload disabled, use SIGUSR2 for manual reload");
        }

        config.log_config(Some(backend_type));

        let geo_times =
            GeoTimes::from_config(&config).context("Failed to initialize geo transition times")?;
        log_block_start!("Detected backend: {}", backend_type.name());
        let initial_period = crate::core::period::get_current_period(&config, geo_times.as_ref());

        let schedule = crate::core::schedule::Schedule::from_config(&config, geo_times.clone());
        let runtime_state = crate::core::runtime_state::RuntimeState::new(
            initial_period,
            &config,
            schedule,
            crate::time::source::now(),
        );

        let (initial_temp, initial_gamma) = runtime_state.values();

        let backend = create_backend(
            backend_type,
            &config,
            self.debug_enabled,
            geo_times.as_ref(),
            Some((initial_temp, initial_gamma)),
        )?;

        let lock_info = if let (Some(lock_file), Some(lock_path)) = (lock_file, lock_path) {
            log_block_start!("Lock acquired, starting sunsetr...");
            Some((lock_file, lock_path))
        } else {
            if !crate::time::source::is_simulated() {
                log_block_start!("Restarting sunsetr...");
            }
            None
        };

        let (ipc_notifier, ipc_server) = if crate::time::source::is_simulated() {
            if self.debug_enabled {
                log_debug!("Skipping IPC server - simulation mode detected");
            }
            (None, None)
        } else {
            if self.debug_enabled {
                log_debug!("Starting IPC server - not in simulation mode");
            }
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

        let core = Core::new(CoreParams {
            backend,
            runtime_state,
            signal_state,
            debug_enabled: self.debug_enabled,
            lock_info,
            bypass_smoothing: self.bypass_smoothing,
            ipc_notifier,
        });

        let result = core.execute();

        if let Some(server) = ipc_server
            && let Err(e) = server.shutdown()
        {
            eprintln!("Warning: IPC server shutdown error: {}", e);
        }

        result
    }
}
