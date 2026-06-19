//! Transition schedule built once from config.
//!
//! `Schedule::from_config` is the single place `transition_mode` is read. A geo
//! schedule forwards to `GeoTimes` and a clock schedule evaluates the
//! `ClockWindows` wall-clock edges. Static mode has no schedule and is
//! represented as the absence of one (`None`).

use chrono::{DateTime, Duration, Local, NaiveDateTime, NaiveTime, TimeZone};
use std::time::Duration as StdDuration;

use crate::common::constants::DEFAULT_UPDATE_INTERVAL;
use crate::config::Config;
use crate::core::period::calculations::{
    adaptive_interval_for_geo, calculate_adaptive_interval, calculate_progress,
    calculate_transition_windows, is_time_in_range,
};
use crate::core::period::{Period, get_stable_period};
use crate::geo::times::GeoTimes;

/// A generator of transitions: geo by coordinate, or clock by wall time.
#[derive(Debug, Clone)]
pub enum Schedule {
    Geo(GeoTimes),
    Clock(ClockWindows),
}

/// The four clock-mode transition edges as wall-clock times.
///
/// Frozen from `calculate_transition_windows` at construction. Edges may cross
/// midnight; the query methods resolve that per call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockWindows {
    sunset_start: NaiveTime,
    sunset_end: NaiveTime,
    sunrise_start: NaiveTime,
    sunrise_end: NaiveTime,
}

impl Schedule {
    /// Build the schedule from config, reading `transition_mode` once.
    ///
    /// `None` for static mode, and for geo mode with no precomputed `GeoTimes`.
    pub fn from_config(config: &Config, geo_times: Option<GeoTimes>) -> Option<Schedule> {
        match (config.transition_mode.as_deref(), geo_times) {
            (Some("static"), _) => None,
            (Some("geo"), Some(times)) => Some(Schedule::Geo(times)),
            (Some("geo"), None) => None,
            _ => Some(Schedule::Clock(ClockWindows::from_config(config))),
        }
    }

    /// Period active at `now`.
    pub fn current_period(&self, now: DateTime<Local>) -> Period {
        match self {
            Schedule::Geo(times) => times.current_period(now),
            Schedule::Clock(windows) => windows.current_period(now.time()),
        }
    }

    /// Transition progress for `period` at `now`, or None when not transitioning.
    pub fn progress(&self, period: Period, now: DateTime<Local>) -> Option<f32> {
        match self {
            Schedule::Geo(times) => match period {
                Period::Sunset => times.sunset_progress(now),
                Period::Sunrise => times.sunrise_progress(now),
                _ => None,
            },
            Schedule::Clock(windows) => match period {
                Period::Sunset => windows.sunset_progress(now.time()),
                Period::Sunrise => windows.sunrise_progress(now.time()),
                _ => None,
            },
        }
    }

    /// Time until the next transition begins.
    ///
    /// Assumes `now` is outside a transition.
    pub fn time_until_next_transition(&self, now: DateTime<Local>) -> StdDuration {
        match self {
            Schedule::Geo(times) => times.time_until_next_transition(now),
            Schedule::Clock(windows) => windows.time_until_next_transition(now),
        }
    }

    /// Time until the current transition ends, or None when not transitioning.
    pub fn time_until_transition_end(&self, now: DateTime<Local>) -> Option<StdDuration> {
        match self {
            Schedule::Geo(times) => times.time_until_transition_end(now),
            Schedule::Clock(windows) => windows.time_until_transition_end(now),
        }
    }

    /// Start of the next period as an absolute local time.
    pub fn next_period_start(
        &self,
        period: Period,
        now: DateTime<Local>,
    ) -> Option<DateTime<Local>> {
        match self {
            Schedule::Geo(times) => match period.next_period() {
                Period::Sunset | Period::Sunrise => {
                    let duration = times.time_until_next_transition(now);
                    Some(now + Duration::from_std(duration).ok()?)
                }
                Period::Day | Period::Night => times
                    .time_until_transition_end(now)
                    .and_then(|duration| Duration::from_std(duration).ok())
                    .map(|duration| now + duration),
                Period::Static => None,
            },
            Schedule::Clock(windows) => windows.next_period_start(period, now),
        }
    }

