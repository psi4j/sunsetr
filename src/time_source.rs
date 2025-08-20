//! Time source abstraction for supporting both real-time and simulated time.
//!
//! This module provides a trait-based abstraction that allows the application
//! to use either real system time or simulated time for testing purposes.
//! The simulation mode is particularly useful for testing geo mode and other
//! time-dependent functionality without waiting for actual time to pass.

use chrono::{DateTime, Duration as ChronoDuration, Local, TimeZone};
use once_cell::sync::OnceCell;
use std::sync::Arc;
use std::time::{Duration as StdDuration, SystemTime};

/// Global time source instance, defaults to RealTimeSource
static TIME_SOURCE: OnceCell<Arc<dyn TimeSource>> = OnceCell::new();

/// Trait for abstracting time operations
pub trait TimeSource: Send + Sync {
    /// Get the current time
    fn now(&self) -> DateTime<Local>;

    /// Get the current system time (for duration calculations)
    fn system_now(&self) -> SystemTime;

    /// Sleep for the specified duration (or simulate it)
    fn sleep(&self, duration: StdDuration);

    /// Check if this is a simulated time source
    fn is_simulated(&self) -> bool;

    /// Check if simulation has ended (always false for real time)
    fn is_ended(&self) -> bool {
        false
    }
}

/// Real-time implementation that uses actual system time
pub struct RealTimeSource;

impl TimeSource for RealTimeSource {
    fn now(&self) -> DateTime<Local> {
        Local::now()
    }

    fn system_now(&self) -> SystemTime {
        SystemTime::now()
    }

    fn sleep(&self, duration: StdDuration) {
        std::thread::sleep(duration);
    }

    fn is_simulated(&self) -> bool {
        false
    }
}

/// Simulated time source for testing and time-accelerated execution.
///
/// This implementation supports two modes:
/// - Linear acceleration: Time flows continuously at a constant multiplier rate
/// - Fast-forward: Time jumps instantly through sleep periods (multiplier = 0.0)
pub struct SimulatedTimeSource {
    /// The starting time for the simulation
    start_time: DateTime<Local>,
    /// The target end time for the simulation
    end_time: DateTime<Local>,
    /// Time acceleration factor (e.g., 60.0 = 1 minute per second)
    /// Special value 0.0 means fast-forward mode
    time_multiplier: f64,
    /// In fast-forward mode, track the current simulated time
    fast_forward_current: std::sync::Mutex<Option<DateTime<Local>>>,
    /// Track accumulated sleep time for accurate timestamps.
    /// Updated only after sleep completes to ensure consistent time progression
    accumulated_sleep: std::sync::Mutex<StdDuration>,
    /// Track in-progress sleep: (start instant, simulated duration being slept).
    /// Used to calculate smooth time progression during long sleep periods
    sleep_in_progress: std::sync::Mutex<Option<(std::time::Instant, StdDuration)>>,
}

impl SimulatedTimeSource {
    /// Create a new simulated time source
    ///
    /// # Arguments
    /// * `start_time` - Starting time for the simulation
    /// * `end_time` - Ending time for the simulation  
    /// * `multiplier` - Time acceleration (e.g., 60.0 = 1 simulated minute per real second)
    ///   0.0 means fast-forward mode
    pub fn new(start_time: DateTime<Local>, end_time: DateTime<Local>, multiplier: f64) -> Self {
        let is_fast_forward = multiplier == 0.0;
        Self {
            start_time,
            end_time,
            time_multiplier: if is_fast_forward {
                0.0 // Fast-forward mode
            } else if multiplier <= 0.0 {
                3600.0 // Default to 1 hour per second
            } else {
                multiplier
            },
            fast_forward_current: std::sync::Mutex::new(if is_fast_forward {
                Some(start_time)
            } else {
                None
            }),
            accumulated_sleep: std::sync::Mutex::new(StdDuration::ZERO),
            sleep_in_progress: std::sync::Mutex::new(None),
        }
    }

