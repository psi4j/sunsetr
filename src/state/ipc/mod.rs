//! Unix socket IPC that broadcasts typed state-change events to external applications.

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

/// Sends typed events from Core to the IPC server thread.
///
/// Delivery is fire-and-forget so Core's main loop never blocks on IPC.
pub struct IpcNotifier {
    event_sender: mpsc::Sender<IpcEvent>,
}

impl IpcNotifier {
    pub fn new() -> (Self, mpsc::Receiver<IpcEvent>) {
        let (event_sender, event_receiver) = mpsc::channel();
        let notifier = Self { event_sender };
        (notifier, event_receiver)
    }

    pub fn send_period_changed(&self, from: Period, to: Period) {
        let event = IpcEvent::period_changed(from, to);
        let _ = self.event_sender.send(event);
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
        let _ = self.event_sender.send(event);
    }

    pub fn send_config_changed(&self, target_period: Period, target_temp: u32, target_gamma: f64) {
        let event = IpcEvent::config_changed(target_period, target_temp, target_gamma);
        let _ = self.event_sender.send(event);
    }

    pub fn send_state_applied(&self, runtime_state: &RuntimeState) {
        let display_state = DisplayState::new(runtime_state);
        let event = IpcEvent::state_applied(display_state);
        let _ = self.event_sender.send(event);
    }
}

/// Runs the Unix socket server on a background thread, keeping IPC off Core's
/// time-critical color temperature loop.
pub struct IpcServer {
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl IpcServer {
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

    /// Waits for the server thread to finish. The thread stops only when the
    /// signal handler clears the running flag.
    pub fn shutdown(mut self) -> Result<()> {
        if let Some(handle) = self.thread_handle.take() {
            handle
                .join()
                .map_err(|_| anyhow::anyhow!("IPC server thread panicked"))?;
        }

        Ok(())
    }

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

        let socket_path = server::socket_path().context("Failed to get IPC socket path")?;

        debug_assert!(
            !socket_path.to_string_lossy().is_empty(),
            "IPC socket path should not be empty"
        );

        #[cfg(debug_assertions)]
        eprintln!("DEBUG: IPC socket path: {:?}", socket_path);

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
