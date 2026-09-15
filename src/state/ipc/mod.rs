//! Unix socket IPC that broadcasts typed state-change events to external applications.

use anyhow::{Context, Result};
use std::sync::mpsc;

use crate::core::period::Period;
use crate::core::runtime_state::RuntimeState;
use crate::state::display::DisplayState;

pub mod client;
pub mod events;
mod server;

use events::IpcEvent;
use server::ServerMsg;

/// Sends typed events from Core to the IPC server thread.
///
/// Delivery is fire-and-forget so Core's main loop never blocks on IPC.
pub struct IpcNotifier {
    event_sender: mpsc::Sender<ServerMsg>,
}

impl IpcNotifier {
    fn new() -> (Self, mpsc::Receiver<ServerMsg>) {
        let (event_sender, event_receiver) = mpsc::channel();
        let notifier = Self { event_sender };
        (notifier, event_receiver)
    }

    fn send(&self, event: IpcEvent) {
        let _ = self.event_sender.send(ServerMsg::Event(event));
    }

    pub fn send_period_changed(&self, from: Period, to: Period) {
        let event = IpcEvent::period_changed(from, to);
        self.send(event);
    }

    pub fn send_preset_changed(
        &self,
        from: Option<String>,
        to: Option<String>,
        target_period: Period,
        target_temp: u32,
        target_gamma: f64,
    ) {
        let event = IpcEvent::preset_changed(from, to, target_period, target_temp, target_gamma);
        self.send(event);
    }

    pub fn send_config_changed(&self, target_period: Period, target_temp: u32, target_gamma: f64) {
        let event = IpcEvent::config_changed(target_period, target_temp, target_gamma);
        self.send(event);
    }

    pub fn send_state_applied(&self, runtime_state: &RuntimeState) {
        let display_state = DisplayState::new(runtime_state);
        let event = IpcEvent::state_applied(display_state);
        self.send(event);
    }
}

/// Runs the Unix socket server on a background thread, keeping IPC off Core's
/// time-critical color temperature loop.
pub struct IpcServer {
    thread_handle: Option<std::thread::JoinHandle<()>>,
    shutdown_sender: mpsc::Sender<ServerMsg>,
}

impl IpcServer {
    pub fn start(debug_enabled: bool) -> Result<(IpcNotifier, Self)> {
        let (notifier, event_receiver) = IpcNotifier::new();
        let event_sender = notifier.event_sender.clone();

        #[cfg(debug_assertions)]
        eprintln!("DEBUG: About to spawn IPC server thread");

        let shutdown_sender = event_sender.clone();

        let thread_handle = std::thread::Builder::new()
            .name("ipc-server".to_string())
            .spawn(move || {
                #[cfg(debug_assertions)]
                eprintln!("DEBUG: IPC server thread closure started");

                match Self::run(event_sender, event_receiver, debug_enabled) {
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

        Ok((
            notifier,
            Self {
                thread_handle: Some(thread_handle),
                shutdown_sender,
            },
        ))
    }

    pub fn shutdown(mut self) -> Result<()> {
        let _ = self.shutdown_sender.send(ServerMsg::Shutdown);

        if let Some(handle) = self.thread_handle.take() {
            handle
                .join()
                .map_err(|_| anyhow::anyhow!("IPC server thread panicked"))?;
        }

        Ok(())
    }

    fn run(
        event_sender: mpsc::Sender<ServerMsg>,
        event_receiver: mpsc::Receiver<ServerMsg>,
        debug_enabled: bool,
    ) -> Result<()> {
        #[cfg(debug_assertions)]
        eprintln!("DEBUG: IPC server run() starting");

        let socket_path = server::socket_path().context("Failed to get IPC socket path")?;

        debug_assert!(
            !socket_path.to_string_lossy().is_empty(),
            "IPC socket path should not be empty"
        );

        #[cfg(debug_assertions)]
        eprintln!("DEBUG: IPC socket path: {:?}", socket_path);

        #[cfg(debug_assertions)]
        eprintln!("DEBUG: Creating IPC socket server");
        let (socket_server, listener) = server::IpcSocketServer::new(socket_path)
            .context("Failed to create IPC socket server")?;

        #[cfg(debug_assertions)]
        eprintln!("DEBUG: Starting IPC socket server main loop");
        socket_server
            .run(listener, event_sender, event_receiver, debug_enabled)
            .context("IPC socket server failed")?;

        #[cfg(debug_assertions)]
        eprintln!("DEBUG: IPC socket server completed");
        Ok(())
    }
}
