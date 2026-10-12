use super::*;
use crate::backend::ColorTemperatureBackend;
use crate::config::{Backend, Config, TransitionMode, UpdateInterval};
use crate::core::context::Context;
use crate::core::period::Period;
use crate::core::runtime_state::{
    AdjustOp, AdjustRequest, Adjustment, FieldAdjustment, RuntimeState,
};
use crate::io::signals::SignalState;
use serial_test::serial;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Backend stub that records the last temperature/gamma it was asked to apply.
struct CaptureBackend {
    last: Arc<Mutex<(u32, f64)>>,
}

impl ColorTemperatureBackend for CaptureBackend {
    fn apply_transition_state(
        &mut self,
        runtime_state: &RuntimeState,
        _running: &AtomicBool,
    ) -> Result<()> {
        let (t, g) = runtime_state.values();
        *self.last.lock().unwrap() = (t, g);
        Ok(())
    }

    fn apply_startup_state(
        &mut self,
        runtime_state: &RuntimeState,
        running: &AtomicBool,
    ) -> Result<()> {
        self.apply_transition_state(runtime_state, running)
    }

    fn apply_temperature_gamma(
        &mut self,
        temperature: u32,
        gamma: f64,
        _running: &AtomicBool,
    ) -> Result<()> {
        *self.last.lock().unwrap() = (temperature, gamma);
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "Wayland"
    }
}

fn static_mode_config() -> Config {
    Config {
        backend: Backend::Wayland,
        transition_mode: TransitionMode::Static,
        smoothing: true,
        startup_duration: 0.2,
        shutdown_duration: 0.2,
        adaptive_interval: 50,
        night_temp: 3300,
        day_temp: 6500,
        night_gamma: 90.0,
        day_gamma: 100.0,
        update_interval: UpdateInterval::Fixed(60),
        static_temp: Some(6500),
        static_gamma: Some(100.0),
        sunset: None,
        sunrise: None,
        transition_duration: crate::common::constants::DEFAULT_TRANSITION_DURATION_MIN,
        latitude: None,
        longitude: None,
    }
}

fn geo_adaptive_config() -> Config {
    Config {
        backend: Backend::Wayland,
        transition_mode: TransitionMode::Geo,
        smoothing: true,
        startup_duration: 0.2,
        shutdown_duration: 0.2,
        adaptive_interval: 50,
        night_temp: 3300,
        day_temp: 6500,
        night_gamma: 90.0,
        day_gamma: 100.0,
        update_interval: UpdateInterval::Adaptive,
        static_temp: Some(6500),
        static_gamma: Some(100.0),
        sunset: None,
        sunrise: None,
        transition_duration: crate::common::constants::DEFAULT_TRANSITION_DURATION_MIN,
        latitude: Some(51.5074),
        longitude: Some(-0.1278),
    }
}

fn empty_signal_state() -> SignalState {
    let (signal_sender, signal_receiver) = std::sync::mpsc::channel();
    SignalState {
        running: Arc::new(AtomicBool::new(true)),
        signal_receiver,
        signal_sender,
        interrupt: Arc::new(AtomicBool::new(false)),
        in_test_mode: Arc::new(AtomicBool::new(false)),
        instant_shutdown: Arc::new(AtomicBool::new(false)),
        current_preset: Arc::new(Mutex::new(None)),
    }
}

