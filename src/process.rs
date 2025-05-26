//! hyprsunset process management and monitoring.
//!
//! This module handles starting, stopping, and monitoring the hyprsunset daemon
//! when sunsetr is configured to manage it directly. It provides process lifecycle
//! management and status checking functionality.

use anyhow::{Context, Result};
use std::{
    os::unix::net::UnixStream,
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

use crate::{hyprsunset::HyprsunsetClient, logger::Log, constants::*};

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
    /// 
    /// # Returns
    /// - `Ok(HyprsunsetProcess)` if the process starts successfully
    /// - `Err` if the process fails to start
    pub fn new(initial_temp: u32, initial_gamma: f32) -> Result<Self> {
        Log::log_pipe();
        Log::log_debug(&format!(
            "Starting hyprsunset process with initial values: {}K, {:.1}%",
            initial_temp, initial_gamma
        ));

        // Validate values before starting hyprsunset
        if !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&initial_temp) {
            return Err(anyhow::anyhow!("Invalid temperature: {}K (must be {}-{})", initial_temp, MINIMUM_TEMP, MAXIMUM_TEMP));
        }
        if !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&initial_gamma) {
            return Err(anyhow::anyhow!("Invalid gamma: {:.1}% (must be {:.1}-{:.1})", initial_gamma, MINIMUM_GAMMA, MAXIMUM_GAMMA));
        }

        let child = Command::new("hyprsunset")
            .arg("-t")
            .arg(initial_temp.to_string())
            .arg("-g")
            .arg(initial_gamma.to_string())
            .stdout(Stdio::null()) // Suppress output to avoid interfering with sunsetr's display
            .stderr(Stdio::null()) // Suppress errors for clean output
            .spawn()
            .context("Failed to start hyprsunset")?;

        let pid = child.id();
        Log::log_debug(&format!(
            "hyprsunset started with PID: {} ({}K, {:.1}%)", 
            pid, initial_temp, initial_gamma
        ));

        // Give hyprsunset time to initialize its socket and IPC system
        Log::log_decorated("Waiting for hyprsunset to initialize...");
        thread::sleep(Duration::from_secs(2));

        Ok(Self { child })
    }

    /// Stop the hyprsunset process gracefully.
    /// 
    /// Attempts to terminate the process cleanly and reaps it to prevent
    /// zombie processes. Handles cases where the process may have already
    /// exited naturally.
    /// 
    /// # Returns
    /// - `Ok(())` if termination is successful or process already exited
    /// - `Err` if there are issues during termination
    pub fn stop(mut self) -> Result<()> {
        let pid = self.child.id();

        // Check if process has already exited
        match self.child.try_wait() {
            Ok(Some(status)) => {
                Log::log_debug(&format!("Hyprsunset process terminated with {}", status));
            }
            Ok(None) => {
                // Process still running, terminate it gracefully
                Log::log_decorated(&format!("Terminating hyprsunset process (PID: {})...", pid));
                match self.child.kill() {
                    Ok(()) => {
                        let _ = self.child.wait(); // Reap the process to prevent zombies
                        Log::log_decorated("hyprsunset process terminated successfully");
                    }
                    Err(e) => {
                        Log::log_error(&format!("Failed to terminate hyprsunset process: {}", e));
                    }
                }
            }
            Err(e) => {
                Log::log_error(&format!("Error checking hyprsunset process status: {}", e));
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
    if let Ok(client) = HyprsunsetClient::new() {
        // Check both that the socket file exists AND that we can connect to it
        if client.socket_path.exists() {
            // Try to connect - if successful, hyprsunset is running
            // If connection fails, the socket file likely exists from a crashed instance
            return UnixStream::connect(&client.socket_path).is_ok();
        }
    }
    false
} 