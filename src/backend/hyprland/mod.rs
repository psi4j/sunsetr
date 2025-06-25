//! Hyprland backend implementation using hyprsunset for gamma control.
//!
//! This module provides color temperature control specifically for the Hyprland compositor
//! by managing the hyprsunset daemon and communicating with it via Hyprland's IPC socket protocol.
//!
//! ## Architecture
//!
//! The Hyprland backend consists of two main components:
//! - **Process Management** ([`HyprsunsetProcess`]): Manages the hyprsunset daemon lifecycle
//! - **Client Communication** ([`HyprsunsetClient`]): Communicates with hyprsunset via IPC socket
//!
//! ## Process Management
//!
//! The backend can operate in two modes:
//! 1. **Managed Mode**: Starts and manages hyprsunset as a child process
//! 2. **External Mode**: Connects to an existing hyprsunset instance (e.g., from systemd service)
//!
//! The mode is determined by the `start_hyprsunset` configuration option and whether
//! an existing hyprsunset instance is detected.
//!
//! ## Communication Protocol
//!
//! The backend communicates with hyprsunset using Hyprland's IPC socket protocol.
//! Commands are sent as formatted strings and responses are parsed for success/failure
//! indication. The IPC socket path is automatically detected from Hyprland's environment.
//!
//! ## Error Handling and Recovery
//!
//! The backend includes robust error handling:
//! - Automatic reconnection attempts when the IPC connection is lost
//! - Process restart capability when hyprsunset crashes
//! - Graceful degradation when hyprsunset becomes unavailable
//! - Proper cleanup during application shutdown

use anyhow::Result;
use std::sync::atomic::AtomicBool;

use crate::backend::ColorTemperatureBackend;
use crate::config::Config;
use crate::constants::*;
use crate::logger::Log;
use crate::time_state::TransitionState;

pub mod client;
pub mod process;

pub use client::HyprsunsetClient;
pub use process::{HyprsunsetProcess, is_hyprsunset_running};

/// Hyprland backend implementation using hyprsunset for gamma control.
///
/// This backend provides color temperature control on Hyprland via the
/// hyprsunset daemon. It can either manage hyprsunset as a child process
/// or connect to an existing hyprsunset instance.
pub struct HyprlandBackend {
    client: HyprsunsetClient,
    process: Option<HyprsunsetProcess>,
}

impl HyprlandBackend {
    /// Create a new Hyprland backend instance.
    ///
    /// This function verifies hyprsunset availability, sets up process management
    /// if configured, and establishes client communication.
    ///
    /// # Arguments
    /// * `config` - Configuration containing Hyprland-specific settings
    /// * `debug_enabled` - Whether to enable debug output for this backend
    ///
    /// # Returns
    /// A new HyprlandBackend instance ready for use
    ///
    /// # Errors
    /// Returns an error if:
    /// - hyprsunset is not installed or incompatible
    /// - Process management conflicts are detected
    /// - Client initialization fails
    pub fn new(config: &Config, debug_enabled: bool) -> Result<Self> {
        // Verify hyprsunset installation and version compatibility
        verify_hyprsunset_installed_and_version()?;

        // Debug logging for reload investigation
        #[cfg(debug_assertions)]
        {
            let start_hyprsunset = config.start_hyprsunset.unwrap_or(DEFAULT_START_HYPRSUNSET);
            let hyprsunset_running = is_hyprsunset_running();
            eprintln!(
                "DEBUG: HyprlandBackend::new() - start_hyprsunset={}, is_hyprsunset_running()={}",
                start_hyprsunset, hyprsunset_running
            );
        }

        // Start hyprsunset if needed
        let process = if config.start_hyprsunset.unwrap_or(DEFAULT_START_HYPRSUNSET) {
            if is_hyprsunset_running() {
                Log::log_pipe();
                Log::log_warning(
                    "hyprsunset is already running but start_hyprsunset is enabled in config.",
                );
                Log::log_pipe();
                anyhow::bail!(
                    "This indicates a configuration conflict. Please choose one:\n\
                    • Kill the existing hyprsunset process: pkill hyprsunset\n\
                    • Change start_hyprsunset = false in sunsetr.toml\n\
                    \n\
                    Choose the first option if you want sunsetr to manage hyprsunset.\n\
                    Choose the second option if you're using another method to start hyprsunset.",
                );
            }

            // Determine initial values for hyprsunset startup
            let startup_transition = config
                .startup_transition
                .unwrap_or(DEFAULT_STARTUP_TRANSITION);
            let (temp, gamma) = if startup_transition {
                // If startup transition is enabled, start with day values
                (
                    config.day_temp.unwrap_or(DEFAULT_DAY_TEMP),
                    config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA),
                )
            } else {
                // If startup transition is disabled, start with current interpolated values
                let current_state = crate::time_state::get_transition_state(config);
                crate::time_state::get_initial_values_for_state(current_state, config)
            };

            Some(HyprsunsetProcess::new(temp, gamma, debug_enabled)?)
        } else {
            None
        };

