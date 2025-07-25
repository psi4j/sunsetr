//! Utility functions shared across the codebase.
//!
//! This module provides common functionality for interpolation, version handling,
//! terminal management, process management, and other helper operations used
//! throughout the application.

use crate::logger::Log;
use anyhow::{Context, Result};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::Print,
    terminal::{self, ClearType},
};
use std::{
    fs::File,
    io::{self, Write},
    os::unix::io::AsRawFd,
    sync::Arc,
    sync::atomic::AtomicBool,
};
use termios::{ECHO, TCSANOW, Termios, os::linux::ECHOCTL, tcsetattr};

/// Interpolate between two u32 values based on progress (0.0 to 1.0).
///
/// This function provides smooth transitions between integer values, commonly
/// used for color temperature transitions during sunrise/sunset.
///
/// # Arguments
/// * `start` - Starting value (returned when progress = 0.0)
/// * `end` - Ending value (returned when progress = 1.0)
/// * `progress` - Interpolation progress, automatically clamped to [0.0, 1.0]
///
/// # Returns
/// Interpolated value rounded to the nearest integer
///
/// # Examples
/// ```
/// use sunsetr::utils::interpolate_u32;
/// assert_eq!(interpolate_u32(1000, 2000, 0.5), 1500);
/// assert_eq!(interpolate_u32(6000, 3000, 0.25), 5250);
/// ```
pub fn interpolate_u32(start: u32, end: u32, progress: f32) -> u32 {
    let start_f = start as f32;
    let end_f = end as f32;
    let result = start_f + (end_f - start_f) * progress.clamp(0.0, 1.0);
    result.round() as u32
}

/// Interpolate between two f32 values based on progress (0.0 to 1.0).
///
/// This function provides smooth transitions between floating-point values,
/// commonly used for gamma/brightness transitions during sunrise/sunset.
///
/// # Arguments
/// * `start` - Starting value (returned when progress = 0.0)
/// * `end` - Ending value (returned when progress = 1.0)
/// * `progress` - Interpolation progress, automatically clamped to [0.0, 1.0]
///
/// # Returns
/// Interpolated floating-point value
///
/// # Examples
/// ```
/// use sunsetr::utils::interpolate_f32;
/// assert_eq!(interpolate_f32(90.0, 100.0, 0.5), 95.0);
/// assert_eq!(interpolate_f32(100.0, 90.0, 0.3), 97.0);
/// ```
pub fn interpolate_f32(start: f32, end: f32, progress: f32) -> f32 {
    start + (end - start) * progress.clamp(0.0, 1.0)
}

