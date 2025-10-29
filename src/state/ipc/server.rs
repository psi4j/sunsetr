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
use std::time::{Duration, Instant};

use crate::state::display::DisplayState;
use crate::state::ipc::events::IpcEvent;

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
    raw_stream: UnixStream,
    writer: BufWriter<UnixStream>,
    connected_at: Instant,
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
    /// * `event_receiver` - Channel to receive IpcEvent updates from Core
    /// * `running` - Atomic flag indicating if the server should continue running
    /// * `debug_enabled` - Whether to show debug logging
    pub fn run(
        mut self,
        event_receiver: mpsc::Receiver<IpcEvent>,
        running: Arc<AtomicBool>,
        debug_enabled: bool,
    ) -> Result<()> {
        if debug_enabled {
            log_debug!("IPC server starting on socket: {:?}", self.socket_path);
        }

        while running.load(Ordering::SeqCst) {
            // Check for new IpcEvent updates (non-blocking)
            while let Ok(event) = event_receiver.try_recv() {
                self.update_state(event, debug_enabled)?;
            }

            // Accept new client connections (non-blocking)
            self.accept(debug_enabled)?;

            // Remove disconnected clients
            self.prune_clients(debug_enabled);

            // Small delay to prevent busy-waiting
            thread::sleep(Duration::from_millis(10));
        }

        // Shutdown message when thread exits
        if debug_enabled {
            log_debug!("IPC server shutting down");
        }

        // Cleanup resources
        self.cleanup()?;
        Ok(())
    }

    /// Update the current state and broadcast event to all clients.
    fn update_state(&mut self, event: IpcEvent, debug_enabled: bool) -> Result<()> {
        // Update our current state if this is a StateApplied event
        if let IpcEvent::StateApplied { ref state } = event {
            self.current_state = Some(state.clone());
        }

        // Broadcast the event to all connected clients
        self.broadcast_event(&event, debug_enabled)
    }

    /// Broadcast an IpcEvent to all connected clients.
    fn broadcast_event(&mut self, event: &IpcEvent, debug_enabled: bool) -> Result<()> {
        // Serialize IpcEvent to JSON
        let json_line =
            serde_json::to_string(event).context("Failed to serialize IpcEvent to JSON")?;
        let message = format!("{}\n", json_line);

        // Send to all clients, marking failed ones for removal
        let mut failed_clients = Vec::new();

        for (client_id, client) in &mut self.clients {
            if client.writer.write_all(message.as_bytes()).is_err()
                || client.writer.flush().is_err()
            {
                failed_clients.push(*client_id);
            }
        }

        // Remove failed clients and log disconnections
        for client_id in failed_clients {
            if let Some(client) = self.clients.remove(&client_id)
                && debug_enabled
            {
                let duration = client.connected_at.elapsed();
                if duration.as_secs() < 2 {
                    log_debug!(
                        "IPC one-shot client served ({}ms) - connections: {}",
                        duration.as_millis(),
                        self.clients.len()
                    );
                } else {
                    log_debug!(
                        "IPC client disconnected after {}s - connections: {}",
                        duration.as_secs(),
                        self.clients.len()
                    );
                }
            }
        }

        Ok(())
    }

    /// Accept new client connections (non-blocking).
    fn accept(&mut self, debug_enabled: bool) -> Result<()> {
        loop {
            match self.listener.accept() {
                Ok((stream, _addr)) => {
                    let client_id = self.next_client_id;
                    self.next_client_id += 1;

                    // Configure client stream
                    stream
                        .set_nonblocking(true) // Keep non-blocking for disconnection detection
                        .context("Failed to set client stream to non-blocking mode")?;

                    // Clone stream for writer
                    let writer_stream = stream
                        .try_clone()
                        .context("Failed to clone stream for writer")?;

                    let mut client = ClientConnection {
                        raw_stream: stream,
                        writer: BufWriter::new(writer_stream),
                        connected_at: Instant::now(),
                    };

                    // Send current state immediately to new client as a StateApplied event
                    if let Some(ref current_state) = self.current_state {
                        let event = IpcEvent::state_applied(current_state.clone());
                        let json_line = serde_json::to_string(&event)
                            .context("Failed to serialize current state event for new client")?;
                        let message = format!("{}\n", json_line);

                        if let Err(e) = client.writer.write_all(message.as_bytes()) {
                            if debug_enabled {
                                log_debug!(
                                    "Failed to send current state to client {}: {}",
                                    client_id,
                                    e
                                );
                            }
                            continue;
                        }
                        if let Err(e) = client.writer.flush() {
                            if debug_enabled {
                                log_debug!(
                                    "Failed to flush current state to client {}: {}",
                                    client_id,
                                    e
                                );
                            }
                            continue;
                        }
                    }

                    // Add client to our list
                    self.clients.insert(client_id, client);
                    if debug_enabled {
                        log_debug!("IPC connections: {}", self.clients.len());
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No new connections available, continue
                    break;
                }
                Err(e) => {
                    if debug_enabled {
                        log_debug!("Error accepting client connection: {}", e);
                    }
                    break;
                }
            }
        }
        Ok(())
    }

    /// Remove disconnected clients by attempting to read from them.
    ///
    /// This is the idiomatic way to detect disconnections in an event-based system:
    /// when a client disconnects, read() will return 0 bytes or ECONNRESET.
    fn prune_clients(&mut self, debug_enabled: bool) {
        use std::io::Read;
        let mut disconnected = Vec::new();

        for (client_id, client) in &mut self.clients {
            // Try to read from the client socket to detect disconnections
            // For our broadcast-only protocol, clients shouldn't send data,
            // so any successful read or specific errors indicate disconnection
            let mut buffer = [0u8; 1];
            match client.raw_stream.read(&mut buffer) {
                Ok(0) => {
                    // read() returned 0 bytes = client disconnected gracefully
                    disconnected.push(*client_id);
                }
                Ok(_) => {
                    // Client sent unexpected data - this shouldn't happen in our protocol
                    // but we'll keep the connection alive
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No data available to read = connection still alive
                }
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::ConnectionReset
                        || e.kind() == std::io::ErrorKind::BrokenPipe =>
                {
                    // Client disconnected ungracefully
                    disconnected.push(*client_id);
                }
                Err(_) => {
                    // Other error - assume disconnection
                    disconnected.push(*client_id);
                }
            }
        }

        // Remove disconnected clients and log disconnections
        for client_id in disconnected {
            if let Some(client) = self.clients.remove(&client_id)
                && debug_enabled
            {
                let duration = client.connected_at.elapsed();
                if duration.as_secs() < 2 {
                    log_debug!(
                        "IPC one-shot client served ({}ms) - connections: {}",
                        duration.as_millis(),
                        self.clients.len()
                    );
                } else {
                    log_debug!(
                        "IPC client disconnected after {}s - connections: {}",
                        duration.as_secs(),
                        self.clients.len()
                    );
                }
            }
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
