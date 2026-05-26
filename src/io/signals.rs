//! Signal handling and inter-process communication for sunsetr.
//!
//! This module provides signal-based communication between sunsetr instances,
//! handling configuration reloads, test mode activation, and process management.

use anyhow::{Context, Result};
use signal_hook::{
    consts::signal::{SIGHUP, SIGINT, SIGTERM, SIGUSR1, SIGUSR2},
    iterator::Signals,
};
use std::{
    sync::Arc,
    sync::atomic::{AtomicBool, Ordering},
    thread,
};

/// Test mode parameters passed via signal
#[derive(Debug, Clone)]
pub struct TestModeParams {
    pub temperature: u32,
    pub gamma: f64,
}

/// Unified signal message type for all signal-based communication
#[derive(Debug)]
pub enum SignalMessage {
    Reload(Box<crate::config::Config>),
    TestMode(TestModeParams),
    Shutdown,
    TimeChange,
    ResumeFromSleep,
}

/// Signal handling state shared between threads.
///
/// The `interrupt` flag carries a non-trivial contract. SIGUSR2 and the dbus
/// sleep/time-change monitors raise it before dispatching a follow-up message,
/// so any in-flight smooth transition aborts on its next frame. Handlers
/// (`handle_config_reload`, `recover_state`) must clear it at entry to keep
/// their own transitions from self-aborting.
pub struct SignalState {
    pub running: Arc<AtomicBool>,
    pub signal_receiver: std::sync::mpsc::Receiver<SignalMessage>,
    pub signal_sender: std::sync::mpsc::Sender<SignalMessage>,
    pub interrupt: Arc<AtomicBool>,
    pub in_test_mode: Arc<AtomicBool>,
    pub instant_shutdown: Arc<AtomicBool>,
    pub current_preset: Arc<std::sync::Mutex<Option<String>>>,
}

impl SignalState {
    /// Drain pending messages from the signal channel, returning the most
    /// recent `Reload`'s config (or `None` if no `Reload` was queued) and
    /// re-emitting any non-`Reload` variants back onto the channel.
    pub(crate) fn drain_to_latest_reload(&self) -> Option<Box<crate::config::Config>> {
        let mut latest_config: Option<Box<crate::config::Config>> = None;
        let mut deferred: Vec<SignalMessage> = Vec::new();
        for msg in self.signal_receiver.try_iter() {
            match msg {
                SignalMessage::Reload(cfg) => {
                    latest_config = Some(cfg);
                }
                msg @ (SignalMessage::TestMode(_)
                | SignalMessage::Shutdown
                | SignalMessage::TimeChange
                | SignalMessage::ResumeFromSleep) => {
                    deferred.push(msg);
                }
            }
        }
        for msg in deferred {
            let _ = self.signal_sender.send(msg);
        }
        latest_config
    }
}