/// Apply a cubic Bezier curve to transition progress.
///
/// This function transforms linear progress (0.0 to 1.0) using a cubic Bezier curve
/// that provides smooth, natural-looking transitions with customizable acceleration.
/// The curve starts at (0,0) and ends at (1,1) with two control points, eliminating
/// sudden jumps at transition boundaries while allowing fine-tuned easing.
///
/// Uses the cubic Bezier formula: B(t) = (1-t)³P₀ + 3(1-t)²tP₁ + 3(1-t)t²P₂ + t³P₃
/// Where P₀=(0,0) and P₃=(1,1) for normalized transitions.
///
/// ## Control Point Guidelines
///
/// For sunrise/sunset transitions:
/// - `(0.25, 0.0), (0.75, 1.0)` - Gentle S-curve, natural feel (recommended)
/// - `(0.42, 0.0), (0.58, 1.0)` - Steeper transition, more dramatic
/// - `(0.33, 0.33), (0.67, 0.67)` - Nearly linear, subtle smoothing
/// - `(0.1, 0.0), (0.9, 1.0)` - Very gentle start/end, sharp middle
///
/// Visual transition effects:
/// - Lower P1x values = slower start
/// - Higher P2x values = slower end  
/// - P1y > 0 = initial overshoot (not recommended for color temperature)
/// - P2y < 1 = final undershoot (not recommended for color temperature)
///
/// # Arguments
/// * `progress` - Linear progress value (0.0 to 1.0), automatically clamped
/// * `p1x` - X coordinate of first control point (typically 0.0 to 0.5)
/// * `p1y` - Y coordinate of first control point (typically 0.0 for smooth start)
/// * `p2x` - X coordinate of second control point (typically 0.5 to 1.0)  
/// * `p2y` - Y coordinate of second control point (typically 1.0 for smooth end)
///
/// # Returns
/// Transformed progress value following the Bezier curve, guaranteed in \[0,1\]
///
/// # Examples
/// ```
/// use sunsetr::utils::bezier_curve;
///
/// // Gentle S-curve (recommended for color temperature transitions)
/// let smooth = bezier_curve(0.5, 0.25, 0.0, 0.75, 1.0);
/// assert!((smooth - 0.5).abs() < 0.1); // Near midpoint
///
/// // Verify smooth endpoints
/// let start = bezier_curve(0.0, 0.25, 0.0, 0.75, 1.0);
/// let end = bezier_curve(1.0, 0.25, 0.0, 0.75, 1.0);
/// assert_eq!(start, 0.0);
/// assert_eq!(end, 1.0);
///
/// // Steeper transition for more dramatic effects
/// let steep = bezier_curve(0.5, 0.42, 0.0, 0.58, 1.0);
/// ```
pub fn bezier_curve(progress: f32, _p1x: f32, p1y: f32, _p2x: f32, p2y: f32) -> f32 {
    let t = progress.clamp(0.0, 1.0);

    // Cubic Bezier formula: B(t) = (1-t)³P0 + 3(1-t)²tP1 + 3(1-t)t²P2 + t³P3
    // Where P0=(0,0) and P3=(1,1) for our normalized curve
    // Note: X coordinates are unused for time-based progress (t maps directly to time)
    let one_minus_t = 1.0 - t;
    let one_minus_t_squared = one_minus_t * one_minus_t;
    let one_minus_t_cubed = one_minus_t_squared * one_minus_t;
    let t_squared = t * t;
    let t_cubed = t_squared * t;

    // Calculate Y value using only the Y coordinates of control points
    let y = one_minus_t_cubed * 0.0
        + 3.0 * one_minus_t_squared * t * p1y
        + 3.0 * one_minus_t * t_squared * p2y
        + t_cubed * 1.0;

    y.clamp(0.0, 1.0)
}

/// Simple semantic version comparison for version strings.
///
/// Compares version strings in the format "vX.Y.Z" or "X.Y.Z" using
/// semantic versioning rules. Handles the optional 'v' prefix automatically.
///
/// # Arguments
/// * `version1` - First version string to compare
/// * `version2` - Second version string to compare
///
/// # Returns
/// - `Ordering::Less` if version1 < version2
/// - `Ordering::Equal` if version1 == version2  
/// - `Ordering::Greater` if version1 > version2
///
/// # Examples
/// ```
/// use std::cmp::Ordering;
/// use sunsetr::utils::compare_versions;
/// assert_eq!(compare_versions("v1.0.0", "v2.0.0"), Ordering::Less);
/// assert_eq!(compare_versions("2.1.0", "v2.0.0"), Ordering::Greater);
/// ```
pub fn compare_versions(version1: &str, version2: &str) -> std::cmp::Ordering {
    let parse_version = |v: &str| -> Vec<u32> {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect()
    };

    let v1 = parse_version(version1);
    let v2 = parse_version(version2);

    v1.cmp(&v2)
}

/// Extract semantic version string from hyprsunset command output.
///
/// Parses hyprsunset output to find version information in various formats.
/// Handles both "vX.Y.Z" and "X.Y.Z" patterns and normalizes to "vX.Y.Z" format.
///
/// # Arguments
/// * `output` - Raw output text from hyprsunset command
///
/// # Returns
/// - `Some(String)` containing normalized version (e.g., "v2.0.0")
/// - `None` if no valid semantic version found
///
/// # Examples
/// ```
/// use sunsetr::utils::extract_version_from_output;
/// assert_eq!(extract_version_from_output("hyprsunset v2.0.0"), Some("v2.0.0".to_string()));
/// assert_eq!(extract_version_from_output("version: 1.5.2"), Some("v1.5.2".to_string()));
/// ```
pub fn extract_version_from_output(output: &str) -> Option<String> {
    for line in output.lines() {
        let line = line.trim();
        // Look for version pattern: vX.Y.Z or X.Y.Z
        if let Some(version) = extract_semver_from_line(line) {
            return Some(version);
        }
    }
    None
}