/// `recover_state` must clear the shared interrupt flag at entry. Otherwise
/// its own `SmoothTransition::reload` reads the flag on the first frame and
/// aborts after applying only the start values.
#[test]
#[serial]
fn recover_state_does_not_self_abort_on_pre_set_interrupt() {
    let config = static_mode_config();
    let last = Arc::new(Mutex::new((0u32, 0.0f64)));

    let backend = Box::new(CaptureBackend { last: last.clone() });
    let runtime_state = RuntimeState::new(
        Period::Night,
        &config,
        crate::core::schedule::Schedule::from_config(&config, None),
        chrono::Local::now(),
    );
    let signal_state = empty_signal_state();
    let interrupt = signal_state.interrupt.clone();

    // Reproduce what the dbus monitor did just before sending
    // ResumeFromSleep into the channel.
    interrupt.store(true, Ordering::SeqCst);

    let mut core = Core::new(CoreParams {
        backend,
        runtime_state,
        signal_state,
        debug_enabled: false,
        bypass_smoothing: false,
        ipc_notifier: None,
    });

    let mut tracker = Context::new();
    core.recover_state(&mut tracker, "wake")
        .expect("recover_state returned an error");

    let (temp, gamma) = *last.lock().unwrap();
    assert_eq!(
        temp, 6500,
        "recovery transition stopped at the start (night) temperature; \
         it self-aborted on the pre-set interrupt"
    );
    assert!(
        (gamma - 100.0).abs() < 0.01,
        "recovery transition stopped at the start (night) gamma {}; \
         it self-aborted on the pre-set interrupt",
        gamma,
    );
    assert!(
        !interrupt.load(Ordering::SeqCst),
        "interrupt flag should be cleared by recover_state"
    );
}

/// A reload that moves only future timing, with the current values, period,
/// and preset all unchanged, must still be adopted. A discarded reload leaves
/// the instance sleeping toward the old schedule's boundaries and reporting
/// the old next-period time over IPC.
#[test]
#[serial]
fn reload_adopts_config_that_changes_only_future_timing() {
    let now = chrono::Local::now();
    let clock_config = |sunset_offset_hours: i64| {
        let mut config = static_mode_config();
        config.transition_mode = TransitionMode::Center;
        config.sunrise = Some(
            (now - chrono::Duration::hours(6))
                .format("%H:%M:%S")
                .to_string(),
        );
        config.sunset = Some(
            (now + chrono::Duration::hours(sunset_offset_hours))
                .format("%H:%M:%S")
                .to_string(),
        );
        config
    };

    let config = clock_config(6);
    let schedule = crate::core::schedule::Schedule::from_config(&config, None);
    let period = schedule
        .as_ref()
        .expect("clock config yields a schedule")
        .current_period(now);
    let runtime_state = RuntimeState::new(period, &config, schedule, now);

    let signal_state = empty_signal_state();
    *signal_state.current_preset.lock().unwrap() =
        crate::state::preset::get_active_preset().ok().flatten();

    let last = Arc::new(Mutex::new((0u32, 0.0f64)));
    let mut core = Core::new(CoreParams {
        backend: Box::new(CaptureBackend { last }),
        runtime_state,
        signal_state,
        debug_enabled: false,
        bypass_smoothing: false,
        ipc_notifier: None,
    });

    let new_config = clock_config(5);
    core.handle_config_reload(new_config.clone())
        .expect("reload returned an error");

    assert_eq!(
        core.runtime_state.config().sunset,
        new_config.sunset,
        "reload with unchanged values and period discarded the new config"
    );
}

