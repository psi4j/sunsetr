//! Shared helpers for interpolation, version handling, terminal state, and progress display.

use anyhow::{Context, Result};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::Print,
    terminal::{self, ClearType},
};
use std::path::PathBuf;
use std::{
    fs::File,
    io::{self, Write},
    os::unix::io::AsRawFd,
    time::Duration,
};
use termios::{ECHO, TCSANOW, Termios, os::linux::ECHOCTL, tcsetattr};

/// Interpolate between two u32 values using a weighted harmonic mean.
///
/// Progress is clamped to [0.0, 1.0]. Used for color temperature transitions.
pub fn interpolate_inverse_u32(start: u32, end: u32, progress: f32) -> u32 {
    let start_f = start as f32;
    let end_f = end as f32;
    let denominator = end_f + (start_f - end_f) * progress.clamp(0.0, 1.0);
    let result = start_f * end_f / denominator;
    result.round() as u32
}

/// Linearly interpolate between two f32 values, clamping progress to [0.0, 1.0].
pub fn interpolate_f32(start: f32, end: f32, progress: f32) -> f32 {
    start + (end - start) * progress.clamp(0.0, 1.0)
}

/// Linearly interpolate between two f64 values, clamping progress to [0.0, 1.0].
pub fn interpolate_f64(start: f64, end: f64, progress: f32) -> f64 {
    start + (end - start) * progress.clamp(0.0, 1.0) as f64
}

/// Round a duration up to whole seconds (4.7s becomes 5) so countdown displays
/// match when updates actually happen.
pub fn format_duration_seconds_ceil(duration: std::time::Duration) -> u64 {
    if duration.subsec_millis() > 0 {
        duration.as_secs() + 1
    } else {
        duration.as_secs()
    }
}

/// Round a chrono duration up to whole seconds, returning 0 for negative durations.
pub fn format_chrono_duration_seconds_ceil(duration: chrono::Duration) -> u64 {
    if duration.num_seconds() <= 0 {
        return 0;
    }

    // Work from total milliseconds to avoid chrono's internal representation quirks
    let total_millis = duration.num_milliseconds();
    let seconds = (total_millis / 1000) as u64;
    let fractional_millis = total_millis % 1000;

    if fractional_millis > 0 {
        seconds + 1
    } else {
        seconds
    }
}

/// Format progress as a percentage, choosing decimal precision from the rate of change.
///
/// Slower change gets more decimals (2 below 0.1%, 1 below 1%, integer otherwise) so the
/// displayed value still moves between updates. Without a previous value the precision is
/// chosen from the current value instead. Logs an error if progress moves backwards.
pub fn format_progress_percentage(progress: f32, previous_progress: Option<f32>) -> String {
    let current_percentage = progress * 100.0;

    let (precision, min_value, max_value) = if let Some(prev) = previous_progress {
        // Monotonicity check
        const EPSILON: f32 = 0.0001;
        if progress < prev - EPSILON {
            log_error!(
                "Progress decreased during transition: {:.4} -> {:.4} (delta = {:.4})",
                prev,
                progress,
                progress - prev
            );
            log_indented!("This indicates a bug in state management or timing logic");
            log_indented!(
                "Previous: {:.2}%, Current: {:.2}%",
                prev * 100.0,
                current_percentage
            );
        }

        let percentage_change = (current_percentage - prev * 100.0).abs();

        if percentage_change < 0.1 {
            (2, 0.01, 99.99)
        } else if percentage_change < 1.0 {
            (1, 0.1, 99.9)
        } else {
            (0, 1.0, 99.0)
        }
    } else {
        let has_significant_decimals = (current_percentage * 10.0).fract() > 0.01;

        if !(0.1..=99.9).contains(&current_percentage) {
            (2, 0.01, 99.99)
        } else if !(1.0..=99.0).contains(&current_percentage) {
            (1, 0.1, 99.9)
        } else if has_significant_decimals {
            (2, 0.01, 99.99)
        } else {
            (1, 0.1, 99.9)
        }
    };

    let clamped = current_percentage.clamp(min_value, max_value);
    match precision {
        0 => format!("{}%", clamped.round() as u8),
        1 => format!("{clamped:.1}%"),
        2 => format!("{clamped:.2}%"),
        _ => unreachable!(),
    }
}