    /// Adaptive update interval in seconds for an in-progress transition.
    ///
    /// None outside a transition, since the quantity is only defined while
    /// Sunset or Sunrise is interpolating.
    pub fn adaptive_interval(
        &self,
        config: &Config,
        period: Period,
        now: DateTime<Local>,
    ) -> Option<u64> {
        match self {
            Schedule::Geo(times) => {
                let (start, end) = match period {
                    Period::Sunset => (times.sunset_start, times.sunset_end),
                    Period::Sunrise => (times.sunrise_start, times.sunrise_end),
                    _ => return None,
                };
                Some(adaptive_interval_for_geo(config, start, end, now))
            }
            Schedule::Clock(windows) => {
                let (start, end) = match period {
                    Period::Sunset => (windows.sunset_start, windows.sunset_end),
                    Period::Sunrise => (windows.sunrise_start, windows.sunrise_end),
                    _ => return None,
                };
                Some(calculate_adaptive_interval(config, start, end, now.time()))
            }
        }
    }

    /// Time the main loop should sleep before its next wake.
    ///
    /// While transitioning this is the update-interval tick so progress stays
    /// smooth; otherwise it is the time until the next transition begins.
    pub fn time_until_next_event(
        &self,
        config: &Config,
        period: Period,
        now: DateTime<Local>,
    ) -> StdDuration {
        if period.is_transitioning() {
            let secs = match &config.update_interval {
                Some(crate::config::UpdateInterval::Fixed(s)) => *s,
                _ => DEFAULT_UPDATE_INTERVAL,
            };
            StdDuration::from_secs(secs)
        } else {
            self.time_until_next_transition(now)
        }
    }
}

impl ClockWindows {
    /// Freeze the clock-mode edges from config.
    pub fn from_config(config: &Config) -> ClockWindows {
        let (sunset_start, sunset_end, sunrise_start, sunrise_end) =
            calculate_transition_windows(config);
        ClockWindows {
            sunset_start,
            sunset_end,
            sunrise_start,
            sunrise_end,
        }
    }

    fn current_period(&self, now: NaiveTime) -> Period {
        if is_time_in_range(now, self.sunset_start, self.sunset_end) {
            Period::Sunset
        } else if is_time_in_range(now, self.sunrise_start, self.sunrise_end) {
            Period::Sunrise
        } else {
            get_stable_period(now, self.sunset_end, self.sunrise_start)
        }
    }

    fn sunset_progress(&self, now: NaiveTime) -> Option<f32> {
        if is_time_in_range(now, self.sunset_start, self.sunset_end) {
            Some(calculate_progress(now, self.sunset_start, self.sunset_end))
        } else {
            None
        }
    }

    fn sunrise_progress(&self, now: NaiveTime) -> Option<f32> {
        if is_time_in_range(now, self.sunrise_start, self.sunrise_end) {
            Some(calculate_progress(
                now,
                self.sunrise_start,
                self.sunrise_end,
            ))
        } else {
            None
        }
    }

    fn time_until_next_transition(&self, now: DateTime<Local>) -> StdDuration {
        let next = [self.sunset_start, self.sunrise_start]
            .into_iter()
            .filter_map(|edge| next_occurrence(edge, now))
            .min();

        match next {
            Some(dt) => {
                let millis = dt.signed_duration_since(now).num_milliseconds().max(0) as u64;
                StdDuration::from_millis(millis)
            }
            None => StdDuration::from_secs(0),
        }
    }

