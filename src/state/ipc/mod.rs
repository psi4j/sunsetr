//! IPC (Inter-Process Communication) system for sunsetr.
//!
//! This module provides Unix socket-based IPC functionality to broadcast
//! state change events to external applications. The design follows
//! a type-safe event-driven architecture where Core broadcasts typed
//! events whenever state changes occur.

use anyhow::{Context, Result};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};

use crate::core::period::Period;
use crate::core::runtime_state::RuntimeState;
use crate::state::display::DisplayState;

pub mod client;
pub mod events;
mod server;

use events::IpcEvent;

/// IPC notifier for sending typed events from Core to IPC server.
///
/// This follows the same pattern as other Core integrations (signals, etc.)
/// using non-blocking channels to avoid any impact on Core's main loop.
pub struct IpcNotifier {
    event_sender: mpsc::Sender<IpcEvent>,
}

impl IpcNotifier {
    /// Create a new IpcNotifier and return both the notifier and receiver.
    ///
    /// # Returns
    /// Tuple of (IpcNotifier for Core, Receiver for IPC server thread)
    pub fn new() -> (Self, mpsc::Receiver<IpcEvent>) {
        let (event_sender, event_receiver) = mpsc::channel();
        let notifier = Self { event_sender };
        (notifier, event_receiver)
    }

    /// Send a period change event from RuntimeState transition.
    ///
    /// This method is called when a time-based period transition occurs.
    pub fn send_period_changed(&self, from: Period, to: Period) {
        let event = IpcEvent::period_changed(from, to);
        let _ = self.event_sender.send(event);
    }

    /// Send a preset change event with target values.
    ///
    /// This method is called when the active preset changes, providing
    /// immediate feedback with the target temperature and gamma values.
    pub fn send_preset_changed(
        &self,
        from: Option<String>,
        to: Option<String>,
        target_temp: u32,
        target_gamma: f32,
    ) {
        let event = IpcEvent::preset_changed(from, to, target_temp, target_gamma);
        let _ = self.event_sender.send(event);
    }

    /// Send a state applied event from RuntimeState.
    ///
    /// Creates DisplayState from RuntimeState and broadcasts the state applied event.
    /// This is the standard way to broadcast state changes from the Core module.
    pub fn send_state_applied(&self, runtime_state: &RuntimeState) {
        let display_state = DisplayState::new(runtime_state);
        let event = IpcEvent::state_applied(display_state);
        let _ = self.event_sender.send(event);
    }
}

/// IPC server that manages Unix socket connections and broadcasts typed events.
///
/// This runs in a separate thread to avoid any impact on Core's time-critical
/// color temperature adjustments.
pub struct IpcServer {
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl IpcServer {
    /// Start the IPC server in a background thread.
    ///
    /// # Arguments
    /// * `event_receiver` - Channel receiver for IpcEvent updates from Core
    /// * `running_flag` - Shared running flag (typically from signal handler)
    /// * `debug_enabled` - Whether to show debug logging
    ///
    /// # Returns
    /// IpcServer instance with running background thread
    pub fn start(
        event_receiver: mpsc::Receiver<IpcEvent>,
        running_flag: Arc<AtomicBool>,
        debug_enabled: bool,
    ) -> Result<Self> {
        let running = Arc::clone(&running_flag);

        #[cfg(debug_assertions)]
        eprintln!("DEBUG: About to spawn IPC server thread");

        let thread_handle = std::thread::Builder::new()
            .name("ipc-server".to_string())
            .spawn(move || {
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: IPC server thread closure started");

                match Self::run(event_receiver, running, debug_enabled) {
                    Ok(()) => {
                        #[cfg(debug_assertions)]
                        eprintln!("DEBUG: IPC server completed successfully");
                    }
                    Err(_e) => {
                        #[cfg(debug_assertions)]
                        {
                            eprintln!("DEBUG: IPC server error: {_e}");
                            eprintln!("DEBUG: IPC server error context: {_e:#}");
                        }
                    }
                }

                #[cfg(debug_assertions)]
                eprintln!("DEBUG: IPC server thread closure finished");
            })
            .context("Failed to spawn IPC server thread")?;

        #[cfg(debug_assertions)]
        eprintln!("DEBUG: IPC server thread spawned successfully");

        Ok(Self {
            thread_handle: Some(thread_handle),
        })
    }

    /// Shutdown the IPC server gracefully.
    ///
    /// Note: The running flag is controlled by the signal handler.
    /// This method just waits for the thread to finish.
    pub fn shutdown(mut self) -> Result<()> {
        // Wait for thread to finish (running flag is controlled by signal handler)
        if let Some(handle) = self.thread_handle.take() {
            handle
                .join()
                .map_err(|_| anyhow::anyhow!("IPC server thread panicked"))?;
        }

        Ok(())
    }

    /// Main IPC server loop (runs in background thread).
    fn run(
        event_receiver: mpsc::Receiver<IpcEvent>,
        running: Arc<AtomicBool>,
        debug_enabled: bool,
    ) -> Result<()> {
        #[cfg(debug_assertions)]
        eprintln!("DEBUG: IPC server run() starting");

        debug_assert!(
            running.load(std::sync::atomic::Ordering::SeqCst),
            "IPC server should start with running flag set to true"
        );

        // Determine socket path
        let socket_path = server::socket_path().context("Failed to get IPC socket path")?;

        debug_assert!(
            !socket_path.to_string_lossy().is_empty(),
            "IPC socket path should not be empty"
        );

        #[cfg(debug_assertions)]
        eprintln!("DEBUG: IPC socket path: {:?}", socket_path);

        // Create and run socket server
        #[cfg(debug_assertions)]
        eprintln!("DEBUG: Creating IPC socket server");
        let socket_server = server::IpcSocketServer::new(socket_path)
            .context("Failed to create IPC socket server")?;

        #[cfg(debug_assertions)]
        eprintln!("DEBUG: Starting IPC socket server main loop");
        socket_server
            .run(event_receiver, running, debug_enabled)
            .context("IPC socket server failed")?;

        #[cfg(debug_assertions)]
        eprintln!("DEBUG: IPC socket server completed");
        Ok(())
    }
}
