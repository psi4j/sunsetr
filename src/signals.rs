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

use crate::logger::Log;

/// Test mode parameters passed via signal
#[derive(Debug, Clone)]
pub struct TestModeParams {
    pub temperature: u32,
    pub gamma: f32,
}

/// Unified signal message type for all signal-based communication
#[derive(Debug, Clone)]
pub enum SignalMessage {
    /// Configuration reload signal (SIGUSR2)
    Reload,
    /// Test mode signal with parameters (SIGUSR1)
    TestMode(TestModeParams),
    /// Shutdown signal (SIGTERM, SIGINT, SIGHUP)
    Shutdown,
}

/// Signal handling state shared between threads
pub struct SignalState {
    /// Atomic flag indicating if the application should keep running
    pub running: Arc<AtomicBool>,
    /// Channel receiver for unified signal messages
    pub signal_receiver: std::sync::mpsc::Receiver<SignalMessage>,
}

/// Handle a signal message received in the main loop
pub fn handle_signal_message(
    signal_msg: SignalMessage,
    backend: &mut Box<dyn crate::backend::ColorTemperatureBackend>,
    config: &mut crate::config::Config,
    signal_state: &SignalState,
    current_state: &mut crate::time_state::TransitionState,
) -> Result<()> {
    match signal_msg {
        SignalMessage::TestMode(test_params) => {
            #[cfg(debug_assertions)]
            {
                eprintln!("DEBUG: Main loop received test signal: {}K @ {}%", test_params.temperature, test_params.gamma);
                let log_msg = format!("Main loop received test signal: {}K @ {}%\n", test_params.temperature, test_params.gamma);
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                    .and_then(|mut f| {
                        use std::io::Write;
                        f.write_all(log_msg.as_bytes())
                    });
            }
            
            // Enter test mode loop (blocks until test mode exits)
            crate::commands::test::run_test_mode_loop(test_params, backend, signal_state, config)?;
            
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: Returned from test mode loop, resuming main loop");
        }
        SignalMessage::Shutdown => {
            #[cfg(debug_assertions)]
            {
                eprintln!("DEBUG: Main loop received shutdown signal");
                let log_msg = "Main loop received shutdown signal\n".to_string();
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                    .and_then(|mut f| {
                        use std::io::Write;
                        f.write_all(log_msg.as_bytes())
                    });
            }
            
            // Set running to false to trigger main loop exit
            signal_state.running.store(false, Ordering::SeqCst);
        }
        SignalMessage::Reload => {
            #[cfg(debug_assertions)]
            {
                eprintln!(
                    "DEBUG: Main loop processing reload message, PID: {}",
                    std::process::id()
                );
                let log_msg = format!(
                    "Main loop processing reload message, PID: {}\n",
                    std::process::id()
                );
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                    .and_then(|mut f| {
                        use std::io::Write;
                        f.write_all(log_msg.as_bytes())
                    });
            }

            // Reload configuration
            match crate::config::Config::load() {
                Ok(new_config) => {
                    #[cfg(debug_assertions)]
                    {
                        eprintln!(
                            "DEBUG: Config reload - old coords: lat={:?}, lon={:?}, new coords: lat={:?}, lon={:?}",
                            config.latitude, config.longitude, new_config.latitude, new_config.longitude
                        );
                        let log_msg = format!(
                            "Config reload - old coords: lat={:?}, lon={:?}, new coords: lat={:?}, lon={:?}\n",
                            config.latitude, config.longitude, new_config.latitude, new_config.longitude
                        );
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                            .and_then(|mut f| {
                                use std::io::Write;
                                f.write_all(log_msg.as_bytes())
                            });
                    }

                    // Replace config with new loaded config
                    *config = new_config;

                    // Check new state and apply immediately
                    let new_state = crate::time_state::get_transition_state(config);

                    #[cfg(debug_assertions)]
                    {
                        let old_state = *current_state;
                        eprintln!("DEBUG: State transition - old: {:?}, new: {:?}", old_state, new_state);
                        let log_msg = format!("State transition - old: {:?}, new: {:?}\n", old_state, new_state);
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                            .and_then(|mut f| {
                                use std::io::Write;
                                f.write_all(log_msg.as_bytes())
                            });
                    }

                    // Only apply state if it actually changed after config reload
                    if *current_state != new_state {
                        Log::log_pipe();
                        Log::log_decorated("State changed after config reload, applying new state...");

                        let (temperature, gamma) = crate::time_state::get_initial_values_for_state(new_state, config);
                        match backend.apply_temperature_gamma(temperature, gamma, &signal_state.running) {
                            Ok(_) => {
                                #[cfg(debug_assertions)]
                                {
                                    eprintln!("DEBUG: New state applied successfully after config reload");
                                    let log_msg = "New state applied successfully after config reload\n";
                                    let _ = std::fs::OpenOptions::new()
                                        .create(true)
                                        .append(true)
                                        .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                                        .and_then(|mut f| {
                                            use std::io::Write;
                                            f.write_all(log_msg.as_bytes())
                                        });
                                }
                                Log::log_decorated("New state applied successfully after config reload");
                                *current_state = new_state;
                            }
                            Err(e) => {
                                Log::log_warning(&format!("Failed to apply new state after config reload: {}", e));
                            }
                        }
                    } else {
                        Log::log_pipe();
                        Log::log_decorated("State unchanged after config reload, no backend update needed");
                        #[cfg(debug_assertions)]
                        eprintln!("DEBUG: State unchanged after config reload - old: {:?}, new: {:?}", current_state, new_state);
                    }
                }
                Err(e) => {
                    Log::log_warning(&format!("Failed to reload config: {}", e));
                }
            }
        }
    }
    
    Ok(())
}

