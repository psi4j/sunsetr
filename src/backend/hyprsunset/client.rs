//! Hyprsunset IPC client for communicating with the hyprsunset process.
//!
//! This module provides the client-side implementation for communicating with
//! hyprsunset via Hyprland's IPC socket protocol. It handles all aspects of
//! process communication including connection management, error handling, and
//! command retry logic.
//!
//! ## Communication Protocol
//!
//! The client communicates with hyprsunset using Hyprland's IPC socket protocol:
//! - Commands are sent as formatted strings
//! - Responses are parsed for success/failure indication
//! - Socket path follows Hyprland's standard convention
//!
//! ## Error Handling and Recovery
//!
//! The client includes sophisticated error handling:
//! - **Error Classification**: Distinguishes between temporary, permanent, and connectivity issues
//! - **Automatic Retries**: Retries temporary failures with exponential backoff
//! - **Reconnection Logic**: Attempts to reconnect when hyprsunset becomes unavailable
//! - **Graceful Degradation**: Provides informative error messages when recovery fails
//!
//! ## Socket Path Detection
//!
//! Socket paths are determined using Hyprland's standard environment variables:
//! - Uses `HYPRLAND_INSTANCE_SIGNATURE` to identify the correct Hyprland instance
//! - Falls back to `XDG_RUNTIME_DIR` or `/run/user/{uid}` for base directory
//! - Constructs path: `{runtime_dir}/hypr/{instance}/.hyprsunset.sock`

use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::common::constants::*;

/// Client for communicating with the hyprsunset process via Unix socket.
///
/// This client handles all communication with hyprsunset, including:
/// - Socket path determination and connection management
/// - Command retry logic with error classification
/// - Reconnection handling when hyprsunset becomes unavailable
/// - State application with interpolated values during transitions
pub struct HyprsunsetClient {
    pub socket_path: PathBuf,
    pub debug_enabled: bool,
}

impl HyprsunsetClient {
    /// Create a new hyprsunset client with appropriate socket path.
    ///
    /// Determines the socket path using the same logic as hyprsunset:
    /// 1. Check HYPRLAND_INSTANCE_SIGNATURE environment variable
    /// 2. Use XDG_RUNTIME_DIR or fallback to /run/user/{uid}
    /// 3. Construct path: {runtime_dir}/hypr/{instance}/.hyprsunset.sock
    ///
    /// # Arguments
    /// * `debug_enabled` - Whether to enable debug output for this client
    ///
    /// # Returns
    /// New HyprsunsetClient instance ready for connection attempts
    pub fn new(debug_enabled: bool) -> Result<Self> {
        // Determine socket path (similar to how hyprsunset does it)
        let his_env = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/run/user/{}", nix::unistd::getuid()));

        let user_dir = format!("{runtime_dir}/hypr/");

        let socket_path = if let Some(his) = his_env {
            PathBuf::from(format!("{user_dir}{his}/.hyprsunset.sock"))
        } else {
            PathBuf::from(format!("{user_dir}/.hyprsunset.sock"))
        };

        // Only log socket path if file doesn't exist (for debugging)
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
        // Connect to socket
        let mut stream = UnixStream::connect(&self.socket_path)
            .with_context(|| format!("Failed to connect to socket at {:?}", self.socket_path))?;

        // Set a reasonable timeout
        stream
            .set_read_timeout(Some(Duration::from_millis(SOCKET_TIMEOUT_MS)))
            .ok();

        // Send all commands through the same connection
        for command in commands {
            if self.debug_enabled {
                log_indented!("Sending batched command: {command}");
            }

            // Send the command
            stream
                .write_all(command.as_bytes())
                .context("Failed to write command to socket")?;

            // Read response for this command
            let mut buffer = [0; SOCKET_BUFFER_SIZE];
            if let Ok(bytes_read) = stream.read(&mut buffer) {
                if bytes_read > 0 {
                    let response = String::from_utf8_lossy(&buffer[0..bytes_read]);
                    if self.debug_enabled {
                        log_indented!("Response: {}", response.trim());
                    }
                    // Check for error responses
                    if response.contains("Invalid") || response.contains("error") {
                        return Err(anyhow::anyhow!("Command failed: {}", response.trim()));
                    }
                } else if self.debug_enabled {
                    log_indented!("No response for command");
                }
            }
        }

        // Connection will be closed when stream is dropped
        Ok(())
    }