/// The adaptive update interval must position the current time within the same
/// timezone frame as the transition window. When the configured coordinates sit
/// in a different timezone than the system clock, mixing a coordinate-frame
/// current time with a local-frame window pins the position to the window end
/// and floors the interval to 1 second, forcing an update every second.
#[test]
fn adaptive_interval_uses_coordinate_frame_for_geo() {
    use crate::geo::solar::SolarTimes;
    use crate::geo::times::GeoTimes;
    use chrono::{Local, NaiveTime, TimeZone};
    use std::time::Duration as StdDuration;

    let solar_result = SolarTimes {
        sunset_time: NaiveTime::from_hms_opt(19, 30, 0).unwrap(),
        sunrise_time: NaiveTime::from_hms_opt(5, 30, 0).unwrap(),
        sunset_duration: StdDuration::from_secs(3600),
        sunrise_duration: StdDuration::from_secs(3600),
        sunset_plus_10_start: NaiveTime::from_hms_opt(19, 0, 0).unwrap(),
        sunset_minus_2_end: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
        sunrise_minus_2_start: NaiveTime::from_hms_opt(5, 0, 0).unwrap(),
        sunrise_plus_10_end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
        civil_dawn: NaiveTime::from_hms_opt(4, 45, 0).unwrap(),
        civil_dusk: NaiveTime::from_hms_opt(20, 15, 0).unwrap(),
        golden_hour_start: NaiveTime::from_hms_opt(18, 30, 0).unwrap(),
        golden_hour_end: NaiveTime::from_hms_opt(6, 30, 0).unwrap(),
        city_timezone: chrono_tz::Europe::London,
        used_extreme_latitude_fallback: false,
        fallback_duration_minutes: 0,
    };

    let now = Local.with_ymd_and_hms(2024, 6, 21, 12, 0, 0).unwrap();
    let base_date = now.date_naive();
    let times =
        GeoTimes::from_solar_result(&solar_result, base_date, now, 51.5074, -0.1278).unwrap();

    let config = geo_adaptive_config();

    // current_time is the instant whose London wall clock reads 19:30, the
    // midpoint of the 19:00-20:00 sunset window. The schedule converts it into
    // the coordinate frame, so the interval uses the coordinate-frame window.
    let current_time = times
        .coordinate_tz
        .from_local_datetime(&base_date.and_hms_opt(19, 30, 0).unwrap())
        .single()
        .unwrap()
        .with_timezone(&Local);
    let state = RuntimeState::new(
        Period::Sunset,
        &config,
        crate::core::schedule::Schedule::from_config(&config, Some(times.clone())),
        current_time,
    );

    // The pre-fix code mixed a local-frame window with the coordinate-frame
    // current time, clamping the position to the window end and flooring the
    // interval to 1 second. At the midpoint of this 1-hour window the correct
    // value is 36 seconds.
    let interval = state.effective_update_interval_secs();
    assert_eq!(interval, 36, "interval at the window midpoint");
}

#[test]
fn watcher_reports_an_output_arriving() {
    let mut state = super::HotplugWatcherState::default();
    state.on_global(7, "wl_output");
    assert!(state.take_changed());
    assert!(!state.take_changed(), "take_changed must clear the flag");
}

#[test]
fn watcher_ignores_globals_that_are_not_outputs() {
    let mut state = super::HotplugWatcherState::default();
    state.on_global(3, "wl_seat");
    state.on_global(4, "zwlr_gamma_control_manager_v1");
    assert!(!state.take_changed());
}

#[test]
fn watcher_reports_a_known_output_leaving() {
    let mut state = super::HotplugWatcherState::default();
    state.on_global(7, "wl_output");
    state.take_changed();

    state.on_global_remove(7);
    assert!(state.take_changed());
}

#[test]
fn watcher_ignores_removal_of_a_name_that_was_not_an_output() {
    let mut state = super::HotplugWatcherState::default();
    state.on_global(7, "wl_output");
    state.take_changed();

    state.on_global_remove(3);
    assert!(!state.take_changed());
}

#[test]
fn watcher_does_not_report_the_same_output_leaving_twice() {
    let mut state = super::HotplugWatcherState::default();
    state.on_global(7, "wl_output");
    state.take_changed();

    state.on_global_remove(7);
    assert!(state.take_changed());

    state.on_global_remove(7);
    assert!(!state.take_changed());
}

#[test]
fn watcher_guard_clears_liveness_and_wakes_the_main_loop() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let alive = Arc::new(AtomicBool::new(true));
    let (tx, rx) = std::sync::mpsc::channel();

    drop(super::WatcherAliveGuard {
        alive: Arc::clone(&alive),
        sender: tx,
    });

    assert!(!alive.load(Ordering::SeqCst));
    assert!(matches!(
        rx.try_recv(),
        Ok(crate::io::signals::SignalMessage::HotplugCheck)
    ));
}

/// A clock-mode state in the middle of the sunset transition. Center mode
/// places the transition around the sunset time, so `sunset = now` lands near
/// progress 0.5.
fn mid_sunset_state() -> RuntimeState {
    let now = chrono::Local::now();
    let mut config = static_mode_config();
    config.transition_mode = TransitionMode::Center;
    config.sunrise = Some(
        (now - chrono::Duration::hours(6))
            .format("%H:%M:%S")
            .to_string(),
    );
    config.sunset = Some(now.format("%H:%M:%S").to_string());

    let schedule = crate::core::schedule::Schedule::from_config(&config, None);
    let period = schedule
        .as_ref()
        .expect("clock config yields a schedule")
        .current_period(now);
    let state = RuntimeState::new(period, &config, schedule, now);
    assert_eq!(state.period(), Period::Sunset);
    state
}