/// Set up signal handling for the application.
/// 
/// Returns a SignalState containing the running flag and signal receiver channel.
/// Spawns a background thread that monitors for signals and sends appropriate
/// messages via the channel.
pub fn setup_signal_handler(debug_enabled: bool) -> Result<SignalState> {
    let running = Arc::new(AtomicBool::new(true));
    let (signal_sender, signal_receiver) = std::sync::mpsc::channel::<SignalMessage>();

    let mut signals = Signals::new([SIGINT, SIGTERM, SIGHUP, SIGUSR1, SIGUSR2])
        .context("failed to register signal handlers")?;

    let running_clone = running.clone();
    let signal_sender_clone = signal_sender.clone();

    thread::spawn(move || {
        #[cfg(debug_assertions)]
        {
            eprintln!("DEBUG: Signal handler setup complete for PID: {}", std::process::id());
            let log_msg = format!("Signal handler setup complete for PID: {}\n", std::process::id());
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                .and_then(|mut f| {
                    use std::io::Write;
                    f.write_all(log_msg.as_bytes())
                });
        }

        #[cfg(debug_assertions)]
        {
            eprintln!("DEBUG: Signal handler thread starting for PID: {}", std::process::id());
            let log_msg = format!("Signal handler thread starting for PID: {}\n", std::process::id());
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                .and_then(|mut f| {
                    use std::io::Write;
                    f.write_all(log_msg.as_bytes())
                });
        }

        #[cfg(debug_assertions)]
        let mut signal_count = 0;
        #[cfg(debug_assertions)]
        let mut sigusr2_count = 0;
        
        for sig in signals.forever() {
            #[cfg(debug_assertions)]
            {
                signal_count += 1;
            }
            
            #[cfg(debug_assertions)]
            {
                let log_msg = format!("Signal handler processing signal #{}: {}\n", signal_count, sig);
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                    .and_then(|mut f| {
                        use std::io::Write;
                        f.write_all(log_msg.as_bytes())
                    });
            }
            
            match sig {
                SIGUSR1 => {
                    // SIGUSR1 is used for test mode
                    Log::log_pipe();
                    Log::log_decorated("Received test mode signal");
                    
                    // Read test parameters from temp file
                    let test_file_path = format!("/tmp/sunsetr-test-{}.tmp", std::process::id());
                    match std::fs::read_to_string(&test_file_path) {
                        Ok(content) => {
                            let lines: Vec<&str> = content.trim().lines().collect();
                            if lines.len() == 2 {
                                if let (Ok(temp), Ok(gamma)) = (lines[0].parse::<u32>(), lines[1].parse::<f32>()) {
                                    let test_params = TestModeParams {
                                        temperature: temp,
                                        gamma,
                                    };
                                    
                                    match signal_sender_clone.send(SignalMessage::TestMode(test_params)) {
                                        Ok(()) => {
                                            #[cfg(debug_assertions)]
                                            {
                                                eprintln!("DEBUG: Test mode parameters sent: {}K @ {}%", temp, gamma);
                                            }
                                        }
                                        Err(_) => {
                                            #[cfg(debug_assertions)]
                                            {
                                                eprintln!("DEBUG: Failed to send test parameters - channel disconnected");
                                            }
                                            break;
                                        }
                                    }
                                }
                            }
                            // Clean up temp file
                            let _ = std::fs::remove_file(&test_file_path);
                        }
                        Err(_) => {
                            #[cfg(debug_assertions)]
                            {
                                eprintln!("DEBUG: Failed to read test parameters from {}", test_file_path);
                            }
                        }
                    }
                }
                SIGUSR2 => {
                    #[cfg(debug_assertions)]
                    {
                        sigusr2_count += 1;
                    }
                    
                    // SIGUSR2 is used for config reload
                    #[cfg(debug_assertions)]
                    {
                        eprintln!("DEBUG: SIGUSR2 #{} received by PID: {}, sending reload message", sigusr2_count, std::process::id());
                        let log_msg = format!("SIGUSR2 #{} received by PID: {}, sending reload message\n", sigusr2_count, std::process::id());
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                            .and_then(|mut f| {
                                use std::io::Write;
                                f.write_all(log_msg.as_bytes())
                            });
                    }
                    
                    Log::log_pipe();
                    Log::log_decorated("Received configuration reload signal");
                    
                    // Send reload message via channel (non-blocking)
                    match signal_sender_clone.send(SignalMessage::Reload) {
                        Ok(()) => {
                            #[cfg(debug_assertions)]
                            {
                                eprintln!("DEBUG: Reload message #{} sent successfully", sigusr2_count);
                                let log_msg = format!("Reload message #{} sent successfully\n", sigusr2_count);
                                let _ = std::fs::OpenOptions::new()
                                    .create(true)
                                    .append(true)
                                    .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                                    .and_then(|mut f| {
                                        use std::io::Write;
                                        f.write_all(log_msg.as_bytes())
                                    });
                            }
                        }
                        Err(_e) => {
                            // Channel receiver was dropped - main thread probably exiting
                            #[cfg(debug_assertions)]
                            {
                                eprintln!("DEBUG: Failed to send reload message #{}: {:?} - channel disconnected", sigusr2_count, _e);
                                let log_msg = format!("Failed to send reload message #{}: {:?} - channel disconnected\n", sigusr2_count, _e);
                                let _ = std::fs::OpenOptions::new()
                                    .create(true)
                                    .append(true)
                                    .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                                    .and_then(|mut f| {
                                        use std::io::Write;
                                        f.write_all(log_msg.as_bytes())
                                    });
                            }
                            
                            // Channel is disconnected, break out of signal loop
                            #[cfg(debug_assertions)]
                            {
                                let log_msg = format!("Signal handler thread exiting due to channel disconnection after {} signals ({} SIGUSR2)\n", signal_count, sigusr2_count);
                                let _ = std::fs::OpenOptions::new()
                                    .create(true)
                                    .append(true)
                                    .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                                    .and_then(|mut f| {
                                        use std::io::Write;
                                        f.write_all(log_msg.as_bytes())
                                    });
                            }
                            break;
                        }
                    }
                }
                _ => {
                    #[cfg(debug_assertions)]
                    {
                        let signal_name = match sig {
                            SIGINT => "SIGINT (Ctrl+C)",
                            SIGTERM => "SIGTERM (termination request)",
                            SIGHUP => "SIGHUP (session logout)",
                            _ => "unknown signal",
                        };
                        eprintln!("DEBUG: Received {} (signal #{}), setting running=false", signal_name, signal_count);
                        let log_msg = format!("Received {} (signal #{}), setting running=false\n", signal_name, signal_count);
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                            .and_then(|mut f| {
                                use std::io::Write;
                                f.write_all(log_msg.as_bytes())
                            });
                    }
                    
                    // Always log shutdown signals for user clarity
                    let user_message = match sig {
                        SIGINT => {
                            if debug_enabled {
                                "Received SIGINT (Ctrl+C), initiating graceful shutdown..."
                            } else {
                                "Received interrupt signal, initiating graceful shutdown..."
                            }
                        },
                        SIGTERM => "Received termination request, initiating graceful shutdown...",
                        SIGHUP => "Received hangup signal, initiating graceful shutdown...",
                        _ => "Received shutdown signal, initiating graceful shutdown...",
                    };
                    
                    Log::log_pipe();
                    Log::log_decorated(user_message);
                    
                    // Send shutdown message to main loop first
                    if let Err(e) = signal_sender.send(SignalMessage::Shutdown) {
                        Log::log_warning(&format!("Failed to send shutdown message: {}", e));
                        Log::log_indented("Cleanup will rely on fallback mechanisms");
                    }
                    
                    // For shutdown signals, set the flag to stop
                    running_clone.store(false, Ordering::SeqCst);
                    
                    // Note: We don't do emergency cleanup here anymore because it interferes
                    // with the normal cleanup path trying to reset gamma to 6500K.
                    // The Drop trait and normal cleanup should handle most cases.
                    
                    #[cfg(debug_assertions)]
                    {
                        let log_msg = format!("Signal handler set running=false after {} signals ({} SIGUSR2), continuing signal processing\n", signal_count, sigusr2_count);
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(format!("/tmp/sunsetr-debug-{}.log", std::process::id()))
                            .and_then(|mut f| {
                                use std::io::Write;
                                f.write_all(log_msg.as_bytes())
                            });
                    }
                    
                    // Continue processing signals until process exits
                    // Don't break - keep signal thread alive
                }
            }
        }
    });

    Ok(SignalState {
        running,
        signal_receiver,
    })
}