        // Initialize hyprsunset client
        let mut client = HyprsunsetClient::new(debug_enabled)?;

        // Verify connection to hyprsunset
        verify_hyprsunset_connection(&mut client)?;

        Ok(Self { client, process })
    }

    /// Get a reference to the managed hyprsunset process, if any.
    #[allow(dead_code)]
    pub fn process(&self) -> Option<&HyprsunsetProcess> {
        self.process.as_ref()
    }

    /// Take ownership of the managed hyprsunset process, if any.
    ///
    /// This is used during cleanup to properly terminate the process.
    #[allow(dead_code)]
    pub fn take_process(self) -> Option<HyprsunsetProcess> {
        self.process
    }
}

impl ColorTemperatureBackend for HyprlandBackend {
    fn apply_transition_state(
        &mut self,
        state: TransitionState,
        config: &Config,
        running: &AtomicBool,
    ) -> Result<()> {
        self.client.apply_transition_state(state, config, running)
    }

    fn apply_startup_state(
        &mut self,
        state: TransitionState,
        config: &Config,
        running: &AtomicBool,
    ) -> Result<()> {
        // Check if we should skip redundant commands when hyprsunset was started by sunsetr
        if self.process.is_some() {
            // We started hyprsunset, so we know what values it was initialized with
            let (target_temp, target_gamma) =
                crate::time_state::get_initial_values_for_state(state, config);

            // Calculate what hyprsunset was started with using the same logic as in new()
            let startup_transition = config
                .startup_transition
                .unwrap_or(DEFAULT_STARTUP_TRANSITION);
            let (hyprsunset_init_temp, hyprsunset_init_gamma) = if startup_transition {
                // hyprsunset was started with day values
                (
                    config.day_temp.unwrap_or(DEFAULT_DAY_TEMP),
                    config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA),
                )
            } else {
                // hyprsunset was started with current interpolated values
                // Note: This uses the current state, which should be the same as when we started
                // hyprsunset moments ago, unless significant time has passed
                crate::time_state::get_initial_values_for_state(state, config)
            };

            // Check if target matches what hyprsunset was initialized with
            if target_temp == hyprsunset_init_temp && target_gamma == hyprsunset_init_gamma {
                // hyprsunset already has the correct values, just announce the mode
                crate::time_state::log_state_announcement(state);
                return Ok(());
            }
        }

        // Either we didn't start hyprsunset, or the values don't match - apply the state normally
        self.client.apply_startup_state(state, config, running)
    }

    fn apply_temperature_gamma(
        &mut self,
        temperature: u32,
        gamma: f32,
        running: &AtomicBool,
    ) -> Result<()> {
        self.client
            .apply_temperature_gamma(temperature, gamma, running)
    }

    fn backend_name(&self) -> &'static str {
        "Hyprland"
    }

    fn cleanup(self: Box<Self>, debug_enabled: bool) {
        // Stop any managed hyprsunset process
        if let Some(process) = self.process {
            if debug_enabled {
                Log::log_decorated("Stopping managed hyprsunset process...");
            }
            match process.stop(debug_enabled) {
                Ok(_) => {
                    if debug_enabled {
                        Log::log_decorated("Hyprsunset process stopped successfully");
                    }
                }
                Err(e) => Log::log_decorated(&format!(
                    "Warning: Failed to stop hyprsunset process: {}",
                    e
                )),
            }
        }
    }
}

