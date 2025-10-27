//! Hyprsunset backend implementation using the external hyprsunset process for gamma control.
//!
//! This module provides color temperature control for Hyprland by managing the hyprsunset
//! process as a child process and communicating with it via Hyprland's IPC socket protocol.
//!
//! **Note**: This is a legacy backend. For Hyprland users, the native CTM backend
//! (`backend = "hyprland"`) is recommended as it doesn't require an external process.
//!
//! ## Architecture
//!
//! The hyprsunset backend consists of two main components:
//! - **Process Management** ([`HyprsunsetProcess`]): Manages the hyprsunset process lifecycle
//! - **Client Communication** ([`HyprsunsetClient`]): Communicates with hyprsunset via IPC socket
//!
//! ## Process Management
//!
//! The backend always operates in managed mode, where it:
//! - Starts hyprsunset as a child process during initialization
//! - Monitors the process health and restarts if necessary
//! - Ensures proper cleanup on shutdown using PR_SET_PDEATHSIG
//! - Manages the process lifecycle independently
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
use crate::common::constants::*;
use crate::config::Config;

pub mod client;
pub mod process;

pub use client::HyprsunsetClient;
pub use process::{HyprsunsetProcess, is_hyprsunset_running};

/// Hyprsunset backend implementation using hyprsunset process for gamma control.
///
/// This backend provides color temperature control on Hyprland via the
/// hyprsunset process. It can either manage hyprsunset as a child process
/// or connect to an existing hyprsunset instance.
pub struct HyprsunsetBackend {
    client: HyprsunsetClient,
    process: Option<HyprsunsetProcess>,
    /// The last temperature and gamma values that were successfully applied to hyprsunset.
    /// Used to avoid redundant state applications.
    last_applied_values: Option<(u32, f32)>,
}

impl HyprsunsetBackend {
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
    /// A new HyprsunsetBackend instance ready for use
    ///
    /// # Errors
    /// Returns an error if:
    /// - hyprsunset is not installed or incompatible
    /// - Process management conflicts are detected
    /// - Client initialization fails
    pub fn new(
        config: &Config,
        debug_enabled: bool,
        geo_times: Option<&crate::geo::times::GeoTimes>,
    ) -> Result<Self> {
        // For normal operation, use current state values from config
        let current_state = crate::core::period::get_current_period(config, geo_times);
        let runtime_state = crate::core::runtime_state::RuntimeState::new(
            current_state,
            config,
            geo_times,
            crate::time::source::now().time(),
        );
        let (temp, gamma) = runtime_state.values();

        Self::new_with_initial_values(debug_enabled, temp, gamma)
    }

    /// Create a new Hyprland backend instance with specific initial values.
    ///
    /// This is used by the test command to start hyprsunset with test values directly,
    /// avoiding the need to change values after initialization.
    ///
    /// # Arguments
    /// * `debug_enabled` - Whether to enable debug output for this backend
    /// * `initial_temp` - Temperature to start hyprsunset with
    /// * `initial_gamma` - Gamma to start hyprsunset with
    ///
    /// # Returns
    /// A new HyprsunsetBackend instance ready for use
    pub fn new_with_initial_values(
        debug_enabled: bool,
        initial_temp: u32,
        initial_gamma: f32,
    ) -> Result<Self> {
        // Verify hyprsunset installation and version compatibility
        verify_hyprsunset_installed_and_version()?;

        // Always start hyprsunset for the Hyprsunset backend
        if is_hyprsunset_running() {
            log_pipe!();
            log_warning!("hyprsunset is already running.");
            log_pipe!();
            log_error!("Please kill the existing hyprsunset process: pkill hyprsunset");
            log_indented!(
                "The Hyprsunset backend manages hyprsunset internally and cannot work with"
            );
            log_indented!("an externally started hyprsunset instance.");
            log_end!();
            std::process::exit(1);
        }

        // Use the provided initial values
        let (process, last_applied_values) = (
            Some(HyprsunsetProcess::new(
                initial_temp,
                initial_gamma,
                debug_enabled,
            )?),
            Some((initial_temp, initial_gamma)),
        );

        // Initialize hyprsunset client
        let mut client = HyprsunsetClient::new(debug_enabled)?;

        // Verify connection to hyprsunset
        verify_hyprsunset_connection(&mut client)?;

        Ok(Self {
            client,
            process,
            last_applied_values,
        })
    }
}

impl ColorTemperatureBackend for HyprsunsetBackend {
    fn apply_transition_state(
        &mut self,
        runtime_state: &crate::core::runtime_state::RuntimeState,
        running: &AtomicBool,
    ) -> Result<()> {
        // Apply the state
        self.client.apply_transition_state(runtime_state, running)?;

        // Update tracked values on success
        let (temp, gamma) = runtime_state.values();
        self.last_applied_values = Some((temp, gamma));

        Ok(())
    }

