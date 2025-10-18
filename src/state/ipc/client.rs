//! IPC client utilities for connecting to the sunsetr process.
//!
//! This module provides client-side utilities for connecting to the IPC socket
//! and receiving DisplayState updates. Used by the status command and testing.

use anyhow::{Context, Result};
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use super::server::socket_path;
use crate::state::display::DisplayState;

/// IPC client for connecting to the sunsetr process.
pub struct IpcClient {
    #[allow(dead_code)]
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl IpcClient {
    /// Connect to the sunsetr IPC socket.
    ///
    /// # Returns
    /// Connected IPC client ready to receive DisplayState updates
    pub fn connect() -> Result<Self> {
        let socket_path = socket_path().context("Failed to get IPC socket path")?;

        let stream = UnixStream::connect(&socket_path).with_context(|| {
            format!(
                "Failed to connect to sunsetr IPC socket at {:?}. Is sunsetr running?",
                socket_path
            )
        })?;

        // Set read timeout to prevent hanging
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .context("Failed to set read timeout on IPC socket")?;

        // Clone stream for the reader (since BufReader takes ownership)
        let reader_stream = stream
            .try_clone()
            .context("Failed to clone stream for reader")?;
        let reader = BufReader::new(reader_stream);

        Ok(Self { stream, reader })
    }

    /// Read the current DisplayState from the server.
    ///
    /// The IPC protocol sends the current DisplayState immediately upon connection,
    /// so this method reads the current state.
    ///
    /// # Returns
    /// Current DisplayState from the running sunsetr process
    pub fn current(&mut self) -> Result<DisplayState> {
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .context("Failed to read current state from IPC socket")?;

        if line.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "Received empty response from IPC server. Check if sunsetr is running properly."
            ));
        }

        let display_state: DisplayState = serde_json::from_str(line.trim())
            .with_context(|| format!("Failed to parse DisplayState JSON: {}", line.trim()))?;

        Ok(display_state)
    }

    /// Receive the next DisplayState update from the server.
    ///
    /// This method blocks until the next state update is received.
    /// Used for implementing follow mode in the status command.
    ///
    /// # Returns
    /// Next DisplayState update from the process
    pub fn receive(&mut self) -> Result<DisplayState> {
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .context("Failed to receive state update from IPC socket")?;

        if line.trim().is_empty() {
            return Err(anyhow::anyhow!("Connection closed by server"));
        }

        let display_state: DisplayState = serde_json::from_str(line.trim())
            .with_context(|| format!("Failed to parse DisplayState JSON: {}", line.trim()))?;

        Ok(display_state)
    }

    /// Check if the sunsetr process is running.
    ///
    /// This is a quick connectivity test without maintaining a connection.
    ///
    /// # Returns
    /// `true` if the process is running, `false` otherwise
    pub fn is_running() -> bool {
        if let Ok(socket_path) = socket_path()
            && socket_path.exists()
        {
            // Try to connect briefly
            if let Ok(_stream) = UnixStream::connect(&socket_path) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_reachability() {
        // When no process is running, should return false
        assert!(!IpcClient::is_running());
    }

    #[test]
    fn test_client_connection_integration() {
        // This test would require a running IPC server
        // For now, just test that the connection attempt fails gracefully
        match IpcClient::connect() {
            Ok(_) => {
                // If connection succeeds, great! process is running
                println!("IPC process is running - connection test passed");
            }
            Err(e) => {
                // Expected when process is not running
                assert!(e.to_string().contains("Failed to connect"));
                println!("IPC process not running - expected error: {}", e);
            }
        }
    }

    #[test]
    fn test_socket_path() {
        let path = socket_path().unwrap();
        assert!(path.to_string_lossy().contains("sunsetr-events.sock"));
        assert!(
            path.to_string_lossy().contains("run/user")
                || path.to_string_lossy().contains("XDG_RUNTIME_DIR")
        );
    }
}
