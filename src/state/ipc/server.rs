//! Unix socket server implementation for sunsetr IPC.

use anyhow::{Context, Result};
use nix::unistd::getuid;
use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::ops::ControlFlow;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::state::display::DisplayState;
use crate::state::ipc::events::IpcEvent;

const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);

/// Everything the server loop waits on. `Shutdown` is sent by
/// `IpcServer::shutdown` before it joins, since the accept thread holds a
/// sender and the channel never disconnects on its own.
pub(crate) enum ServerMsg {
    Event(IpcEvent),
    Client(UnixStream),
    Shutdown,
}

pub struct IpcSocketServer {
    socket_path: PathBuf,
    socket_file_id: (u64, u64),
    clients: HashMap<u32, ClientConnection>,
    next_client_id: u32,
    current_state: Option<DisplayState>,
}

struct ClientConnection {
    raw_stream: UnixStream,
    writer: BufWriter<UnixStream>,
    connected_at: Instant,
}

impl IpcSocketServer {
    pub fn new(socket_path: PathBuf) -> Result<(Self, UnixListener)> {
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)
                .with_context(|| format!("Failed to remove existing socket: {:?}", socket_path))?;
        }

        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create socket directory: {:?}", parent))?;
        }

        let listener = UnixListener::bind(&socket_path)
            .with_context(|| format!("Failed to bind Unix socket: {:?}", socket_path))?;
        let socket_file_id = file_id(&socket_path)
            .with_context(|| format!("Failed to stat Unix socket: {:?}", socket_path))?;

        Ok((
            Self {
                socket_path,
                socket_file_id,
                clients: HashMap::new(),
                next_client_id: 1,
                current_state: None,
            },
            listener,
        ))
    }

    pub fn run(
        mut self,
        listener: UnixListener,
        event_sender: mpsc::Sender<ServerMsg>,
        event_receiver: mpsc::Receiver<ServerMsg>,
        debug_enabled: bool,
    ) -> Result<()> {
        if debug_enabled {
            log_debug!("IPC server starting on socket: {:?}", self.socket_path);
        }

        Self::spawn_accept_thread(listener, event_sender, debug_enabled)?;

        'serve: loop {
            let msg = match event_receiver.recv() {
                Ok(msg) => msg,
                Err(_) => {
                    if debug_enabled {
                        log_debug!("IPC event channel disconnected");
                    }
                    break 'serve;
                }
            };

            if self.handle(msg, debug_enabled)?.is_break() {
                break 'serve;
            }
            while let Ok(msg) = event_receiver.try_recv() {
                if self.handle(msg, debug_enabled)?.is_break() {
                    break 'serve;
                }
            }

            self.prune_clients(debug_enabled);
        }

        if debug_enabled {
            log_debug!("IPC server shutting down");
        }

        self.cleanup()?;
        Ok(())
    }

    fn spawn_accept_thread(
        listener: UnixListener,
        sender: mpsc::Sender<ServerMsg>,
        debug_enabled: bool,
    ) -> Result<()> {
        std::thread::Builder::new()
            .name("ipc-accept".into())
            .spawn(move || {
                loop {
                    match listener.accept() {
                        Ok((stream, _addr)) => {
                            if sender.send(ServerMsg::Client(stream)).is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            // std retries EINTR internally, so an error here
                            // means resource exhaustion. Returning would close
                            // the listener for the rest of the process while
                            // the socket file stayed on disk.
                            if debug_enabled {
                                log_debug!("Error accepting client connection: {}", e);
                            }
                            std::thread::sleep(ACCEPT_RETRY_DELAY);
                        }
                    }
                }
            })
            .context("Failed to spawn IPC accept thread")?;
        Ok(())
    }

    fn handle(&mut self, msg: ServerMsg, debug_enabled: bool) -> Result<ControlFlow<()>> {
        match msg {
            ServerMsg::Event(event) => self.update_state(event, debug_enabled)?,
            ServerMsg::Client(stream) => self.register_client(stream, debug_enabled)?,
            ServerMsg::Shutdown => return Ok(ControlFlow::Break(())),
        }
        Ok(ControlFlow::Continue(()))
    }

    fn update_state(&mut self, event: IpcEvent, debug_enabled: bool) -> Result<()> {
        if let IpcEvent::StateApplied { ref state } = event {
            self.current_state = Some(state.clone());
        }
        self.broadcast_event(&event, debug_enabled)
    }

    fn broadcast_event(&mut self, event: &IpcEvent, debug_enabled: bool) -> Result<()> {
        let json_line =
            serde_json::to_string(event).context("Failed to serialize IpcEvent to JSON")?;
        let message = format!("{}\n", json_line);

        let mut failed_clients = Vec::new();

        for (client_id, client) in &mut self.clients {
            if client.writer.write_all(message.as_bytes()).is_err()
                || client.writer.flush().is_err()
            {
                failed_clients.push(*client_id);
            }
        }

        for client_id in failed_clients {
            if let Some(client) = self.clients.remove(&client_id)
                && debug_enabled
            {
                let duration = client.connected_at.elapsed();
                if duration.as_secs() < 2 {
                    log_debug!(
                        "IPC one-shot client served ({}ms), connections: {}",
                        duration.as_millis(),
                        self.clients.len()
                    );
                } else {
                    log_debug!(
                        "IPC client disconnected after {}s, connections: {}",
                        duration.as_secs(),
                        self.clients.len()
                    );
                }
            }
        }

        Ok(())
    }

    fn register_client(&mut self, stream: UnixStream, debug_enabled: bool) -> Result<()> {
        let client_id = self.next_client_id;
        self.next_client_id += 1;

        stream
            .set_nonblocking(true)
            .context("Failed to set client stream to non-blocking mode")?;

        let writer_stream = stream
            .try_clone()
            .context("Failed to clone stream for writer")?;

        let mut client = ClientConnection {
            raw_stream: stream,
            writer: BufWriter::new(writer_stream),
            connected_at: Instant::now(),
        };

        if let Some(ref current_state) = self.current_state {
            let event = IpcEvent::state_applied(current_state.clone());
            let json_line = serde_json::to_string(&event)
                .context("Failed to serialize current state event for new client")?;
            let message = format!("{}\n", json_line);

            if let Err(e) = client
                .writer
                .write_all(message.as_bytes())
                .and_then(|()| client.writer.flush())
            {
                if debug_enabled {
                    log_debug!(
                        "Failed to send current state to client {}: {}",
                        client_id,
                        e
                    );
                }
                return Ok(());
            }
        }

        self.clients.insert(client_id, client);
        if debug_enabled {
            log_debug!("IPC connections: {}", self.clients.len());
        }
        Ok(())
    }

    fn prune_clients(&mut self, debug_enabled: bool) {
        use std::io::Read;
        let mut disconnected = Vec::new();

        for (client_id, client) in &mut self.clients {
            let mut buffer = [0u8; 1];
            match client.raw_stream.read(&mut buffer) {
                Ok(0) => {
                    disconnected.push(*client_id);
                }
                Ok(_) => {
                    // The protocol is server-to-client only, so unexpected client
                    // data is ignored and the connection kept alive.
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // WouldBlock means no data is waiting, so the connection is still alive.
                }
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::ConnectionReset
                        || e.kind() == std::io::ErrorKind::BrokenPipe =>
                {
                    disconnected.push(*client_id);
                }
                Err(_) => {
                    disconnected.push(*client_id);
                }
            }
        }

        for client_id in disconnected {
            if let Some(client) = self.clients.remove(&client_id)
                && debug_enabled
            {
                let duration = client.connected_at.elapsed();
                if duration.as_secs() < 2 {
                    log_debug!(
                        "IPC one-shot client served ({}ms), connections: {}",
                        duration.as_millis(),
                        self.clients.len()
                    );
                } else {
                    log_debug!(
                        "IPC client disconnected after {}s, connections: {}",
                        duration.as_secs(),
                        self.clients.len()
                    );
                }
            }
        }
    }

    /// Unlinks only the file this server bound. A later instance can bind the
    /// same path between the lock release and this call.
    fn cleanup(&self) -> Result<()> {
        if file_id(&self.socket_path).ok() != Some(self.socket_file_id) {
            return Ok(());
        }
        std::fs::remove_file(&self.socket_path)
            .with_context(|| format!("Failed to remove socket file: {:?}", self.socket_path))
    }
}