/// Apply the smoothstep S-curve `3t^2 - 2t^3` to progress, clamping to [0.0, 1.0].
///
/// The zero first derivative at both endpoints gives an ease-in-out with no jump at
/// transition boundaries.
pub fn smoothstep(progress: f32) -> f32 {
    let t = progress.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Compare two semantic version strings, ignoring an optional leading `v`.
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

/// Find the first semantic version in command output, normalized to `vX.Y.Z`.
pub fn extract_version_from_output(output: &str) -> Option<String> {
    for line in output.lines() {
        let line = line.trim();
        if let Some(version) = extract_semver_from_line(line) {
            return Some(version);
        }
    }
    None
}

/// Find and normalize a semantic version in a single line to `vX.Y.Z`.
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
    /// Hide the cursor and suppress keyboard echo, returning `Ok(None)` when no tty
    /// is available (e.g. running as a service).
    pub fn new() -> io::Result<Option<Self>> {
        let tty = match File::open("/dev/tty") {
            Ok(tty) => tty,
            Err(e) if e.kind() == io::ErrorKind::NotFound || e.raw_os_error() == Some(6) => {
                return Ok(None);
            }
            Err(e) => return Err(e),
        };

        let fd = tty.as_raw_fd();
        let mut term = Termios::from_fd(fd)?;
        let original = term;
        term.c_lflag &= !(ECHO | ECHOCTL);
        tcsetattr(fd, TCSANOW, &term)?;
        print!("\x1b[?25l");
        io::stdout().flush()?;

        Ok(Some(Self {
            original_termios: original,
        }))
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if let Ok(tty) = File::open("/dev/tty") {
            let _ = tcsetattr(tty.as_raw_fd(), TCSANOW, &self.original_termios);
        }
        let _ = write!(io::stdout(), "\x1b[?25h");
        let _ = io::stdout().flush();
    }
}

/// Clean up backend, lock file handle, and lock file on disk.
///
/// Resource cleanup only. The caller resets gamma if needed based on whether a
/// smooth shutdown transition was performed.
pub(crate) fn cleanup_application(
    backend: Box<dyn crate::backend::ColorTemperatureBackend>,
    lock_file: crate::io::lock::LockFile,
    lock_path: &PathBuf,
    debug_enabled: bool,
) {
    log_decorated!("Performing cleanup...");

    if debug_enabled {
        log_decorated!("Starting backend-specific cleanup...");
    }
    backend.cleanup(debug_enabled);

    drop(lock_file);

    if let Err(e) = std::fs::remove_file(lock_path) {
        log_pipe!();
        log_error!("Failed to remove lock file: {e}");
    } else if debug_enabled {
        log_block_start!("Lock file removed successfully");
    }

    log_decorated!("Cleanup complete");
}

/// Result type for dropdown menu operations.
///
/// This distinguishes between successful selection, user cancellation,
/// and actual errors (I/O failures, etc.).
pub enum DropdownResult {
    Selected(usize),
    Cancelled,
}

/// Display an interactive dropdown menu and return the selected index.
///
/// Navigable with arrow keys or j/k. ESC or Ctrl+C yields `Cancelled`.
pub fn show_dropdown_menu<T>(
    options: &[(String, T)],
    prompt: Option<&str>,
) -> Result<DropdownResult> {
    if let Some(p) = prompt {
        log_block_start!(p);
    }

    if options.is_empty() {
        log_pipe!();
        anyhow::bail!("No options provided to dropdown menu");
    }

    terminal::enable_raw_mode().context("Failed to enable raw mode")?;
    let mut selected = 0;
    let mut stdout = io::stdout();

    let cleanup = || {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), cursor::Show);
    };

    let result = loop {
        execute!(
            stdout,
            cursor::Hide,
            terminal::Clear(ClearType::FromCursorDown)
        )?;

        for (i, (option, _)) in options.iter().enumerate() {
            if i == selected {
                execute!(stdout, Print("┃ ► "), Print(format!("{option}\r\n")))?;
            } else {
                execute!(stdout, Print("┃   "), Print(format!("{option}\r\n")))?;
            }
        }

        execute!(
            stdout,
            Print("┃\r\n"),
            Print("┃ Use ↑/↓ arrows or j/k keys to navigate, Enter to select, Ctrl+C to exit\r\n")
        )?;

        stdout.flush()?;
        execute!(stdout, cursor::MoveUp((options.len() + 2) as u16))?;

        match event::read() {
            Ok(Event::Key(KeyEvent {
                code, modifiers, ..
            })) => match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if selected > 0 {
                        selected -= 1;
                    } else {
                        selected = options.len() - 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if selected < options.len() - 1 {
                        selected += 1;
                    } else {
                        selected = 0;
                    }
                }
                KeyCode::Enter => {
                    break Ok(DropdownResult::Selected(selected));
                }
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    break Ok(DropdownResult::Cancelled);
                }
                KeyCode::Esc => {
                    break Ok(DropdownResult::Cancelled);
                }
                _ => {}
            },
            Ok(_) => {}
            Err(e) => {
                log_pipe!();
                break Err(anyhow::anyhow!("Error reading input: {}", e));
            }
        }
    };

    cleanup();

    execute!(
        stdout,
        cursor::MoveDown((options.len() + 2) as u16),
        cursor::Show
    )?;
    stdout.flush()?;

    result
}

