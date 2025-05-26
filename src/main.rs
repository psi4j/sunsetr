use anyhow::{Context, Result};
use fs2::FileExt;
use signal_hook::{
    consts::signal::{SIGINT, SIGTERM},
    iterator::Signals,
};
use std::{
    fs::File,
    io::{self, Write},
    os::unix::io::AsRawFd,
    sync::Arc,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
use termios::{os::linux::ECHOCTL, *};

mod config;
mod constants;
mod hyprsunset;
mod logger;
mod process;
mod startup_transition;
mod time_state;
mod utils;

use config::Config;
use constants::*;
use hyprsunset::HyprsunsetClient;
use logger::Log;
use process::{HyprsunsetProcess, is_hyprsunset_running};
use startup_transition::StartupTransition;
use time_state::{TimeState, TransitionState, get_transition_state, time_until_next_event, get_initial_values_for_state};
use utils::{compare_versions, extract_version_from_output};

// Constants
const CHECK_INTERVAL: Duration = Duration::from_secs(CHECK_INTERVAL_SECS);

/// Verify that hyprsunset is installed and check version compatibility.
///
/// This function performs both installation verification and version checking
/// in a single step for efficiency. It will:
/// 1. Check if hyprsunset command exists
/// 2. Extract version information from output
/// 3. Validate version compatibility against requirements
///
/// # Returns
/// - `Ok(())` if hyprsunset is installed and compatible
/// - `Err` with detailed error message if issues are found
fn verify_hyprsunset_installed_and_version() -> Result<()> {
    // Check if hyprsunset exists and get version in one go
    match std::process::Command::new("hyprsunset")
        .arg("--version")
        .output()
    {
        Ok(output) => {
            // Check both stdout and stderr for version info
            let version_output = if !output.stdout.is_empty() {
                String::from_utf8_lossy(&output.stdout)
            } else {
                String::from_utf8_lossy(&output.stderr)
            };

            if let Some(version) = extract_version_from_output(&version_output) {
                Log::log_decorated(&format!("Found hyprsunset {}", version));

                if is_version_compatible(&version) {
                    Ok(())
                } else {
                    anyhow::bail!(
                        "hyprsunset {} is not compatible with sunsetr.\n\
                        Required minimum version: {}\n\
                        Compatible versions: {}\n\
                        Please update hyprsunset to a compatible version.",
                        version,
                        REQUIRED_HYPRSUNSET_VERSION,
                        COMPATIBLE_HYPRSUNSET_VERSIONS.join(", ")
                    )
                }
            } else {
                Log::log_warning("Could not parse version from hyprsunset output");
                Log::log_decorated("Attempting to proceed with compatibility test...");
                Ok(()) // Fall back to functional testing
            }
        }
        Err(_) => {
            // hyprsunset command failed - check if it's installed at all
            match std::process::Command::new("which")
                .arg("hyprsunset")
                .output()
            {
                Ok(which_output) if which_output.status.success() => {
                    Log::log_warning("hyprsunset found but version check failed");
                    Log::log_decorated(
                        "This might be an older version. Will attempt compatibility test...",
                    );
                    Ok(())
                }
                _ => anyhow::bail!("hyprsunset is not installed on the system"),
            }
        }
    }
}

/// Check if a hyprsunset version is compatible with sunsetr.
///
/// This function first checks against an explicit compatibility list,
/// then falls back to semantic version comparison if not found.
///
/// # Arguments
/// * `version` - Version string to check (e.g., "v0.2.0")
///
/// # Returns
/// `true` if the version is compatible, `false` otherwise
fn is_version_compatible(version: &str) -> bool {
    // Check if it's in our explicit compatibility list
    if COMPATIBLE_HYPRSUNSET_VERSIONS.contains(&version) {
        return true;
    }

    // Use the utility function for version comparison
    compare_versions(version, REQUIRED_HYPRSUNSET_VERSION) >= std::cmp::Ordering::Equal
}

/// Verify that we can establish a connection to the hyprsunset socket.
///
/// This function attempts connection with a retry mechanism to handle
/// cases where hyprsunset might still be starting up. It provides
/// detailed error messages to help users troubleshoot connection issues.
///
/// # Arguments
/// * `client` - Mutable reference to HyprsunsetClient for connection testing
///
/// # Returns
/// - `Ok(())` if connection is successful
/// - `Err` with troubleshooting information if connection fails
fn verify_hyprsunset_connection(client: &mut HyprsunsetClient) -> Result<()> {
    // First connection attempt
    if client.test_connection() {
        return Ok(());
    }

    // If first attempt fails, hyprsunset might still be starting up
    Log::log_decorated("Waiting 10 seconds for hyprsunset to become available...");

    thread::sleep(Duration::from_secs(10));

    // Second connection attempt after waiting
    if client.test_connection() {
        Log::log_decorated("Successfully connected to hyprsunset after waiting.");
        return Ok(());
    }

    // Both attempts failed - log critical error
    Log::log_critical("Cannot connect to hyprsunset socket.");
    println!();

    // Both attempts failed - provide helpful error message
    anyhow::bail!(
        "This usually means:\n\
          • hyprsunset is not running\n\
          • hyprsunset service is not enabled\n\
          • You're not running on Hyprland\n\
        \n\
        Please ensure hyprsunset is running and try again.\n\
        \n\
        Suggested hyprsunset startup methods:\n\
          1. Autostart hyprsunset: set start_hyprsunset to true in sunsetr.toml\n\
          2. Start hyprsunset manually: hyprsunset\n\
          3. Enable the service: systemctl --user enable hyprsunset.service"
    );
}

/// Manages terminal state to hide cursor and suppress control character echoing.
///
/// This struct automatically restores the original terminal state when dropped,
/// ensuring clean cleanup even if the program exits unexpectedly.
struct TerminalGuard {
    original_termios: Termios,
}

impl TerminalGuard {
    /// Create a new terminal guard and modify terminal settings.
    ///
    /// Sets up the terminal to:
    /// - Hide the cursor for cleaner output
    /// - Suppress echoing of control characters like ^C
    ///
    /// # Returns
    /// - `Ok(Some(guard))` if terminal is available and settings were applied
    /// - `Ok(None)` if no terminal is available (e.g., running as a service)
    /// - `Err` only for unexpected errors
    fn new() -> io::Result<Option<Self>> {
        // Try to open the controlling tty - if it fails, we're likely running headless
        let tty = match File::open("/dev/tty") {
            Ok(tty) => tty,
            Err(e) if e.kind() == io::ErrorKind::NotFound || e.raw_os_error() == Some(6) => {
                // No controlling terminal (common in systemd services) - this is not an error
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        
        let fd = tty.as_raw_fd();

        // Take a snapshot of the current settings for restoration
        let mut term = Termios::from_fd(fd)?;
        let original = term;

        // Disable the "^C" echo to prevent visual noise during shutdown
        term.c_lflag &= !ECHOCTL;
        tcsetattr(fd, TCSANOW, &term)?;

        // Hide the cursor for cleaner output display
        print!("\x1b[?25l");
        io::stdout().flush()?; // always flush control sequences

        Ok(Some(Self {
            original_termios: original,
        }))
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Best-effort restore of termios + cursor visibility
        if let Ok(tty) = File::open("/dev/tty") {
            let _ = tcsetattr(tty.as_raw_fd(), TCSANOW, &self.original_termios);
        }
        let _ = write!(io::stdout(), "\x1b[?25h");
        let _ = io::stdout().flush();
    }
}

/// Perform cleanup operations when shutting down the application.
///
/// This function handles:
/// - Stopping any hyprsunset process we started
/// - Releasing the lock file
/// - Removing the lock file from disk
///
/// # Arguments
/// * `hyprsunset_process` - Optional process handle if we started hyprsunset
/// * `lock_file` - File handle for the application lock
/// * `lock_path` - Path to the lock file for removal
fn cleanup(hyprsunset_process: Option<HyprsunsetProcess>, lock_file: File, lock_path: &str) {
    Log::log_decorated("Performing cleanup...");

    // Stop hyprsunset process if we started it
    if let Some(process) = hyprsunset_process {
        if let Err(e) = process.stop() {
            Log::log_error(&format!("Error stopping hyprsunset: {}", e));
        }
    }

    // Drop the lock file handle to release the lock
    drop(lock_file);

    // Remove the lock file from disk
    if let Err(e) = std::fs::remove_file(lock_path) {
        Log::log_decorated(&format!("Warning: Failed to remove lock file: {}", e));
    } else {
        Log::log_decorated("Lock file removed successfully");
    }

    Log::log_decorated("Cleanup complete");
}

/// Determine whether the application state should be updated.
///
/// This function implements the logic for deciding when to apply state changes
/// to hyprsunset. It considers:
/// - Transition start/end detection
/// - Progress during ongoing transitions  
/// - State changes between stable periods
/// - Time jump detection (system sleep/resume)
///
/// # Arguments
/// * `current_state` - The last known transition state
/// * `new_state` - The newly calculated transition state
/// * `time_since_last_check` - Duration since last state check
///
/// # Returns
/// `true` if the state should be updated, `false` to skip this update cycle
fn should_update_state(
    current_state: &TransitionState,
    new_state: &TransitionState,
    time_since_last_check: Duration,
) -> bool {
    match (current_state, new_state) {
        // Detect entering a transition (from stable to transitioning)
        (TransitionState::Stable(_), TransitionState::Transitioning { progress, from, to })
            if *progress < 0.01 =>
        {
            Log::log_pipe();
            let transition_type = match (from, to) {
                (TimeState::Day, TimeState::Night) => "sunset 󰖛 ",
                (TimeState::Night, TimeState::Day) => "sunrise 󰖜 ",
                _ => "transition",
            };
            Log::log_decorated(&format!("Commencing {}", transition_type));
            true
        }
        // Detect completing a transition (progress near 100%)
        (
            TransitionState::Transitioning {
                progress: prev_progress,
                ..
            },
            TransitionState::Transitioning {
                progress: curr_progress,
                from,
                to,
            },
        ) if *prev_progress < 0.99 && *curr_progress >= 0.99 => {
            Log::log_pipe();
            let transition_type = match (from, to) {
                (TimeState::Day, TimeState::Night) => "sunset 󰖛 ",
                (TimeState::Night, TimeState::Day) => "sunrise 󰖜 ",
                _ => "transition",
            };
            Log::log_decorated(&format!("Completed {}", transition_type));
            true
        }
        // Detect change from transitioning to stable state
        (TransitionState::Transitioning { .. }, TransitionState::Stable(_)) => true,
        // Detect change from one stable state to another (should be rare)
        (TransitionState::Stable(prev), TransitionState::Stable(curr)) if prev != curr => {
            Log::log_block_start(&format!("State changed from {:?} to {:?}", prev, curr));
            true
        }
        // We're in a transition and it's time for a regular update
        (TransitionState::Transitioning { .. }, TransitionState::Transitioning { .. }) => true,
        // Large time jump detected - force update to handle system sleep/resume
        _ if time_since_last_check > Duration::from_secs(SLEEP_DETECTION_THRESHOLD_SECS) => {
            Log::log_decorated("Applying state due to time jump detection");
            true
        }
        _ => false,
    }
}

fn main() -> Result<()> {
    // Try to set up terminal features (cursor hiding, echo suppression)
    // This will gracefully handle cases where no terminal is available (e.g., systemd service)
    let _term = TerminalGuard::new()
        .context("failed to initialize terminal features")?;

    // Handle version flag
    if std::env::args()
        .nth(1)
        .is_some_and(|arg| arg == "--version" || arg == "-v")
    {
        println!("sunsetr {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    Log::log_version();

    // First thing: verify hyprsunset is installed and compatible version
    verify_hyprsunset_installed_and_version()?;

    // Set up signal handling
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    let mut signals = Signals::new([SIGTERM, SIGINT])?;
    thread::spawn(move || {
        for signal in signals.forever() {
            Log::log_pipe();
            Log::log_info(&format!("Shutdown signal received: {:?}", signal));
            r.store(false, Ordering::SeqCst);
            Log::log_info("Set running flag to false");
        }
    });

    // Create and acquire lock file
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let lock_path = format!("{}/sunsetr.lock", runtime_dir);
    let lock_file = File::create(&lock_path)?;

    // Try to acquire exclusive lock
    match lock_file.try_lock_exclusive() {
        Ok(_) => {
            Log::log_decorated("Lock acquired, starting sunsetr...");

            // Load configuration first
            let config = Config::load()?;

            // Log configuration before starting hyprsunset
            config.log_config();

            // Track hyprsunset process if we start it
            let hyprsunset_process = if config.start_hyprsunset.unwrap_or(false) {
                // Check if hyprsunset is already running
                if is_hyprsunset_running() {
                    Log::log_pipe();
                    Log::log_error(
                        "hyprsunset is already running but start_hyprsunset is set to true.\n\
                        This conflict prevents sunsetr from starting its own hyprsunset instance.\n\
                        \n\
                        To fix this, either:\n\
                        • Kill the existing hyprsunset process: pkill hyprsunset\n\
                        • Change start_hyprsunset = false in sunsetr.toml\n\
                        \n\
                        Choose the first option if you want sunsetr to manage hyprsunset.\n\
                        Choose the second option if you're using another method to start hyprsunset.",
                    );
                    std::process::exit(1);
                }

                // Determine initial values based on startup_transition setting
                let startup_transition = config
                    .startup_transition
                    .unwrap_or(DEFAULT_STARTUP_TRANSITION);
                    
                let (initial_temp, initial_gamma) = if startup_transition {
                    // If startup transition is enabled, always start with day values
                    // so the startup transition can smoothly animate from day to current state
                    (
                        config.day_temp.unwrap_or(DEFAULT_DAY_TEMP),
                        config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA),
                    )
                } else {
                    // If startup transition is disabled, start with current interpolated values
                    // for immediate correctness without any transition
                    let current_state = get_transition_state(&config);
                    get_initial_values_for_state(current_state, &config)
                };

                // Start hyprsunset with the calculated initial values
                let process = HyprsunsetProcess::new(initial_temp, initial_gamma)?;
                Some(process)
            } else {
                None
            };

            // Initialize hyprsunset client
            let mut client = HyprsunsetClient::new()?;

            // Verify hyprsunset connection and IPC compatibility - exit if it fails
            verify_hyprsunset_connection(&mut client)?;

            let mut current_transition_state = get_transition_state(&config);
            let mut last_check_time = Instant::now();

            // Apply initial settings (should work since we verified connection)
            // Note: We don't update current_transition_state after these applications because
            // the main loop skips its first iteration to prevent startup timing conflicts.
            // The startup transition system uses the originally captured state to avoid
            // timing-related issues where the state might change during startup.
            if running.load(Ordering::SeqCst) {
                // Check if startup transition is enabled
                let startup_transition = config
                    .startup_transition
                    .unwrap_or(DEFAULT_STARTUP_TRANSITION);
                let startup_duration = config
                    .startup_transition_duration
                    .unwrap_or(DEFAULT_STARTUP_TRANSITION_DURATION);

                if startup_transition && startup_duration > 0 {
                    // Use the smooth transition system (from day values to current state)

                    // Create and execute the startup transition
                    let mut transition = StartupTransition::new(current_transition_state, &config);
                    match transition.execute(&mut client, &config, &running) {
                        Ok(_) => {}
                        Err(e) => {
                            Log::log_warning(&format!(
                                "Failed to apply smooth startup transition: {}",
                                e
                            ));
                            Log::log_decorated("Falling back to immediate transition...");

                            // Fallback to immediate application
                            match client.apply_startup_state(
                                current_transition_state,
                                &config,
                                &running,
                            ) {
                                Ok(_) => {
                                    Log::log_block_start(
                                        "Initial state applied successfully (fallback)",
                                    );
                                }
                                Err(e) => {
                                    Log::log_warning(&format!(
                                        "Failed to apply initial state: {}",
                                        e
                                    ));
                                    Log::log_decorated(
                                        "Continuing anyway - will retry during operation...",
                                    );
                                }
                            }
                        }
                    }
                } else {
                    // Use immediate transition to current interpolated values
                    match client.apply_startup_state(current_transition_state, &config, &running) {
                        Ok(_) => {
                            Log::log_block_start("Initial state applied successfully");
                        }
                        Err(e) => {
                            Log::log_warning(&format!("Failed to apply initial state: {}", e));
                            Log::log_decorated(
                                "Continuing anyway - will retry during operation...",
                            );
                        }
                    }
                }
            }

            // Main loop with transition support and sleep/resume detection
            // Skip first iteration to prevent false state change detection due to startup timing
            let mut first_iteration = true;
            while running.load(Ordering::SeqCst) {
                // Detect large time jumps (system sleep/resume scenarios)
                let current_time = Instant::now();
                let time_since_last_check = current_time.duration_since(last_check_time);
                if time_since_last_check > Duration::from_secs(SLEEP_DETECTION_THRESHOLD_SECS) {
                    // 5+ minutes
                    Log::log_decorated(&format!(
                        "Large time jump detected ({} minutes). System may have resumed from sleep.",
                        time_since_last_check.as_secs() / 60
                    ));
                    Log::log_decorated("Forcing immediate state recalculation...");
                }
                last_check_time = current_time;

                let new_state = get_transition_state(&config);

                // Skip first iteration to prevent false state change detection caused by
                // timing differences between startup state application and main loop start
                let should_update = if first_iteration {
                    first_iteration = false;
                    false
                } else {
                    should_update_state(
                        &current_transition_state,
                        &new_state,
                        time_since_last_check,
                    )
                };

                if should_update && running.load(Ordering::SeqCst) {
                    match client.apply_transition_state(new_state, &config, &running) {
                        Ok(_) => {
                            // Success - update our state
                            current_transition_state = new_state;
                            // Only log completion for non-transition states or specific transition points
                            // which are already handled in the should_update match above
                        }
                        Err(e) => {
                            // Failure - check if it's a connection issue that couldn't be resolved
                            if e.to_string().contains("reconnection attempt") {
                                Log::log_error(&format!(
                                    "Cannot communicate with hyprsunset: {}",
                                    e
                                ));
                                Log::log_decorated(
                                    "hyprsunset appears to be permanently unavailable. Exiting...",
                                );
                                break; // Exit the main loop
                            } else {
                                // Other error - just log it and retry next cycle
                                Log::log_warning(&format!("Failed to apply state: {}", e));
                                Log::log_decorated("Will retry on next cycle...");
                            }
                            // Don't update current_transition_state - try again next cycle
                        }
                    }
                }

                // Determine sleep duration based on state (simple, no special failure handling)
                let sleep_duration = match new_state {
                    TransitionState::Transitioning { .. } => Duration::from_secs(
                        config.update_interval.unwrap_or(DEFAULT_UPDATE_INTERVAL),
                    ),
                    TransitionState::Stable(_) => time_until_next_event(&config),
                };

                // Show next update timing with more context
                match new_state {
                    TransitionState::Transitioning { progress, .. } => {
                        Log::log_decorated(&format!(
                            "Transition {}% complete. Next update in {} seconds",
                            (progress * 100.0) as u8,
                            sleep_duration.as_secs()
                        ));
                    }
                    TransitionState::Stable(_) => {
                        Log::log_block_start(&format!(
                            "Next transition in {} minutes {} seconds",
                            sleep_duration.as_secs() / 60,
                            sleep_duration.as_secs() % 60
                        ));
                    }
                }

                // Sleep in smaller intervals to check running status
                let mut slept = Duration::from_secs(0);
                while slept < sleep_duration && running.load(Ordering::SeqCst) {
                    let sleep_chunk = CHECK_INTERVAL.min(sleep_duration - slept);
                    thread::sleep(sleep_chunk);
                    slept += sleep_chunk;
                }
            }

            // Ensure proper cleanup on shutdown
            Log::log_block_start("Shutting down sunsetr...");
            cleanup(hyprsunset_process, lock_file, &lock_path);
            Log::log_end();
        }
        Err(_) => {
            Log::log_error(
                "Another instance of sunsetr is already running.\n\
                • Kill sunsetr before restarting.",
            );
            std::process::exit(1);
        }
    }

    Ok(())
}