/// Extract semantic version from a single line of text using regex.
///
/// Internal helper function that uses regex to find and normalize semantic versions.
///
/// # Arguments
/// * `line` - Single line of text to search
///
/// # Returns
/// - `Some(String)` with normalized version if found
/// - `None` if no semantic version pattern found
fn extract_semver_from_line(line: &str) -> Option<String> {
    use regex::Regex;
    let re = Regex::new(r"v?(\d+\.\d+\.\d+)").ok()?;
    if let Some(captures) = re.captures(line) {
        let full_match = captures.get(0)?.as_str();
        if full_match.starts_with('v') {
            Some(full_match.to_string())
        } else {
            Some(format!("v{}", captures.get(1)?.as_str()))
        }
    } else {
        None
    }
}

/// Manages terminal state to hide cursor and suppress all keyboard echoing.
///
/// This struct automatically restores the original terminal state when dropped,
/// ensuring clean cleanup even if the program exits unexpectedly.
pub struct TerminalGuard {
    original_termios: Termios,
}

impl TerminalGuard {
    /// Create a new terminal guard and modify terminal settings.
    ///
    /// Sets up the terminal to:
    /// - Hide the cursor for cleaner output
    /// - Suppress echoing of all keyboard input (including regular keys and control characters)
    ///
    /// # Returns
    /// - `Ok(Some(guard))` if terminal is available and settings were applied
    /// - `Ok(None)` if no terminal is available (e.g., running as a service)
    /// - `Err` only for unexpected errors
    pub fn new() -> io::Result<Option<Self>> {
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

        // Disable all keyboard echo (regular keys and control characters)
        term.c_lflag &= !(ECHO | ECHOCTL);
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

/// Get the PID of the currently running sunsetr instance
pub fn get_running_sunsetr_pid() -> Result<u32> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let lock_path = format!("{}/sunsetr.lock", runtime_dir);

    // Read the lock file content
    let lock_content = std::fs::read_to_string(&lock_path)
        .context("Failed to read lock file - no sunsetr instance running?")?;

    let lines: Vec<&str> = lock_content.trim().lines().collect();

    if lines.len() != 2 {
        anyhow::bail!("Lock file format invalid");
    }

    let pid = lines[0]
        .parse::<u32>()
        .context("Invalid PID format in lock file")?;

    // Verify the process is still running
    if is_process_running(pid) {
        Ok(pid)
    } else {
        anyhow::bail!(
            "Lock file exists but process {} is not running (stale lock file)",
            pid
        );
    }
}

/// Check if a process with the given PID is still running
pub fn is_process_running(pid: u32) -> bool {
    // On Unix, we can use kill -0 which doesn't send a signal but checks existence
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Kill the specified process
pub fn kill_process(pid: u32) -> bool {
    // Send SIGTERM to the process
    let result = std::process::Command::new("kill")
        .args([&pid.to_string()])
        .status();

    match result {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

/// Spawn a background sunsetr process using compositor-specific commands
pub fn spawn_background_process(debug_enabled: bool) -> Result<()> {
    use crate::backend::{Compositor, detect_compositor};

    #[cfg(debug_assertions)]
    eprintln!(
        "DEBUG: spawn_background_process() entry, PID: {}",
        std::process::id()
    );

    let compositor = detect_compositor();

    #[cfg(debug_assertions)]
    eprintln!("DEBUG: Detected compositor: {:?}", compositor);

    if debug_enabled {
        Log::log_pipe();
        Log::log_debug(&format!("Detected compositor: {:?}", compositor));
    }

    // Get the current executable path for the sunsetr command
    let current_exe = std::env::current_exe().context("Failed to get current executable path")?;
    let sunsetr_path = current_exe.to_string_lossy();

    #[cfg(debug_assertions)]
    eprintln!("DEBUG: sunsetr_path: {}", sunsetr_path);

    match compositor {
        Compositor::Niri => {
            #[cfg(debug_assertions)]
            eprintln!(
                "DEBUG: About to spawn via niri: niri msg action spawn -- {}",
                sunsetr_path
            );

            Log::log_pipe();
            Log::log_decorated("Starting sunsetr via niri compositor...");

            let output = std::process::Command::new("niri")
                .args(["msg", "action", "spawn", "--", &sunsetr_path])
                .output()
                .context("Failed to execute niri command")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("niri spawn command failed: {}", stderr);
            }

            Log::log_decorated("Background process started.");
        }
        Compositor::Hyprland => {
            #[cfg(debug_assertions)]
            eprintln!(
                "DEBUG: About to spawn via Hyprland: hyprctl dispatch exec {}",
                sunsetr_path
            );

            Log::log_pipe();
            Log::log_decorated("Starting sunsetr via Hyprland compositor...");

            let output = std::process::Command::new("hyprctl")
                .args(["dispatch", "exec", &sunsetr_path])
                .output()
                .context("Failed to execute hyprctl command")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("hyprctl dispatch exec command failed: {}", stderr);
            }

            Log::log_decorated("Background process started.");
        }
        Compositor::Sway => {
            #[cfg(debug_assertions)]
            eprintln!(
                "DEBUG: About to spawn via Sway: swaymsg exec {}",
                sunsetr_path
            );

            Log::log_pipe();
            Log::log_decorated("Starting sunsetr via Sway compositor...");

            let output = std::process::Command::new("swaymsg")
                .args(["exec", &sunsetr_path])
                .output()
                .context("Failed to execute swaymsg command")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("swaymsg exec command failed: {}", stderr);
            }

            Log::log_decorated("Background process started.");
        }
        Compositor::Other(name) => {
            Log::log_pipe();
            Log::log_warning(&format!("Unknown compositor '{}' detected", name));
            Log::log_indented(
                "Starting sunsetr directly (may not have proper parent relationship)",
            );

            // Fallback to direct spawn - not ideal but better than nothing
            let _child = std::process::Command::new(&*sunsetr_path)
                .spawn()
                .context("Failed to spawn sunsetr process directly")?;

            Log::log_decorated("Background process started (direct spawn).");
        }
    }

    #[cfg(debug_assertions)]
    eprintln!(
        "DEBUG: spawn_background_process() exit, PID: {}",
        std::process::id()
    );

    Ok(())
}

/// Perform comprehensive application cleanup before shutdown.
///
/// This function handles three critical cleanup operations:
/// - Backend-specific cleanup (stopping managed processes)
/// - Releasing the lock file handle
/// - Removing the lock file from disk
///
/// This function is designed to be called during normal shutdown or signal handling
/// to ensure resources are properly cleaned up and no stale lock files remain.
///
/// # Arguments
/// * `backend` - The backend instance to clean up (will call backend.cleanup())
/// * `lock_file` - File handle for the application lock (will be dropped to release)
/// * `lock_path` - Path to the lock file for removal from filesystem
/// * `debug_enabled` - Whether debug mode is enabled (affects logging separation)
///
/// # Examples
/// ```no_run
/// use sunsetr::utils::cleanup_application;
/// use sunsetr::backend::{create_backend, detect_backend};
/// use sunsetr::config::Config;
/// use std::fs::File;
///
/// // Example usage during application shutdown
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let config = Config::load()?;
/// let backend_type = detect_backend(&config)?;
/// let backend = create_backend(backend_type, &config, false)?;
/// let lock_file = File::create("/tmp/sunsetr.lock")?;
///
/// // During normal shutdown
/// cleanup_application(backend, lock_file, "/tmp/sunsetr.lock", false);
/// # Ok(())
/// # }
/// ```
pub fn cleanup_application(
    mut backend: Box<dyn crate::backend::ColorTemperatureBackend>,
    lock_file: File,
    lock_path: &str,
    debug_enabled: bool,
) {
    Log::log_decorated("Performing cleanup...");

    // Reset color temperature to neutral before cleanup
    if debug_enabled {
        Log::log_decorated("Resetting color temperature and gamma...");
        Log::log_indented("About to reset gamma via backend before stopping managed processes");
    }
    let running = Arc::new(AtomicBool::new(true));
    if let Err(e) = backend.apply_temperature_gamma(6500, 100.0, &running) {
        Log::log_pipe();
        Log::log_error(&format!("Failed to reset color temperature: {}", e));
        if debug_enabled {
            Log::log_indented("This failure occurred before backend cleanup was called");
        }
    } else if debug_enabled {
        Log::log_decorated("Gamma reset completed successfully");
    }

    // Handle backend-specific cleanup
    if debug_enabled {
        Log::log_decorated("Starting backend-specific cleanup...");
    }
    backend.cleanup(debug_enabled);

    // Drop the lock file handle to release the lock
    drop(lock_file);

    // Remove the lock file from disk
    if let Err(e) = std::fs::remove_file(lock_path) {
        Log::log_pipe();
        Log::log_decorated(&format!("Warning: Failed to remove lock file: {}", e));
    } else if debug_enabled {
        Log::log_pipe();
        Log::log_decorated("Lock file removed successfully");
    }

    Log::log_decorated("Cleanup complete");
}

/// Display an interactive dropdown menu and return the selected index.
///
/// This function shows a menu with arrow-key navigation, maintaining
/// the visual style of the logger output with pipe characters.
///
/// # Arguments
/// * `options` - Vector of tuples containing display string and associated value
/// * `prompt` - Optional prompt to display before the menu
/// * `cancel_message` - Optional custom message to display when user cancels
///
/// # Returns
/// * `Ok(usize)` - The index of the selected option
/// * `Err(_)` - If an error occurs or user cancels
#[allow(dead_code)]
pub fn show_dropdown_menu<T>(
    options: &[(String, T)],
    prompt: Option<&str>,
    cancel_message: Option<&str>,
) -> Result<usize> {
    Log::log_pipe();
    if let Some(p) = prompt {
        Log::log_block_start(p);
    }

    if options.is_empty() {
        Log::log_pipe();
        anyhow::bail!("No options provided to dropdown menu");
    }

    // Enable raw mode to capture key events
    terminal::enable_raw_mode().context("Failed to enable raw mode")?;

    let mut selected = 0;
    let mut stdout = io::stdout();

    // Ensure we clean up on any exit
    let cleanup = || {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), cursor::Show);
    };

    // Set up cleanup handler
    let result = loop {
        // Clear the current menu display
        execute!(
            stdout,
            cursor::Hide,
            terminal::Clear(ClearType::FromCursorDown)
        )?;

        // Display options
        for (i, (option, _)) in options.iter().enumerate() {
            if i == selected {
                execute!(stdout, Print("┃ ► "), Print(format!("{}\r\n", option)))?;
            } else {
                execute!(stdout, Print("┃   "), Print(format!("{}\r\n", option)))?;
            }
        }

        execute!(
            stdout,
            Print("┃\r\n"),
            Print("┃ Use ↑/↓ arrows or j/k keys to navigate, Enter to select, Ctrl+C to exit\r\n")
        )?;

        stdout.flush()?;

        // Move cursor back to start of menu for next update
        execute!(stdout, cursor::MoveUp((options.len() + 2) as u16))?;

        // Wait for key event
        match event::read() {
            Ok(Event::Key(KeyEvent {
                code, modifiers, ..
            })) => {
                match code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if selected > 0 {
                            selected -= 1;
                        } else {
                            selected = options.len() - 1; // Wrap to bottom
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if selected < options.len() - 1 {
                            selected += 1;
                        } else {
                            selected = 0; // Wrap to top
                        }
                    }
                    KeyCode::Enter => {
                        break Ok(selected);
                    }
                    KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                        cleanup();
                        // Move cursor past the menu before returning
                        execute!(
                            stdout,
                            cursor::MoveDown((options.len() + 2) as u16),
                            cursor::Show
                        )?;
                        stdout.flush()?;
                        Log::log_pipe();
                        if let Some(msg) = cancel_message {
                            Log::log_warning(msg);
                        }
                        anyhow::bail!("Operation cancelled by user");
                    }
                    KeyCode::Esc => {
                        cleanup();
                        // Move cursor past the menu before returning
                        execute!(
                            stdout,
                            cursor::MoveDown((options.len() + 2) as u16),
                            cursor::Show
                        )?;
                        stdout.flush()?;
                        Log::log_pipe();
                        if let Some(msg) = cancel_message {
                            Log::log_warning(msg);
                        }
                        anyhow::bail!("Operation cancelled by user");
                    }
                    _ => {
                        // Ignore other keys
                    }
                }
            }
            Ok(_) => {
                // Ignore other events (mouse, etc.)
            }
            Err(e) => {
                Log::log_pipe();
                break Err(anyhow::anyhow!("Error reading input: {}", e));
            }
        }
    };

    // Clean up terminal state
    cleanup();

    // Move cursor past the menu
    execute!(
        stdout,
        cursor::MoveDown((options.len() + 2) as u16),
        cursor::Show
    )?;
    stdout.flush()?;

    result
}