    /// Get the current simulated time based on accumulated sleep time.
    ///
    /// For in-progress sleeps, this calculates the partial progress to provide
    /// smooth time advancement for progress monitoring and other time queries
    fn current_time(&self) -> DateTime<Local> {
        // Fast-forward mode: return the manually tracked time
        if self.time_multiplier == 0.0 {
            let guard = self.fast_forward_current.lock().unwrap();
            guard.unwrap_or(self.end_time)
        } else {
            // Normal mode: calculate based on accumulated sleep time plus any in-progress sleep
            let accumulated = self.accumulated_sleep.lock().unwrap();
            let mut total_secs = accumulated.as_secs_f64();

            // Check if there's a sleep in progress and add its elapsed portion
            let sleep_guard = self.sleep_in_progress.lock().unwrap();
            if let Some((start_instant, simulated_duration)) = *sleep_guard {
                // Calculate how much of the sleep has elapsed in real time
                let real_elapsed = start_instant.elapsed().as_secs_f64();
                // Convert to simulated time based on multiplier
                let simulated_elapsed = real_elapsed * self.time_multiplier;
                // Cap at the total duration of the sleep
                let simulated_progress = simulated_elapsed.min(simulated_duration.as_secs_f64());
                total_secs += simulated_progress;
            }
            drop(sleep_guard);
            drop(accumulated);

            // Convert total sleep time to chrono duration
            let simulated_elapsed = ChronoDuration::seconds(total_secs as i64)
                + ChronoDuration::nanoseconds((total_secs.fract() * 1_000_000_000.0) as i64);

            // Add to start time and cap at end time
            let simulated = self.start_time + simulated_elapsed;
            if simulated > self.end_time {
                self.end_time
            } else {
                simulated
            }
        }
    }

    /// Check if the simulation has reached its end time
    pub fn is_ended(&self) -> bool {
        self.current_time() >= self.end_time
    }
}

impl TimeSource for SimulatedTimeSource {
    fn now(&self) -> DateTime<Local> {
        self.current_time()
    }

    fn system_now(&self) -> SystemTime {
        // Convert current simulated time to SystemTime
        let current = self.current_time();
        SystemTime::UNIX_EPOCH + StdDuration::from_millis(current.timestamp_millis() as u64)
    }

    fn sleep(&self, duration: StdDuration) {
        if self.time_multiplier == 0.0 {
            // Fast-forward mode: advance time by exactly the requested duration
            // The main loop will handle checking at appropriate intervals
            let mut guard = self.fast_forward_current.lock().unwrap();
            if let Some(current) = *guard {
                let new_time = current + ChronoDuration::milliseconds(duration.as_millis() as i64);
                *guard = Some(new_time.min(self.end_time));
            }
            // Minimal sleep to allow other threads to run and logs to be output
            std::thread::sleep(StdDuration::from_millis(1));
        } else {
            // Linear acceleration mode: sleep for scaled real duration.
            // Cap at end time to ensure clean termination
            let duration_to_add = {
                let accumulated = self.accumulated_sleep.lock().unwrap();
                let accumulated_secs = accumulated.as_secs_f64();

                // Calculate current simulated time
                let simulated_elapsed = ChronoDuration::seconds(accumulated_secs as i64)
                    + ChronoDuration::nanoseconds(
                        (accumulated_secs.fract() * 1_000_000_000.0) as i64,
                    );
                let current_simulated = self.start_time + simulated_elapsed;

                // Check if we would exceed end time
                if current_simulated >= self.end_time {
                    // Already at or past end time, don't sleep
                    StdDuration::ZERO
                } else {
                    let remaining = self.end_time - current_simulated;
                    let remaining_secs = remaining.num_seconds() as f64
                        + (remaining.num_nanoseconds().unwrap_or(0) as f64 / 1_000_000_000.0);

                    // Use the smaller of requested duration or remaining time
                    if duration.as_secs_f64() > remaining_secs {
                        StdDuration::from_secs_f64(remaining_secs)
                    } else {
                        duration
                    }
                }
            };

            // Perform the sleep with progress tracking
            if duration_to_add > StdDuration::ZERO {
                // Mark the start of this sleep for smooth progress tracking
                {
                    let mut sleep_guard = self.sleep_in_progress.lock().unwrap();
                    *sleep_guard = Some((std::time::Instant::now(), duration_to_add));
                }

                // Sleep for the scaled real duration
                let real_sleep_secs = duration_to_add.as_secs_f64() / self.time_multiplier;
                if real_sleep_secs > 0.0 {
                    std::thread::sleep(StdDuration::from_secs_f64(real_sleep_secs));
                }

                // After sleeping completes, clear the in-progress marker and update accumulated time.
                // This ensures time only advances after the sleep actually completes
                {
                    let mut sleep_guard = self.sleep_in_progress.lock().unwrap();
                    *sleep_guard = None;
                }
                {
                    let mut accumulated = self.accumulated_sleep.lock().unwrap();
                    *accumulated += duration_to_add;
                }
            }
        }
    }

