//! Structured logging system with visual formatting.
//!
//! This module provides a logging system designed for sunsetr's visual output style.
//! It includes different log levels and special formatting functions for creating
//! visually appealing, structured output with Unicode box drawing characters.
//!
//! The logger supports runtime enable/disable functionality for quiet operation
//! during automated processes or testing.

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

/// Main logging interface providing structured output formatting.
///
/// ## Logging Conventions
///
/// To maintain a consistent and readable log output, adhere to the following conventions
/// when using the visual formatting macros:
///
/// - **`log_block_start!`**:
///   - **Purpose**: Always use this to initiate a new, distinct conceptual block of log information,
///     especially for major state changes, phase indications, or significant events (e.g., "Commencing sunrise",
///     "Loading configuration", "Backend detected").
///   - **Output**: Prepends an empty pipe `┃` for spacing from any previous log, then prints `┣ message`.
///   - **Usage**: Subsequent related messages within this conceptual block should typically use
///     `log_decorated!` or `log_indented!`.
///
/// - **`log_decorated!`**:
///   - **Purpose**: For logging messages that are part of an existing block started by `log_block_start!`,
///     or for simple, single-line status messages that don't warrant a full block but still fit the pipe structure.
///   - **Output**: Prints `┣ message`.
///   - **Context**: If this message is a continuation of a `log_block_start!`, it will appear visually connected.
///
/// - **`log_indented!`**:
///   - **Purpose**: For nested data or detailed sub-items that belong to a parent message
///     (often logged with `log_block_start!` or `log_decorated!`). Useful for listing configuration items,
///     multi-part details, etc.
///   - **Output**: Prints `┃   message` (pipe, three spaces, then message).
///
/// - **`log_pipe!`**:
///   - **Purpose**: Used explicitly to insert a single, empty, prefixed line (`┃`) for vertical spacing.
///   - **Usage**: Its primary use-case is to create visual separation to initiate a block *before* using
///     `log_warning!`, `log_error!`, `log_critical!`, `log_info!`, `log_debug!`, or logging
///     an `anyhow` error message.
///     Avoid using it if it might lead to double pipes or unnecessary empty lines before a `log_block_start!`
///     (which already provides top spacing) or `log_end!`. *Not for use at the end of a block.
///
/// - **`log_version!`**:
///   - **Purpose**: Prints the application startup header. Typically called once at the beginning.
///   - **Output**: `┏ sunsetr vX.Y.Z ━━╸`.
///
/// - **`log_end!`**:
///   - **Purpose**: Prints the final log termination marker. Called once at shutdown.
///   - **Output**: `╹`.
///
/// - **`log_info!`, `log_warning!`, `log_error!`, `log_debug!`, `log_critical!`**:
///   - **Purpose**: These are standard semantic logging macros. They use a `[LEVEL]` prefix
///     (e.g., `[INFO]`, `[WARNING]`, `[ERROR]`) and do not use the box-drawing characters.
///   - **Usage**: Use them for their semantic meaning when a message doesn't fit the structured
///     box-drawing style or when a specific log level prefix is more appropriate.
///     If they begin a new conceptual block of information that is *not* part of the primary
///     box-drawing flow, they ought to begin with a `log_pipe!`.
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

        // Install the channel
        LOG_CHANNEL
            .set(Some(tx.clone()))
            .map_err(|_| anyhow::anyhow!("Logger channel already initialized"))?;

        // Spawn logger thread
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

    // # Helper Functions

    /// Get timestamp prefix for simulation mode.
    /// In geo mode, shows [HH:MM:SSC] [HH:MM:SSL] for coordinate and local times.
    /// In other modes, shows [HH:MM:SS] for local time only.
    /// Returns empty string if not in simulation mode.
    /// Now public for macro access.
    pub fn get_timestamp_prefix() -> String {
        // Only add timestamps if we're actually in simulation mode
        // Check this without initializing the time source
        if crate::time_source::is_initialized() && crate::time_source::is_simulated() {
            let local_now = crate::time_source::now();

            // Check if we have a geo timezone to show coordinate time
            if let Some(geo_tz) = Self::get_geo_timezone() {
                use chrono::TimeZone;

                // Convert local time to coordinate timezone
                let coord_time = geo_tz.from_utc_datetime(&local_now.naive_utc());
                let local_time = local_now;

                // Show both times if they differ (comparing the actual times)
                // If the times are different when formatted, they're in different zones
                let coord_str = coord_time.format("%H:%M:%S").to_string();
                let local_str = local_time.format("%H:%M:%S").to_string();

                if coord_str != local_str {
                    // Different times - show both with C and L suffixes
                    format!("[{coord_str}C] [{local_str}L] ")
                } else {
                    // Same time, just show one without suffix
                    format!("[{local_str}] ")
                }
            } else {
                // No geo timezone set, just show local time
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

// Helper function to strip ANSI color codes from text
fn strip_ansi_codes(text: &str) -> String {
    // Regex pattern for ANSI escape sequences
    // Matches: ESC [ ... m where ... is any sequence of digits and semicolons
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Check if this is the start of an ANSI sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Skip until we find 'm'
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

// Public function that routes output (needed by macros)
pub fn write_output(text: &str) {
    if let Some(Some(tx)) = LOG_CHANNEL.get() {
        // Send to file logger thread - strip ANSI codes for clean file output
        let clean_text = strip_ansi_codes(text);
        let _ = tx.send(LogMessage::Formatted(clean_text));
    } else {
        // Normal output with colors
        print!("{text}");
        let _ = std::io::stdout().flush();
    }
}

// # Logging Macros

/// Log a decorated message, typically as part of an existing block or for standalone emphasis.
#[macro_export]
macro_rules! log_decorated {
    // Format string literal (with or without args) - always pass through format!
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┣ {message}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
    // Non-literal expression - convert to string
    ($expr:expr) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┣ {expr}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
}

/// Log an indented message for sub-items or details within a block.
#[macro_export]
macro_rules! log_indented {
    // Format string literal (with or without args) - always pass through format!
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┃   {message}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
    // Non-literal expression - convert to string
    ($expr:expr) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┃   {expr}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
}

/// Log a visual pipe separator for vertical spacing.
#[macro_export]
macro_rules! log_pipe {
    () => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let formatted = format!("{prefix}┃\n");
            $crate::logger::write_output(&formatted);
        }
    }};
}

/// Log a block start message, initiating a new conceptual block of information.
#[macro_export]
macro_rules! log_block_start {
    // Format string literal (with or without args) - always pass through format!
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┃\n{prefix}┣ {message}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
    // Non-literal expression - convert to string
    ($expr:expr) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┃\n{prefix}┣ {expr}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
}

/// Log the application version header.
#[macro_export]
macro_rules! log_version {
    () => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let version = env!("CARGO_PKG_VERSION");
            let formatted = format!("{prefix}┏ sunsetr v{version} ━━╸\n");
            $crate::logger::write_output(&formatted);
        }
    }};
}

