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
use crate::utils::{interpolate_f32, interpolate_u32};

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
}

impl HyprsunsetClient {
    /// Create a new hyprsunset client with appropriate socket path.
    /// 
    /// Determines the socket path using the same logic as hyprsunset:
    /// 1. Check HYPRLAND_INSTANCE_SIGNATURE environment variable
    /// 2. Use XDG_RUNTIME_DIR or fallback to /run/user/{uid}
    /// 3. Construct path: {runtime_dir}/hypr/{instance}/.hyprsunset.sock
    /// 
    /// # Returns
    /// New HyprsunsetClient instance ready for connection attempts
    pub fn new() -> Result<Self> {
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
        if !socket_path.exists() {
            Log::log_warning(&format!("Socket file doesn't exist at {:?}", socket_path));
        }

        Ok(Self { socket_path })
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
        if Log::is_enabled() {
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
            if self.test_connection() {
                if Log::is_enabled() {
                    Log::log_decorated("Successfully reconnected to hyprsunset");
                }
                return true;
            }

            if attempt + 1 < max_attempts {
                thread::sleep(Duration::from_millis(SOCKET_RECOVERY_DELAY_MS));
            }
        }

        if Log::is_enabled() {
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
                    if attempt > 0 && Log::is_enabled() {
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
                            if Log::is_enabled() {
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
                            if Log::is_enabled() {
                                Log::log_warning(
                                    "hyprsunset appears to be unavailable. Attempting to reconnect...",
                                );
                                Log::log_indented(
                                    "This might happen if hyprsunset was restarted or crashed.",
                                );
                                Log::log_indented("Waiting for hyprsunset to become available...");
                            }

                            // Back off for a longer time to allow hyprsunset to restart
                            thread::sleep(Duration::from_millis(SOCKET_RECOVERY_DELAY_MS));

                            // Attempt to connect again
                            if attempt + 1 < max_retries && Log::is_enabled() {
                                Log::log_indented(&format!(
                                    "Retrying connection (attempt {}/{})",
                                    attempt + 2,
                                    max_retries
                                ));
                            }
                        }
                        ErrorType::Permanent => {
                            // Permanent error, no sense in retrying
                            if Log::is_enabled() {
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
        if Log::is_enabled() {
            Log::log_warning(&format!(
                "Command '{}' failed after {} attempts. Checking if hyprsunset is still available...",
                command, max_retries
            ));
        }

        if self.attempt_reconnection() {
            // Successfully reconnected, try the command one more time
            if Log::is_enabled() {
                Log::log_decorated("Retrying command after successful reconnection...");
            }

            match self.try_send_command(command) {
                Ok(_) => {
                    if Log::is_enabled() {
                        Log::log_decorated("Command succeeded after reconnection!");
                    }
                    return Ok(());
                }
                Err(e) => {
                    if Log::is_enabled() {
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
                if Log::is_enabled() {
                    Log::log_indented(&format!("Response: {}", response.trim()));
                }
            } else if Log::is_enabled() {
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
        // Check if socket file exists first
        if !self.socket_path.exists() {
            return false;
        }

        // Try to connect to the socket without sending any command
        match UnixStream::connect(&self.socket_path) {
            Ok(_) => {
                if Log::is_enabled() {
                    Log::log_pipe();
                    Log::log_debug("Successfully connected to hyprsunset socket");
                }
                true
            }
            Err(e) => {
                if Log::is_enabled() {
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
    /// - Day mode: identity + day gamma
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
            if Log::is_enabled() {
                Log::log_decorated("Skipping state application during shutdown");
            }
            return Ok(());
        }

        match state {
            TimeState::Day => {
                // Execute identity command to reset color temperature
                if Log::is_enabled() {
                    Log::log_debug("Setting identity mode (natural colors)...");
                }
                let identity_success = match self.send_command("identity") {
                    Ok(_) => true,
                    Err(e) => {
                        if Log::is_enabled() {
                            Log::log_indented(&format!("Error setting identity mode: {}", e));
                        }
                        false
                    }
                };

                // Add delay between commands to prevent conflicts
                thread::sleep(Duration::from_millis(COMMAND_DELAY_MS));

                // Execute gamma command
                let day_gamma = config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA);
                if Log::is_enabled() {
                    Log::log_debug(&format!("Setting gamma to {:.1}%...", day_gamma));
                }
                let gamma_success = self.run_gamma_command(day_gamma);

                // Result handling - consider partial success acceptable
                match (identity_success, gamma_success) {
                    (true, true) => Ok(()),
                    (true, false) => {
                        if Log::is_enabled() {
                            Log::log_warning("Partial success: identity applied, gamma failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, true) => {
                        if Log::is_enabled() {
                            Log::log_warning("Partial success: gamma applied, identity failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, false) => {
                        // Log the error and then return it
                        let error_msg = "Both identity and gamma commands failed";
                        if Log::is_enabled() {
                            Log::log_error(error_msg);
                        }
                        Err(anyhow::anyhow!(error_msg))
                    }
                }
            }
            TimeState::Night => {
                // Execute temperature command
                let night_temp = config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP);
                if Log::is_enabled() {
                    Log::log_debug(&format!("Setting temperature to {}K...", night_temp));
                }
                let temp_success = self.run_temperature_command(night_temp);

                // Add delay between commands to prevent conflicts
                thread::sleep(Duration::from_millis(COMMAND_DELAY_MS));

                // Execute gamma command
                let night_gamma = config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA);
                if Log::is_enabled() {
                    Log::log_debug(&format!("Setting gamma to {:.1}%...", night_gamma));
                }
                let gamma_success = self.run_gamma_command(night_gamma);

                // Result handling - consider partial success acceptable
                match (temp_success, gamma_success) {
                    (true, true) => Ok(()),
                    (true, false) => {
                        if Log::is_enabled() {
                            Log::log_warning("Partial success: temperature applied, gamma failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, true) => {
                        if Log::is_enabled() {
                            Log::log_warning("Partial success: gamma applied, temperature failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, false) => {
                        // Log the error and then return it
                        let error_msg = "Both temperature and gamma commands failed";
                        if Log::is_enabled() {
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
            if Log::is_enabled() {
                Log::log_decorated("Skipping state application during shutdown");
            }
            return Ok(());
        }

        match state {
            TransitionState::Stable(time_state) => {
                // Announce stable mode entry with appropriate icons
                if Log::is_enabled() {
                    match time_state {
                        TimeState::Day => Log::log_block_start("Entering day mode 󰖨 "),
                        TimeState::Night => Log::log_block_start("Entering night mode  "),
                    }
                    Log::log_pipe();
                }

                // Use existing apply_state method for stable periods
                self.apply_state(time_state, config, running)
            }
            TransitionState::Transitioning { from, to, progress } => {
                // Visual spacer before commands
                if Log::is_enabled() {
                    Log::log_pipe();
                }

                // Calculate interpolated values based on transition progress
                let current_temp = Self::calculate_interpolated_temp(from, to, progress, config);
                let current_gamma = Self::calculate_interpolated_gamma(from, to, progress, config);

                // Apply temperature command with progress-based value
                if Log::is_enabled() {
                    Log::log_debug(&format!("Setting temperature to {}K...", current_temp));
                }
                let temp_success = self.run_temperature_command(current_temp);

                // Add delay between commands to prevent conflicts
                thread::sleep(Duration::from_millis(COMMAND_DELAY_MS));

                // Apply gamma command with progress-based value
                if Log::is_enabled() {
                    Log::log_debug(&format!("Setting gamma to {:.1}%...", current_gamma));
                }
                let gamma_success = self.run_gamma_command(current_gamma);

                // Visual separator after commands
                if Log::is_enabled() {
                    Log::log_pipe();
                }

                // Result handling - consider partial success acceptable
                match (temp_success, gamma_success) {
                    (true, true) => Ok(()),
                    (true, false) => {
                        if Log::is_enabled() {
                            Log::log_warning("Partial success: temperature applied, gamma failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, true) => {
                        if Log::is_enabled() {
                            Log::log_warning("Partial success: gamma applied, temperature failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, false) => {
                        // Log the error and then return it
                        let error_msg = "Both temperature and gamma commands failed";
                        if Log::is_enabled() {
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
                if Log::is_enabled() {
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
                if Log::is_enabled() {
                    Log::log_indented(&format!("Error setting gamma: {}", e));
                }
                false
            }
        }
    }

    /// Calculate interpolated temperature value during transitions.
    /// 
    /// Determines the appropriate start and end temperature values based on
    /// the transition direction and interpolates between them using the progress.
    /// 
    /// # Arguments
    /// * `from` - Starting time state (Day or Night)
    /// * `to` - Target time state (Day or Night)
    /// * `progress` - Transition progress (0.0 to 1.0)
    /// * `config` - Configuration containing temperature values
    /// 
    /// # Returns
    /// Interpolated temperature value in Kelvin
    fn calculate_interpolated_temp(
        from: TimeState,
        to: TimeState,
        progress: f32,
        config: &Config,
    ) -> u32 {
        let (start_temp, end_temp) = match (from, to) {
            (TimeState::Day, TimeState::Night) => (
                config.day_temp.unwrap_or(DEFAULT_DAY_TEMP),
                config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP),
            ),
            (TimeState::Night, TimeState::Day) => (
                config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP),
                config.day_temp.unwrap_or(DEFAULT_DAY_TEMP),
            ),
            // Handle edge cases where from == to
            (TimeState::Day, TimeState::Day) => {
                let day_temp = config.day_temp.unwrap_or(DEFAULT_DAY_TEMP);
                (day_temp, day_temp)
            }
            (TimeState::Night, TimeState::Night) => {
                let night_temp = config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP);
                (night_temp, night_temp)
            }
        };

        interpolate_u32(start_temp, end_temp, progress)
    }

    /// Calculate interpolated gamma value during transitions.
    /// 
    /// Determines the appropriate start and end gamma values based on
    /// the transition direction and interpolates between them using the progress.
    /// 
    /// # Arguments
    /// * `from` - Starting time state (Day or Night)
    /// * `to` - Target time state (Day or Night)
    /// * `progress` - Transition progress (0.0 to 1.0)
    /// * `config` - Configuration containing gamma values
    /// 
    /// # Returns
    /// Interpolated gamma value as percentage (0.0 to 100.0)
    fn calculate_interpolated_gamma(
        from: TimeState,
        to: TimeState,
        progress: f32,
        config: &Config,
    ) -> f32 {
        let (start_gamma, end_gamma) = match (from, to) {
            (TimeState::Day, TimeState::Night) => (
                config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA),
                config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA),
            ),
            (TimeState::Night, TimeState::Day) => (
                config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA),
                config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA),
            ),
            // Handle edge cases where from == to
            (TimeState::Day, TimeState::Day) => {
                let day_gamma = config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA);
                (day_gamma, day_gamma)
            }
            (TimeState::Night, TimeState::Night) => {
                let night_gamma = config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA);
                (night_gamma, night_gamma)
            }
        };

        interpolate_f32(start_gamma, end_gamma, progress)
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
            if Log::is_enabled() {
                Log::log_decorated("Skipping state application during shutdown");
            }
            return Ok(());
        }

        // First announce what mode we're entering
        if Log::is_enabled() {
            match state {
                TransitionState::Stable(time_state) => match time_state {
                    TimeState::Day => Log::log_block_start("Entering day mode 󰖨 "),
                    TimeState::Night => Log::log_block_start("Entering night mode   "),
                },
                TransitionState::Transitioning { from, to, .. } => {
                    let transition_type = match (from, to) {
                        (TimeState::Day, TimeState::Night) => "Commencing sunset 󰖛 ",
                        (TimeState::Night, TimeState::Day) => "Commencing sunrise 󰖜 ",
                        _ => "Commencing transition",
                    };
                    Log::log_block_start(transition_type);
                }
            }
            Log::log_pipe();
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
                let current_temp = Self::calculate_interpolated_temp(from, to, progress, config);
                let current_gamma = Self::calculate_interpolated_gamma(from, to, progress, config);

                // Temperature command with logging
                if Log::is_enabled() {
                    Log::log_debug(&format!("Setting temperature to {}K...", current_temp));
                }
                let temp_success = self.run_temperature_command(current_temp);

                // Add delay between commands
                thread::sleep(Duration::from_millis(COMMAND_DELAY_MS));

                // Gamma command with logging
                if Log::is_enabled() {
                    Log::log_debug(&format!("Setting gamma to {:.1}%...", current_gamma));
                }
                let gamma_success = self.run_gamma_command(current_gamma);

                // Add pipe at the end
                if Log::is_enabled() {
                    Log::log_pipe();
                }

                // Result handling
                match (temp_success, gamma_success) {
                    (true, true) => Ok(()),
                    (true, false) => {
                        if Log::is_enabled() {
                            Log::log_warning("Partial success: temperature applied, gamma failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, true) => {
                        if Log::is_enabled() {
                            Log::log_warning("Partial success: gamma applied, temperature failed");
                        }
                        Ok(()) // Consider partial success acceptable
                    }
                    (false, false) => {
                        // Log the error and then return it
                        let error_msg = "Both temperature and gamma commands failed";
                        if Log::is_enabled() {
                            Log::log_error(error_msg);
                        }
                        Err(anyhow::anyhow!(error_msg))
                    }
                }
            }
        }
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