    fn is_simulated(&self) -> bool {
        true
    }

    fn is_ended(&self) -> bool {
        self.is_ended()
    }
}

/// Initialize the global time source (call once at startup)
pub fn init_time_source(source: Arc<dyn TimeSource>) {
    TIME_SOURCE.set(source).ok();
}

/// Check if the time source has been initialized
pub fn is_initialized() -> bool {
    TIME_SOURCE.get().is_some()
}

/// Get the current time from the global time source
pub fn now() -> DateTime<Local> {
    TIME_SOURCE.get_or_init(|| Arc::new(RealTimeSource)).now()
}

/// Get the current system time from the global time source
pub fn system_now() -> SystemTime {
    TIME_SOURCE
        .get_or_init(|| Arc::new(RealTimeSource))
        .system_now()
}

/// Sleep for the specified duration using the global time source
pub fn sleep(duration: StdDuration) {
    TIME_SOURCE
        .get_or_init(|| Arc::new(RealTimeSource))
        .sleep(duration)
}

/// Check if we're running in simulation mode
pub fn is_simulated() -> bool {
    TIME_SOURCE
        .get_or_init(|| Arc::new(RealTimeSource))
        .is_simulated()
}

/// Check if simulation has reached its end time (always false for real time)
pub fn simulation_ended() -> bool {
    TIME_SOURCE
        .get_or_init(|| Arc::new(RealTimeSource))
        .is_ended()
}

/// Parse a datetime string in the format "YYYY-MM-DD HH:MM:SS"
pub fn parse_datetime(s: &str) -> Result<DateTime<Local>, String> {
    use chrono::NaiveDateTime;

    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .map(|naive| {
            // Convert to local timezone
            Local::now()
                .timezone()
                .from_local_datetime(&naive)
                .single()
                .ok_or_else(|| "Ambiguous or invalid local time".to_string())
        })
        .map_err(|e| format!("Invalid datetime format: {e}. Use YYYY-MM-DD HH:MM:SS"))
        .and_then(|r| r)
}

/// Parse a datetime string in a specific timezone
pub fn parse_datetime_in_tz(s: &str, tz: chrono_tz::Tz) -> Result<DateTime<chrono_tz::Tz>, String> {
    use chrono::{NaiveDateTime, TimeZone};

    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .map(|naive| {
            // Convert to specified timezone
            tz.from_local_datetime(&naive)
                .single()
                .ok_or_else(|| format!("Ambiguous or invalid time in timezone {tz}"))
        })
        .map_err(|e| format!("Invalid datetime format: {e}. Use YYYY-MM-DD HH:MM:SS"))
        .and_then(|r| r)
}
