//! Structured logging with box-drawing visual formatting.
//!
//! Supports runtime enable/disable for quiet operation and optional file logging.

use std::io::Write;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};

// Use an AtomicBool instead of thread_local for thread safety
static LOGGING_ENABLED: AtomicBool = AtomicBool::new(true);

// Store geo mode coordinate timezone for simulation timestamps
static GEO_TIMEZONE: OnceLock<Option<chrono_tz::Tz>> = OnceLock::new();

// Channel for routing output to file when --log is active
static LOG_CHANNEL: OnceLock<Option<Sender<LogMessage>>> = OnceLock::new();

enum LogMessage {
    Formatted(String),
    Shutdown,
}

/// Main logging interface for structured output.
///
/// # Logging conventions
///
/// - `log_block_start!`: begin a new conceptual block, such as a major state
///   change, phase, or significant event. Follow it with `log_decorated!` or
///   `log_indented!` for related lines.
/// - `log_decorated!`: a line within an existing block, or a simple standalone
///   status line that fits the pipe structure.
/// - `log_indented!`: nested sub-items or details belonging to a parent line.
/// - `log_pipe!`: a single empty spacer line before `log_warning!`, `log_error!`,
///   `log_critical!`, `log_info!`, or `log_debug!`. Avoid it where it would
///   double up, as before `log_block_start!` (which already adds top spacing) or
///   `log_end!`.
/// - `log_version!`: the startup header, printed once at the beginning.
/// - `log_end!`: the termination marker, printed once at shutdown.
/// - `log_info!`, `log_warning!`, `log_error!`, `log_debug!`, `log_critical!`:
///   semantic level-prefixed lines outside the box-drawing flow.
pub struct Log;

impl Log {
    /// Enable or disable logging temporarily.
    ///
    /// This is useful for quiet operation during automated processes
    /// or testing where log output would interfere with results.
    pub fn set_enabled(enabled: bool) {
        LOGGING_ENABLED.store(enabled, Ordering::SeqCst);
    }

    /// Check if logging is currently enabled.
    pub fn is_enabled() -> bool {
        LOGGING_ENABLED.load(Ordering::SeqCst)
    }

    /// Set the geo mode timezone for simulation timestamps.
    /// Call this when entering geo mode with coordinates.
    pub fn set_geo_timezone(tz: Option<chrono_tz::Tz>) {
        let _ = GEO_TIMEZONE.set(tz);
    }

    /// Get the geo mode timezone if set.
    fn get_geo_timezone() -> Option<chrono_tz::Tz> {
        GEO_TIMEZONE.get().and_then(|tz| *tz)
    }

