//! Time source abstraction for supporting both real-time and simulated time.

use chrono::{DateTime, Duration as ChronoDuration, Local, TimeZone};
use once_cell::sync::OnceCell;
use std::sync::Arc;
use std::time::Duration as StdDuration;

static TIME_SOURCE: OnceCell<Arc<dyn TimeSource>> = OnceCell::new();

pub trait TimeSource: Send + Sync {
    fn now(&self) -> DateTime<Local>;
    fn sleep(&self, duration: StdDuration);
    fn is_simulated(&self) -> bool;
    fn is_ended(&self) -> bool {
        false
    }
}

pub struct RealTimeSource;

impl TimeSource for RealTimeSource {
    fn now(&self) -> DateTime<Local> {
        Local::now()
    }

    fn sleep(&self, duration: StdDuration) {
        std::thread::sleep(duration);
    }

    fn is_simulated(&self) -> bool {
        false
    }
}

/// How simulated time advances. FastForward jumps instantly through sleep
/// periods. Multiplier advances at a fixed acceleration in simulated seconds
/// per real second, and is always greater than zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SimulationPace {
    FastForward,
    Multiplier(f64),
}

/// Simulated time source for time-accelerated execution.
///
/// Linear mode advances time at the multiplier rate, deriving the current time
/// from accumulated sleep durations. Fast-forward mode jumps instantly through
/// sleep periods, tracking the current time directly.
pub struct SimulatedTimeSource {
    start_time: DateTime<Local>,
    end_time: DateTime<Local>,
    pace: SimulationPace,
    fast_forward_current: std::sync::Mutex<Option<DateTime<Local>>>,
    accumulated_sleep: std::sync::Mutex<StdDuration>,
    sleep_in_progress: std::sync::Mutex<Option<(std::time::Instant, StdDuration)>>,
}

impl SimulatedTimeSource {
    pub fn new(
        start_time: DateTime<Local>,
        end_time: DateTime<Local>,
        pace: SimulationPace,
    ) -> Self {
        let is_fast_forward = matches!(pace, SimulationPace::FastForward);
        Self {
            start_time,
            end_time,
            pace,
            fast_forward_current: std::sync::Mutex::new(if is_fast_forward {
                Some(start_time)
            } else {
                None
            }),
            accumulated_sleep: std::sync::Mutex::new(StdDuration::ZERO),
            sleep_in_progress: std::sync::Mutex::new(None),
        }
    }

    /// Current simulated time, accumulated from completed sleeps plus any
    /// in-progress sleep's partial progress.
    fn current_time(&self) -> DateTime<Local> {
        match self.pace {
            SimulationPace::FastForward => {
                let guard = self.fast_forward_current.lock().unwrap();
                guard.unwrap_or(self.end_time)
            }
            SimulationPace::Multiplier(mult) => {
                let accumulated = self.accumulated_sleep.lock().unwrap();
                let mut total_secs = accumulated.as_secs_f64();

                let sleep_guard = self.sleep_in_progress.lock().unwrap();
                if let Some((start_instant, simulated_duration)) = *sleep_guard {
                    let real_elapsed = start_instant.elapsed().as_secs_f64();
                    let simulated_elapsed = real_elapsed * mult;
                    let simulated_progress =
                        simulated_elapsed.min(simulated_duration.as_secs_f64());
                    total_secs += simulated_progress;
                }
                drop(sleep_guard);
                drop(accumulated);

                let simulated_elapsed = ChronoDuration::seconds(total_secs as i64)
                    + ChronoDuration::nanoseconds((total_secs.fract() * 1_000_000_000.0) as i64);

                let simulated = self.start_time + simulated_elapsed;
                if simulated > self.end_time {
                    self.end_time
                } else {
                    simulated
                }
            }
        }
    }

    pub fn is_ended(&self) -> bool {
        self.current_time() >= self.end_time
    }
}

impl TimeSource for SimulatedTimeSource {
    fn now(&self) -> DateTime<Local> {
        self.current_time()
    }

