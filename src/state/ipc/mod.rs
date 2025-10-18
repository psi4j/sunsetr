//! IPC (Inter-Process Communication) system for sunsetr.
//!
//! This module provides Unix socket-based IPC functionality to broadcast
//! DisplayState updates to external applications. The design follows
//! a simplified event-driven architecture where Core broadcasts DisplayState
//! updates whenever state changes occur.

use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// This is non-blocking and will not impact Core's performance.
    /// Uses unbounded channel so messages queue in memory if IPC server falls behind.
    pub fn send(&self, display_state: DisplayState) {
        // Non-blocking send via unbounded channel - never blocks Core
        // Messages queue in memory if IPC server falls behind
        let _ = self.state_sender.send(display_state);
    }
}

/// IPC server that manages Unix socket connections and broadcasts DisplayState updates.
///
/// This runs in a separate thread to avoid any impact on Core's time-critical
/// color temperature adjustments.
pub struct IpcServer {
    shutdown_flag: Arc<AtomicBool>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl IpcServer {
    /// Start the IPC server in a background thread.
    ///
    /// # Arguments
    /// * `state_receiver` - Channel receiver for DisplayState updates from Core
    ///
    /// # Returns
    /// IpcServer instance with running background thread
    pub fn start(state_receiver: mpsc::Receiver<DisplayState>) -> Result<Self> {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::clone(&shutdown_flag);

        let thread_handle = std::thread::Builder::new()
            .name("ipc-server".to_string())
            .spawn(move || {
                if let Err(e) = Self::run(state_receiver, shutdown) {
                    eprintln!("IPC server error: {e}");
                }
            })
            .context("Failed to spawn IPC server thread")?;

        Ok(Self {
            shutdown_flag,
            thread_handle: Some(thread_handle),
        })
    }

    /// Shutdown the IPC server gracefully.
    pub fn shutdown(mut self) -> Result<()> {
        // Signal shutdown
        self.shutdown_flag.store(true, Ordering::SeqCst);

        // Wait for thread to finish
        if let Some(handle) = self.thread_handle.take() {
            handle
                .join()
                .map_err(|_| anyhow::anyhow!("IPC server thread panicked"))?;
        }

        Ok(())
    }

    /// Main IPC server loop (runs in background thread).
    fn run(state_receiver: mpsc::Receiver<DisplayState>, shutdown: Arc<AtomicBool>) -> Result<()> {
        // Determine socket path
        let socket_path = server::socket_path().context("Failed to get IPC socket path")?;

        // Create and run socket server
        let socket_server = server::IpcSocketServer::new(socket_path)
            .context("Failed to create IPC socket server")?;

        socket_server
            .run(state_receiver, shutdown)
            .context("IPC socket server failed")?;

        Ok(())
    }
}