/// Verify that hyprsunset is installed and check version compatibility.
///
/// This function is moved from main.rs and performs both installation verification
/// and version checking in a single step for efficiency.
pub fn verify_hyprsunset_installed_and_version() -> Result<()> {
    use crate::utils::extract_version_from_output;

    match std::process::Command::new("hyprsunset")
        .arg("--version")
        .output()
    {
        Ok(output) => {
            let version_output = if !output.stdout.is_empty() {
                String::from_utf8_lossy(&output.stdout)
            } else {
                String::from_utf8_lossy(&output.stderr)
            };

            if let Some(version) = extract_version_from_output(&version_output) {
                Log::log_decorated(&format!("Found hyprsunset {}", version));

                if is_version_compatible(&version) {
                    Ok(())
                } else {
                    Log::log_pipe();
                    anyhow::bail!(
                        "hyprsunset {} is not compatible with sunsetr.\n\
                        Required minimum version: {}\n\
                        Compatible versions: {}\n\
                        Please update hyprsunset to a compatible version.",
                        version,
                        REQUIRED_HYPRSUNSET_VERSION,
                        COMPATIBLE_HYPRSUNSET_VERSIONS.join(", ")
                    )
                }
            } else {
                Log::log_warning("Could not parse version from hyprsunset output");
                Log::log_decorated("Attempting to proceed with compatibility test...");
                Ok(())
            }
        }
        Err(_) => {
            match std::process::Command::new("which")
                .arg("hyprsunset")
                .output()
            {
                Ok(which_output) if which_output.status.success() => {
                    Log::log_warning("hyprsunset found but version check failed");
                    Log::log_decorated(
                        "This might be an older version. Will attempt compatibility test...",
                    );
                    Ok(())
                }
                _ => {
                    Log::log_pipe();
                    anyhow::bail!("hyprsunset is not installed on the system");
                }
            }
        }
    }
}

/// Check if a hyprsunset version is compatible with sunsetr.
pub fn is_version_compatible(version: &str) -> bool {
    use crate::utils::compare_versions;

    if COMPATIBLE_HYPRSUNSET_VERSIONS.contains(&version) {
        return true;
    }

    compare_versions(version, REQUIRED_HYPRSUNSET_VERSION) >= std::cmp::Ordering::Equal
}

/// Verify that we can establish a connection to the hyprsunset socket.
pub fn verify_hyprsunset_connection(client: &mut HyprsunsetClient) -> Result<()> {
    use std::{thread, time::Duration};

    if client.test_connection() {
        return Ok(());
    }

    Log::log_decorated("Waiting 10 seconds for hyprsunset to become available...");
    thread::sleep(Duration::from_secs(10));

    // Use non-logging version for second attempt to avoid duplicate success messages
    if client.test_connection_with_logging(false) {
        Log::log_decorated("Successfully connected to hyprsunset after waiting.");
        return Ok(());
    }

    Log::log_critical("Cannot connect to hyprsunset socket.");

    Log::log_pipe();
    anyhow::bail!(
        "\nThis usually means:\n\
          • hyprsunset is not running\n\
          • hyprsunset service is not enabled\n\
          • You're not running on Hyprland\n\
        \n\
        Please ensure hyprsunset is running and try again.\n\
        \n\
        Suggested hyprsunset startup methods:\n\
          1. Autostart hyprsunset: set start_hyprsunset to true in sunsetr.toml\n\
          2. Start hyprsunset manually: hyprsunset\n\
          3. Enable the service: systemctl --user enable hyprsunset.service"
    );
}
