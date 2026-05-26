use super::*;
use crate::backend::ColorTemperatureBackend;
use crate::config::{Backend, Config, UpdateInterval};
use crate::core::context::Context;
use crate::core::period::Period;
use crate::core::runtime_state::RuntimeState;
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
        backend: Some(Backend::Wayland),
        transition_mode: Some("static".to_string()),
        smoothing: Some(true),
        startup_duration: Some(0.2),
        shutdown_duration: Some(0.2),
        adaptive_interval: Some(50),
        night_temp: Some(3300),
        day_temp: Some(6500),
        night_gamma: Some(90.0),
        day_gamma: Some(100.0),
        update_interval: Some(UpdateInterval::Fixed(60)),
        static_temp: Some(6500),
        static_gamma: Some(100.0),
        sunset: None,
        sunrise: None,
        transition_duration: None,
        latitude: None,
        longitude: None,
        start_hyprsunset: None,
        startup_transition: None,
        startup_transition_duration: None,
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
    let runtime_state =
        RuntimeState::new(Period::Night, &config, None, chrono::Local::now().time());
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
        lock_info: None,
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
