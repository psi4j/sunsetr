//! IPC client utilities for connecting to the sunsetr process.

use anyhow::{Context, Result};
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use super::events::IpcEvent;
use super::server::socket_path;
use crate::state::display::DisplayState;

/// The IPC connection to the sunsetr process has closed.
///
/// Returned by [`IpcClient::try_receive_event`] so callers can distinguish a
/// closed connection from other errors by downcasting.
#[derive(Debug)]
pub struct ConnectionClosed;

impl std::fmt::Display for ConnectionClosed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "IPC connection closed")
    }
}

impl std::error::Error for ConnectionClosed {}

pub struct IpcClient {
    #[allow(dead_code)]
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl IpcClient {
    pub fn connect() -> Result<Self> {
        let socket_path = socket_path().context("Failed to get IPC socket path")?;

        let stream = UnixStream::connect(&socket_path).with_context(|| {
            format!(
                "Failed to connect to sunsetr IPC socket at {:?}. Is sunsetr running?",
                socket_path
            )
        })?;

        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .context("Failed to set read timeout on IPC socket")?;

        let reader_stream = stream
            .try_clone()
            .context("Failed to clone stream for reader")?;
        let reader = BufReader::new(reader_stream);

        Ok(Self { stream, reader })
    }

    /// Read the current DisplayState from the server.
    ///
    /// The server emits a StateApplied event immediately on connection, so this
    /// reads that initial event.
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

        let event: IpcEvent = serde_json::from_str(line.trim())
            .with_context(|| format!("Failed to parse IPC event JSON: {}", line.trim()))?;

        match event {
            IpcEvent::StateApplied { state } => Ok(state),
            _ => Err(anyhow::anyhow!(
                "Expected StateApplied event on connection, got: {:?}",
                event
            )),
        }
    }

    /// Try to receive the next IpcEvent from the server without blocking.
    ///
    /// Returns `Ok(None)` when no data is available yet, and a downcastable
    /// [`ConnectionClosed`] error once the server has closed the connection.
    pub fn try_receive_event(&mut self) -> Result<Option<IpcEvent>> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Err(ConnectionClosed.into()),
            Ok(_) => {
                if line.trim().is_empty() {
                    return Ok(None);
                }

                let event: IpcEvent = serde_json::from_str(line.trim())
                    .with_context(|| format!("Failed to parse IPC event JSON: {}", line.trim()))?;

                Ok(Some(event))
            }
            Err(e) => match e.kind() {
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => Ok(None),
                std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::NotFound => Err(ConnectionClosed.into()),
                _ => Err(anyhow::Error::from(e).context("Failed to receive event from IPC socket")),
            },
        }
    }

    pub fn set_nonblocking(&self, nonblocking: bool) -> Result<()> {
        self.stream
            .set_nonblocking(nonblocking)
            .context("Failed to set socket non-blocking mode")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_connection_integration() {
        match IpcClient::connect() {
            Ok(_) => {
                println!("IPC process is running. Connection test passed");
            }
            Err(e) => {
                assert!(e.to_string().contains("Failed to connect"));
                println!("IPC process not running. Expected error: {}", e);
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
