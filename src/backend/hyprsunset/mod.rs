//! Hyprsunset backend implementation using the external hyprsunset process for gamma control.
//!
//! Controls color temperature on Hyprland by managing the hyprsunset process as a child and
//! communicating with it over Hyprland's IPC socket. This is a legacy backend. For Hyprland
//! the native CTM backend (`backend = "hyprland"`) is recommended because it needs no external
//! process.
//!
//! The backend always operates in managed mode: it starts hyprsunset as a child during
//! initialization, refuses to run alongside an externally started instance, and ensures the
//! child is cleaned up on shutdown via PR_SET_PDEATHSIG. Commands are sent as formatted
//! strings over the IPC socket, whose path is detected from Hyprland's environment.

use anyhow::Result;
use std::sync::atomic::AtomicBool;

use crate::backend::ColorTemperatureBackend;
use crate::common::error::Silent;
use crate::config::Config;

pub mod client;
pub mod process;

pub use client::HyprsunsetClient;
pub use process::{HyprsunsetProcess, is_hyprsunset_running};

/// Hyprsunset backend that manages the hyprsunset process for gamma control on Hyprland.
pub struct HyprsunsetBackend {
    client: HyprsunsetClient,
    process: Option<HyprsunsetProcess>,
    /// The last temperature and gamma values that were successfully applied to hyprsunset.
    /// Used to avoid redundant state applications.
    last_applied_values: Option<(u32, f64)>,
}

impl HyprsunsetBackend {
    /// Create a backend by computing the current temperature and gamma from the schedule,
    /// then starting the managed hyprsunset process with those initial values.
    pub fn new(
        config: &Config,
        debug_enabled: bool,
        geo_times: Option<&crate::geo::times::GeoTimes>,
    ) -> Result<Self> {
        let schedule = crate::core::schedule::Schedule::from_config(config, geo_times.cloned());
        let now = crate::time::source::now();
        let current_state = schedule
            .as_ref()
            .map_or(crate::core::period::Period::Static, |schedule| {
                schedule.current_period(now)
            });
        let runtime_state =
            crate::core::runtime_state::RuntimeState::new(current_state, config, schedule, now);
        let (temp, gamma) = runtime_state.values();

        Self::new_with_initial_values(debug_enabled, temp, gamma)
    }

    /// Create a backend that starts hyprsunset directly with the given temperature and gamma.
    ///
    /// Used by the test command to start with test values, avoiding a second apply after
    /// initialization.
    pub fn new_with_initial_values(
        debug_enabled: bool,
        initial_temp: u32,
        initial_gamma: f64,
    ) -> Result<Self> {
        verify_hyprsunset_installed_and_version()?;

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
            return Err(Silent.into());
        }

        let (process, last_applied_values) = (
            Some(HyprsunsetProcess::new(
                initial_temp,
                initial_gamma,
                debug_enabled,
            )?),
            Some((initial_temp, initial_gamma)),
        );

        let mut client = HyprsunsetClient::new(debug_enabled)?;

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
        self.client.apply_transition_state(runtime_state, running)?;

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

        // Skip the redundant apply if hyprsunset already has the target values
        if let Some((last_temp, last_gamma)) = self.last_applied_values
            && target_temp == last_temp
            && target_gamma == last_gamma
        {
            crate::core::period::log_state_announcement(runtime_state.period());
            return Ok(());
        }

        self.client.apply_startup_state(runtime_state, running)?;

        self.last_applied_values = Some((target_temp, target_gamma));

        Ok(())
    }

    fn apply_temperature_gamma(
        &mut self,
        temperature: u32,
        gamma: f64,
        running: &AtomicBool,
    ) -> Result<()> {
        self.client
            .apply_temperature_gamma(temperature, gamma, running)?;

        self.last_applied_values = Some((temperature, gamma));

        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "Hyprsunset"
    }

    fn cleanup(self: Box<Self>, debug_enabled: bool) {
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
                    log_indented!("Please update hyprsunset to a compatible version.");
                    log_end!();
                    Err(Silent.into())
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
                    Err(Silent.into())
                }
            }
        }
    }
}

const REQUIRED_HYPRSUNSET_VERSION: &str = "v0.2.0";

/// Check if a hyprsunset version is compatible with sunsetr.
pub fn is_version_compatible(version: &str) -> bool {
    use crate::common::utils::compare_versions;

    compare_versions(version, REQUIRED_HYPRSUNSET_VERSION) >= std::cmp::Ordering::Equal
}

/// Verify that we can establish a connection to the hyprsunset socket.
pub fn verify_hyprsunset_connection(client: &mut HyprsunsetClient) -> Result<()> {
    use std::{thread, time::Duration, time::Instant};

    if client.test_connection() {
        return Ok(());
    }

    // Wait for the spawned hyprsunset to create its socket
    let start_time = Instant::now();
    let max_wait = Duration::from_millis(2000); // 2 second maximum
    let mut delay = Duration::from_millis(5);
    let max_delay = Duration::from_millis(80);

    if client.debug_enabled {
        log_debug!("Waiting for hyprsunset to create socket...");
    }

    while start_time.elapsed() < max_wait {
        thread::sleep(delay);

        if client.test_connection() {
            let elapsed = start_time.elapsed();
            if elapsed > Duration::from_millis(50) {
                log_decorated!("Connected to hyprsunset after {}ms", elapsed.as_millis());
            }
            return Ok(());
        }

        delay = std::cmp::min(delay * 2, max_delay);
    }

    log_critical!("Failed to connect to hyprsunset socket after 2 seconds.");
    log_block_start!("The Hyprsunset backend manages hyprsunset internally. This error means");
    log_indented!("the backend couldn't connect to its managed hyprsunset process.");
    log_block_start!("This should not happen. Please report this issue.");
    log_end!();
    Err(Silent.into())
}
