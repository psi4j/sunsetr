//! D-Bus integration for sleep/resume monitoring.
//!
//! This module provides D-Bus-based sleep/resume detection using the systemd-logind
//! PrepareForSleep signal. This is the systemd-recommended approach, replacing
//! system-sleep scripts with proper D-Bus integration.
//!
//! The implementation uses zbus's blocking API in a dedicated thread that sends
//! SignalMessage::Reload when the system resumes from sleep.

use anyhow::{Context, Result};
use std::sync::mpsc::Sender;
use std::thread;
use zbus::blocking::Connection;

use crate::signals::SignalMessage;

/// D-Bus proxy trait for systemd-logind Manager interface.
#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait LogindManager {
    /// PrepareForSleep signal emitted by systemd-logind.
    ///
    /// The `start` parameter indicates:
    /// - `true`: System is about to sleep/suspend
    /// - `false`: System is resuming from sleep/suspend
    #[zbus(signal)]
    fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;
}

/// Start D-Bus sleep/resume monitoring in a dedicated thread.
///
/// This function spawns a blocking thread that connects to the system D-Bus
/// and monitors for PrepareForSleep signals from systemd-logind. When the
/// system resumes (start=false), it sends a SignalMessage::Reload to trigger
/// color temperature reapplication.
///
/// # Arguments
/// * `signal_sender` - Channel sender for communicating with the main loop
///
/// # Returns
/// * `Ok(())` - If D-Bus monitoring started successfully
/// * `Err(...)` - If D-Bus connection failed (graceful degradation)
///
/// # Graceful Degradation
/// If D-Bus is unavailable, this function logs a warning and returns an error.
/// The caller should handle this gracefully and continue without sleep/resume
/// detection functionality.
pub fn start_sleep_resume_monitor(
    signal_sender: Sender<SignalMessage>,
    debug_enabled: bool,
) -> Result<()> {
    if debug_enabled {
        log_pipe!();
        log_debug!("Starting D-Bus sleep/resume monitoring...");
    }

    spawn_monitor_thread(signal_sender, debug_enabled, 0);
    Ok(())
}

/// Spawn the D-Bus monitor thread with retry capability.
///
/// This function spawns a new thread for D-Bus monitoring. If the connection
/// fails, it will automatically respawn the thread up to MAX_THREAD_RESTARTS times.
fn spawn_monitor_thread(
    signal_sender: Sender<SignalMessage>,
    debug_enabled: bool,
    restart_count: u8,
) {
    const MAX_THREAD_RESTARTS: u8 = 3;
    const RESTART_DELAY_MS: u64 = 2000;

    thread::spawn(move || {
        match run_dbus_monitor_loop(signal_sender.clone(), debug_enabled) {
            Ok(_) => {
                // Normal exit (channel disconnected)
                if debug_enabled {
                    log_pipe!();
                    log_debug!("D-Bus monitoring thread exiting normally");
                }
            }
            Err(e) => {
                log_pipe!();
                log_warning!("D-Bus monitoring error: {}", e);

                if restart_count < MAX_THREAD_RESTARTS {
                    log_indented!(
                        "Restarting D-Bus monitor thread (attempt {}/{})",
                        restart_count + 1,
                        MAX_THREAD_RESTARTS
                    );

                    // Wait before restarting to avoid rapid restart loops
                    thread::sleep(std::time::Duration::from_millis(RESTART_DELAY_MS));

                    // Restart the monitor thread
                    spawn_monitor_thread(signal_sender, debug_enabled, restart_count + 1);
                } else {
                    log_indented!("Maximum restart attempts reached");
                    log_indented!("Sleep/resume detection will not be available");
                    log_indented!("Sunsetr will continue to work normally otherwise");
                }
            }
        }
    });
}

/// Main D-Bus monitoring loop (runs in dedicated thread).
///
/// This function handles the actual D-Bus connection and signal monitoring.
/// It runs in a blocking loop until the channel disconnects or the connection
/// is lost. Connection losses will trigger a thread restart via spawn_monitor_thread.
fn run_dbus_monitor_loop(signal_sender: Sender<SignalMessage>, debug_enabled: bool) -> Result<()> {
    // Connect to system D-Bus (blocking operation)
    let connection = Connection::system().context("Failed to connect to system D-Bus")?;

    if debug_enabled {
        log_indented!("Connected to system D-Bus successfully");
    }

    // Create blocking proxy for logind Manager interface
    let proxy =
        LogindManagerProxyBlocking::new(&connection).context("Failed to create logind proxy")?;

    // Get signal stream for PrepareForSleep signals
    let mut signals = proxy
        .receive_prepare_for_sleep()
        .context("Failed to subscribe to PrepareForSleep signals")?;

    if debug_enabled {
        log_indented!("Subscribed to systemd-logind PrepareForSleep signals");
    }

    // Main monitoring loop - blocks until signals arrive
    loop {
        match signals.next() {
            Some(signal) => {
                // Process the PrepareForSleep signal
                match signal.args() {
                    Ok(prepare_for_sleep_args) => {
                        // The PrepareForSleep signal has one boolean parameter
                        let going_to_sleep: bool = prepare_for_sleep_args.start;

                        if going_to_sleep {
                            // System is about to sleep - just log it
                            log_pipe!();
                            log_info!("System entering sleep/suspend mode");
                        } else {
                            // System is resuming - trigger reload
                            log_pipe!();
                            log_info!("System resuming from sleep/suspend - reloading");

                            match signal_sender.send(SignalMessage::Reload) {
                                Ok(_) => {
                                    if debug_enabled {
                                        log_indented!("Resume reload signal sent successfully");
                                    }
                                }
                                Err(_) => {
                                    // Channel disconnected - main thread probably exiting
                                    if debug_enabled {
                                        log_indented!(
                                            "Signal channel disconnected - exiting D-Bus monitor"
                                        );
                                    }
                                    return Ok(()); // Normal exit
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log_pipe!();
                        log_warning!("Failed to parse PrepareForSleep signal args: {}", e);
                        log_indented!("Continuing to monitor for future signals...");
                    }
                }
            }
            None => {
                // Signal stream ended - connection lost
                return Err(anyhow::anyhow!(
                    "D-Bus connection lost - signal stream ended"
                ));
            }
        }
    }
}
