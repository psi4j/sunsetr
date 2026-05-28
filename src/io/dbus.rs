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
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use zbus::blocking::Connection;

use crate::io::signals::SignalMessage;

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

/// Tracks sleep state to coordinate between sleep and time change detection.
///
/// The `wake_events_pending` counter dedups events between the two monitor
/// threads. The sleep monitor increments it on resume before dispatching
/// `ResumeFromSleep`. The time-change monitor decrements and silently
/// filters when the counter is positive, attributing the kernel
/// cancel-on-set firing to the wake rather than to an independent clock
/// adjustment.
#[derive(Clone)]
struct SleepTracker {
    /// Whether the system is currently sleeping
    is_sleeping: Arc<AtomicBool>,
    /// Count of kernel cancel-on-set firings expected as a consequence of
    /// recent wakes.
    wake_events_pending: Arc<AtomicU8>,
}

impl SleepTracker {
    fn new() -> Self {
        Self {
            is_sleeping: Arc::new(AtomicBool::new(false)),
            wake_events_pending: Arc::new(AtomicU8::new(0)),
        }
    }

    /// Attribute one time-change event to a pending wake if any are expected.
    /// Returns true when the caller should silently filter the event.
    fn consume_pending_wake_event(&self) -> bool {
        self.wake_events_pending
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                (v > 0).then(|| v - 1)
            })
            .is_ok()
    }
}

