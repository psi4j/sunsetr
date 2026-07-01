//! hyprsunset process management.
//!
//! Starts and stops the hyprsunset process that sunsetr manages, and checks whether one is
//! already running. Initial temperature and gamma are passed on the command line (`-t`/`-g`)
//! so hyprsunset starts at sunsetr's values, avoiding a jump from hyprsunset's defaults.

use anyhow::{Context, Result};
use std::{
    os::unix::net::UnixStream,
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

use crate::{backend::hyprsunset::client::HyprsunsetClient, common::constants::*};

/// Manages a hyprsunset process started by sunsetr, terminating and reaping it on shutdown.
pub struct HyprsunsetProcess {
    child: Child,
}

impl HyprsunsetProcess {
    /// Spawn hyprsunset with the given initial temperature and gamma.
    ///
    /// Starts with the target values to avoid a visible jump from hyprsunset's defaults, and
    /// redirects stdout/stderr to null so they do not interfere with sunsetr's output.
    pub fn new(initial_temp: u32, initial_gamma: f64, debug_enabled: bool) -> Result<Self> {
        if debug_enabled {
            log_pipe!();
            log_debug!(
                "Starting hyprsunset process with initial values: {}K, {:.1}%",
                initial_temp,
                initial_gamma
            );
        }

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
            .stdout(Stdio::null())
            .stderr(Stdio::null());

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

        Ok(Self { child })
    }

    /// Terminate the process (SIGTERM, then SIGKILL if needed) and reap it to avoid a
    /// zombie, tolerating a process that already exited.
    pub fn stop(mut self, debug_enabled: bool) -> Result<()> {
        let pid = self.child.id();

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
                if debug_enabled {
                    log_decorated!("Terminating hyprsunset process (PID: {})...", pid);
                } else {
                    log_decorated!("Terminating hyprsunset process...");
                }

                use nix::sys::signal::{Signal, kill};
                use nix::unistd::Pid;
                let nix_pid = Pid::from_raw(pid as i32);

                if let Err(e) = kill(nix_pid, Signal::SIGTERM)
                    && debug_enabled
                {
                    log_warning!("Failed to send SIGTERM to hyprsunset: {}", e);
                }

                thread::sleep(Duration::from_millis(100));

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
                        if debug_enabled {
                            log_indented!("Process still running after SIGTERM, using SIGKILL");
                        }
                        match self.child.kill() {
                            Ok(()) => {
                                let _ = self.child.wait();
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

/// Check whether hyprsunset is running by connecting to its Unix socket.
///
/// Connecting rather than just checking for the file handles a stale socket left behind
/// when the process is gone.
pub fn is_hyprsunset_running() -> bool {
    if let Ok(client) = HyprsunsetClient::new(false) {
        let socket_exists = client.socket_path.exists();

        let can_connect = if socket_exists {
            UnixStream::connect(&client.socket_path).is_ok()
        } else {
            false
        };

        return can_connect;
    }

    #[cfg(debug_assertions)]
    eprintln!("DEBUG: is_hyprsunset_running() - failed to create client, result=false");

    false
}

/// Terminate the process on drop as a safety net if stop() was not called.
impl Drop for HyprsunsetProcess {
    fn drop(&mut self) {
        let pid = self.child.id();

        match self.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                use nix::sys::signal::{Signal, kill};
                use nix::unistd::Pid;
                let nix_pid = Pid::from_raw(pid as i32);
                let _ = kill(nix_pid, Signal::SIGTERM);
                thread::sleep(Duration::from_millis(50));

                match self.child.try_wait() {
                    Ok(Some(_)) => (),
                    _ => {
                        let _ = self.child.kill();
                        let _ = self.child.wait();
                    }
                }
            }
            Err(_) => {
                let _ = self.child.kill();
            }
        }
    }
}