fn offset(temp: i32, gamma: f64) -> Adjustment {
    Adjustment {
        temp: FieldAdjustment::Offset(temp),
        gamma: FieldAdjustment::Offset(gamma),
    }
}

fn hold(temp: i32, gamma: f64) -> Adjustment {
    Adjustment {
        temp: FieldAdjustment::Hold(temp),
        gamma: FieldAdjustment::Hold(gamma),
    }
}

#[test]
fn adjustment_shifts_values_by_exactly_the_offset_mid_transition() {
    let base = mid_sunset_state();
    let (base_temp, base_gamma) = base.values();

    let adjusted = base.with_adjustment(offset(500, -5.0));
    let (temp, gamma) = adjusted.values();

    assert_eq!(temp, base_temp + 500);
    assert!((gamma - (base_gamma - 5.0)).abs() < 1e-9);
}

#[test]
fn a_hold_replaces_the_scheduled_value() {
    let base = mid_sunset_state();
    let held = base.with_adjustment(hold(4000, 80.0));
    assert_eq!(held.values(), (4000, 80.0));

    let (later, _) = held.with_next_period();
    assert_eq!(later.values(), (4000, 80.0));
}

#[test]
fn adjusted_values_clamp_to_the_supported_range() {
    use crate::common::constants::{MAXIMUM_GAMMA, MAXIMUM_TEMP, MINIMUM_GAMMA, MINIMUM_TEMP};

    let base = RuntimeState::new(
        Period::Static,
        &static_mode_config(),
        None,
        chrono::Local::now(),
    );

    let high = base.with_adjustment(offset(1_000_000, 1_000.0));
    assert_eq!(high.values(), (MAXIMUM_TEMP, MAXIMUM_GAMMA));
    let low = base.with_adjustment(offset(-1_000_000, -1_000.0));
    assert_eq!(low.values(), (MINIMUM_TEMP, MINIMUM_GAMMA));

    let high = base.with_adjustment(hold(1_000_000, 1_000.0));
    assert_eq!(high.values(), (MAXIMUM_TEMP, MAXIMUM_GAMMA));
    let low = base.with_adjustment(hold(-5, -5.0));
    assert_eq!(low.values(), (MINIMUM_TEMP, MINIMUM_GAMMA));
}

#[test]
fn adjustment_survives_every_rebuild() {
    let adjustment = Adjustment {
        temp: FieldAdjustment::Offset(-300),
        gamma: FieldAdjustment::Hold(7.5),
    };
    let state = mid_sunset_state().with_adjustment(adjustment);
    assert_eq!(
        RuntimeState::new(Period::Static, state.config(), None, chrono::Local::now()).adjustment(),
        Adjustment::default()
    );

    let (current, _) = state.with_current_period();
    assert_eq!(current.adjustment(), adjustment);

    let (next, _) = state.with_next_period();
    assert_eq!(next.adjustment(), adjustment);

    let reloaded = state
        .with_config(state.config())
        .expect("reloading the same config succeeds");
    assert_eq!(reloaded.adjustment(), adjustment);
}

fn core_with(config: &Config, signal_state: SignalState) -> (Core, Arc<Mutex<(u32, f64)>>) {
    let last = Arc::new(Mutex::new((0u32, 0.0f64)));
    let runtime_state = RuntimeState::new(
        Period::Static,
        config,
        crate::core::schedule::Schedule::from_config(config, None),
        chrono::Local::now(),
    );
    let core = Core::new(CoreParams {
        backend: Box::new(CaptureBackend { last: last.clone() }),
        runtime_state,
        signal_state,
        debug_enabled: false,
        bypass_smoothing: false,
        ipc_notifier: None,
    });
    (core, last)
}

