//! D-Bus and system event monitoring.
//!
//! This module provides detection for:
//! - Sleep/resume events via systemd-logind PrepareForSleep signal (D-Bus)
//! - Time changes via timerfd with TFD_TIMER_CANCEL_ON_SET (kernel mechanism)
//!
//! The implementation uses:
//! - zbus's blocking API for D-Bus sleep/resume monitoring
//! - timerfd for detecting system time changes (clock adjustments, NTP sync, etc.)
//!   Each detection mechanism runs in its own thread and sends SignalMessage::Reload
//!   when relevant system events occur.

use anyhow::{Context, Result};
use nix::errno::Errno;
use nix::sys::time::TimeSpec;
use nix::sys::timerfd::{ClockId, Expiration, TimerFd, TimerFlags, TimerSetTimeFlags};
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

/// Start system event monitoring in dedicated threads.
///
/// This function spawns two separate threads that monitor for:
/// - PrepareForSleep signals from systemd-logind (sleep/resume)
/// - System time changes using timerfd (clock adjustments, NTP sync)
///
/// When relevant events occur, they send a SignalMessage::Reload to trigger
/// color temperature reapplication.
///
/// # Arguments
/// * `signal_sender` - Channel sender for communicating with the main loop
/// * `debug_enabled` - Whether debug logging is enabled
///
/// # Returns
/// * `Ok(())` - If monitoring started successfully
/// * `Err(...)` - If setup failed (graceful degradation)
///
/// # Graceful Degradation
/// If D-Bus or timerfd are unavailable, this function logs warnings and
/// the application continues without those specific detection capabilities.
pub fn start_sleep_resume_monitor(
    signal_sender: Sender<SignalMessage>,
    debug_enabled: bool,
) -> Result<()> {
    // Start both monitor threads
    spawn_monitor_threads(signal_sender, debug_enabled, 0);
    Ok(())
}

/// Spawn the monitor threads with retry capability.
///
/// This function spawns two threads:
/// 1. D-Bus thread for PrepareForSleep monitoring
/// 2. Timerfd thread for system time change monitoring
///
/// If the D-Bus connection fails, it will automatically retry up to MAX_THREAD_RESTARTS times.
fn spawn_monitor_threads(
    signal_sender: Sender<SignalMessage>,
    debug_enabled: bool,
    restart_count: u8,
) {
    const MAX_THREAD_RESTARTS: u8 = 3;
    const RESTART_DELAY_MS: u64 = 2000;

    // Clone for the second thread
    let signal_sender_clone = signal_sender.clone();

    // Spawn thread for PrepareForSleep monitoring (D-Bus)
    thread::spawn({
        let signal_sender = signal_sender.clone();
        move || {
            match monitor_sleep_signals(signal_sender.clone(), debug_enabled) {
                Ok(_) => {
                    if debug_enabled {
                        log_pipe!();
                        log_debug!("Sleep monitor thread exiting normally");
                    }
                }
                Err(e) => {
                    log_pipe!();
                    log_warning!("Sleep monitor error: {}", e);

                    if restart_count < MAX_THREAD_RESTARTS {
                        log_indented!(
                            "Will restart D-Bus monitor (attempt {}/{})",
                            restart_count + 1,
                            MAX_THREAD_RESTARTS
                        );
                        thread::sleep(std::time::Duration::from_millis(RESTART_DELAY_MS));
                        // Only restart the D-Bus monitor, not the timerfd monitor
                        thread::spawn(move || {
                            if let Err(e) = monitor_sleep_signals(signal_sender, debug_enabled) {
                                log_pipe!();
                                log_warning!("Sleep monitor restart failed: {}", e);
                            }
                        });
                    } else {
                        log_indented!("Maximum restart attempts reached for sleep monitor");
                        log_indented!("Sleep/resume detection will not be available");
                    }
                }
            }
        }
    });

    // Spawn thread for time change monitoring (timerfd)
    thread::spawn(move || {
        if let Err(e) = monitor_time_changes(signal_sender_clone, debug_enabled) {
            log_pipe!();
            log_warning!("Time change monitor error: {}", e);
            log_indented!("System time change detection will not be available");
            log_indented!("Sunsetr will continue to work normally otherwise");
        }
    });
}