/// Log the final termination marker.
#[macro_export]
macro_rules! log_end {
    () => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let formatted = format!("{prefix}╹\n");
            $crate::logger::write_output(&formatted);
        }
    }};
}

/// Log a warning message with pipe prefix and yellow-colored text.
#[macro_export]
macro_rules! log_warning {
    // Format string literal (with or without args) - always pass through format!
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┣[\x1b[33mWARNING\x1b[0m] {message}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
    // Non-literal expression - convert to string
    ($expr:expr) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┣[\x1b[33mWARNING\x1b[0m] {expr}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
}

/// Log a warning message with a pipe prefix and terminal corner (standalone).
/// This adds a pipe before the warning, similar to log_block_start!, for visual consistency.
#[macro_export]
macro_rules! log_warning_standalone {
    // Format string literal (with or without args) - always pass through format!
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}[\x1b[33mWARNING\x1b[0m] {message}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
    // Non-literal expression - convert to string
    ($expr:expr) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┃[\x1b[33mWARNING\x1b[0m] {expr}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
}

/// Log an error message without the pipe prefix (standalone).
/// This formats like log_warning_standalone! but uses ERROR in red.
#[macro_export]
macro_rules! log_error_standalone {
    // Format string literal (with or without args) - always pass through format!
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}[\x1b[31mERROR\x1b[0m] {message}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
    // Non-literal expression - convert to string
    ($expr:expr) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┃[\x1b[31mERROR\x1b[0m] {expr}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
}

/// Log an error message with pipe prefix and red-colored text.
#[macro_export]
macro_rules! log_error {
    // Format string literal (with or without args) - always pass through format!
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┣[\x1b[31mERROR\x1b[0m] {message}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
    // Non-literal expression - convert to string
    ($expr:expr) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┣[\x1b[31mERROR\x1b[0m] {expr}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
}

/// Log an error message with a pipe prefix and terminal corner (standalone).
/// This adds a pipe before the error, similar to log_block_start!, to indicate flow termination.
#[macro_export]
macro_rules! log_error_exit {
    // Format string literal (with or without args) - always pass through format!
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┃\n{prefix}┗[\x1b[31mERROR\x1b[0m] {message}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
    // Non-literal expression - convert to string
    ($expr:expr) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┃\n{prefix}┗[\x1b[31mERROR\x1b[0m] {expr}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
}

/// Log an informational message with pipe prefix and green-colored text.
#[macro_export]
macro_rules! log_info {
    // Format string literal (with or without args) - always pass through format!
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┣[\x1b[32mINFO\x1b[0m] {message}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
    // Non-literal expression - convert to string
    ($expr:expr) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┣[\x1b[32mINFO\x1b[0m] {expr}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
}

/// Log a debug/operational message with pipe prefix and green-colored text.
#[macro_export]
macro_rules! log_debug {
    // Format string literal (with or without args) - always pass through format!
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┣[\x1b[32mDEBUG\x1b[0m] {message}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
    // Non-literal expression - convert to string
    ($expr:expr) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┣[\x1b[32mDEBUG\x1b[0m] {expr}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
}

/// Log a critical message with pipe prefix and red-colored text.
#[macro_export]
macro_rules! log_critical {
    // Format string literal (with or without args) - always pass through format!
    ($fmt:literal $($arg:tt)*) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let message = format!($fmt $($arg)*);
            let formatted = format!("{prefix}┣[\x1b[31mCRITICAL\x1b[0m] {message}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
    // Non-literal expression - convert to string
    ($expr:expr) => {{
        use $crate::logger::Log;
        if Log::is_enabled() {
            let prefix = Log::get_timestamp_prefix();
            let expr = $expr;
            let formatted = format!("{prefix}┣[\x1b[31mCRITICAL\x1b[0m] {expr}\n");
            $crate::logger::write_output(&formatted);
        }
    }};
}
