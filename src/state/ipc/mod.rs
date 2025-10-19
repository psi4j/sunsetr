//! IPC (Inter-Process Communication) system for sunsetr.
//!
//! This module provides Unix socket-based IPC functionality to broadcast
//! DisplayState updates to external applications. The design follows
//! a simplified event-driven architecture where Core broadcasts DisplayState
//! updates whenever state changes occur.

use anyhow::{Context, Result};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};

use crate::state::display::DisplayState;

pub mod client;
mod server;

/// IPC notifier for sending DisplayState updates from Core to IPC server.
///
/// This follows the same pattern as other Core integrations (signals, etc.)
/// using non-blocking channels to avoid any impact on Core's main loop.
pub struct IpcNotifier {
    state_sender: mpsc::Sender<DisplayState>,
}

impl IpcNotifier {
    /// Create a new IpcNotifier and return both the notifier and receiver.
    ///
    /// # Returns
    /// Tuple of (IpcNotifier for Core, Receiver for IPC server thread)
    pub fn new() -> (Self, mpsc::Receiver<DisplayState>) {
        let (state_sender, state_receiver) = mpsc::channel();
        let notifier = Self { state_sender };
        (notifier, state_receiver)
    }

    /// Send a DisplayState update to the IPC server.
    ///
    /// This uses a synchronous channel that queues messages in memory.
    /// The IPC server processes messages quickly, preventing backpressure on Core.
    pub fn send(&self, display_state: DisplayState) {
        // Send via synchronous channel - messages queue in memory
        // IPC server processes quickly to prevent blocking Core
        let _ = self.state_sender.send(display_state);
    }
}

/// IPC server that manages Unix socket connections and broadcasts DisplayState updates.
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
    /// * `state_receiver` - Channel receiver for DisplayState updates from Core
    /// * `running_flag` - Shared running flag (typically from signal handler)
    /// * `debug_enabled` - Whether to show debug logging
    ///
    /// # Returns
    /// IpcServer instance with running background thread
    pub fn start(
        state_receiver: mpsc::Receiver<DisplayState>,
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

                match Self::run(state_receiver, running, debug_enabled) {
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
        state_receiver: mpsc::Receiver<DisplayState>,
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
            .run(state_receiver, running, debug_enabled)
            .context("IPC socket server failed")?;

        #[cfg(debug_assertions)]
        eprintln!("DEBUG: IPC socket server completed");
        Ok(())
    }
}