/// Monitor PrepareForSleep signals using D-Bus in a dedicated thread
fn monitor_sleep_signals(signal_sender: Sender<SignalMessage>, debug_enabled: bool) -> Result<()> {
    // Connect to system D-Bus
    let connection = Connection::system().context("Failed to connect to system D-Bus")?;

    if debug_enabled {
        log_debug!("Connected to system D-Bus successfully");
    }

    // Create blocking proxy for logind Manager interface
    let logind_proxy =
        LogindManagerProxyBlocking::new(&connection).context("Failed to create logind proxy")?;

    // Get signal stream for PrepareForSleep signals
    let mut sleep_signals = logind_proxy
        .receive_prepare_for_sleep()
        .context("Failed to subscribe to PrepareForSleep signals")?;

    if debug_enabled {
        log_debug!("Subscribed to systemd-logind PrepareForSleep signals");
    }

    // Monitoring loop for sleep signals
    loop {
        match sleep_signals.next() {
            Some(signal) => {
                match signal.args() {
                    Ok(prepare_for_sleep_args) => {
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
                                            "Signal channel disconnected - exiting sleep monitor"
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
                log_pipe!();
                return Err(anyhow::anyhow!(
                    "D-Bus connection lost - PrepareForSleep signal stream ended"
                ));
            }
        }
    }
}

/// Time change detector using nix crate's timerfd API.
///
/// This implementation properly handles ECANCELED errors that occur when
/// the system clock undergoes discontinuous changes, unlike the timerfd
/// crate which panics on ECANCELED.
struct TimeChangeDetector {
    timer: TimerFd,
}

impl TimeChangeDetector {
    /// Creates a new time change detector.
    fn new() -> nix::Result<Self> {
        // Create timer with CLOCK_REALTIME for time change detection
        let timer = TimerFd::new(ClockId::CLOCK_REALTIME, TimerFlags::empty())?;
        let mut detector = TimeChangeDetector { timer };
        detector.arm_timer()?;
        Ok(detector)
    }

    /// Arms the timer for time change detection.
    fn arm_timer(&mut self) -> nix::Result<()> {
        // Combine flags for time change detection
        let flags =
            TimerSetTimeFlags::TFD_TIMER_ABSTIME | TimerSetTimeFlags::TFD_TIMER_CANCEL_ON_SET;

        // Set timer far in the future to avoid normal expiration
        // Use a very large value that won't overflow
        // i64::MAX is ~292 billion years from epoch, so divide by 1000 for safety
        let far_future = TimeSpec::new(i64::MAX / 1000, 0);

        self.timer.set(Expiration::OneShot(far_future), flags)?;
        Ok(())
    }

    /// Waits for the next time change event.
    /// This method blocks until a time change occurs or an error happens.
    fn wait_for_time_change(&mut self, debug_enabled: bool) -> Result<bool> {
        match self.timer.wait() {
            Ok(_) => {
                // Timer expired normally (unexpected with far future time)
                // Re-arm and report
                if debug_enabled {
                    log_pipe!();
                    log_warning!("Timer wait returned Ok - timer expired (unexpected)");
                }
                self.arm_timer()
                    .context("Failed to re-arm timer after expiration")?;
                Ok(false) // false = timer expired, not a time change
            }
            Err(Errno::ECANCELED) => {
                // System time changed! Re-arm timer for continued monitoring
                if debug_enabled {
                    log_pipe!();
                    log_warning!("Timer wait returned ECANCELED - time change detected!");
                }
                self.arm_timer()
                    .context("Failed to re-arm timer after time change")?;
                Ok(true) // true = time change detected
            }
            Err(other_error) => {
                // Unexpected error
                if debug_enabled {
                    log_pipe!();
                    log_error!("Timer wait returned error: {}", other_error);
                }
                log_pipe!();
                Err(anyhow::anyhow!("Timer wait error: {}", other_error))
            }
        }
    }
}

/// Monitor system time changes using timerfd in a dedicated thread
///
/// This uses the Linux kernel's timerfd mechanism with TFD_TIMER_CANCEL_ON_SET
/// to detect discontinuous changes to the system clock, such as:
/// - Manual time adjustments (date command, timedatectl)
/// - NTP synchronization jumps
/// - Other system time modifications
///
/// Note: This does NOT detect DST transitions (which don't change system time)
/// or gradual NTP slewing adjustments.
fn monitor_time_changes(signal_sender: Sender<SignalMessage>, debug_enabled: bool) -> Result<()> {
    if debug_enabled {
        log_pipe!();
        log_debug!("Starting timerfd-based time change monitoring");
    }

    // Create time change detector
    let mut detector =
        TimeChangeDetector::new().context("Failed to create time change detector")?;

    // Monitoring loop for time changes
    loop {
        // Wait for time change (blocks until time change or error)
        match detector.wait_for_time_change(debug_enabled) {
            Ok(true) => {
                // Time change detected
                log_pipe!();
                log_info!("System time changed (clock adjustment/NTP/manual) - reloading");

                match signal_sender.send(SignalMessage::TimeChange) {
                    Ok(_) => {
                        if debug_enabled {
                            log_indented!("Time change reload signal sent successfully");
                        }
                    }
                    Err(_) => {
                        // Channel disconnected - main thread probably exiting
                        if debug_enabled {
                            log_indented!("Signal channel disconnected - exiting time monitor");
                        }
                        return Ok(()); // Normal exit
                    }
                }
            }
            Ok(false) => {
                // Timer expired (shouldn't happen with far future timer)
                // This can occur when time is set forward significantly
                // Treat it as a time change event
                log_pipe!();
                log_info!("Timer expired unexpectedly (likely time set forward) - reloading");

                match signal_sender.send(SignalMessage::TimeChange) {
                    Ok(_) => {
                        if debug_enabled {
                            log_indented!("Unexpected timer expiration reload signal sent");
                        }
                    }
                    Err(_) => {
                        // Channel disconnected - main thread probably exiting
                        if debug_enabled {
                            log_indented!("Signal channel disconnected - exiting time monitor");
                        }
                        return Ok(()); // Normal exit
                    }
                }
            }
            Err(e) => {
                // Error in time change detection
                log_pipe!();
                return Err(e).context("Time change detection failed");
            }
        }
    }
}