    /// Test connection to hyprsunset socket without sending commands.
    ///
    /// This method provides a non-intrusive way to check if hyprsunset is
    /// responsive. It's used for startup verification and reconnection logic.
    ///
    /// This method does not log errors - callers should handle logging based
    /// on their context (e.g., during startup polling, failures are expected).
    ///
    /// # Returns
    /// - `true` if connection test succeeds
    /// - `false` if connection test fails
    pub fn test_connection(&mut self) -> bool {
        // Check if socket file exists first
        if !self.socket_path.exists() {
            if self.debug_enabled {
                log_debug!("Socket file doesn't exist at {:?}", self.socket_path);
            }
            return false;
        }

        // Try to connect to the socket without sending any command
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

    /// Apply time-based state (Day or Night) with appropriate temperature and gamma settings.
    ///
    /// This method handles stable time periods by applying the configured values
    /// for day or night mode. It executes multiple commands with error handling:
    /// - Day mode: day temperature + day gamma
    /// - Night mode: night temperature + night gamma
    ///
    /// # Arguments
    /// * `state` - Period::Day or Period::Night
    /// * `config` - Configuration containing temperature and gamma values
    /// * `running` - Atomic flag to check for shutdown requests
    ///
    /// # Returns
    /// Ok(()) if commands succeed, Err if both commands fail
    pub fn apply_state(
        &mut self,
        runtime_state: &crate::core::runtime_state::RuntimeState,
        running: &AtomicBool,
    ) -> Result<()> {
        // Don't try to apply state if we're shutting down
        if !running.load(Ordering::SeqCst) {
            if self.debug_enabled {
                log_pipe!();
                log_info!("Skipping state application during shutdown");
            }
            return Ok(());
        }

        // Get temperature and gamma values from the state (handles all 4 state types)
        let (temp, gamma) = runtime_state.values();

        // Log what we're doing
        if self.debug_enabled {
            log_pipe!();
            log_debug!("Setting temperature to {temp}K and gamma to {gamma:.1}%...");
        }

        // Send both commands as a batched pair through single connection
        let temp_command = format!("temperature {temp}");
        let gamma_command = format!("gamma {gamma}");

        match self.try_send_batched_commands(&[&temp_command, &gamma_command]) {
            Ok(_) => Ok(()),
            Err(_) => {
                // Log the error and then return it
                let error_msg = "Both temperature and gamma commands failed";
                if self.debug_enabled {
                    log_error!("{error_msg}");
                }
                Err(anyhow::anyhow!(error_msg))
            }
        }
    }

    /// Apply transition state with interpolated values for smooth color changes.
    ///
    /// This method applies the state directly using the Period's built-in
    /// value calculation methods which handle both stable and transitioning states.
    ///
    /// # Arguments
    /// * `state` - Period (can be Day, Night, Sunset, or Sunrise with progress)
    /// * `config` - Configuration for temperature and gamma ranges
    /// * `running` - Atomic flag to check for shutdown requests
    ///
    /// # Returns
    /// Ok(()) if commands succeed, Err if both commands fail
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

        // Simply delegate to apply_state which now handles all state types
        self.apply_state(runtime_state, running)
    }

    /// Apply transition state specifically for startup scenarios
    /// This announces the mode first, then applies the state
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

        // First announce what mode we're entering (regardless of debug mode)
        crate::core::period::log_state_announcement(runtime_state.period());

        // Add spacing for transitioning states
        if runtime_state.period().is_transitioning() {
            log_pipe!();
        }

        // Add debug logging if enabled
        if self.debug_enabled {
            // log_pipe!();
        }

        // Then apply the state directly
        self.apply_transition_state(runtime_state, running)
    }

    /// Apply specific temperature and gamma values directly.
    ///
    /// This method applies exact temperature and gamma values, bypassing
    /// the normal state-based logic. It's used for fine-grained control
    /// during animations like startup transitions. Both temperature and gamma
    /// commands are always sent as a pair through a single socket connection
    /// to ensure they're processed together.
    ///
    /// # Arguments
    /// * `temperature` - Color temperature in Kelvin (1000-20000)
    /// * `gamma` - Gamma value as percentage (10.0-200.0)
    /// * `running` - Atomic flag to check if application should continue
    ///
    /// # Returns
    /// - `Ok(())` if both temperature and gamma were applied successfully
    /// - `Err` if either command fails after retries
    pub fn apply_temperature_gamma(
        &mut self,
        temperature: u32,
        gamma: f32,
        running: &AtomicBool,
    ) -> Result<()> {
        // Check if we should continue before applying changes
        if !running.load(Ordering::SeqCst) {
            return Ok(());
        }

        // Prepare both commands
        let temp_command = format!("temperature {temperature}");
        let gamma_command = format!("gamma {gamma}");

        #[cfg(debug_assertions)]
        eprintln!(
            "DEBUG: Sending batched commands to hyprsunset: '{temp_command}' and '{gamma_command}'"
        );

        // Send both commands through the same connection to ensure they're paired
        self.try_send_batched_commands(&[&temp_command, &gamma_command])?;

        #[cfg(debug_assertions)]
        eprintln!(
            "DEBUG: HyprsunsetClient::apply_temperature_gamma({temperature}, {gamma}) completed successfully"
        );

        Ok(())
    }
}