#[test]
#[serial]
fn preset_switch_clears_the_adjustment_on_reload() {
    let config = static_mode_config();
    let signal_state = empty_signal_state();
    *signal_state.current_preset.lock().unwrap() = Some("previous-test-preset".to_string());
    let (mut core, last) = core_with(&config, signal_state);
    core.runtime_state = core.runtime_state.with_adjustment(hold(4000, 100.0));

    core.handle_config_reload(config)
        .expect("reload returned an error");

    assert_eq!(core.runtime_state.adjustment(), Adjustment::default());
    assert_eq!(last.lock().unwrap().0, 6500);
}

/// One preset switch reaches Core as several `Reload` messages, from the file
/// watcher and from SIGUSR2. The first one handled must fold in the rest and
/// apply the latest config once.
#[test]
#[serial]
fn reload_folds_queued_reloads_and_applies_the_latest() {
    let config = static_mode_config();
    let (mut core, last) = core_with(&config, empty_signal_state());
    let mut first = config.clone();
    first.static_temp = Some(5000);
    let mut latest = config.clone();
    latest.static_temp = Some(4000);
    for queued in [first, latest.clone()] {
        core.signal_state
            .signal_sender
            .send(crate::io::signals::SignalMessage::Reload(Box::new(queued)))
            .unwrap();
    }

    core.handle_config_reload(config)
        .expect("reload returned an error");

    assert_eq!(core.runtime_state.config(), &latest);
    assert_eq!(last.lock().unwrap().0, 4000);
    assert!(core.signal_state.signal_receiver.try_recv().is_err());
}

fn temp_delta(temp: i32) -> AdjustRequest {
    AdjustRequest {
        temp: Some(AdjustOp::Delta(temp)),
        ..AdjustRequest::default()
    }
}

fn static_request(temp: AdjustOp<i32>) -> AdjustRequest {
    AdjustRequest {
        is_static: true,
        temp: Some(temp),
        ..AdjustRequest::default()
    }
}

#[test]
fn adjust_request_moves_an_offset_and_resets_it() {
    let held = offset(-300, 2.0);
    let applied = (6200, 102.0);
    assert_eq!(temp_delta(-200).resolve(held, applied), offset(-500, 2.0));

    let reset = AdjustRequest {
        reset: true,
        ..AdjustRequest::default()
    };
    assert_eq!(reset.resolve(held, applied), Adjustment::default());
    assert_eq!(
        reset.resolve(hold(4000, 80.0), applied),
        Adjustment::default()
    );

    assert_eq!(
        temp_delta(i32::MAX).resolve(held, applied).temp,
        FieldAdjustment::Offset(i32::MAX - 300)
    );
    assert_eq!(
        temp_delta(i32::MAX).resolve(offset(1, 0.0), applied).temp,
        FieldAdjustment::Offset(i32::MAX)
    );
}

/// A static request holds the value the display shows now, moved by the
/// delta, and a hold then stays until reset no matter how it is moved.
#[test]
fn adjust_request_holds_are_sticky() {
    let applied = (6200, 102.0);

    assert_eq!(
        static_request(AdjustOp::Assign(3500)).resolve(offset(-300, 2.0), applied),
        Adjustment {
            temp: FieldAdjustment::Hold(3500),
            gamma: FieldAdjustment::Offset(2.0),
        }
    );
    assert_eq!(
        static_request(AdjustOp::Delta(-200))
            .resolve(offset(-300, 2.0), applied)
            .temp,
        FieldAdjustment::Hold(6000)
    );
    assert_eq!(
        static_request(AdjustOp::Delta(-200))
            .resolve(hold(3500, 80.0), applied)
            .temp,
        FieldAdjustment::Hold(3300)
    );
    assert_eq!(
        temp_delta(-200).resolve(hold(3500, 80.0), applied).temp,
        FieldAdjustment::Hold(3300)
    );
    assert_eq!(
        static_request(AdjustOp::Assign(4000))
            .resolve(hold(3500, 80.0), applied)
            .temp,
        FieldAdjustment::Hold(4000)
    );
}