    fn time_until_transition_end<Tz: TimeZone>(&self, now: DateTime<Tz>) -> Option<StdDuration> {
        let end = match self.current_period(now.time()) {
            Period::Sunset => self.sunset_end,
            Period::Sunrise => self.sunrise_end,
            _ => return None,
        };

        let today = now.date_naive();
        let end_date = if end < now.time() {
            today + Duration::days(1)
        } else {
            today
        };
        let end_dt = resolve_local(&now.timezone(), end_date.and_time(end))?;

        let millis = end_dt.signed_duration_since(now).num_milliseconds().max(0) as u64;
        Some(StdDuration::from_millis(millis))
    }

    fn next_period_start(&self, period: Period, now: DateTime<Local>) -> Option<DateTime<Local>> {
        let edge = match period.next_period() {
            Period::Sunset => self.sunset_start,
            Period::Night => self.sunset_end,
            Period::Sunrise => self.sunrise_start,
            Period::Day => self.sunrise_end,
            Period::Static => return None,
        };
        next_occurrence(edge, now)
    }
}

/// Next strictly-future occurrence of `target`, today or tomorrow.
///
/// The `>` is strict on purpose. `current_period` owns the start instant as the
/// inclusive edge of `[start, end)`, so this must not relax to `>=`.
fn next_occurrence<Tz: TimeZone>(target: NaiveTime, now: DateTime<Tz>) -> Option<DateTime<Tz>> {
    let today = now.date_naive();
    let tomorrow = today + Duration::days(1);

    [today.and_time(target), tomorrow.and_time(target)]
        .into_iter()
        .filter(|dt| *dt > now.naive_local())
        .min()
        .and_then(|naive_dt| resolve_local(&now.timezone(), naive_dt))
}