/// Start system event monitoring in dedicated threads.
///
/// This function spawns two separate threads that monitor for:
/// - PrepareForSleep signals from systemd-logind (sleep/resume)
/// - System time changes using timerfd (clock adjustments, NTP sync)
///
/// When relevant events occur, they send appropriate SignalMessage to trigger
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
    interrupt: Arc<AtomicBool>,
    debug_enabled: bool,
) -> Result<()> {
    let sleep_tracker = SleepTracker::new();

    spawn_monitor_threads(signal_sender, interrupt, debug_enabled, 0, sleep_tracker);
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
    interrupt: Arc<AtomicBool>,
    debug_enabled: bool,
    restart_count: u8,
    sleep_tracker: SleepTracker,
) {
    const MAX_THREAD_RESTARTS: u8 = 3;
    const RESTART_DELAY_MS: u64 = 2000;

    let signal_sender_clone = signal_sender.clone();
    let interrupt_clone = interrupt.clone();
    let sleep_tracker_clone = sleep_tracker.clone();

    thread::spawn({
        let signal_sender = signal_sender.clone();
        let interrupt = interrupt.clone();
        let sleep_tracker = sleep_tracker.clone();
        move || {
            match monitor_sleep_signals(
                signal_sender.clone(),
                interrupt.clone(),
                debug_enabled,
                sleep_tracker.clone(),
            ) {
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
                            if let Err(e) = monitor_sleep_signals(
                                signal_sender,
                                interrupt,
                                debug_enabled,
                                sleep_tracker,
                            ) {
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

    thread::spawn(move || {
        if let Err(e) = monitor_time_changes(
            signal_sender_clone,
            interrupt_clone,
            debug_enabled,
            sleep_tracker_clone,
        ) {
            log_pipe!();
            log_warning!("Time change monitor error: {}", e);
            log_indented!("System time change detection will not be available");
            log_indented!("Sunsetr will continue to work normally otherwise");
        }
    });
}

/// Monitor PrepareForSleep signals using D-Bus in a dedicated thread
fn monitor_sleep_signals(
    signal_sender: Sender<SignalMessage>,
    interrupt: Arc<AtomicBool>,
    debug_enabled: bool,
    sleep_tracker: SleepTracker,
) -> Result<()> {
    let connection = Connection::system().context("Failed to connect to system D-Bus")?;

    if debug_enabled {
        log_debug!("Connected to system D-Bus successfully");
    }

    let logind_proxy =
        LogindManagerProxyBlocking::new(&connection).context("Failed to create logind proxy")?;

    let mut sleep_signals = logind_proxy
        .receive_prepare_for_sleep()
        .context("Failed to subscribe to PrepareForSleep signals")?;

    if debug_enabled {
        log_debug!("Subscribed to systemd-logind PrepareForSleep signals");
    }

    loop {
        match sleep_signals.next() {
            Some(signal) => {
                match signal.args() {
                    Ok(prepare_for_sleep_args) => {
                        let going_to_sleep: bool = prepare_for_sleep_args.start;

                        if going_to_sleep {
                            // Mark that we're sleeping FIRST (before any logging).
                            sleep_tracker.is_sleeping.store(true, Ordering::SeqCst);

                            log_pipe!();
                            log_info!("System entering sleep/suspend mode");
                            // Don't send a signal - let the main loop continue sleeping naturally
                        } else {
                            sleep_tracker
                                .wake_events_pending
                                .fetch_add(1, Ordering::SeqCst);
                            sleep_tracker.is_sleeping.store(false, Ordering::SeqCst);

                            log_pipe!();
                            log_info!("System resuming from sleep/suspend - reloading");

                            interrupt.store(true, Ordering::SeqCst);

                            match signal_sender.send(SignalMessage::ResumeFromSleep) {
                                Ok(_) => {}
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
/// Sets a timer far in the future with TFD_TIMER_CANCEL_ON_SET.
/// Any timer firing indicates a time change since it shouldn't expire naturally.
/// The SleepTracker filters out sleep-related timer events.
struct TimeChangeDetector {
    timer: TimerFd,
}

impl TimeChangeDetector {
    /// Creates a new time change detector.
    fn new() -> nix::Result<Self> {
        let timer = TimerFd::new(ClockId::CLOCK_REALTIME, TimerFlags::empty())?;
        let mut detector = TimeChangeDetector { timer };
        detector.arm_timer()?;
        Ok(detector)
    }

    /// Arms the timer for time change detection.
    fn arm_timer(&mut self) -> nix::Result<()> {
        let flags =
            TimerSetTimeFlags::TFD_TIMER_ABSTIME | TimerSetTimeFlags::TFD_TIMER_CANCEL_ON_SET;

        // i64::MAX is ~292 billion years from epoch; divide by 1000 for overflow safety.
        let far_future = TimeSpec::new(i64::MAX / 1000, 0);

        self.timer.set(Expiration::OneShot(far_future), flags)?;
        Ok(())
    }

    /// Waits for a timer event that indicates a potential time change.
    /// Any timer firing (expiration or ECANCELED) indicates the system time changed,
    /// since the timer is set far in the future.
    fn wait_for_time_change(&mut self) -> Result<()> {
        match self.timer.wait() {
            Ok(_) => {
                self.arm_timer()
                    .context("Failed to re-arm timer after expiration")?;
                Ok(())
            }
            Err(Errno::ECANCELED) => {
                self.arm_timer()
                    .context("Failed to re-arm timer after cancellation")?;
                Ok(())
            }
            Err(other_error) => {
                log_pipe!();
                log_error!("Timer wait returned error: {}", other_error);
                Err(anyhow::anyhow!("Timer wait error: {}", other_error))
            }
        }
    }
}

/// Monitor system time changes using timerfd in a dedicated thread
///
/// Uses a far-future timer that fires when system time changes.
/// The SleepTracker distinguishes real time changes from sleep/resume events.
///
/// Detects:
/// - Manual time adjustments (date command, timedatectl)
/// - NTP synchronization jumps
/// - System suspend/resume (filtered out by SleepTracker)
///
/// Does NOT detect:
/// - DST transitions (which don't change system time)
/// - Gradual NTP slewing adjustments
fn monitor_time_changes(
    signal_sender: Sender<SignalMessage>,
    interrupt: Arc<AtomicBool>,
    debug_enabled: bool,
    sleep_tracker: SleepTracker,
) -> Result<()> {
    if debug_enabled {
        log_pipe!();
        log_debug!("Starting timerfd-based time change monitoring");
    }

    let mut detector =
        TimeChangeDetector::new().context("Failed to create time change detector")?;

    loop {
        match detector.wait_for_time_change() {
            Ok(()) => {
                if sleep_tracker.consume_pending_wake_event() {
                    // Attributed to a recent wake - silently ignore
                } else if sleep_tracker.is_sleeping.load(Ordering::Relaxed) {
                    // System is currently sleeping - silently ignore
                } else {
                    // Real time change not related to sleep
                    log_pipe!();
                    log_info!("System time changed (clock adjustment/NTP/manual) - reloading");

                    interrupt.store(true, Ordering::SeqCst);

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
            }
            Err(e) => {
                log_pipe!();
                return Err(e).context("Time change detection failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consume_pending_wake_event_returns_false_when_no_wakes_recorded() {
        let tracker = SleepTracker::new();
        assert!(!tracker.consume_pending_wake_event());
        assert_eq!(tracker.wake_events_pending.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn consume_pending_wake_event_decrements_once_per_call_until_drained() {
        let tracker = SleepTracker::new();
        tracker.wake_events_pending.store(2, Ordering::SeqCst);

        assert!(tracker.consume_pending_wake_event());
        assert_eq!(tracker.wake_events_pending.load(Ordering::SeqCst), 1);

        assert!(tracker.consume_pending_wake_event());
        assert_eq!(tracker.wake_events_pending.load(Ordering::SeqCst), 0);

        assert!(!tracker.consume_pending_wake_event());
        assert_eq!(tracker.wake_events_pending.load(Ordering::SeqCst), 0);
    }
}