/// Replace the home directory prefix with `~` for privacy in logs and error messages.
pub fn private_path(path: &std::path::Path) -> String {
    if let Some(home_dir) = dirs::home_dir()
        && let Ok(relative_path) = path.strip_prefix(&home_dir)
    {
        return format!("~/{}", relative_path.display());
    }
    path.display().to_string()
}

/// Animated progress bar written directly to stdout, bypassing logger routing.
///
/// Redraws only when the percentage changes to reduce flicker.
pub struct ProgressBar {
    width: usize,
    last_percentage: Option<usize>,
    throttle: AdaptiveThrottle,
    last_update: Option<std::time::Instant>,
}

impl ProgressBar {
    /// Create a progress bar of the given character width.
    pub fn new(width: usize) -> Self {
        Self {
            width,
            last_percentage: None,
            throttle: AdaptiveThrottle::new_for_progress_bar(),
            last_update: None,
        }
    }

    /// Redraw the bar for the current progress, writing straight to stdout so it
    /// shows even when file logging is active. Skips the redraw when the percentage
    /// is unchanged.
    pub fn update(&mut self, progress: f32, suffix: Option<&str>) {
        let update_start = std::time::Instant::now();
        let percentage = (progress * 100.0) as usize;

        if self.last_percentage == Some(percentage) && percentage < 100 {
            if let Some(last) = self.last_update {
                let latency = last.elapsed();
                self.throttle.update(latency);
            }
            self.last_update = Some(update_start);
            return;
        }

        let filled = (self.width as f32 * progress) as usize;
        let empty = self.width - filled;

        let bar = if filled > 0 {
            format!(
                "{}>{}",
                "=".repeat(filled.saturating_sub(1)),
                " ".repeat(empty)
            )
        } else {
            " ".repeat(self.width)
        };

        print!("\r\x1B[K┃[{bar}] {percentage}%");
        if let Some(s) = suffix {
            print!(" {s}");
        }
        io::stdout().flush().ok();

        self.last_percentage = Some(percentage);

        if let Some(last) = self.last_update {
            let latency = last.elapsed();
            self.throttle.update(latency);
        }
        self.last_update = Some(update_start);
    }

    /// Adaptive interval to sleep before the next update.
    pub fn recommended_sleep(&self) -> std::time::Duration {
        self.throttle.current_interval()
    }

    /// Finish the bar and move to the next line.
    pub fn finish(&mut self) {
        println!();
        io::stdout().flush().ok();
    }
}

/// Update-interval throttle that adapts to measured system latency.
///
/// Tracks latency with an exponential moving average and, unlike a fixed rate limiter,
/// can speed up on fast hardware or slow down on slow systems to keep animations smooth
/// while limiting CPU usage.
pub struct AdaptiveThrottle {
    /// Exponential moving average of measured latencies in milliseconds
    ema_latency: f64,
    /// Base interval (target/ideal interval)
    base_interval: Duration,
    /// Current adaptive interval
    interval: Duration,
    /// Count of consecutive fast measurements (for confidence)
    consecutive_fast: u32,
    /// Count of consecutive slow measurements (for confidence)
    consecutive_slow: u32,
}

impl AdaptiveThrottle {
    /// Create a throttle with the given target interval.
    pub fn new(base_interval: Duration) -> Self {
        Self {
            ema_latency: 1.0,
            base_interval,
            interval: base_interval,
            consecutive_fast: 0,
            consecutive_slow: 0,
        }
    }

    /// Throttle tuned for progress bars: a 10ms base for smooth 100 FPS on capable hardware.
    pub fn new_for_progress_bar() -> Self {
        Self::new(Duration::from_millis(10))
    }