/// Resolve a wall-clock time to a concrete instant in `tz`.
///
/// At a daylight-saving fold (a repeated hour) the time is ambiguous, so
/// `earliest` resolves it to a concrete instant. At a gap (a skipped hour) it
/// is nonexistent and the result is None. Generic over the zone so the
/// resolution is testable without changing the process timezone.
fn resolve_local<Tz: TimeZone>(tz: &Tz, naive: NaiveDateTime) -> Option<DateTime<Tz>> {
    tz.from_local_datetime(&naive).earliest()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UpdateInterval;

    fn clock_config(mode: &str, sunset: &str, sunrise: &str) -> Config {
        Config {
            backend: Some(crate::config::Backend::Auto),
            smoothing: Some(false),
            startup_duration: Some(10.0),
            shutdown_duration: Some(10.0),
            startup_transition: Some(false),
            startup_transition_duration: Some(10.0),
            start_hyprsunset: None,
            adaptive_interval: None,
            latitude: None,
            longitude: None,
            sunset: Some(sunset.to_string()),
            sunrise: Some(sunrise.to_string()),
            night_temp: Some(3300),
            day_temp: Some(6500),
            night_gamma: Some(90.0),
            day_gamma: Some(100.0),
            static_temp: None,
            static_gamma: None,
            transition_duration: Some(30),
            update_interval: Some(UpdateInterval::Adaptive),
            transition_mode: Some(mode.to_string()),
        }
    }

    fn clock_schedule(mode: &str, sunset: &str, sunrise: &str) -> Schedule {
        Schedule::from_config(&clock_config(mode, sunset, sunrise), None)
            .expect("clock mode yields a schedule")
    }

    #[test]
    fn from_config_geo_without_times_is_none() {
        // The test command builds a day RuntimeState without geo times; a geo
        // config must not route into the clock-only window math and panic.
        let config = clock_config("geo", "19:00:00", "06:00:00");
        assert!(Schedule::from_config(&config, None).is_none());
    }

    fn local_at(hour: u32, min: u32) -> DateTime<Local> {
        let today = crate::time::source::now().date_naive();
        Local
            .from_local_datetime(&today.and_hms_opt(hour, min, 0).unwrap())
            .single()
            .unwrap()
    }

    #[test]
    fn from_config_clock_mode_is_some_clock() {
        let schedule =
            Schedule::from_config(&clock_config("finish_by", "19:00:00", "06:00:00"), None);
        assert!(matches!(schedule, Some(Schedule::Clock(_))));
    }

    #[test]
    fn from_config_static_mode_is_none() {
        let config = clock_config("static", "19:00:00", "06:00:00");
        assert!(Schedule::from_config(&config, None).is_none());
    }

    #[test]
    fn clock_windows_finish_by_edges() {
        let windows = ClockWindows::from_config(&clock_config("finish_by", "19:00:00", "06:00:00"));
        assert_eq!(
            windows.sunset_start,
            NaiveTime::from_hms_opt(18, 30, 0).unwrap()
        );
        assert_eq!(
            windows.sunset_end,
            NaiveTime::from_hms_opt(19, 0, 0).unwrap()
        );
        assert_eq!(
            windows.sunrise_start,
            NaiveTime::from_hms_opt(5, 30, 0).unwrap()
        );
        assert_eq!(
            windows.sunrise_end,
            NaiveTime::from_hms_opt(6, 0, 0).unwrap()
        );
    }

    #[test]
    fn clock_current_period_normal_schedule() {
        let schedule = clock_schedule("finish_by", "19:00:00", "06:00:00");
        assert_eq!(schedule.current_period(local_at(12, 0)), Period::Day);
        assert_eq!(schedule.current_period(local_at(18, 45)), Period::Sunset);
        assert_eq!(schedule.current_period(local_at(3, 0)), Period::Night);
        assert_eq!(schedule.current_period(local_at(5, 45)), Period::Sunrise);
    }

    #[test]
    fn clock_current_period_inverted_schedule() {
        // Overnight worker: warm period spans the daytime hours.
        let schedule = clock_schedule("finish_by", "07:00:00", "19:00:00");
        assert_eq!(schedule.current_period(local_at(12, 0)), Period::Night);
        assert_eq!(schedule.current_period(local_at(0, 0)), Period::Day);
        assert_eq!(schedule.current_period(local_at(6, 45)), Period::Sunset);
        assert_eq!(schedule.current_period(local_at(18, 45)), Period::Sunrise);
    }

    #[test]
    fn clock_progress_only_within_window() {
        let schedule = clock_schedule("finish_by", "19:00:00", "06:00:00");

        let mid = schedule.progress(Period::Sunset, local_at(18, 45));
        assert!(mid.is_some_and(|p| p > 0.0 && p < 1.0));

        assert!(schedule.progress(Period::Sunset, local_at(12, 0)).is_none());
        assert!(schedule.progress(Period::Day, local_at(18, 45)).is_none());
    }

    #[test]
    fn clock_next_period_start_from_day_is_today_sunset() {
        let schedule = clock_schedule("finish_by", "19:00:00", "06:00:00");
        let now = local_at(12, 0);
        let next = schedule.next_period_start(Period::Day, now).unwrap();
        assert_eq!(next, local_at(18, 30));
    }

    #[test]
    fn clock_time_until_next_transition_picks_nearest_start() {
        let schedule = clock_schedule("finish_by", "19:00:00", "06:00:00");
        let now = local_at(12, 0);
        // Nearest future start is today's sunset_start at 18:30, 6.5 hours away.
        let duration = schedule.time_until_next_transition(now);
        assert_eq!(duration, StdDuration::from_secs(6 * 3600 + 30 * 60));
    }

    #[test]
    fn clock_time_until_transition_end_none_outside_transition() {
        let schedule = clock_schedule("finish_by", "19:00:00", "06:00:00");
        assert!(
            schedule
                .time_until_transition_end(local_at(12, 0))
                .is_none()
        );
        assert!(
            schedule
                .time_until_transition_end(local_at(18, 45))
                .is_some()
        );
    }

    #[test]
    fn clock_current_period_across_midnight() {
        // start_at: the sunset window 23:45 -> 00:15 crosses midnight.
        let schedule = clock_schedule("start_at", "23:45:00", "06:00:00");
        assert_eq!(schedule.current_period(local_at(23, 50)), Period::Sunset);
        assert_eq!(schedule.current_period(local_at(0, 5)), Period::Sunset);
        assert_eq!(schedule.current_period(local_at(0, 30)), Period::Night);
        assert_eq!(schedule.current_period(local_at(12, 0)), Period::Day);
    }

    #[test]
    fn clock_time_until_transition_end_across_midnight() {
        let schedule = clock_schedule("start_at", "23:45:00", "06:00:00");
        // Before midnight inside the window: the end rolls to tomorrow.
        assert_eq!(
            schedule.time_until_transition_end(local_at(23, 50)),
            Some(StdDuration::from_secs(25 * 60))
        );
        // After midnight inside the window: the end is later today.
        assert_eq!(
            schedule.time_until_transition_end(local_at(0, 5)),
            Some(StdDuration::from_secs(10 * 60))
        );
    }

    #[test]
    fn resolve_local_handles_dst_transitions() {
        use chrono_tz::America::New_York;

        // Auto-locate the next daylight-saving gap and fold
        let start = crate::time::source::now().date_naive();
        let mut gap = None;
        let mut fold = None;
        for d in 0..400 {
            if gap.is_some() && fold.is_some() {
                break;
            }
            let day = start + Duration::days(d);
            for h in 0..24 {
                let naive = day.and_hms_opt(h, 30, 0).unwrap();
                let mapped = New_York.from_local_datetime(&naive);
                if mapped.single().is_none() {
                    if mapped.earliest().is_some() {
                        if fold.is_none() {
                            fold = Some(naive);
                        }
                    } else if gap.is_none() {
                        gap = Some(naive);
                    }
                }
            }
        }
        let gap = gap.expect("a DST gap within a year");
        let fold = fold.expect("a DST fold within a year");

        // The fold is ambiguous to single() but the resolver recovers an instant.
        assert!(New_York.from_local_datetime(&fold).single().is_none());
        assert!(resolve_local(&New_York, fold).is_some());

        // The gap time never occurs, so it stays unresolved.
        assert!(resolve_local(&New_York, gap).is_none());
    }

    #[test]
    fn time_until_transition_end_recovers_fold() {
        use chrono_tz::America::New_York;

        // Locate a fold to place a transition end on its repeated wall time.
        let start = crate::time::source::now().date_naive();
        let fold = (0..400)
            .flat_map(|d| {
                let day = start + Duration::days(d);
                (0..24).map(move |h| day.and_hms_opt(h, 30, 0).unwrap())
            })
            .find(|naive| {
                let mapped = New_York.from_local_datetime(naive);
                mapped.single().is_none() && mapped.earliest().is_some()
            })
            .expect("a DST fold within a year");

        let windows = ClockWindows {
            sunset_start: NaiveTime::from_hms_opt(18, 30, 0).unwrap(),
            sunset_end: NaiveTime::from_hms_opt(19, 0, 0).unwrap(),
            sunrise_start: fold.time() - Duration::minutes(30),
            sunrise_end: fold.time(),
        };
        let now = resolve_local(
            &New_York,
            fold.date().and_time(fold.time() - Duration::minutes(10)),
        )
        .expect("an instant inside the sunrise window");

        // current_period reports the transition, and a bare single() on the end
        // edge drops it, yet the method still reports the time remaining.
        assert_eq!(windows.current_period(now.time()), Period::Sunrise);
        assert!(New_York.from_local_datetime(&fold).single().is_none());
        assert!(windows.time_until_transition_end(now).is_some());
    }

    #[test]
    fn adaptive_interval_some_only_during_transition() {
        let config = clock_config("finish_by", "19:00:00", "06:00:00");
        let schedule = Schedule::from_config(&config, None).unwrap();
        assert!(
            schedule
                .adaptive_interval(&config, Period::Sunset, local_at(18, 45))
                .is_some_and(|secs| secs >= 1)
        );
        assert!(
            schedule
                .adaptive_interval(&config, Period::Day, local_at(12, 0))
                .is_none()
        );
    }
}
