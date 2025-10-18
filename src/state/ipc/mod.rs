//! IPC (Inter-Process Communication) system for sunsetr.
//!
//! This module provides Unix socket-based IPC functionality to broadcast
//! DisplayState updates to external applications. The design follows
//! a simplified event-driven architecture where Core broadcasts DisplayState
//! updates whenever state changes occur.

use anyhow::{Context, Result};
use std::sync::mpsc;

use crate::state::display::DisplayState;

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
    pub fn try_send_display_state(&self, display_state: DisplayState) {
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
    // Will be implemented in the next step
}

impl IpcServer {
    /// Start the IPC server in a background thread.
    ///
    /// # Arguments
    /// * `state_receiver` - Channel receiver for DisplayState updates from Core
    ///
    /// # Returns
    /// Handle to the background thread running the IPC server
    pub fn start(
        state_receiver: mpsc::Receiver<DisplayState>,
    ) -> Result<std::thread::JoinHandle<()>> {
        std::thread::Builder::new()
            .name("ipc-server".to_string())
            .spawn(move || {
                if let Err(e) = Self::run(state_receiver) {
                    eprintln!("IPC server error: {e}");
                }
            })
            .context("Failed to spawn IPC server thread")
    }

    /// Main IPC server loop (runs in background thread).
    fn run(state_receiver: mpsc::Receiver<DisplayState>) -> Result<()> {
        // TODO: Implement Unix socket server and client management
        // For now, just consume messages to prevent channel blocking
        while let Ok(_display_state) = state_receiver.recv() {
            // Placeholder - will implement socket broadcasting
        }
        Ok(())
    }
}
