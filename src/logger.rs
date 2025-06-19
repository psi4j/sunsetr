//! Structured logging system with visual formatting.
//!
//! This module provides a logging system designed for sunsetr's visual output style.
//! It includes different log levels and special formatting functions for creating
//! visually appealing, structured output with Unicode box drawing characters.
//!
//! The logger supports runtime enable/disable functionality for quiet operation
//! during automated processes or testing.

use std::sync::atomic::{AtomicBool, Ordering};

// Use an AtomicBool instead of thread_local for thread safety
static LOGGING_ENABLED: AtomicBool = AtomicBool::new(true);

/// Log level enumeration for categorizing message importance.
#[derive(Debug)]
pub enum LogLevel {
    Log,  // Normal operational logs
    Warn, // Warning messages (non-fatal issues)
    Err,  // Error messages (recoverable failures)
    Crit, // Critical errors (may require user intervention)
    Info, // Informational messages (status updates)
}

/// Main logging interface providing structured output formatting.
///
/// ## Logging Conventions
///
/// To maintain a consistent and readable log output, adhere to the following conventions
/// when using the visual formatting functions:
///
/// - **`log_block_start(message: &str)`**:
///   - **Purpose**: Always use this to initiate a new, distinct conceptual block of log information,
///     especially for major state changes, phase indications, or significant events (e.g., "Commencing sunrise",
///     "Loading configuration", "Backend detected").
///   - **Output**: Prepends an empty pipe `┃` for spacing from any previous log, then prints `┣ message`.
///   - **Usage**: Subsequent related messages within this conceptual block should typically use
///     `log_decorated()` or `log_indented()`.
///
/// - **`log_decorated(message: &str)`**:
///   - **Purpose**: For logging messages that are part of an existing block started by `log_block_start()`,
///     or for simple, single-line status messages that don't warrant a full block but still fit the pipe structure.
///   - **Output**: Prints `┣ message`.
///   - **Context**: If this message is a continuation of a `log_block_start`, it will appear visually connected.
///
/// - **`log_indented(message: &str)`**:
///   - **Purpose**: For nested data or detailed sub-items that belong to a parent message
///     (often logged with `log_block_start()` or `log_decorated()`). Useful for listing configuration items,
///     multi-part details, etc.
///   - **Output**: Prints `┃   message` (pipe, three spaces, then message).
///
/// - **`log_pipe()`**:
///   - **Purpose**: Used explicitly to insert a single, empty, prefixed line (`┃`) for vertical spacing.
///   - **Usage**: Its primary use-case is to create visual separation to initiate a block *before* using
///     `log_warning()`, `log_error()`, `log_critical()`, `log_info()`, `log_debug()`, or logging
///     an `anyhow` error message.
///     Avoid using it if it might lead to double pipes or unnecessary empty lines before a `log_block_start()`
///     (which already provides top spacing) or `log_end()`. *Not for use at the end of a block.
///
/// - **`log_version()`**:
///   - **Purpose**: Prints the application startup header. Typically called once at the beginning.
///   - **Output**: `┏ sunsetr vX.Y.Z ━━╸` followed by `┃`.
///
/// - **`log_end()`**:
///   - **Purpose**: Prints the final log termination marker. Called once at shutdown.
///   - **Output**: `╹`.
///
/// - **`log_info()`, `log_warning()`, `log_error()`, `log_debug()`, `log_critical()`**:
///   - **Purpose**: These are standard semantic logging methods. They use a `[LEVEL]` prefix
///     (e.g., `[INFO] message`) and do not use the box-drawing characters.
///   - **Usage**: Use them for their semantic meaning when a message doesn't fit the structured
///     box-drawing style or when a specific log level prefix is more appropriate.
///     If they begin a new conceptual block of information that is *not* part of the primary
///     box-drawing flow, they ought to begin with a `log_pipe()`.
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

    /// Main log function with level-based prefixes.
    ///
    /// Outputs messages with appropriate prefixes to indicate severity.
    /// Matches the style used by hyprsunset's debug logging.
    ///
    /// # Arguments
    /// * `level` - LogLevel indicating message importance
    /// * `message` - Text content to log
    pub fn log(level: LogLevel, message: &str) {
        // Skip logging if disabled
        if !Self::is_enabled() {
            return;
        }

        match level {
            LogLevel::Log => print!("[LOG] "),
            LogLevel::Warn => print!("[WARN] "),
            LogLevel::Err => print!("[ERR] "),
            LogLevel::Crit => print!("[CRIT] "),
            LogLevel::Info => print!("[INFO] "),
        }

        // Print the message with a newline at the end
        println!("{}", message);
    }

    // ═══ Convenience Methods for Common Log Levels ═══

    /// Log an error message (e.g., `[ERR] message`).
    pub fn log_error(message: &str) {
        Self::log(LogLevel::Err, message);
    }

    /// Log a warning message (e.g., `[WARN] message`).
    pub fn log_warning(message: &str) {
        Self::log(LogLevel::Warn, message);
    }

    /// Log an informational message (e.g., `[INFO] message`).
    /// This is for printing important information regardless if debug mode is enabled or not
    pub fn log_info(message: &str) {
        Self::log(LogLevel::Info, message);
    }

    /// Log a default debug/operational message (e.g., `[LOG] message`).
    pub fn log_debug(message: &str) {
        Self::log(LogLevel::Log, message);
    }

    /// Log a critical error message (e.g., `[CRIT] message`).
    pub fn log_critical(message: &str) {
        Self::log(LogLevel::Crit, message);
    }

    // ═══ Visual Formatting Functions ═══

    /// Log a decorated message, typically as part of an existing block or for standalone emphasis.
    ///
    /// **Purpose**: For logging messages that are part of an existing block started by `log_block_start()`.
    ///
    /// **Output**: Prints `┣ message`.
    ///
    /// **Context**: This should be a continuation of a `log_block_start()`, it will appear visually connected.
    /// Consider if a `log_block_start()` is more appropriate if this message initiates a new conceptual block.
    pub fn log_decorated(message: &str) {
        if !Self::is_enabled() {
            return;
        }
        println!("┣ {}", message);
    }

    /// Log an indented message for sub-items or details within a block.
    ///
    /// **Purpose**: For nested data or detailed sub-items that belong to a parent message
    /// (always logged with `log_block_start()`, `log_decorated()`, or a LogLevel type log message). Useful for listing configuration items,
    /// multi-part details, etc.
    ///
    /// **Output**: Prints `┃   message` (pipe, three spaces, then message).
    pub fn log_indented(message: &str) {
        if !Self::is_enabled() {
            return;
        }
        println!("┃   {}", message);
    }

    /// Log a visual pipe separator for vertical spacing at the *start* of a LogLevel type conceptual block.
    /// **Never use this at the end of a block.**
    ///
    /// **Purpose**: Used explicitly to insert a single, empty, prefixed line (`┃`) for vertical spacing.
    ///
    /// **Usage**: Its primary use-case is to create visual separation *before* a LogLevel type block containing
    /// `log_warning()`, `log_error()`, `log_critical()`, `log_info()`, or `log_debug()` messages.
    /// Avoid using it if it might lead to double pipes or unnecessary empty lines before another `log_block_start()`
    /// (which already provides top spacing). Using this only at the start of a block ensures we don't create
    /// an additional pipe before a`log_end()`.
    pub fn log_pipe() {
        if !Self::is_enabled() {
            return;
        }
        println!("┃");
    }

    /// Log a block start message, initiating a new conceptual block of information.
    ///
    /// **Purpose**: Always use this to initiate a new, distinct conceptual block of non-LogLevel type log information,
    /// especially for major state changes, phase indications, or significant events (e.g., "Commencing sunrise",
    /// "Loading configuration", "Backend detected").
    ///
    /// **Output**: Prepends an empty pipe `┃` for spacing from any previous log, then prints `┣ message`.
    ///
    /// **Usage**: Subsequent related messages within this conceptual block should typically use
    /// `log_decorated()` or `log_indented()`.
    pub fn log_block_start(message: &str) {
        if !Self::is_enabled() {
            return;
        }
        println!("┃");
        println!("┣ {}", message);
    }

    /// Log the application version header. Typically called once at application start.
    ///
    /// **Output**: `┏ sunsetr vX.Y.Z ━━╸` followed by `┃`.
    pub fn log_version() {
        if !Self::is_enabled() {
            return;
        }
        println!("┏ sunsetr v{} ━━╸", env!("CARGO_PKG_VERSION"));
    }

    /// Log the final termination marker. Always called once at application shutdown.
    ///
    /// **Output**: `╹`.
    pub fn log_end() {
        if !Self::is_enabled() {
            return;
        }
        println!("╹");
    }
}
