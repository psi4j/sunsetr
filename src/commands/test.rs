//! Interactive gamma/temperature testing. With an existing sunsetr process, signal it via SIGUSR1
//! to apply the values temporarily. Without one, apply them directly through the configured
//! backend. Escape or Ctrl+C restores the previous state.

use crate::backend::ColorTemperatureBackend;
use crate::config::Config;
use crate::core::period::Period;
use crate::core::runtime_state::RuntimeState;
use crate::io::signals::{SignalMessage, TestModeParams};
use anyhow::{Context, Result};
use std::ops::ControlFlow;
use std::sync::mpsc::Sender;

/// Dispatch a signal received inside the test-mode loop, returning whether the loop should break.
///
/// `Reload` and `ResumeFromSleep` are the main loop's responsibility, so they are re-emitted via
/// `sender` before breaking, letting the main loop process them once test mode returns.
fn handle_test_mode_signal(msg: SignalMessage, sender: &Sender<SignalMessage>) -> ControlFlow<()> {
    match msg {
        SignalMessage::TestMode(new_params) => {
            if new_params.temperature == 0 {
                log_indented!("Exiting test mode, restoring normal operation...");
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }
        SignalMessage::Reload(config) => {
            log_decorated!("Reload signal received, exiting test mode...");
            let _ = sender.send(SignalMessage::Reload(config));
            ControlFlow::Break(())
        }
        SignalMessage::TimeChange => {
            log_decorated!("Time change detected, exiting test mode...");
            ControlFlow::Break(())
        }
        SignalMessage::Shutdown => {
            log_decorated!("Shutdown signal received, exiting test mode...");
            ControlFlow::Break(())
        }
        SignalMessage::ResumeFromSleep => {
            log_decorated!("System resuming from sleep, exiting test mode...");
            let _ = sender.send(SignalMessage::ResumeFromSleep);
            ControlFlow::Break(())
        }
    }
}

fn validate_temperature(temp: u32) -> Result<()> {
    use crate::common::constants::{MAXIMUM_TEMP, MINIMUM_TEMP};

    if temp < MINIMUM_TEMP {
        anyhow::bail!(
            "Temperature {} is too low (minimum: {}K)",
            temp,
            MINIMUM_TEMP
        );
    }

    if temp > MAXIMUM_TEMP {
        anyhow::bail!(
            "Temperature {} is too high (maximum: {}K)",
            temp,
            MAXIMUM_TEMP
        );
    }

    Ok(())
}

fn validate_gamma(gamma: f64) -> Result<()> {
    use crate::common::constants::{MAXIMUM_GAMMA, MINIMUM_GAMMA};

    if gamma < MINIMUM_GAMMA {
        anyhow::bail!("Gamma {} is too low (minimum: {})", gamma, MINIMUM_GAMMA);
    }

    if gamma > MAXIMUM_GAMMA {
        anyhow::bail!("Gamma {} is too high (maximum: {})", gamma, MAXIMUM_GAMMA);
    }

    Ok(())
}

pub fn handle_test_command(temperature: u32, gamma: f64, debug_enabled: bool) -> Result<()> {
    log_version!();

    validate_temperature(temperature)?;
    validate_gamma(gamma)?;
    let config = Config::load()?;
    log_block_start!("Testing display settings: {}K @ {}%", temperature, gamma);

    match crate::io::instance::get_running_instance_pid() {
        Ok(pid) => match crate::io::instance::acquire_test_lock() {
            Ok(_lock_guard) => {
                log_decorated!(
                    "Found existing sunsetr process (PID: {pid}), sending test signal..."
                );

                if debug_enabled {
                    log_pipe!();
                    log_debug!(
                        "Sending SIGUSR1 to PID {pid} with test params: {temperature}K @ {gamma}%"
                    );
                }

                match crate::io::instance::send_test_signal(pid, temperature, gamma) {
                    Ok(_) => {
                        log_indented!("Test signal sent successfully");
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        log_decorated!("Applied test values: {temperature}K @ {gamma}%");
                        log_block_start!("Press Escape or Ctrl+C to restore previous settings");
                        let _terminal_guard = crate::common::utils::TerminalGuard::new();
                        let instance_exited = wait_for_user_exit(Some(pid))?;
                        if instance_exited {
                            log_pipe!();
                            log_info!("sunsetr process (PID {pid}) exited, ending test mode");
                        } else {
                            log_decorated!("Restoring normal operation...");
                            let _ = crate::io::instance::send_test_signal(pid, 0, 0.0);
                            log_decorated!("Test complete");
                        }
                    }
                    Err(e) => {
                        return Err(e).context("Failed to send test signal to existing process");
                    }
                }
            }
            Err(_) => {
                log_pipe!();
                log_warning!("Test mode is already active in another terminal");
                log_indented!("Exit the current test mode first (press Escape)");
                log_end!();
                return Ok(());
            }
        },
        Err(_) => match crate::io::instance::acquire_test_lock() {
            Ok(_lock_guard) => {
                log_decorated!("No existing sunsetr process found, running direct test...");
                run_direct_test(temperature, gamma, debug_enabled, &config)?;
            }
            Err(_) => {
                log_pipe!();
                log_warning!("Test mode is already active in another terminal");
                log_indented!("Exit the current test mode first (press Escape)");
                log_end!();
                return Ok(());
            }
        },
    }

    log_end!();
    Ok(())
}

fn run_direct_test(
    temperature: u32,
    gamma: f64,
    debug_enabled: bool,
    config: &Config,
) -> Result<()> {
    let backend_type = crate::backend::detect_backend(config)?;
    let backend_result = match backend_type {
        crate::backend::BackendType::Hyprsunset => {
            crate::backend::hyprsunset::HyprsunsetBackend::new_with_initial_values(
                debug_enabled,
                temperature,
                gamma,
            )
            .map(|backend| Box::new(backend) as Box<dyn crate::backend::ColorTemperatureBackend>)
        }
        _ => crate::backend::create_backend(backend_type, config, debug_enabled, None, None),
    };

    match backend_result {
        Ok(mut backend) => {
            log_decorated!(
                "Applying test values via {} backend...",
                backend.backend_name()
            );
            use std::sync::Arc;
            use std::sync::atomic::AtomicBool;
            let running = Arc::new(AtomicBool::new(true));
            let is_wayland = backend.backend_name() == "Wayland";

            let smoothing_enabled = is_wayland && config.smoothing;
            let startup_duration = config.startup_duration;

            let day_runtime_state = RuntimeState::new(
                Period::Day,
                config,
                crate::core::schedule::Schedule::from_config(config, None),
                crate::time::source::now(),
            );

            if smoothing_enabled && startup_duration >= 0.1 {
                let mut transition = crate::core::smoothing::SmoothTransition::test_mode(
                    &day_runtime_state,
                    temperature,
                    gamma,
                )
                .silent();

                match transition.execute(backend.as_mut(), &day_runtime_state, &running, None) {
                    Ok(_) => {
                        log_pipe!();
                        log_info!("Applied test values: {temperature}K @ {gamma}%");
                    }
                    Err(e) => {
                        log_pipe!();
                        log_error!("Failed to apply test values: {e}");

                        match backend.apply_temperature_gamma(temperature, gamma, &running) {
                            Ok(_) => {
                                log_pipe!();
                                log_info!("Test values applied immediately (fallback)");
                            }
                            Err(e) => {
                                return Err(e).context("Failed to apply test values");
                            }
                        }
                    }
                }
            } else if backend.backend_name() != "Hyprsunset" {
                match backend.apply_temperature_gamma(temperature, gamma, &running) {
                    Ok(_) => {
                        log_block_start!("Applied test values: {temperature}K @ {gamma}%");
                    }
                    Err(e) => {
                        return Err(e).context("Failed to apply test values");
                    }
                }
            } else {
                log_block_start!("Applied test values: {temperature}K @ {gamma}%");
            }

            log_block_start!("Press Escape or Ctrl+C to restore previous settings");
            let _terminal_guard = crate::common::utils::TerminalGuard::new();
            wait_for_user_exit(None)?;

            if is_wayland {
                log_block_start!("Restoring display...");

                let shutdown_duration = config.shutdown_duration;

                if smoothing_enabled && shutdown_duration >= 0.1 {
                    let mut transition = crate::core::smoothing::SmoothTransition::test_restore(
                        &day_runtime_state,
                        temperature,
                        gamma,
                    )
                    .silent();

                    match transition.execute(backend.as_mut(), &day_runtime_state, &running, None) {
                        Ok(_) => {
                            let (day_temp, day_gamma) = day_runtime_state.values();
                            log_decorated!(
                                "Display restored to day values ({}K, {}%)",
                                day_temp,
                                day_gamma
                            );
                        }
                        Err(e) => {
                            log_pipe!();
                            log_error!("Failed to restore with transition: {e}");

                            let (day_temp, day_gamma) = day_runtime_state.values();
                            match backend.apply_temperature_gamma(day_temp, day_gamma, &running) {
                                Ok(_) => {
                                    log_pipe!();
                                    log_info!(
                                        "Display restored to day values ({}K, {}%)",
                                        day_temp,
                                        day_gamma
                                    );
                                }
                                Err(e) => {
                                    return Err(e).context("Failed to restore display");
                                }
                            }
                        }
                    }
                } else {
                    let (day_temp, day_gamma) = day_runtime_state.values();
                    backend.apply_temperature_gamma(day_temp, day_gamma, &running)?;
                    log_pipe!();
                    log_info!(
                        "Display restored to day values ({}K, {}%)",
                        day_temp,
                        day_gamma
                    );
                }
            }
        }
        Err(e) => return Err(e),
    }

    log_block_start!("Test complete");
    Ok(())
}

/// Take over when the main loop receives a SIGUSR1 test signal: apply the test values, wait for an
/// exit signal, then restore the calculated values. Preserves the main loop's state and timing so
/// it resumes unchanged.
pub fn run_test_mode_loop(
    test_params: TestModeParams,
    backend: &mut Box<dyn ColorTemperatureBackend>,
    signal_state: &crate::io::signals::SignalState,
    current_runtime_state: &crate::core::runtime_state::RuntimeState,
    debug_enabled: bool,
) -> Result<()> {
    if debug_enabled {
        log_pipe!();
        log_debug!(
            "Entering test mode loop with {}K @ {}%",
            test_params.temperature,
            test_params.gamma
        );
    }

    log_indented!(
        "Entering test mode: {}K @ {}%",
        test_params.temperature,
        test_params.gamma
    );

    let is_wayland = backend.backend_name() == "Wayland";
    let smoothing_enabled = is_wayland && current_runtime_state.config().smoothing;

    let startup_duration = current_runtime_state.config().startup_duration;

    if smoothing_enabled && startup_duration >= 0.1 {
        let mut transition = crate::core::smoothing::SmoothTransition::test_mode(
            current_runtime_state,
            test_params.temperature,
            test_params.gamma,
        );

        match transition.execute(
            backend.as_mut(),
            current_runtime_state,
            &signal_state.running,
            None,
        ) {
            Ok(_) => {
                log_pipe!();
                log_info!("Test values applied with smooth transition");
            }
            Err(e) => {
                log_warning!("Failed to apply test values with transition: {e}");

                match backend.apply_temperature_gamma(
                    test_params.temperature,
                    test_params.gamma,
                    &signal_state.running,
                ) {
                    Ok(_) => {
                        log_decorated!("Test values applied immediately (fallback)");
                    }
                    Err(e) => {
                        log_pipe!();
                        log_error!("Failed to apply test values: {e}");
                        return Ok(());
                    }
                }
            }
        }
    } else {
        if debug_enabled {
            log_pipe!();
            log_debug!(
                "Applying test values directly: {}K @ {}%",
                test_params.temperature,
                test_params.gamma
            );
        }
        match backend.apply_temperature_gamma(
            test_params.temperature,
            test_params.gamma,
            &signal_state.running,
        ) {
            Ok(_) => {
                log_pipe!();
                log_info!("Test values applied successfully");
            }
            Err(e) => {
                log_pipe!();
                log_error!("Failed to apply test values: {e}");
                return Ok(());
            }
        }
    }

    loop {
        if !signal_state
            .running
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            break;
        }

        match signal_state
            .signal_receiver
            .recv_timeout(std::time::Duration::from_millis(100))
        {
            Ok(signal_msg) => {
                if handle_test_mode_signal(signal_msg, &signal_state.signal_sender).is_break() {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }

    let (restore_temp, restore_gamma) = current_runtime_state.values();

    let shutdown_duration = current_runtime_state.config().shutdown_duration;

    if smoothing_enabled && shutdown_duration >= 0.1 {
        let mut transition = crate::core::smoothing::SmoothTransition::test_restore(
            current_runtime_state,
            test_params.temperature,
            test_params.gamma,
        );

        match transition.execute(
            backend.as_mut(),
            current_runtime_state,
            &signal_state.running,
            None,
        ) {
            Ok(_) => {
                if debug_enabled {
                    log_pipe!();
                    log_debug!("Restored normal values: {restore_temp}K @ {restore_gamma}%");
                }
            }
            Err(e) => {
                log_pipe!();
                log_error!("Failed to restore: {e}");

                match backend.apply_temperature_gamma(
                    restore_temp,
                    restore_gamma,
                    &signal_state.running,
                ) {
                    Ok(_) => {
                        log_pipe!();
                        log_info!("Normal operation restored: {restore_temp}K @ {restore_gamma}%");
                    }
                    Err(e) => {
                        log_pipe!();
                        log_error!("Failed to restore normal operation: {e}");
                    }
                }
            }
        }
    } else {
        match backend.apply_temperature_gamma(restore_temp, restore_gamma, &signal_state.running) {
            Ok(_) => {
                log_pipe!();
                log_info!("Normal operation restored: {restore_temp}K @ {restore_gamma}%");
            }
            Err(e) => {
                log_pipe!();
                log_error!("Failed to restore normal operation: {e}");
            }
        }
    }

    if debug_enabled {
        log_pipe!();
        log_debug!("Exiting test mode loop");
    }

    Ok(())
}

/// Block until the user presses Escape or Ctrl+C.
///
/// Polls for input so that `monitor_pid`, when given, can be checked
/// for liveness between polls. Returns `true` if that monitored
/// process exited while waiting, `false` if the user requested exit.
fn wait_for_user_exit(monitor_pid: Option<u32>) -> Result<bool> {
    use crossterm::{
        event::{self, Event, KeyCode},
        terminal::{disable_raw_mode, enable_raw_mode},
    };

    enable_raw_mode()?;

    let result = loop {
        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Esc => break Ok(false),
                    KeyCode::Char('c')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        break Ok(false);
                    }
                    _ => {}
                }
            }
        } else if let Some(pid) = monitor_pid
            && !crate::io::instance::is_instance_running(pid)
        {
            break Ok(true);
        }
    };

    disable_raw_mode()?;

    result
}