    fn sleep(&self, duration: StdDuration) {
        let mult = match self.pace {
            SimulationPace::FastForward => {
                let mut guard = self.fast_forward_current.lock().unwrap();
                if let Some(current) = *guard {
                    let new_time =
                        current + ChronoDuration::milliseconds(duration.as_millis() as i64);
                    *guard = Some(new_time.min(self.end_time));
                }
                std::thread::sleep(StdDuration::from_millis(1));
                return;
            }
            SimulationPace::Multiplier(mult) => mult,
        };

        let duration_to_add = {
            let accumulated = self.accumulated_sleep.lock().unwrap();
            let accumulated_secs = accumulated.as_secs_f64();

            let simulated_elapsed = ChronoDuration::seconds(accumulated_secs as i64)
                + ChronoDuration::nanoseconds((accumulated_secs.fract() * 1_000_000_000.0) as i64);
            let current_simulated = self.start_time + simulated_elapsed;

            if current_simulated >= self.end_time {
                StdDuration::ZERO
            } else {
                let remaining = self.end_time - current_simulated;
                let remaining_secs = remaining.num_seconds() as f64
                    + (remaining.num_nanoseconds().unwrap_or(0) as f64 / 1_000_000_000.0);

                if duration.as_secs_f64() > remaining_secs {
                    StdDuration::from_secs_f64(remaining_secs)
                } else {
                    duration
                }
            }
        };

        if duration_to_add > StdDuration::ZERO {
            // Record the sleep start so current_time() reflects partial progress.
            {
                let mut sleep_guard = self.sleep_in_progress.lock().unwrap();
                *sleep_guard = Some((std::time::Instant::now(), duration_to_add));
            }

            let real_sleep_secs = duration_to_add.as_secs_f64() / mult;
            if real_sleep_secs > 0.0 {
                std::thread::sleep(StdDuration::from_secs_f64(real_sleep_secs));
            }

            // Time advances only after the sleep completes.
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

    fn is_simulated(&self) -> bool {
        true
    }

    fn is_ended(&self) -> bool {
        self.is_ended()
    }
}

pub fn init_time_source(source: Arc<dyn TimeSource>) {
    TIME_SOURCE.set(source).ok();
}

pub fn is_initialized() -> bool {
    TIME_SOURCE.get().is_some()
}

pub fn now() -> DateTime<Local> {
    TIME_SOURCE.get_or_init(|| Arc::new(RealTimeSource)).now()
}

pub fn sleep(duration: StdDuration) {
    TIME_SOURCE
        .get_or_init(|| Arc::new(RealTimeSource))
        .sleep(duration)
}

pub fn is_simulated() -> bool {
    TIME_SOURCE
        .get_or_init(|| Arc::new(RealTimeSource))
        .is_simulated()
}

pub fn simulation_ended() -> bool {
    TIME_SOURCE
        .get_or_init(|| Arc::new(RealTimeSource))
        .is_ended()
}

pub fn parse_datetime(s: &str) -> Result<DateTime<Local>, String> {
    use chrono::NaiveDateTime;

    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .map(|naive| {
            Local::now()
                .timezone()
                .from_local_datetime(&naive)
                .single()
                .ok_or_else(|| "Ambiguous or invalid local time".to_string())
        })
        .map_err(|e| format!("Invalid datetime format: {e}. Use YYYY-MM-DD HH:MM:SS"))
        .and_then(|r| r)
}

pub fn parse_datetime_in_tz(s: &str, tz: chrono_tz::Tz) -> Result<DateTime<chrono_tz::Tz>, String> {
    use chrono::{NaiveDateTime, TimeZone};

    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .map(|naive| {
            tz.from_local_datetime(&naive)
                .single()
                .ok_or_else(|| format!("Ambiguous or invalid time in timezone {tz}"))
        })
        .map_err(|e| format!("Invalid datetime format: {e}. Use YYYY-MM-DD HH:MM:SS"))
        .and_then(|r| r)
}