/// Get the PID of a currently running sunsetr process.
///
/// Returns the process ID if found, or an error if no process is running
/// or if multiple processes are found.
pub fn get_running_sunsetr_pid() -> Result<u32> {
    use std::process::Command;
    
    let output = Command::new("pgrep")
        .arg("-f")
        .arg("sunsetr")
        .output()
        .context("failed to run pgrep command")?;
    
    if !output.status.success() {
        anyhow::bail!("No sunsetr process found");
    }
    
    let stdout = String::from_utf8(output.stdout)
        .context("pgrep output contains invalid UTF-8")?;
    
    let current_pid = std::process::id();
    let pids: Vec<u32> = stdout
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .filter(|&pid| pid != current_pid) // Exclude current process
        .collect();
    
    match pids.len() {
        0 => anyhow::bail!("No other sunsetr process found"),
        1 => Ok(pids[0]),
        _ => anyhow::bail!("Multiple sunsetr processes found: {:?}", pids),
    }
}

/// Check if a process with the given PID is currently running.
pub fn is_process_running(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{}", pid)).exists()
}

/// Spawn a new sunsetr process in the background using compositor-specific commands.
pub fn spawn_background_process(debug_enabled: bool) -> Result<()> {
    use crate::backend::{detect_compositor, Compositor};
    
    #[cfg(debug_assertions)]
    eprintln!("DEBUG: spawn_background_process() entry, PID: {}", std::process::id());
    
    let current_exe = std::env::current_exe()
        .context("failed to get current executable path")?;
    
    let sunsetr_path = current_exe.to_string_lossy();
    
    let compositor = detect_compositor();
    
    #[cfg(debug_assertions)]
    eprintln!("DEBUG: Detected compositor: {:?}", compositor);
    #[cfg(debug_assertions)]
    eprintln!("DEBUG: sunsetr_path: {}", sunsetr_path);
    
    match compositor {
        Compositor::Niri => {
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: About to spawn via niri: niri msg action spawn -- {}", sunsetr_path);
            
            Log::log_pipe();
            Log::log_decorated("Starting sunsetr via niri compositor...");
            
            let output = std::process::Command::new("niri")
                .args(["msg", "action", "spawn", "--", &sunsetr_path])
                .output()
                .context("failed to execute niri spawn command")?;
            
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("niri spawn command failed: {}", stderr);
            }
            
            Log::log_decorated("Background process started.");
        }
        Compositor::Hyprland => {
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: About to spawn via hyprland: hyprctl dispatch exec {}", sunsetr_path);
            
            Log::log_pipe();
            Log::log_decorated("Starting sunsetr via Hyprland compositor...");
            
            let output = std::process::Command::new("hyprctl")
                .args(["dispatch", "exec", &sunsetr_path])
                .output()
                .context("failed to execute hyprctl dispatch command")?;
            
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("hyprctl dispatch command failed: {}", stderr);
            }
            
            Log::log_decorated("Background process started.");
        }
        Compositor::Sway => {
            #[cfg(debug_assertions)]
            eprintln!("DEBUG: About to spawn via sway: swaymsg exec {}", sunsetr_path);
            
            Log::log_pipe();
            Log::log_decorated("Starting sunsetr via Sway compositor...");
            
            let output = std::process::Command::new("swaymsg")
                .args(["exec", &sunsetr_path])
                .output()
                .context("failed to execute swaymsg exec command")?;
            
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("swaymsg exec command failed: {}", stderr);
            }
            
            Log::log_decorated("Background process started.");
        }
        Compositor::Other(name) => {
            Log::log_warning(&format!("Unknown compositor '{}' - starting sunsetr directly", name));
            Log::log_indented("For best results, use compositor-specific spawn commands:");
            Log::log_indented(&format!("  niri msg action spawn -- {}", sunsetr_path));
            Log::log_indented(&format!("  hyprctl dispatch exec {}", sunsetr_path));
            Log::log_indented(&format!("  swaymsg exec {}", sunsetr_path));
            
            let mut cmd = std::process::Command::new(sunsetr_path.as_ref());
            if debug_enabled {
                cmd.arg("--debug");
            }
            
            cmd.spawn()
                .context("failed to spawn sunsetr process directly")?;
            
            Log::log_decorated("Background process started directly.");
        }
    }
    
    #[cfg(debug_assertions)]
    eprintln!("DEBUG: spawn_background_process() exit, PID: {}", std::process::id());
    
    Ok(())
}