/// Set up signal handling for the application.
///
/// Returns a SignalState containing the running flag and signal receiver channel.
/// Spawns a background thread that monitors for signals and sends appropriate
/// messages via the channel.
pub fn setup_signal_handler(debug_enabled: bool) -> Result<SignalState> {
    let running = Arc::new(AtomicBool::new(true));
    let in_test_mode = Arc::new(AtomicBool::new(false));
    let instant_shutdown = Arc::new(AtomicBool::new(false));
    let interrupt = Arc::new(AtomicBool::new(false));
    let (signal_sender, signal_receiver) = std::sync::mpsc::channel::<SignalMessage>();

    let mut signals = Signals::new([SIGINT, SIGTERM, SIGHUP, SIGUSR1, SIGUSR2])
        .context("failed to register signal handlers")?;

    let running_clone = running.clone();
    let instant_shutdown_clone = instant_shutdown.clone();
    let interrupt_clone = interrupt.clone();
    let signal_sender_clone = signal_sender.clone();

    thread::spawn(move || {
        #[cfg(debug_assertions)]
        eprintln!(
            "DEBUG: Signal handler setup complete for PID: {}",
            std::process::id()
        );

        #[cfg(debug_assertions)]
        eprintln!(
            "DEBUG: Signal handler thread starting for PID: {}",
            std::process::id()
        );

        #[cfg(debug_assertions)]
        let mut signal_count = 0;
        #[cfg(debug_assertions)]
        let mut sigusr2_count = 0;

        for sig in signals.forever() {
            #[cfg(debug_assertions)]
            {
                signal_count += 1;
                eprintln!("DEBUG: Signal handler processing signal #{signal_count}: {sig}");
            }

            match sig {
                SIGUSR1 => {
                    let test_file_path = format!("/tmp/sunsetr-test-{}.tmp", std::process::id());

                    if let Ok(content) = std::fs::read_to_string(&test_file_path) {
                        let lines: Vec<&str> = content.trim().lines().collect();
                        if lines.len() == 2
                            && let (Ok(temp), Ok(gamma)) =
                                (lines[0].parse::<u32>(), lines[1].parse::<f64>())
                        {
                            log_pipe!();
                            if temp == 0 {
                                log_info!("Received test mode exit signal");
                            } else {
                                log_info!("Received test mode signal");
                            }

                            let test_params = TestModeParams {
                                temperature: temp,
                                gamma,
                            };

                            match signal_sender_clone.send(SignalMessage::TestMode(test_params)) {
                                Ok(()) => {
                                    #[cfg(debug_assertions)]
                                    {
                                        eprintln!(
                                            "DEBUG: Test mode parameters sent: {temp}K @ {gamma}%"
                                        );
                                    }
                                }
                                Err(_) => {
                                    #[cfg(debug_assertions)]
                                    {
                                        eprintln!(
                                            "DEBUG: Failed to send test parameters - channel disconnected"
                                        );
                                    }
                                    break;
                                }
                            }
                        }
                        let _ = std::fs::remove_file(&test_file_path);
                    }
                }
                SIGUSR2 => {
                    #[cfg(debug_assertions)]
                    {
                        sigusr2_count += 1;
                        eprintln!(
                            "DEBUG: SIGUSR2 #{} received by PID: {}, sending reload message",
                            sigusr2_count,
                            std::process::id()
                        );
                    }

                    let new_config = match crate::config::Config::load() {
                        Ok(config) => config,
                        Err(e) => {
                            log_pipe!();
                            crate::common::error::log_error_chain("Failed to reload config", &e);
                            log_indented!("Continuing with previous configuration");
                            continue;
                        }
                    };

                    interrupt_clone.store(true, Ordering::SeqCst);

                    match signal_sender_clone.send(SignalMessage::Reload(Box::new(new_config))) {
                        Ok(()) => {
                            log_pipe!();
                            log_info!("Received configuration reload signal");
                        }
                        Err(_) => {
                            #[cfg(debug_assertions)]
                            {
                                eprintln!(
                                    "DEBUG: Reload signal send failed - channel disconnected"
                                );
                            }
                            break;
                        }
                    }
                }
                SIGTERM => {
                    let shutdown_file_path =
                        format!("/tmp/sunsetr-shutdown-{}.tmp", std::process::id());
                    let is_instant_shutdown = std::fs::read_to_string(&shutdown_file_path)
                        .map(|content| content.trim() == "instant")
                        .unwrap_or(false);

                    let _ = std::fs::remove_file(&shutdown_file_path);

                    if is_instant_shutdown {
                        #[cfg(debug_assertions)]
                        {
                            eprintln!(
                                "DEBUG: Received SIGTERM with instant shutdown flag (signal #{signal_count}), setting running=false"
                            );
                        }

                        log_pipe!();
                        log_info!("Received instant shutdown request for restart");

                        instant_shutdown_clone.store(true, Ordering::SeqCst);
                        running_clone.store(false, Ordering::SeqCst);

                        if let Err(e) = signal_sender_clone.send(SignalMessage::Shutdown) {
                            log_warning!("Failed to send instant shutdown message: {e}");
                        }

                        break;
                    } else {
                        #[cfg(debug_assertions)]
                        {
                            eprintln!(
                                "DEBUG: Received SIGTERM (termination request) (signal #{signal_count}), setting running=false"
                            );
                        }

                        log_pipe!();
                        log_info!("Received termination request, initiating graceful shutdown...");

                        if let Err(e) = signal_sender_clone.send(SignalMessage::Shutdown) {
                            log_warning!("Failed to send shutdown message: {e}");
                        }

                        running_clone.store(false, Ordering::SeqCst);
                        break;
                    }
                }
                SIGINT => {
                    #[cfg(debug_assertions)]
                    {
                        eprintln!(
                            "DEBUG: Received SIGINT (Ctrl+C) (signal #{signal_count}), setting running=false"
                        );
                    }

                    log_pipe!();
                    if debug_enabled {
                        log_info!("Received SIGINT (Ctrl+C), initiating graceful shutdown...");
                    } else {
                        log_info!("Received interrupt signal, initiating graceful shutdown...");
                    }

                    if let Err(e) = signal_sender_clone.send(SignalMessage::Shutdown) {
                        log_warning!("Failed to send shutdown message: {e}");
                    }

                    running_clone.store(false, Ordering::SeqCst);
                    break;
                }
                _ => {
                    if sig == SIGHUP {
                        #[cfg(debug_assertions)]
                        eprintln!("DEBUG: Received SIGHUP - terminal disconnected, forcing exit");

                        running_clone.store(false, Ordering::SeqCst);
                        std::process::exit(0);
                    }

                    #[cfg(debug_assertions)]
                    {
                        let signal_name = match sig {
                            SIGINT => "SIGINT (Ctrl+C)",
                            SIGTERM => "SIGTERM (termination request)",
                            _ => "unknown signal",
                        };
                        eprintln!(
                            "DEBUG: Received {signal_name} (signal #{signal_count}), setting running=false"
                        );
                    }

                    let user_message = match sig {
                        SIGINT => {
                            if debug_enabled {
                                "Received SIGINT (Ctrl+C), initiating graceful shutdown..."
                            } else {
                                "Received interrupt signal, initiating graceful shutdown..."
                            }
                        }
                        SIGTERM => "Received termination request, initiating graceful shutdown...",
                        _ => "Received shutdown signal, initiating graceful shutdown...",
                    };

                    log_pipe!();
                    log_info!("{}", user_message);

                    if let Err(e) = signal_sender_clone.send(SignalMessage::Shutdown) {
                        log_pipe!();
                        log_warning!("Failed to send shutdown message: {e}");
                        log_indented!("Main loop appears to have already exited");
                        log_end!();
                        running_clone.store(false, Ordering::SeqCst);
                        break;
                    }

                    running_clone.store(false, Ordering::SeqCst);

                    #[cfg(debug_assertions)]
                    eprintln!(
                        "DEBUG: Signal handler set running=false after {signal_count} signals ({sigusr2_count} SIGUSR2)"
                    );
                }
            }
        }
    });

    let initial_preset = crate::state::preset::get_active_preset().ok().flatten();

    Ok(SignalState {
        running,
        signal_receiver,
        signal_sender,
        interrupt,
        in_test_mode,
        instant_shutdown,
        current_preset: Arc::new(std::sync::Mutex::new(initial_preset)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn make_signal_state() -> SignalState {
        let (signal_sender, signal_receiver) = std::sync::mpsc::channel();
        SignalState {
            running: Arc::new(AtomicBool::new(true)),
            signal_receiver,
            signal_sender,
            interrupt: Arc::new(AtomicBool::new(false)),
            in_test_mode: Arc::new(AtomicBool::new(false)),
            instant_shutdown: Arc::new(AtomicBool::new(false)),
            current_preset: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn config_with_marker(night_temp: u32) -> Config {
        Config {
            backend: None,
            transition_mode: None,
            smoothing: None,
            startup_duration: None,
            shutdown_duration: None,
            adaptive_interval: None,
            night_temp: Some(night_temp),
            day_temp: None,
            night_gamma: None,
            day_gamma: None,
            update_interval: None,
            static_temp: None,
            static_gamma: None,
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

    #[test]
    fn drain_empty_channel_returns_none() {
        let state = make_signal_state();
        assert!(state.drain_to_latest_reload().is_none());
        assert!(state.signal_receiver.try_recv().is_err());
    }

    #[test]
    fn drain_two_reloads_keeps_the_second() {
        let state = make_signal_state();
        state
            .signal_sender
            .send(SignalMessage::Reload(Box::new(config_with_marker(3000))))
            .unwrap();
        state
            .signal_sender
            .send(SignalMessage::Reload(Box::new(config_with_marker(4000))))
            .unwrap();

        let kept = state.drain_to_latest_reload().expect("expected a Reload");
        assert_eq!(kept.night_temp, Some(4000));
        assert!(state.signal_receiver.try_recv().is_err());
    }

    #[test]
    fn drain_keeps_latest_reload_and_reemits_others_in_order() {
        let state = make_signal_state();
        state.signal_sender.send(SignalMessage::Shutdown).unwrap();
        state
            .signal_sender
            .send(SignalMessage::Reload(Box::new(config_with_marker(3000))))
            .unwrap();
        state.signal_sender.send(SignalMessage::TimeChange).unwrap();
        state
            .signal_sender
            .send(SignalMessage::Reload(Box::new(config_with_marker(4000))))
            .unwrap();
        state
            .signal_sender
            .send(SignalMessage::ResumeFromSleep)
            .unwrap();

        let kept = state.drain_to_latest_reload().expect("expected a Reload");
        assert_eq!(kept.night_temp, Some(4000));

        assert!(matches!(
            state.signal_receiver.try_recv(),
            Ok(SignalMessage::Shutdown)
        ));
        assert!(matches!(
            state.signal_receiver.try_recv(),
            Ok(SignalMessage::TimeChange)
        ));
        assert!(matches!(
            state.signal_receiver.try_recv(),
            Ok(SignalMessage::ResumeFromSleep)
        ));
        assert!(state.signal_receiver.try_recv().is_err());
    }

    #[test]
    fn drain_with_only_non_reload_returns_none_and_reemits() {
        let state = make_signal_state();
        state
            .signal_sender
            .send(SignalMessage::ResumeFromSleep)
            .unwrap();
        state.signal_sender.send(SignalMessage::Shutdown).unwrap();

        assert!(state.drain_to_latest_reload().is_none());

        assert!(matches!(
            state.signal_receiver.try_recv(),
            Ok(SignalMessage::ResumeFromSleep)
        ));
        assert!(matches!(
            state.signal_receiver.try_recv(),
            Ok(SignalMessage::Shutdown)
        ));
        assert!(state.signal_receiver.try_recv().is_err());
    }
}