#[test]
#[serial]
fn apply_adjustment_sums_the_queued_requests() {
    let config = static_mode_config();
    let (mut core, last) = core_with(&config, empty_signal_state());
    for delta in [-200, -600] {
        core.signal_state
            .signal_sender
            .send(crate::io::signals::SignalMessage::Adjust(temp_delta(delta)))
            .unwrap();
    }

    core.apply_adjustment(temp_delta(-100))
        .expect("adjustment returned an error");

    assert_eq!(
        core.runtime_state.adjustment().temp,
        FieldAdjustment::Offset(-900)
    );
    assert_eq!(last.lock().unwrap().0, 5600);
    assert!(core.signal_state.signal_receiver.try_recv().is_err());
}

/// A queued static delta holds the value the request before it produces, so
/// the queue must resolve in order rather than as one summed request.
#[test]
#[serial]
fn apply_adjustment_resolves_queued_requests_in_order() {
    let config = static_mode_config();
    let (mut core, last) = core_with(&config, empty_signal_state());
    core.signal_state
        .signal_sender
        .send(crate::io::signals::SignalMessage::Adjust(static_request(
            AdjustOp::Delta(-100),
        )))
        .unwrap();

    core.apply_adjustment(temp_delta(-100))
        .expect("adjustment returned an error");

    assert_eq!(
        core.runtime_state.adjustment().temp,
        FieldAdjustment::Hold(6300)
    );
    assert_eq!(last.lock().unwrap().0, 6300);
}

#[test]
#[serial]
fn apply_adjustment_adds_the_delta_to_the_held_offset() {
    let config = static_mode_config();
    let (mut core, last) = core_with(&config, empty_signal_state());
    core.runtime_state = core.runtime_state.with_adjustment(offset(-500, 0.0));

    core.apply_adjustment(temp_delta(-100))
        .expect("adjustment returned an error");

    assert_eq!(
        core.runtime_state.adjustment().temp,
        FieldAdjustment::Offset(-600)
    );
    assert_eq!(last.lock().unwrap().0, 5900);
}

/// A message that is not an `Adjust` interrupts the smooth transition. The
/// display must still land on the adjusted target and the message must stay
/// queued for the main loop.
#[test]
#[serial]
fn apply_adjustment_lands_on_target_when_interrupted_by_a_reload() {
    let config = static_mode_config();
    let (mut core, last) = core_with(&config, empty_signal_state());
    let sender = core.signal_state.signal_sender.clone();
    let interrupt = core.signal_state.interrupt.clone();
    let reload_config = config.clone();
    let interrupter = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        interrupt.store(true, Ordering::SeqCst);
        sender
            .send(crate::io::signals::SignalMessage::Reload(Box::new(
                reload_config,
            )))
            .unwrap();
    });

    core.apply_adjustment(temp_delta(-500))
        .expect("adjustment returned an error");
    interrupter.join().unwrap();

    assert_eq!(last.lock().unwrap().0, 6000);
    assert!(matches!(
        core.signal_state.signal_receiver.try_recv(),
        Ok(crate::io::signals::SignalMessage::Reload(_))
    ));
}

#[test]
fn adjustment_is_zero_at_display_precision() {
    assert!(offset(0, 0.1 + 0.1 + 0.1 - 0.3).is_zero());
    assert!(offset(0, 0.04).is_zero());
    assert!(!offset(0, 0.1).is_zero());
    assert!(!offset(1, 0.0).is_zero());
    assert!(!hold(0, 0.0).is_zero());
}

#[test]
fn adjustment_display_marks_holds() {
    assert_eq!(offset(-300, 5.0).to_string(), "-300K, +5.0%");
    assert_eq!(hold(3500, 90.0).to_string(), "=3500K, =90.0%");
}

#[test]
fn has_same_effective_values_sees_an_adjustment() {
    let base = mid_sunset_state();
    let adjusted = base.with_adjustment(offset(0, 1.0));

    assert!(base.has_same_effective_values(&base.clone()));
    assert!(!base.has_same_effective_values(&adjusted));
}
