//! hyprsunset process management and monitoring.
//!
//! This module handles starting, stopping, and monitoring the hyprsunset daemon
//! when sunsetr is configured to manage it directly. It provides process lifecycle
//! management and status checking functionality.
//!
//! # Initial Value Handling
//!
//! When starting hyprsunset, initial temperature and gamma values are passed as
//! command line arguments (-t for temperature, -g for gamma). This ensures that
//! hyprsunset starts with the correct values immediately, preventing jarring
//! transitions from hyprsunset's internal defaults to sunsetr's configuration.

use anyhow::{Context, Result};
use std::{
    os::unix::net::UnixStream,
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

use crate::{backend::hyprsunset::client::HyprsunsetClient, constants::*};

/// Manages the lifecycle of a hyprsunset process started by sunsetr.
///
/// This structure tracks a hyprsunset process that was started by sunsetr
/// and provides methods for graceful termination when shutting down.
/// It ensures proper cleanup and process reaping.
pub struct HyprsunsetProcess {
    child: Child,
}

impl HyprsunsetProcess {
    /// Start a new hyprsunset process with specified initial temperature and gamma values.
    ///
    /// Spawns hyprsunset as a background daemon with stdout/stderr redirected
    /// to null to prevent interference with sunsetr's output. Starts hyprsunset
    /// with the provided temperature and gamma values to prevent initial jumps
    /// from hyprsunset's defaults to sunsetr's configuration.
    ///
    /// # Arguments
    /// * `initial_temp` - Initial temperature in Kelvin to start hyprsunset with
    /// * `initial_gamma` - Initial gamma percentage (0.0-100.0) to start hyprsunset with
    /// * `debug_enabled` - Whether to enable debug logging for process management
    ///
    /// # Returns
    /// - `Ok(HyprsunsetProcess)` if the process starts successfully
    /// - `Err` if the process fails to start
    pub fn new(initial_temp: u32, initial_gamma: f32, debug_enabled: bool) -> Result<Self> {
        if debug_enabled {
            log_pipe!();
            log_debug!(
                "Starting hyprsunset process with initial values: {}K, {:.1}%",
                initial_temp,
                initial_gamma
            );
        }

        // Validate values before starting hyprsunset
        if !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&initial_temp) {
            return Err(anyhow::anyhow!(
                "Invalid temperature: {}K (must be {}-{})",
                initial_temp,
                MINIMUM_TEMP,
                MAXIMUM_TEMP
            ));
        }
        if !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&initial_gamma) {
            return Err(anyhow::anyhow!(
                "Invalid gamma: {:.1}% (must be {:.1}-{:.1})",
                initial_gamma,
                MINIMUM_GAMMA,
                MAXIMUM_GAMMA
            ));
        }

        let mut cmd = Command::new("hyprsunset");
        cmd.arg("-t")
            .arg(initial_temp.to_string())
            .arg("-g")
            .arg(initial_gamma.to_string())
            .stdout(Stdio::null()) // Suppress output to avoid interfering with sunsetr's display
            .stderr(Stdio::null()); // Suppress errors for clean output

        // Create new process group to isolate hyprsunset from terminal signals
        // This prevents Ctrl+C from killing hyprsunset before sunsetr can reset gamma
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);

            // Set up pre_exec to make hyprsunset die when sunsetr dies
            // This ensures cleanup even if sunsetr is forcefully killed
            unsafe {
                cmd.pre_exec(|| {
                    use nix::sys::prctl;
                    use nix::sys::signal::Signal;

                    // When parent dies, send SIGTERM to this process
                    prctl::set_pdeathsig(Signal::SIGTERM)?;
                    Ok(())
                });
            }
        }

        let child = cmd.spawn().context("Failed to start hyprsunset")?;

        let pid = child.id();
        if debug_enabled {
            log_debug!(
                "hyprsunset started with PID: {} ({}K, {:.1}%)",
                pid,
                initial_temp,
                initial_gamma
            );
        }

        // Don't wait here - let the backend handle connection readiness
        // This allows for faster startup when hyprsunset is ready quickly
        Ok(Self { child })
    }

    /// Stop the hyprsunset process gracefully.
    ///
    /// Attempts to terminate the process cleanly and reaps it to prevent
    /// zombie processes. Handles cases where the process may have already
    /// exited naturally.
    ///
    /// # Arguments
    /// * `debug_enabled` - Whether to enable debug logging for process termination
    ///
    /// # Returns
    /// - `Ok(())` if termination is successful or process already exited
    /// - `Err` if there are issues during termination
    pub fn stop(mut self, debug_enabled: bool) -> Result<()> {
        let pid = self.child.id();

        // Check if process has already exited
        match self.child.try_wait() {
            Ok(Some(status)) => {
                if debug_enabled {
                    log_warning!(
                        "Hyprsunset process (PID: {}) already terminated with {}",
                        pid,
                        status
                    );
                    log_indented!(
                        "This suggests hyprsunset received a signal or crashed before cleanup"
                    );
                } else {
                    log_warning!("Hyprsunset process already terminated with {}", status);
                }
            }
            Ok(None) => {
                // Process still running, terminate it gracefully
                if debug_enabled {
                    log_decorated!("Terminating hyprsunset process (PID: {})...", pid);
                } else {
                    log_decorated!("Terminating hyprsunset process...");
                }

                // First try SIGTERM for graceful shutdown
                use nix::sys::signal::{Signal, kill};
                use nix::unistd::Pid;

                let nix_pid = Pid::from_raw(pid as i32);

                // Send SIGTERM first for graceful shutdown
                if let Err(e) = kill(nix_pid, Signal::SIGTERM)
                    && debug_enabled
                {
                    log_warning!("Failed to send SIGTERM to hyprsunset: {}", e);
                }

                // Give it a brief moment to exit gracefully
                thread::sleep(Duration::from_millis(100));

                // Check if it exited after SIGTERM
                match self.child.try_wait() {
                    Ok(Some(_)) => {
                        if debug_enabled {
                            log_decorated!(
                                "hyprsunset process (PID: {}) terminated gracefully after SIGTERM",
                                pid
                            );
                        } else {
                            log_decorated!("hyprsunset process terminated successfully");
                        }
                    }
                    Ok(None) => {
                        // Still running, use SIGKILL
                        if debug_enabled {
                            log_indented!("Process still running after SIGTERM, using SIGKILL");
                        }
                        match self.child.kill() {
                            Ok(()) => {
                                let _ = self.child.wait(); // Reap the process to prevent zombies
                                if debug_enabled {
                                    log_decorated!(
                                        "hyprsunset process (PID: {}) terminated with SIGKILL",
                                        pid
                                    );
                                } else {
                                    log_decorated!("hyprsunset process terminated successfully");
                                }
                            }
                            Err(e) => {
                                log_error!("Failed to terminate hyprsunset process: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        log_error!("Error checking process status after SIGTERM: {}", e);
                    }
                }
            }
            Err(e) => {
                log_error!("Error checking hyprsunset process status: {}", e);
            }
        }

        Ok(())
    }
}

/// Check if hyprsunset is already running by testing socket connectivity.
///
/// This function provides a reliable way to detect if hyprsunset is running
/// by attempting to connect to its Unix socket. It handles the case where
/// a socket file exists but the process is no longer running (stale socket).
///
/// # Returns
/// - `true` if hyprsunset is running and responsive
/// - `false` if hyprsunset is not running or not responsive
pub fn is_hyprsunset_running() -> bool {
    // Initialize a client to determine the socket path
    if let Ok(client) = HyprsunsetClient::new(false) {
        // Check both that the socket file exists AND that we can connect to it
        let socket_exists = client.socket_path.exists();
        let can_connect = if socket_exists {
            UnixStream::connect(&client.socket_path).is_ok()
        } else {
            false
        };

        // Debug logging for reload investigation
        #[cfg(debug_assertions)]
        eprintln!(
            "DEBUG: is_hyprsunset_running() - socket_exists={socket_exists}, can_connect={can_connect}, result={can_connect}"
        );

        return can_connect;
    }

    #[cfg(debug_assertions)]
    eprintln!("DEBUG: is_hyprsunset_running() - failed to create client, result=false");

    false
}

// Implement Drop to ensure hyprsunset is always cleaned up
impl Drop for HyprsunsetProcess {
    fn drop(&mut self) {
        let pid = self.child.id();

        // Try to check if process is still running
        match self.child.try_wait() {
            Ok(Some(_)) => {
                // Process already exited, nothing to do
            }
            Ok(None) => {
                // Process still running, try to terminate it
                use nix::sys::signal::{Signal, kill};
                use nix::unistd::Pid;

                let nix_pid = Pid::from_raw(pid as i32);

                // First try SIGTERM
                let _ = kill(nix_pid, Signal::SIGTERM);

                // Give it a very brief moment (we can't wait long in Drop)
                thread::sleep(Duration::from_millis(50));

                // Check again
                match self.child.try_wait() {
                    Ok(Some(_)) => (), // Exited after SIGTERM
                    _ => {
                        // Still running or error, use SIGKILL
                        let _ = self.child.kill();
                        let _ = self.child.wait(); // Try to reap it
                    }
                }
            }
            Err(_) => {
                // Error checking status, try to kill anyway
                let _ = self.child.kill();
            }
        }
    }
}
