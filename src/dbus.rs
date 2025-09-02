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

    thread::spawn(move || {
        if let Err(e) = run_dbus_monitor_loop(signal_sender, debug_enabled) {
            log_pipe!();
            log_warning!("D-Bus sleep/resume monitoring stopped: {}", e);
            log_indented!("Sleep/resume detection will not be available");
            log_indented!("Sunsetr will continue to work normally otherwise");
        }
    });

    Ok(())
}

/// Main D-Bus monitoring loop (runs in dedicated thread).
///
/// This function handles the actual D-Bus connection and signal monitoring.
/// It runs in a blocking loop until the channel disconnects or an
/// unrecoverable error occurs.
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

    // Connection retry state
    let mut consecutive_failures = 0;
    const MAX_RETRIES: u8 = 3;
    const RETRY_DELAY_MS: u64 = 1000;

    // Main monitoring loop - blocks until signals arrive
    loop {
        match signals.next() {
            Some(signal) => {
                // Reset failure count on successful signal reception
                consecutive_failures = 0;

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
                            log_info!(
                                "System resuming from sleep/suspend - reloading color temperature"
                            );

                            match signal_sender.send(SignalMessage::Reload) {
                                Ok(_) => {
                                    log_indented!("Resume reload signal sent successfully");
                                }
                                Err(_) => {
                                    // Channel disconnected - main thread probably exiting
                                    log_indented!(
                                        "Signal channel disconnected - exiting D-Bus monitor"
                                    );
                                    break;
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
                // Signal stream ended - this usually means connection lost
                consecutive_failures += 1;

                log_pipe!();
                log_warning!(
                    "D-Bus signal stream ended (attempt {}/{})",
                    consecutive_failures,
                    MAX_RETRIES
                );

                if consecutive_failures >= MAX_RETRIES {
                    return Err(anyhow::anyhow!(
                        "D-Bus connection lost after {} retry attempts",
                        MAX_RETRIES
                    ));
                }

                // Try to reconnect by recreating the whole setup
                log_indented!("Attempting to reconnect to D-Bus...");
                thread::sleep(std::time::Duration::from_millis(RETRY_DELAY_MS));

                // Recreate connection and signal stream inline to avoid complex type signatures
                match Connection::system() {
                    Ok(new_connection) => {
                        match LogindManagerProxyBlocking::new(&new_connection) {
                            Ok(new_proxy) => {
                                match new_proxy.receive_prepare_for_sleep() {
                                    Ok(new_signals) => {
                                        log_indented!("D-Bus reconnection successful");
                                        let _connection = new_connection; // Keep connection alive
                                        signals = new_signals;
                                    }
                                    Err(e) => {
                                        log_indented!("Failed to resubscribe to signals: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                log_indented!("Failed to recreate logind proxy: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        log_indented!("D-Bus reconnection failed: {}", e);
                    }
                }
            }
        }
    }

    Ok(())
}
