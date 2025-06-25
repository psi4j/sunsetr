//! Hyprsunset IPC client for communicating with the hyprsunset daemon.
//!
//! This module provides the client-side implementation for communicating with
//! hyprsunset via Hyprland's IPC socket protocol. It handles all aspects of
//! daemon communication including connection management, error handling, and
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
use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::config::Config;
use crate::constants::*;
use crate::logger::Log;
use crate::time_state::{TimeState, TransitionState};

/// Error classification for retry logic.
///
/// Different types of errors require different handling strategies:
/// - Temporary: Should retry (network issues, timeouts)
/// - Permanent: Don't retry (permission denied, invalid commands)
/// - SocketGone: hyprsunset may be restarting, need recovery approach
#[derive(Debug)]
enum ErrorType {
    Temporary,  // Should retry with standard delay
    Permanent,  // Don't retry - fundamental issue
    SocketGone, // hyprsunset might be restarting - try reconnection
}

/// Client for communicating with the hyprsunset daemon via Unix socket.
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

        let user_dir = format!("{}/hypr/", runtime_dir);

        let socket_path = if let Some(his) = his_env {
            PathBuf::from(format!("{}{}/.hyprsunset.sock", user_dir, his))
        } else {
            PathBuf::from(format!("{}/.hyprsunset.sock", user_dir))
        };

        // Only log socket path if file doesn't exist (for debugging)
        if !socket_path.exists() && debug_enabled {
            Log::log_warning(&format!("Socket file doesn't exist at {:?}", socket_path));
        }

        Ok(Self {
            socket_path,
            debug_enabled,
        })
    }

    /// Send a command to hyprsunset with proper logging and retry logic.
    ///
    /// This is the main interface for sending commands to hyprsunset. It automatically
    /// handles retries, error classification, and reconnection attempts.
    ///
    /// # Arguments
    /// * `command` - Command string to send to hyprsunset
    ///
    /// # Returns
    /// - `Ok(())` if command is sent successfully
    /// - `Err` if all retry attempts fail
    pub fn send_command(&mut self, command: &str) -> Result<()> {
        // Log the command being sent with appropriate log level
        if self.debug_enabled {
            Log::log_indented(&format!("Sending command: {}", command));
        }

        self.send_command_with_retry(command, MAX_RETRIES)
    }

    /// Try to reconnect to hyprsunset if it becomes unavailable during operation.
    ///
    /// This method handles the case where hyprsunset becomes unresponsive or restarts
    /// during operation. It implements a recovery strategy with multiple attempts
    /// and appropriate delays.
    ///
    /// # Returns
    /// - `true` if reconnection is successful
    /// - `false` if all reconnection attempts fail
    fn attempt_reconnection(&mut self) -> bool {
        // Socket might be temporarily unavailable - give it time to recover
        thread::sleep(Duration::from_millis(SOCKET_RECOVERY_DELAY_MS));

        let max_attempts = 3;
        for attempt in 0..max_attempts {
            // Use non-logging version to avoid spam during reconnection attempts
            if self.test_connection_with_logging(false) {
                if self.debug_enabled {
                    Log::log_decorated("Successfully reconnected to hyprsunset");
                }
                return true;
            }

            if attempt + 1 < max_attempts {
                thread::sleep(Duration::from_millis(SOCKET_RECOVERY_DELAY_MS));
            }
        }

        if self.debug_enabled {
            Log::log_critical("Cannot reconnect to hyprsunset after multiple attempts.");
            Log::log_decorated(
                "Please check if hyprsunset is still running. You may need to restart sunsetr.",
            );
        }
        false
    }

    /// Send a command with retry logic and error classification.
    ///
    /// This method implements the core retry logic with different strategies
    /// based on error type classification. It handles temporary failures,
    /// permanent errors, and socket disconnections differently.
    ///
    /// # Arguments
    /// * `command` - Command string to send
    /// * `max_retries` - Maximum number of retry attempts
    ///
    /// # Returns
    /// Result indicating success or failure after all attempts
    fn send_command_with_retry(&mut self, command: &str, max_retries: u32) -> Result<()> {
        // Try multiple attempts with error classification
        let mut last_error = None;

        for attempt in 0..max_retries {
            // Try to send the command
            match self.try_send_command(command) {
                Ok(_) => {
                    // Success - log if this required retries
                    if attempt > 0 && self.debug_enabled {
                        Log::log_decorated(&format!(
                            "Command succeeded on attempt {}/{}",
                            attempt + 1,
                            max_retries
                        ));
                    }
                    return Ok(());
                }
                Err(e) => {
                    // Determine error type to decide whether to retry
                    let error_type = classify_error(&e);
                    last_error = Some(e);

                    match error_type {
                        ErrorType::Temporary => {
                            // Temporary error, retry after standard delay
                            if self.debug_enabled {
                                Log::log_error(&format!(
                                    "Temporary error on attempt {}/{}: {}",
                                    attempt + 1,
                                    max_retries,
                                    last_error.as_ref().unwrap()
                                ));
                            }
                            thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
                        }
                        ErrorType::SocketGone => {
                            // Socket disappeared, possible hyprsunset restart
                            if self.debug_enabled {
                                Log::log_warning(
                                    "hyprsunset appears to be unavailable. Attempting to reconnect...",
                                );
                                Log::log_indented(
                                    "This might happen if hyprsunset was restarted or crashed.",
                                );
                                Log::log_indented("Waiting for hyprsunset to become available...");
                            }

                            // Wait for hyprsunset to restart
                            thread::sleep(Duration::from_millis(SOCKET_RECOVERY_DELAY_MS));

                            // Attempt to connect again
                            if attempt + 1 < max_retries && self.debug_enabled {
                                Log::log_indented(&format!(
                                    "Retrying connection (attempt {}/{})",
                                    attempt + 2,
                                    max_retries
                                ));
                            }
                        }
                        ErrorType::Permanent => {
                            // Permanent error, no sense in retrying
                            if self.debug_enabled {
                                Log::log_decorated(&format!(
                                    "Command failed with permanent error: {}",
                                    last_error.as_ref().unwrap()
                                ));
                            }
                            break;
                        }
                    }
                }
            }
        }

        // All attempts failed, try one final reconnection
        if self.debug_enabled {
            Log::log_warning(&format!(
                "Command '{}' failed after {} attempts. Checking if hyprsunset is still available...",
                command, max_retries
            ));
        }

        if self.attempt_reconnection() {
            // Successfully reconnected, try the command one more time
            if self.debug_enabled {
                Log::log_decorated("Retrying command after successful reconnection...");
            }

            match self.try_send_command(command) {
                Ok(_) => {
                    if self.debug_enabled {
                        Log::log_decorated("Command succeeded after reconnection!");
                    }
                    return Ok(());
                }
                Err(e) => {
                    if self.debug_enabled {
                        Log::log_critical(&format!(
                            "Command still failed after reconnection: {}",
                            e
                        ));
                    }
                    last_error = Some(e);
                }
            }
        }

        // Return the last error with context
        Err(last_error.unwrap().context(format!(
            "Failed to send command '{}' after {} attempts and reconnection attempt",
            command, max_retries
        )))
    }

    /// Attempt to send a single command without retry logic.
    ///
    /// This is the low-level command sending method that handles the actual
    /// socket communication. It connects, sends the command, attempts to read
    /// a response, and handles cleanup.
    ///
    /// # Arguments
    /// * `command` - Command string to send
    ///
    /// # Returns
    /// Result indicating success or the specific failure
    fn try_send_command(&mut self, command: &str) -> Result<()> {
        // Connect to socket
        let mut stream = UnixStream::connect(&self.socket_path)
            .with_context(|| format!("Failed to connect to socket at {:?}", self.socket_path))?;

        // Set a reasonable timeout, but don't fail if we can't set it
        stream
            .set_read_timeout(Some(Duration::from_millis(SOCKET_TIMEOUT_MS)))
            .ok();

        // Send the command
        stream
            .write_all(command.as_bytes())
            .context("Failed to write command to socket")?;

        // Try to read a response, but don't fail if we can't get one
        // hyprsunset may close connections without sending responses
        let mut buffer = [0; SOCKET_BUFFER_SIZE];
        if let Ok(bytes_read) = stream.read(&mut buffer) {
            if bytes_read > 0 {
                let response = String::from_utf8_lossy(&buffer[0..bytes_read]);
                if self.debug_enabled {
                    Log::log_indented(&format!("Response: {}", response.trim()));
                }
            } else if self.debug_enabled {
                Log::log_indented("Connection closed without response");
            }
        }

        // Explicitly close the stream for cleanup
        drop(stream);
        Ok(())
    }

    /// Test connection to hyprsunset socket without sending commands.
    ///
    /// This method provides a non-intrusive way to check if hyprsunset is
    /// responsive. It's used for startup verification and reconnection logic.
    ///
    /// # Returns
    /// - `true` if connection test succeeds
    /// - `false` if connection test fails
    pub fn test_connection(&mut self) -> bool {
        self.test_connection_with_logging(true)
    }

    /// Test connection with optional logging control.
    ///
    /// This allows callers to test the connection without generating log output,
    /// which is useful when multiple connection tests happen during initialization.
    pub fn test_connection_with_logging(&mut self, enable_logging: bool) -> bool {
        // Check if socket file exists first
        if !self.socket_path.exists() {
            return false;
        }

        // Try to connect to the socket without sending any command
        match UnixStream::connect(&self.socket_path) {
            Ok(_) => {
                if self.debug_enabled && enable_logging {
                    Log::log_debug("Successfully connected to hyprsunset socket");
                }
                true
            }
            Err(e) => {
                if self.debug_enabled && enable_logging {
                    Log::log_pipe();
                    Log::log_decorated(&format!("Failed to connect to hyprsunset: {}", e));
                }
                false
            }
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
    /// * `state` - TimeState::Day or TimeState::Night
    /// * `config` - Configuration containing temperature and gamma values
    /// * `running` - Atomic flag to check for shutdown requests
    ///
    /// # Returns
    /// Ok(()) if commands succeed, Err if both commands fail
    pub fn apply_state(
        &mut self,
        state: TimeState,
        config: &Config,
        running: &AtomicBool,
    ) -> Result<()> {
        // Don't try to apply state if we're shutting down
        if !running.load(Ordering::SeqCst) {
            if self.debug_enabled {
                Log::log_pipe();
                Log::log_info("Skipping state application during shutdown");
            }
            return Ok(());
        }

        match state {
            TimeState::Day => {
                // Execute temperature command with configured day temperature
                let day_temp = config.day_temp.unwrap_or(DEFAULT_DAY_TEMP);
                if self.debug_enabled {
                    Log::log_pipe();
                    Log::log_debug(&format!("Setting temperature to {}K...", day_temp));
                }
                let temp_success = self.run_temperature_command(day_temp);

                // Add delay between commands to prevent conflicts
                thread::sleep(Duration::from_millis(COMMAND_DELAY_MS));

                // Execute gamma command
                let day_gamma = config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA);
                if self.debug_enabled {
                    Log::log_debug(&format!("Setting gamma to {:.1}%...", day_gamma));
                }
                let gamma_success = self.run_gamma_command(day_gamma);

                // Result handling - consider partial success acceptable
                match (temp_success, gamma_success) {
                    (true, true) => Ok(()),
                    (true, false) => {
                        if self.debug_enabled {
                            Log::log_warning("Partial success: temperature applied, gamma failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, true) => {
                        if self.debug_enabled {
                            Log::log_warning("Partial success: gamma applied, temperature failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, false) => {
                        // Log the error and then return it
                        let error_msg = "Both temperature and gamma commands failed";
                        if self.debug_enabled {
                            Log::log_error(error_msg);
                        }
                        Err(anyhow::anyhow!(error_msg))
                    }
                }
            }
            TimeState::Night => {
                // Execute temperature command
                let night_temp = config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP);
                if self.debug_enabled {
                    Log::log_pipe();
                    Log::log_debug(&format!("Setting temperature to {}K...", night_temp));
                }
                let temp_success = self.run_temperature_command(night_temp);

                // Add delay between commands to prevent conflicts
                thread::sleep(Duration::from_millis(COMMAND_DELAY_MS));

                // Execute gamma command
                let night_gamma = config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA);
                if self.debug_enabled {
                    Log::log_debug(&format!("Setting gamma to {:.1}%...", night_gamma));
                }
                let gamma_success = self.run_gamma_command(night_gamma);

                // Result handling - consider partial success acceptable
                match (temp_success, gamma_success) {
                    (true, true) => Ok(()),
                    (true, false) => {
                        if self.debug_enabled {
                            Log::log_warning("Partial success: temperature applied, gamma failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, true) => {
                        if self.debug_enabled {
                            Log::log_warning("Partial success: gamma applied, temperature failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, false) => {
                        // Log the error and then return it
                        let error_msg = "Both temperature and gamma commands failed";
                        if self.debug_enabled {
                            Log::log_error(error_msg);
                        }
                        Err(anyhow::anyhow!(error_msg))
                    }
                }
            }
        }
    }

    /// Apply transition state with interpolated values for smooth color changes.
    ///
    /// This method handles both stable and transitioning states:
    /// - Stable states: Delegates to apply_state() with mode announcement
    /// - Transitioning states: Calculates interpolated temperature and gamma values
    ///   based on transition progress and applies them smoothly
    ///
    /// # Arguments
    /// * `state` - TransitionState (stable or transitioning with progress)
    /// * `config` - Configuration for temperature and gamma ranges
    /// * `running` - Atomic flag to check for shutdown requests
    ///
    /// # Returns
    /// Ok(()) if commands succeed, Err if both commands fail
    pub fn apply_transition_state(
        &mut self,
        state: TransitionState,
        config: &Config,
        running: &AtomicBool,
    ) -> Result<()> {
        if !running.load(Ordering::SeqCst) {
            if self.debug_enabled {
                Log::log_decorated("Skipping state application during shutdown");
            }
            return Ok(());
        }

        match state {
            TransitionState::Stable(time_state) => {
                // Use existing apply_state method for stable periods
                self.apply_state(time_state, config, running)
            }
            TransitionState::Transitioning { from, to, progress } => {
                // Calculate interpolated values based on transition progress
                let current_temp =
                    crate::time_state::calculate_interpolated_temp(from, to, progress, config);
                let current_gamma =
                    crate::time_state::calculate_interpolated_gamma(from, to, progress, config);

                // Apply temperature command with progress-based value
                if self.debug_enabled {
                    Log::log_pipe();
                    Log::log_debug(&format!("Setting temperature to {}K...", current_temp));
                }
                let temp_success = self.run_temperature_command(current_temp);

                // Add delay between commands to prevent conflicts
                thread::sleep(Duration::from_millis(COMMAND_DELAY_MS));

                // Apply gamma command with progress-based value
                if self.debug_enabled {
                    Log::log_debug(&format!("Setting gamma to {:.1}%...", current_gamma));
                }
                let gamma_success = self.run_gamma_command(current_gamma);

                // Result handling - consider partial success acceptable
                match (temp_success, gamma_success) {
                    (true, true) => Ok(()),
                    (true, false) => {
                        if self.debug_enabled {
                            Log::log_warning("Partial success: temperature applied, gamma failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, true) => {
                        if self.debug_enabled {
                            Log::log_warning("Partial success: gamma applied, temperature failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, false) => {
                        // Log the error and then return it
                        let error_msg = "Both temperature and gamma commands failed";
                        if self.debug_enabled {
                            Log::log_error(error_msg);
                        }
                        Err(anyhow::anyhow!(error_msg))
                    }
                }
            }
        }
    }

    /// Helper method for sending temperature commands.
    ///
    /// Wraps the temperature value in the appropriate command format
    /// and handles error logging.
    ///
    /// # Arguments
    /// * `temp` - Temperature value in Kelvin
    ///
    /// # Returns
    /// `true` if command succeeds, `false` if it fails
    fn run_temperature_command(&mut self, temp: u32) -> bool {
        let temp_cmd = format!("temperature {}", temp);
        match self.send_command(&temp_cmd) {
            Ok(_) => true,
            Err(e) => {
                if self.debug_enabled {
                    Log::log_indented(&format!("Error setting temperature: {}", e));
                }
                false
            }
        }
    }

    /// Helper method for sending gamma commands.
    ///
    /// Wraps the gamma value in the appropriate command format
    /// and handles error logging.
    ///
    /// # Arguments
    /// * `gamma` - Gamma value as percentage (0.0 to 100.0)
    ///
    /// # Returns
    /// `true` if command succeeds, `false` if it fails
    fn run_gamma_command(&mut self, gamma: f32) -> bool {
        let gamma_cmd = format!("gamma {}", gamma);
        match self.send_command(&gamma_cmd) {
            Ok(_) => true,
            Err(e) => {
                if self.debug_enabled {
                    Log::log_indented(&format!("Error setting gamma: {}", e));
                }
                false
            }
        }
    }

    /// Apply transition state specifically for startup scenarios
    /// This announces the mode first, then applies the state
    pub fn apply_startup_state(
        &mut self,
        state: TransitionState,
        config: &Config,
        running: &AtomicBool,
    ) -> Result<()> {
        if !running.load(Ordering::SeqCst) {
            if self.debug_enabled {
                Log::log_decorated("Skipping state application during shutdown");
            }
            return Ok(());
        }

        // First announce what mode we're entering (regardless of debug mode)
        crate::time_state::log_state_announcement(state);

        // Add spacing for transitioning states
        if matches!(state, TransitionState::Transitioning { .. }) {
            Log::log_pipe();
        }

        // Add debug logging if enabled
        if self.debug_enabled {
            // Log::log_pipe();
        }

        // Then apply the state (this will handle the actual commands and logging)
        match state {
            TransitionState::Stable(time_state) => {
                // For stable states, use apply_state but skip the mode announcement since we already did it
                self.apply_state(time_state, config, running)
            }
            TransitionState::Transitioning { from, to, progress } => {
                // For transitioning states, apply the interpolated values directly
                // Calculate interpolated values
                let current_temp =
                    crate::time_state::calculate_interpolated_temp(from, to, progress, config);
                let current_gamma =
                    crate::time_state::calculate_interpolated_gamma(from, to, progress, config);

                // Temperature command with logging
                if self.debug_enabled {
                    Log::log_pipe();
                    Log::log_debug(&format!("Setting temperature to {}K...", current_temp));
                }
                let temp_success = self.run_temperature_command(current_temp);

                // Add delay between commands
                thread::sleep(Duration::from_millis(COMMAND_DELAY_MS));

                // Gamma command with logging
                if self.debug_enabled {
                    Log::log_debug(&format!("Setting gamma to {:.1}%...", current_gamma));
                }
                let gamma_success = self.run_gamma_command(current_gamma);

                // Add pipe at the end
                if self.debug_enabled {
                    Log::log_pipe();
                }

                // Result handling
                match (temp_success, gamma_success) {
                    (true, true) => Ok(()),
                    (true, false) => {
                        if self.debug_enabled {
                            Log::log_pipe();
                            Log::log_warning("Partial success: temperature applied, gamma failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, true) => {
                        if self.debug_enabled {
                            Log::log_pipe();
                            Log::log_warning("Partial success: gamma applied, temperature failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, false) => {
                        // Log the error and then return it
                        let error_msg = "Both temperature and gamma commands failed";
                        if self.debug_enabled {
                            Log::log_pipe();
                            Log::log_error(error_msg);
                        }
                        Err(anyhow::anyhow!(error_msg))
                    }
                }
            }
        }
    }

    /// Apply specific temperature and gamma values directly.
    ///
    /// This method applies exact temperature and gamma values, bypassing
    /// the normal state-based logic. It's used for fine-grained control
    /// during animations like startup transitions. The commands are sent
    /// sequentially with a small delay between them to prevent conflicts.
    ///
    /// # Arguments
    /// * `temperature` - Color temperature in Kelvin (1000-20000)
    /// * `gamma` - Gamma value as percentage (0.0-100.0)
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
        // Debug logging for reload investigation
        #[cfg(debug_assertions)]
        eprintln!(
            "DEBUG: HyprsunsetClient::apply_temperature_gamma({}, {}) called",
            temperature, gamma
        );

        // Check if we should continue before applying changes
        if !running.load(Ordering::SeqCst) {
            return Ok(());
        }

        // Apply temperature
        let temp_command = format!("temperature {}", temperature);

        #[cfg(debug_assertions)]
        eprintln!("DEBUG: Sending command to hyprsunset: '{}'", temp_command);

        self.send_command(&temp_command)?;

        // Small delay between commands to prevent conflicts
        thread::sleep(Duration::from_millis(COMMAND_DELAY_MS));

        // Check again before second command
        if !running.load(Ordering::SeqCst) {
            return Ok(());
        }

        // Apply gamma
        let gamma_command = format!("gamma {}", gamma);

        #[cfg(debug_assertions)]
        eprintln!("DEBUG: Sending command to hyprsunset: '{}'", gamma_command);

        self.send_command(&gamma_command)?;

        #[cfg(debug_assertions)]
        eprintln!(
            "DEBUG: HyprsunsetClient::apply_temperature_gamma({}, {}) completed successfully",
            temperature, gamma
        );

        Ok(())
    }
}

/// Classify errors to determine appropriate retry strategy.
///
/// This function analyzes error messages and types to categorize them into:
/// - Temporary errors: Should be retried (timeouts, temporary failures)
/// - Permanent errors: Should not be retried (permission denied, invalid commands)
/// - Socket gone errors: Indicate hyprsunset may be restarting (connection refused, broken pipe)
///
/// # Arguments
/// * `error` - The error to classify
///
/// # Returns
/// ErrorType indicating the recommended handling strategy
fn classify_error(error: &anyhow::Error) -> ErrorType {
    let error_string = error.to_string().to_lowercase();

    // Check for connection-related errors that might indicate hyprsunset restart
    if error_string.contains("connection refused")
        || error_string.contains("no such file or directory")
        || error_string.contains("broken pipe")
    {
        return ErrorType::SocketGone;
    }

    // Check for permanent errors we shouldn't retry
    if error_string.contains("permission denied")
        || error_string.contains("invalid command")
        || error_string.contains("not supported")
    {
        return ErrorType::Permanent;
    }

    // Check the underlying IO error if available for more specific classification
    if let Some(io_error) = error.downcast_ref::<std::io::Error>() {
        match io_error.kind() {
            // Socket/connection issues - hyprsunset might be restarting
            ErrorKind::ConnectionRefused
            | ErrorKind::NotFound
            | ErrorKind::BrokenPipe
            | ErrorKind::ConnectionAborted => ErrorType::SocketGone,

            // Permanent issues - don't retry
            ErrorKind::PermissionDenied | ErrorKind::InvalidInput => ErrorType::Permanent,

            // Temporary issues - safe to retry
            ErrorKind::TimedOut
            | ErrorKind::WouldBlock
            | ErrorKind::Interrupted
            | ErrorKind::UnexpectedEof => ErrorType::Temporary,

            _ => ErrorType::Temporary, // Default to temporary for unknown IO errors
        }
    } else {
        // For non-IO errors, default to temporary and let retry logic handle it
        ErrorType::Temporary
    }
}