fn file_id(path: &std::path::Path) -> std::io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

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

        let (server, _listener) = IpcSocketServer::new(socket_path.clone()).unwrap();

        assert!(socket_path.exists());

        server.cleanup().unwrap();

        assert!(!socket_path.exists());
    }

    #[test]
    fn cleanup_leaves_a_socket_bound_by_a_later_instance() {
        let temp_dir = tempfile::tempdir().unwrap();
        let socket_path = temp_dir.path().join("replaced.sock");

        let (old_server, _old_listener) = IpcSocketServer::new(socket_path.clone()).unwrap();
        let (_new_server, _new_listener) = IpcSocketServer::new(socket_path.clone()).unwrap();

        old_server.cleanup().unwrap();

        assert!(
            socket_path.exists(),
            "the newer instance's socket was removed"
        );
    }

    #[test]
    fn shutdown_ends_the_loop_with_no_clients() {
        let temp_dir = tempfile::tempdir().unwrap();
        let socket_path = temp_dir.path().join("shutdown-idle.sock");
        let (server, listener) = IpcSocketServer::new(socket_path.clone()).unwrap();

        let (tx, rx) = mpsc::channel();
        let sender = tx.clone();
        let handle = std::thread::spawn(move || server.run(listener, sender, rx, false));

        tx.send(ServerMsg::Shutdown).unwrap();

        let joined = join_before(handle, Duration::from_secs(5));
        assert!(joined.is_some(), "server loop did not stop on Shutdown");
        joined
            .unwrap()
            .expect("server thread panicked")
            .expect("server returned an error");
        assert!(!socket_path.exists(), "socket file was not removed");
    }

    #[test]
    fn shutdown_ends_the_loop_with_a_connected_client() {
        use std::io::{BufRead, BufReader};

        let temp_dir = tempfile::tempdir().unwrap();
        let socket_path = temp_dir.path().join("shutdown-client.sock");
        let (server, listener) = IpcSocketServer::new(socket_path.clone()).unwrap();

        let (tx, rx) = mpsc::channel();
        let sender = tx.clone();
        let handle = std::thread::spawn(move || server.run(listener, sender, rx, false));

        tx.send(ServerMsg::Event(IpcEvent::state_applied(
            sample_display_state(),
        )))
        .unwrap();

        let client = UnixStream::connect(&socket_path).expect("connect to the server");
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(client);

        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("read the registration snapshot");
        assert!(
            line.contains("state_applied"),
            "unexpected snapshot: {line}"
        );

        tx.send(ServerMsg::Shutdown).unwrap();

        let joined = join_before(handle, Duration::from_secs(5));
        assert!(
            joined.is_some(),
            "server loop did not stop on Shutdown with a client attached"
        );
        joined
            .unwrap()
            .expect("server thread panicked")
            .expect("server returned an error");
        assert!(!socket_path.exists(), "socket file was not removed");
    }

    fn sample_display_state() -> DisplayState {
        DisplayState {
            active_preset: "default".to_string(),
            period: crate::core::period::Period::Day,
            period_type: crate::core::period::PeriodType::Stable,
            progress: None,
            current_temp: 6500,
            current_gamma: 100.0,
            target_temp: None,
            target_gamma: None,
            next_period: None,
        }
    }

    /// `JoinHandle` has no timed join.
    fn join_before<T>(
        handle: std::thread::JoinHandle<T>,
        timeout: Duration,
    ) -> Option<std::thread::Result<T>> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if handle.is_finished() {
                return Some(handle.join());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }
}