    /// Update the moving average from the measured latency and return the next sleep interval.
    pub fn update(&mut self, measured_latency: Duration) -> Duration {
        let latency_ms = measured_latency.as_secs_f64() * 1000.0;

        let alpha = if self.consecutive_fast > 3 || self.consecutive_slow > 3 {
            0.5
        } else {
            0.2
        };

        self.ema_latency = alpha * latency_ms + (1.0 - alpha) * self.ema_latency;

        let base_ms = self.base_interval.as_millis() as f64;
        let current_ms = self.interval.as_millis() as f64;

        let new_interval_ms = if self.ema_latency < base_ms * 0.1 {
            self.consecutive_fast = self.consecutive_fast.saturating_add(1);
            self.consecutive_slow = 0;

            (current_ms * 0.8).max(1.0)
        } else if self.ema_latency < base_ms * 0.5 {
            self.consecutive_fast = 0;
            self.consecutive_slow = 0;

            if current_ms > base_ms {
                (current_ms * 0.9).max(base_ms)
            } else {
                (current_ms * 0.95).max(base_ms * 0.5)
            }
        } else if self.ema_latency > base_ms * 2.0 {
            self.consecutive_slow = self.consecutive_slow.saturating_add(1);
            self.consecutive_fast = 0;

            (current_ms * 1.3).min(base_ms * 10.0).min(100.0)
        } else {
            self.consecutive_slow = 0;
            self.consecutive_fast = 0;

            if current_ms > base_ms {
                (current_ms * 0.95).max(base_ms)
            } else {
                (current_ms * 1.05).min(base_ms)
            }
        };

        self.interval = Duration::from_millis(new_interval_ms as u64);
        self.interval
    }

    /// Get the current interval without updating.
    pub fn current_interval(&self) -> Duration {
        self.interval
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn test_interpolate_inverse_u32_basic() {
        assert_eq!(interpolate_inverse_u32(1000, 2000, 0.0), 1000);
        assert_eq!(interpolate_inverse_u32(1000, 2000, 1.0), 2000);
        assert_eq!(interpolate_inverse_u32(1000, 2000, 0.5), 1333);
    }

    #[test]
    fn test_interpolate_inverse_u32_extreme_values() {
        assert_eq!(interpolate_inverse_u32(1000, 20000, 0.0), 1000);
        assert_eq!(interpolate_inverse_u32(1000, 20000, 1.0), 20000);
        assert_eq!(interpolate_inverse_u32(1000, 20000, 0.5), 1905);

        assert_eq!(interpolate_inverse_u32(5000, 5000, 0.5), 5000);

        assert_eq!(interpolate_inverse_u32(6000, 3000, 0.0), 6000);
        assert_eq!(interpolate_inverse_u32(6000, 3000, 1.0), 3000);
        assert_eq!(interpolate_inverse_u32(6000, 3000, 0.5), 4000);
    }

    #[test]
    fn test_interpolate_inverse_u32_clamping() {
        assert_eq!(interpolate_inverse_u32(1000, 2000, -0.5), 1000);
        assert_eq!(interpolate_inverse_u32(1000, 2000, 1.5), 2000);
        assert_eq!(interpolate_inverse_u32(1000, 2000, -100.0), 1000);
        assert_eq!(interpolate_inverse_u32(1000, 2000, 100.0), 2000);
    }

    #[test]
    fn test_interpolate_f32_basic() {
        assert_eq!(interpolate_f32(0.0, 100.0, 0.0), 0.0);
        assert_eq!(interpolate_f32(0.0, 100.0, 1.0), 100.0);
        assert_eq!(interpolate_f32(0.0, 100.0, 0.5), 50.0);
    }

    #[test]
    fn test_interpolate_f32_gamma_range() {
        assert_eq!(interpolate_f32(90.0, 100.0, 0.0), 90.0);
        assert_eq!(interpolate_f32(90.0, 100.0, 1.0), 100.0);
        assert_eq!(interpolate_f32(90.0, 100.0, 0.5), 95.0);

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
    fn test_extract_version_from_output_malformed() {
        let output = "version 1.0";
        assert_eq!(extract_version_from_output(output), None);

        let output = "";
        assert_eq!(extract_version_from_output(output), None);

        let output = "v1.0.0.0";
        assert_eq!(
            extract_version_from_output(output),
            Some("v1.0.0".to_string())
        );
    }

    #[cfg(test)]
    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn interpolate_inverse_u32_bounds(start in 1000u32..20000, end in 1000u32..20000, progress in 0.0f32..1.0) {
                let result = interpolate_inverse_u32(start, end, progress);
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
            fn interpolate_inverse_u32_endpoints(start in 1000u32..20000, end in 1000u32..20000) {
                prop_assert_eq!(interpolate_inverse_u32(start, end, 0.0), start);
                prop_assert_eq!(interpolate_inverse_u32(start, end, 1.0), end);
            }
        }
    }
}