pub fn show_usage() {
    log_version!();
    log_block_start!("Usage: sunsetr test <temperature> <gamma>");
    log_block_start!("Arguments:");
    log_indented!("<temperature>  Color temperature in Kelvin (1000-20000)");
    log_indented!("<gamma>        Gamma percentage (10-200)");
    log_pipe!();
    log_info!("For detailed help with examples, try: sunsetr help test");
    log_end!();
}

pub fn display_help() {
    log_version!();
    log_block_start!("Test specific temperature and gamma values");
    log_block_start!("Usage: sunsetr test <temperature> <gamma>");
    log_block_start!("Arguments:");
    log_indented!("<temperature>  Color temperature in Kelvin (1000-20000)");
    log_indented!("<gamma>        Gamma percentage (10-200)");
    log_block_start!("Behavior:");
    log_indented!("- If sunsetr is running: Signals test mode via SIGUSR1");
    log_indented!("- If not running: Applies values directly via backend");
    log_indented!("- Smooth transitions applied if configured");
    log_indented!("- Automatically restores on exit");
    log_block_start!("Examples:");
    log_indented!("# Test warm evening values");
    log_indented!("sunsetr test 3500 85");
    log_pipe!();
    log_indented!("# Test very warm night values");
    log_indented!("sunsetr test 2800 75");
    log_pipe!();
    log_indented!("# Test neutral daylight");
    log_indented!("sunsetr test 6500 100");
    log_end!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, TransitionMode};

    fn empty_config() -> Config {
        use crate::common::constants::*;
        Config {
            backend: DEFAULT_BACKEND,
            transition_mode: TransitionMode::Geo,
            smoothing: DEFAULT_SMOOTHING,
            startup_duration: DEFAULT_STARTUP_DURATION_SEC,
            shutdown_duration: DEFAULT_SHUTDOWN_DURATION_SEC,
            adaptive_interval: DEFAULT_ADAPTIVE_INTERVAL_MS,
            night_temp: DEFAULT_NIGHT_TEMP,
            day_temp: DEFAULT_DAY_TEMP,
            night_gamma: DEFAULT_NIGHT_GAMMA,
            day_gamma: DEFAULT_DAY_GAMMA,
            update_interval: crate::config::UpdateInterval::Adaptive,
            static_temp: None,
            static_gamma: None,
            sunset: None,
            sunrise: None,
            transition_duration: DEFAULT_TRANSITION_DURATION_MIN,
            latitude: None,
            longitude: None,
        }
    }

    #[test]
    fn resume_from_sleep_re_emits_and_breaks() {
        let (tx, rx) = std::sync::mpsc::channel();
        let result = handle_test_mode_signal(SignalMessage::ResumeFromSleep, &tx);
        assert!(result.is_break());
        assert!(matches!(rx.try_recv(), Ok(SignalMessage::ResumeFromSleep)));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn reload_re_emits_with_payload_and_breaks() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut cfg = empty_config();
        cfg.night_temp = 3500;

        let result = handle_test_mode_signal(SignalMessage::Reload(Box::new(cfg)), &tx);
        assert!(result.is_break());

        match rx.try_recv() {
            Ok(SignalMessage::Reload(boxed)) => assert_eq!(boxed.night_temp, 3500),
            other => panic!("expected Reload, got {other:?}"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn shutdown_breaks_without_reemit() {
        let (tx, rx) = std::sync::mpsc::channel();
        let result = handle_test_mode_signal(SignalMessage::Shutdown, &tx);
        assert!(result.is_break());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn time_change_breaks_without_reemit() {
        let (tx, rx) = std::sync::mpsc::channel();
        let result = handle_test_mode_signal(SignalMessage::TimeChange, &tx);
        assert!(result.is_break());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_mode_zero_temperature_breaks() {
        let (tx, rx) = std::sync::mpsc::channel();
        let result = handle_test_mode_signal(
            SignalMessage::TestMode(TestModeParams {
                temperature: 0,
                gamma: 0.0,
            }),
            &tx,
        );
        assert!(result.is_break());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_mode_nonzero_temperature_continues() {
        let (tx, rx) = std::sync::mpsc::channel();
        let result = handle_test_mode_signal(
            SignalMessage::TestMode(TestModeParams {
                temperature: 4500,
                gamma: 90.0,
            }),
            &tx,
        );
        assert!(result.is_continue());
        assert!(rx.try_recv().is_err());
    }
}