/// Convert a file path to a privacy-friendly format using tilde notation.
///
/// Replaces the user's home directory path with `~` to protect privacy
/// when sharing debug logs or error messages.
///
/// # Arguments
/// * `path` - The path to convert to privacy-friendly format
///
/// # Returns
/// String with home directory replaced by `~`, or original path if no replacement needed
///
/// # Examples
/// ```
/// use std::path::PathBuf;
/// use sunsetr::utils::path_for_display;
///
/// let path = PathBuf::from("/home/user/.config/sunsetr/sunsetr.toml");
/// let display_path = path_for_display(&path);
/// // Returns: "~/.config/sunsetr/sunsetr.toml"
/// ```
pub fn path_for_display(path: &std::path::Path) -> String {
    if let Some(home_dir) = dirs::home_dir() {
        if let Ok(relative_path) = path.strip_prefix(&home_dir) {
            return format!("~/{}", relative_path.display());
        }
    }
    // Fallback to original path if home directory detection fails
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn test_interpolate_u32_basic() {
        assert_eq!(interpolate_u32(1000, 2000, 0.0), 1000);
        assert_eq!(interpolate_u32(1000, 2000, 1.0), 2000);
        assert_eq!(interpolate_u32(1000, 2000, 0.5), 1500);
    }

    #[test]
    fn test_interpolate_u32_extreme_values() {
        // Test with extreme temperature values
        assert_eq!(interpolate_u32(1000, 20000, 0.0), 1000);
        assert_eq!(interpolate_u32(1000, 20000, 1.0), 20000);
        assert_eq!(interpolate_u32(1000, 20000, 0.5), 10500);

        // Test with same values
        assert_eq!(interpolate_u32(5000, 5000, 0.5), 5000);

        // Test with reversed order
        assert_eq!(interpolate_u32(6000, 3000, 0.0), 6000);
        assert_eq!(interpolate_u32(6000, 3000, 1.0), 3000);
        assert_eq!(interpolate_u32(6000, 3000, 0.5), 4500);
    }

    #[test]
    fn test_interpolate_u32_clamping() {
        // Progress values outside 0.0-1.0 should be clamped
        assert_eq!(interpolate_u32(1000, 2000, -0.5), 1000);
        assert_eq!(interpolate_u32(1000, 2000, 1.5), 2000);
        assert_eq!(interpolate_u32(1000, 2000, -100.0), 1000);
        assert_eq!(interpolate_u32(1000, 2000, 100.0), 2000);
    }

    #[test]
    fn test_interpolate_f32_basic() {
        assert_eq!(interpolate_f32(0.0, 100.0, 0.0), 0.0);
        assert_eq!(interpolate_f32(0.0, 100.0, 1.0), 100.0);
        assert_eq!(interpolate_f32(0.0, 100.0, 0.5), 50.0);
    }

    #[test]
    fn test_interpolate_f32_gamma_range() {
        // Test with typical gamma range
        assert_eq!(interpolate_f32(90.0, 100.0, 0.0), 90.0);
        assert_eq!(interpolate_f32(90.0, 100.0, 1.0), 100.0);
        assert_eq!(interpolate_f32(90.0, 100.0, 0.5), 95.0);

        // Test precision
        let result = interpolate_f32(90.0, 100.0, 0.3);
        assert!((result - 93.0).abs() < 0.001);
    }

    #[test]
    fn test_interpolate_f32_clamping() {
        assert_eq!(interpolate_f32(0.0, 100.0, -0.5), 0.0);
        assert_eq!(interpolate_f32(0.0, 100.0, 1.5), 100.0);
    }

    #[test]
    fn test_compare_versions_basic() {
        assert_eq!(compare_versions("v1.0.0", "v1.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("v1.0.0", "v2.0.0"), Ordering::Less);
        assert_eq!(compare_versions("v2.0.0", "v1.0.0"), Ordering::Greater);
    }

    #[test]
    fn test_compare_versions_without_v_prefix() {
        assert_eq!(compare_versions("1.0.0", "2.0.0"), Ordering::Less);
        assert_eq!(compare_versions("2.0.0", "1.0.0"), Ordering::Greater);
        assert_eq!(compare_versions("1.5.0", "1.5.0"), Ordering::Equal);
    }

    #[test]
    fn test_compare_versions_mixed_prefix() {
        assert_eq!(compare_versions("v1.0.0", "2.0.0"), Ordering::Less);
        assert_eq!(compare_versions("1.0.0", "v2.0.0"), Ordering::Less);
    }

    #[test]
    fn test_compare_versions_patch_levels() {
        assert_eq!(compare_versions("v1.0.0", "v1.0.1"), Ordering::Less);
        assert_eq!(compare_versions("v1.0.5", "v1.0.1"), Ordering::Greater);
        assert_eq!(compare_versions("v1.2.0", "v1.1.9"), Ordering::Greater);
    }

    #[test]
    fn test_extract_version_from_output_hyprsunset_format() {
        let output = "hyprsunset v2.0.0";
        assert_eq!(
            extract_version_from_output(output),
            Some("v2.0.0".to_string())
        );

        let output = "hyprsunset 2.0.0";
        assert_eq!(
            extract_version_from_output(output),
            Some("v2.0.0".to_string())
        );
    }

    #[test]
    fn test_extract_version_from_output_multiline() {
        let output = "hyprsunset - some description\nversion: v1.5.2\nother info";
        assert_eq!(
            extract_version_from_output(output),
            Some("v1.5.2".to_string())
        );
    }

    #[test]
    fn test_extract_version_from_output_no_version() {
        let output = "hyprsunset - no version info here";
        assert_eq!(extract_version_from_output(output), None);

        let output = "";
        assert_eq!(extract_version_from_output(output), None);
    }

    #[test]
    fn test_extract_version_from_output_malformed() {
        let output = "version 1.0"; // Missing patch version
        assert_eq!(extract_version_from_output(output), None);

        let output = "v1.0.0.0"; // Too many components
        assert_eq!(
            extract_version_from_output(output),
            Some("v1.0.0".to_string())
        );
    }

    // Property-based tests using proptest
    #[cfg(test)]
    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn interpolate_u32_bounds(start in 0u32..20000, end in 0u32..20000, progress in 0.0f32..1.0) {
                let result = interpolate_u32(start, end, progress);
                let min_val = start.min(end);
                let max_val = start.max(end);
                prop_assert!(result >= min_val && result <= max_val);
            }

            #[test]
            fn interpolate_f32_bounds(start in 0.0f32..100.0, end in 0.0f32..100.0, progress in 0.0f32..1.0) {
                let result = interpolate_f32(start, end, progress);
                let min_val = start.min(end);
                let max_val = start.max(end);
                prop_assert!(result >= min_val && result <= max_val);
            }

            #[test]
            fn interpolate_u32_endpoints(start in 0u32..20000, end in 0u32..20000) {
                prop_assert_eq!(interpolate_u32(start, end, 0.0), start);
                prop_assert_eq!(interpolate_u32(start, end, 1.0), end);
            }
        }
    }
}