    fn apply_startup_state(
        &mut self,
        runtime_state: &crate::core::runtime_state::RuntimeState,
        running: &AtomicBool,
    ) -> Result<()> {
        let (target_temp, target_gamma) = runtime_state.values();

        // Check if we should skip redundant commands
        if let Some((last_temp, last_gamma)) = self.last_applied_values {
            // Check if target matches what hyprsunset currently has
            if target_temp == last_temp && target_gamma == last_gamma {
                // hyprsunset already has the correct values, just announce the mode
                crate::core::period::log_state_announcement(runtime_state.period());
                return Ok(());
            }
        }

        // Apply the state and update our tracking
        self.client.apply_startup_state(runtime_state, running)?;

        // Update the last applied values on success
        self.last_applied_values = Some((target_temp, target_gamma));

        Ok(())
    }

    fn apply_temperature_gamma(
        &mut self,
        temperature: u32,
        gamma: f32,
        running: &AtomicBool,
    ) -> Result<()> {
        // Apply the values
        self.client
            .apply_temperature_gamma(temperature, gamma, running)?;

        // Update tracked values on success
        self.last_applied_values = Some((temperature, gamma));

        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "Hyprsunset"
    }

    fn cleanup(self: Box<Self>, debug_enabled: bool) {
        // Stop any managed hyprsunset process
        if let Some(process) = self.process {
            if debug_enabled {
                log_decorated!("Stopping managed hyprsunset process...");
            }
            match process.stop(debug_enabled) {
                Ok(_) => {
                    if debug_enabled {
                        log_decorated!("Hyprsunset process stopped successfully");
                    }
                }
                Err(e) => {
                    log_decorated!("Warning: Failed to stop hyprsunset process: {e}")
                }
            }
        }
    }
}

/// Verify that hyprsunset is installed and check version compatibility.
///
/// This function is moved from main.rs and performs both installation verification
/// and version checking in a single step for efficiency.
pub fn verify_hyprsunset_installed_and_version() -> Result<()> {
    use crate::common::utils::extract_version_from_output;

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
                log_decorated!("Found hyprsunset {version}");

                if is_version_compatible(&version) {
                    Ok(())
                } else {
                    log_pipe!();
                    log_error!("hyprsunset {} is not compatible with sunsetr.", version);
                    log_indented!("Required minimum version: {}", REQUIRED_HYPRSUNSET_VERSION);
                    log_indented!(
                        "Compatible versions: {}",
                        COMPATIBLE_HYPRSUNSET_VERSIONS.join(", ")
                    );
                    log_indented!("Please update hyprsunset to a compatible version.");
                    log_end!();
                    std::process::exit(1)
                }
            } else {
                log_warning!("Could not parse version from hyprsunset output");
                log_decorated!("Attempting to proceed with compatibility test...");
                Ok(())
            }
        }
        Err(_) => {
            match std::process::Command::new("which")
                .arg("hyprsunset")
                .output()
            {
                Ok(which_output) if which_output.status.success() => {
                    log_warning!("hyprsunset found but version check failed");
                    log_decorated!(
                        "This might be an older version. Will attempt compatibility test..."
                    );
                    Ok(())
                }
                _ => {
                    log_pipe!();
                    log_error!("hyprsunset is not installed on the system");
                    log_end!();
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Check if a hyprsunset version is compatible with sunsetr.
pub fn is_version_compatible(version: &str) -> bool {
    use crate::common::utils::compare_versions;

    if COMPATIBLE_HYPRSUNSET_VERSIONS.contains(&version) {
        return true;
    }

    compare_versions(version, REQUIRED_HYPRSUNSET_VERSION) >= std::cmp::Ordering::Equal
}

/// Verify that we can establish a connection to the hyprsunset socket.
pub fn verify_hyprsunset_connection(client: &mut HyprsunsetClient) -> Result<()> {
    use std::{thread, time::Duration, time::Instant};

    // First, try immediate connection (in case hyprsunset is already running)
    if client.test_connection() {
        return Ok(());
    }

    // Poll for socket readiness with exponential backoff
    // Start with 5ms intervals, doubling up to 80ms
    let start_time = Instant::now();
    let max_wait = Duration::from_millis(2000); // 2 second maximum
    let mut delay = Duration::from_millis(5);
    let max_delay = Duration::from_millis(80);

    if client.debug_enabled {
        log_debug!("Waiting for hyprsunset to create socket...");
    }

    while start_time.elapsed() < max_wait {
        thread::sleep(delay);

        // Try to connect
        if client.test_connection() {
            // Connection successful
            let elapsed = start_time.elapsed();
            if elapsed > Duration::from_millis(50) {
                // Only log if it took more than 50ms
                log_decorated!("Connected to hyprsunset after {}ms", elapsed.as_millis());
            }
            return Ok(());
        }

        // Exponential backoff, capped at max_delay
        delay = std::cmp::min(delay * 2, max_delay);
    }

    log_critical!("Failed to connect to hyprsunset socket after 2 seconds.");
    log_block_start!("The Hyprsunset backend manages hyprsunset internally. This error means");
    log_indented!("the backend couldn't connect to its managed hyprsunset process.");
    log_block_start!("This should not happen. Please report this issue.");
    log_end!();
    std::process::exit(1);
}
