//! Hyprsunset IPC client for communicating with the hyprsunset process.
//!
//! Commands are sent as formatted strings over Hyprland's IPC Unix socket, and the
//! response is checked for a success or failure indication. The socket path is derived
//! from Hyprland's environment: `HYPRLAND_INSTANCE_SIGNATURE` selects the instance and
//! `XDG_RUNTIME_DIR` (or `/run/user/{uid}`) the base directory, giving
//! `{runtime_dir}/hypr/{instance}/.hyprsunset.sock`.

use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const SOCKET_TIMEOUT_MS: u64 = 1000;
const SOCKET_BUFFER_SIZE: usize = 1024;

/// Client for communicating with the hyprsunset process via Unix socket.
///
/// Resolves the socket path, connects per command batch, and applies temperature and
/// gamma values, interpolated during transitions.
pub struct HyprsunsetClient {
    pub socket_path: PathBuf,
    pub debug_enabled: bool,
}

impl HyprsunsetClient {
    /// Create a client with the resolved socket path. Does not require hyprsunset to be
    /// running yet. The connection is attempted per command.
    pub fn new(debug_enabled: bool) -> Result<Self> {
        let his_env = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/run/user/{}", nix::unistd::getuid()));

        let user_dir = format!("{runtime_dir}/hypr/");

        let socket_path = if let Some(his) = his_env {
            PathBuf::from(format!("{user_dir}{his}/.hyprsunset.sock"))
        } else {
            PathBuf::from(format!("{user_dir}/.hyprsunset.sock"))
        };

        if !socket_path.exists() && debug_enabled {
            log_warning!("Socket file doesn't exist at {socket_path:?}");
        }

        Ok(Self {
            socket_path,
            debug_enabled,
        })
    }

    /// Send multiple commands through a single socket connection.
    /// This is used to batch temperature and gamma updates to avoid animation interruptions.
    fn try_send_batched_commands(&mut self, commands: &[&str]) -> Result<()> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .with_context(|| format!("Failed to connect to socket at {:?}", self.socket_path))?;

        stream
            .set_read_timeout(Some(Duration::from_millis(SOCKET_TIMEOUT_MS)))
            .ok();

        for command in commands {
            if self.debug_enabled {
                log_indented!("Sending batched command: {command}");
            }

            stream
                .write_all(command.as_bytes())
                .context("Failed to write command to socket")?;

            let mut buffer = [0; SOCKET_BUFFER_SIZE];
            if let Ok(bytes_read) = stream.read(&mut buffer) {
                if bytes_read > 0 {
                    let response = String::from_utf8_lossy(&buffer[0..bytes_read]);
                    if self.debug_enabled {
                        log_indented!("Response: {}", response.trim());
                    }
                    if response.contains("Invalid") || response.contains("error") {
                        return Err(anyhow::anyhow!("Command failed: {}", response.trim()));
                    }
                } else if self.debug_enabled {
                    log_indented!("No response for command");
                }
            }
        }

        Ok(())
    }

    /// Non-intrusive check of whether hyprsunset is responsive, used for startup
    /// connection polling. Does not log. Callers decide what to report, since failures
    /// while polling for the socket are expected.
    pub fn test_connection(&mut self) -> bool {
        if !self.socket_path.exists() {
            if self.debug_enabled {
                log_debug!("Socket file doesn't exist at {:?}", self.socket_path);
            }
            return false;
        }

        match UnixStream::connect(&self.socket_path) {
            Ok(_) => {
                if self.debug_enabled {
                    log_debug!("Successfully connected to hyprsunset socket");
                }
                true
            }
            Err(_) => false,
        }
    }

    /// Apply the runtime state's current temperature and gamma as a single batched
    /// command pair.
    pub fn apply_state(
        &mut self,
        runtime_state: &crate::core::runtime_state::RuntimeState,
        running: &AtomicBool,
    ) -> Result<()> {
        if !running.load(Ordering::SeqCst) {
            if self.debug_enabled {
                log_pipe!();
                log_info!("Skipping state application during shutdown");
            }
            return Ok(());
        }

        let (temp, gamma) = runtime_state.values();

        if self.debug_enabled {
            log_pipe!();
            log_debug!("Setting temperature to {temp}K and gamma to {gamma:.1}%...");
        }

        let temp_command = format!("temperature {temp}");
        let gamma_command = format!("gamma {gamma}");

        match self.try_send_batched_commands(&[&temp_command, &gamma_command]) {
            Ok(_) => Ok(()),
            Err(_) => {
                let error_msg = "Both temperature and gamma commands failed";
                if self.debug_enabled {
                    log_error!("{error_msg}");
                }
                Err(anyhow::anyhow!(error_msg))
            }
        }
    }

    /// Apply the current state, including interpolated values during transitions.
    pub fn apply_transition_state(
        &mut self,
        runtime_state: &crate::core::runtime_state::RuntimeState,
        running: &AtomicBool,
    ) -> Result<()> {
        if !running.load(Ordering::SeqCst) {
            if self.debug_enabled {
                log_decorated!("Skipping state application during shutdown");
            }
            return Ok(());
        }

        self.apply_state(runtime_state, running)
    }

    /// Announce the current period, then apply its state.
    pub fn apply_startup_state(
        &mut self,
        runtime_state: &crate::core::runtime_state::RuntimeState,
        running: &AtomicBool,
    ) -> Result<()> {
        if !running.load(Ordering::SeqCst) {
            if self.debug_enabled {
                log_decorated!("Skipping state application during shutdown");
            }
            return Ok(());
        }

        crate::core::period::log_state_announcement(runtime_state.period());

        if runtime_state.period().is_transitioning() {
            log_pipe!();
        }

        self.apply_transition_state(runtime_state, running)
    }

    /// Apply exact temperature (Kelvin, 1000-20000) and gamma (percentage, 10.0-200.0),
    /// bypassing state-based logic. Both are sent as a pair over one connection so they
    /// are processed together.
    pub fn apply_temperature_gamma(
        &mut self,
        temperature: u32,
        gamma: f64,
        running: &AtomicBool,
    ) -> Result<()> {
        if !running.load(Ordering::SeqCst) {
            return Ok(());
        }

        let temp_command = format!("temperature {temperature}");
        let gamma_command = format!("gamma {gamma}");

        #[cfg(debug_assertions)]
        eprintln!(
            "DEBUG: Sending batched commands to hyprsunset: '{temp_command}' and '{gamma_command}'"
        );

        self.try_send_batched_commands(&[&temp_command, &gamma_command])?;

        #[cfg(debug_assertions)]
        eprintln!(
            "DEBUG: HyprsunsetClient::apply_temperature_gamma({temperature}, {gamma}) completed successfully"
        );

        Ok(())
    }
}