    /// Start file logging to the specified path.
    pub fn start_file_logging(file_path: String) -> anyhow::Result<LoggerGuard> {
        let (tx, rx) = channel();

        LOG_CHANNEL
            .set(Some(tx.clone()))
            .map_err(|_| anyhow::anyhow!("Logger channel already initialized"))?;

        let handle = std::thread::spawn(move || {
            let mut file = std::fs::File::create(&file_path)?;

            loop {
                match rx.recv() {
                    Ok(LogMessage::Formatted(text)) => {
                        file.write_all(text.as_bytes())?;
                    }
                    Ok(LogMessage::Shutdown) | Err(_) => {
                        file.flush()?;
                        break;
                    }
                }
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(LoggerGuard {
            tx,
            handle: Some(handle),
        })
    }

    /// Timestamp prefix for simulation mode, or an empty string outside it.
    ///
    /// Geo mode shows coordinate and local times as `[HH:MM:SSC] [HH:MM:SSL]`.
    /// Other modes show local time as `[HH:MM:SS]`.
    ///
    /// Public so the exported logging macros can call it.
    pub fn get_timestamp_prefix() -> String {
        // Probe state without initializing the time source.
        if crate::time::source::is_initialized() && crate::time::source::is_simulated() {
            let local_now = crate::time::source::now();

            if let Some(geo_tz) = Self::get_geo_timezone() {
                use chrono::TimeZone;

                let coord_time = geo_tz.from_utc_datetime(&local_now.naive_utc());
                let local_time = local_now;

                // If both zones render the same wall clock, show a single timestamp.
                let coord_str = coord_time.format("%H:%M:%S").to_string();
                let local_str = local_time.format("%H:%M:%S").to_string();

                if coord_str != local_str {
                    format!("[{coord_str}C] [{local_str}L] ")
                } else {
                    format!("[{local_str}] ")
                }
            } else {
                format!("[{}] ", local_now.format("%H:%M:%S"))
            }
        } else {
            String::new()
        }
    }
}

/// Guard for file logging that ensures clean shutdown.
pub struct LoggerGuard {
    tx: Sender<LogMessage>,
    handle: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
}

impl Drop for LoggerGuard {
    fn drop(&mut self) {
        let _ = self.tx.send(LogMessage::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        // Note: We don't clear LOG_CHANNEL since OnceLock can only be set once
        // This is fine since the process exits after simulation
    }
}

fn strip_ansi_codes(text: &str) -> String {
    // Strip ANSI CSI sequences of the form ESC [ ... m.
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for ch in chars.by_ref() {
                    if ch == 'm' {
                        break;
                    }
                }
            } else {
                result.push(ch);
            }
        } else {
            result.push(ch);
        }
    }

    result
}

// Public so the exported macros can call it.
pub fn write_output(text: &str) {
    if let Some(Some(tx)) = LOG_CHANNEL.get() {
        // Send to the file logger thread, stripping ANSI codes for clean file output
        let clean_text = strip_ansi_codes(text);
        let _ = tx.send(LogMessage::Formatted(clean_text));
    } else {
        // Normal output with colors
        print!("{text}");
        let _ = std::io::stdout().flush();
    }
}

// Logging Macros

#[macro_export]
macro_rules! log_decorated {
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┣ {message}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
    ($expr:expr) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┣ {expr}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
}

#[macro_export]
macro_rules! log_indented {
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┃   {message}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
    ($expr:expr) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┃   {expr}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
}

#[macro_export]
macro_rules! log_pipe {
    () => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let formatted = format!("{prefix}┃\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
}

#[macro_export]
macro_rules! log_block_start {
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┃\n{prefix}┣ {message}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
    ($expr:expr) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┃\n{prefix}┣ {expr}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
}

#[macro_export]
macro_rules! log_version {
    () => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let version = env!("SUNSETR_VERSION");
            let formatted = format!("{prefix}┏ sunsetr v{version} ━━╸\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
}

#[macro_export]
macro_rules! log_end {
    () => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let formatted = format!("{prefix}╹\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
}

#[macro_export]
macro_rules! log_warning {
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┣[\x1b[33mWARNING\x1b[0m] {message}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
    ($expr:expr) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┣[\x1b[33mWARNING\x1b[0m] {expr}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
}

/// Log a standalone warning, for use outside the box-drawing block flow.
#[macro_export]
macro_rules! log_warning_standalone {
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}[\x1b[33mWARNING\x1b[0m] {message}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
    ($expr:expr) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┃[\x1b[33mWARNING\x1b[0m] {expr}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
}

/// Log a standalone error, the error-level counterpart to `log_warning_standalone!`.
#[macro_export]
macro_rules! log_error_standalone {
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}[\x1b[31mERROR\x1b[0m] {message}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
    ($expr:expr) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┃[\x1b[31mERROR\x1b[0m] {expr}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
}

#[macro_export]
macro_rules! log_error {
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┣[\x1b[31mERROR\x1b[0m] {message}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
    ($expr:expr) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┣[\x1b[31mERROR\x1b[0m] {expr}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
}

/// Log an error as the closing line of a command's output.
///
/// Renders the closing-corner style but does not exit the process. Use
/// `log_error!` instead for an error the log continues past.
#[macro_export]
macro_rules! log_error_end {
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┃\n{prefix}┗[\x1b[31mERROR\x1b[0m] {message}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
    ($expr:expr) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┃\n{prefix}┗[\x1b[31mERROR\x1b[0m] {expr}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
}

#[macro_export]
macro_rules! log_info {
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┣[\x1b[32mINFO\x1b[0m] {message}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
    ($expr:expr) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┣[\x1b[32mINFO\x1b[0m] {expr}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
}

#[macro_export]
macro_rules! log_debug {
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┣[\x1b[32mDEBUG\x1b[0m] {message}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
    ($expr:expr) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┣[\x1b[32mDEBUG\x1b[0m] {expr}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
}

#[macro_export]
macro_rules! log_critical {
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┣[\x1b[31mCRITICAL\x1b[0m] {message}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
    ($expr:expr) => {{
        use $crate::common::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┣[\x1b[31mCRITICAL\x1b[0m] {expr}\n");
            $crate::common::logger::write_output(&formatted);
        }
    }};
}
