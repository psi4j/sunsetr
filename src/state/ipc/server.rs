//! Unix socket server implementation for sunsetr IPC.
//!
//! This module provides the low-level Unix domain socket server that accepts
//! client connections and manages the IPC communication protocol.

use anyhow::{Context, Result};
use nix::unistd::getuid;
use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use crate::state::display::DisplayState;

/// Unix socket server for handling IPC client connections.
pub struct IpcSocketServer {
    socket_path: PathBuf,
    listener: UnixListener,
    clients: HashMap<u32, ClientConnection>,
    next_client_id: u32,
    current_state: Option<DisplayState>,
}

/// Represents a connected IPC client.
struct ClientConnection {
    #[allow(dead_code)] // Used for logging in debug messages
    id: u32,
    stream: BufWriter<UnixStream>,
}

impl IpcSocketServer {
    /// Create a new IPC socket server.
    ///
    /// # Arguments
    /// * `socket_path` - Path where the Unix socket should be created
    ///
    /// # Returns
    /// Configured IPC socket server ready to accept connections
    pub fn new(socket_path: PathBuf) -> Result<Self> {
        // Remove existing socket file if it exists
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)
                .with_context(|| format!("Failed to remove existing socket: {:?}", socket_path))?;
        }

        // Create parent directory if it doesn't exist
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create socket directory: {:?}", parent))?;
        }

        // Bind to Unix socket
        let listener = UnixListener::bind(&socket_path)
            .with_context(|| format!("Failed to bind Unix socket: {:?}", socket_path))?;

        // Set socket to non-blocking mode for connection acceptance
        listener
            .set_nonblocking(true)
            .context("Failed to set socket to non-blocking mode")?;

        Ok(Self {
            socket_path,
            listener,
            clients: HashMap::new(),
            next_client_id: 1,
            current_state: None,
        })
    }

    /// Run the main server loop.
    ///
    /// This method blocks and runs the server until the shutdown signal is received.
    ///
    /// # Arguments
    /// * `state_receiver` - Channel to receive DisplayState updates from Core
    /// * `shutdown` - Atomic flag to signal server shutdown
    pub fn run(
        mut self,
        state_receiver: mpsc::Receiver<DisplayState>,
        shutdown: Arc<AtomicBool>,
    ) -> Result<()> {
        eprintln!("IPC server starting on socket: {:?}", self.socket_path);

        while !shutdown.load(Ordering::SeqCst) {
            // Check for new DisplayState updates (non-blocking)
            while let Ok(display_state) = state_receiver.try_recv() {
                self.update_state(display_state)?;
            }

            // Accept new client connections (non-blocking)
            self.accept()?;

            // Remove disconnected clients
            self.prune_clients();

            // Small delay to prevent busy-waiting
            thread::sleep(Duration::from_millis(10));
        }

        eprintln!("IPC server shutting down");
        self.cleanup()?;
        Ok(())
    }

    /// Update the current state and broadcast to all clients.
    fn update_state(&mut self, display_state: DisplayState) -> Result<()> {
        // Update our current state
        self.current_state = Some(display_state.clone());

        // Broadcast to all connected clients
        self.broadcast(&display_state)
    }

    /// Broadcast DisplayState to all connected clients.
    fn broadcast(&mut self, display_state: &DisplayState) -> Result<()> {
        // Serialize DisplayState to JSON
        let json_line = serde_json::to_string(display_state)
            .context("Failed to serialize DisplayState to JSON")?;
        let message = format!("{}\n", json_line);

        // Send to all clients, marking failed ones for removal
        let mut failed_clients = Vec::new();

        for (client_id, client) in &mut self.clients {
            if client.stream.write_all(message.as_bytes()).is_err()
                || client.stream.flush().is_err()
            {
                failed_clients.push(*client_id);
            }
        }

        // Remove failed clients
        for client_id in failed_clients {
            self.clients.remove(&client_id);
            eprintln!("Removed disconnected client: {}", client_id);
        }

        Ok(())
    }

    /// Accept new client connections (non-blocking).
    fn accept(&mut self) -> Result<()> {
        loop {
            match self.listener.accept() {
                Ok((stream, _addr)) => {
                    let client_id = self.next_client_id;
                    self.next_client_id += 1;

                    // Configure client stream
                    stream
                        .set_nonblocking(false)
                        .context("Failed to set client stream to blocking mode")?;

                    let mut client = ClientConnection {
                        id: client_id,
                        stream: BufWriter::new(stream),
                    };

                    // Send current state immediately to new client
                    if let Some(ref current_state) = self.current_state {
                        let json_line = serde_json::to_string(current_state)
                            .context("Failed to serialize current state for new client")?;
                        let message = format!("{}\n", json_line);

                        if let Err(e) = client.stream.write_all(message.as_bytes()) {
                            eprintln!(
                                "Failed to send current state to client {}: {}",
                                client_id, e
                            );
                            continue;
                        }
                        if let Err(e) = client.stream.flush() {
                            eprintln!(
                                "Failed to flush current state to client {}: {}",
                                client_id, e
                            );
                            continue;
                        }
                    }

                    // Add client to our list
                    self.clients.insert(client_id, client);
                    eprintln!(
                        "New IPC client connected: {} (total: {})",
                        client_id,
                        self.clients.len()
                    );
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No new connections available, continue
                    break;
                }
                Err(e) => {
                    eprintln!("Error accepting client connection: {}", e);
                    break;
                }
            }
        }
        Ok(())
    }

    /// Remove disconnected clients by attempting to write to them.
    fn prune_clients(&mut self) {
        let mut disconnected = Vec::new();

        for (client_id, client) in &mut self.clients {
            // Try to flush the stream to detect disconnected clients
            if client.stream.flush().is_err() {
                disconnected.push(*client_id);
            }
        }

        for client_id in disconnected {
            self.clients.remove(&client_id);
            eprintln!("Removed disconnected client: {}", client_id);
        }
    }

    /// Clean up server resources on shutdown.
    fn cleanup(&self) -> Result<()> {
        // Remove socket file
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)
                .with_context(|| format!("Failed to remove socket file: {:?}", self.socket_path))?;
        }
        Ok(())
    }
}

/// Get the socket path for the IPC server.
///
/// Uses the same pattern as sunsetr's lock files:
/// - Primary: `$XDG_RUNTIME_DIR/sunsetr-events.sock`
/// - Fallback: `/run/user/{uid}/sunsetr-events.sock`
pub fn socket_path() -> Result<PathBuf> {
    let runtime_dir = if let Ok(xdg_runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(xdg_runtime_dir)
    } else {
        let uid = getuid();
        PathBuf::from(format!("/run/user/{}", uid))
    };

    Ok(runtime_dir.join("sunsetr-events.sock"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_path() {
        let path = socket_path().unwrap();
        assert!(path.to_string_lossy().contains("sunsetr-events.sock"));
    }

    #[test]
    fn test_server_creation_and_cleanup() {
        let temp_dir = tempfile::tempdir().unwrap();
        let socket_path = temp_dir.path().join("test-sunsetr.sock");

        // Create server
        let server = IpcSocketServer::new(socket_path.clone()).unwrap();

        // Verify socket was created
        assert!(socket_path.exists());

        // Cleanup
        server.cleanup().unwrap();

        // Verify socket was removed
        assert!(!socket_path.exists());
    }
}
