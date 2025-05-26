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
    
    /// Log an error message.
    pub fn log_error(message: &str) {
        Self::log(LogLevel::Err, message);
    }

    /// Log a warning message.
    pub fn log_warning(message: &str) {
        Self::log(LogLevel::Warn, message);
    }

    /// Log an informational message.
    pub fn log_info(message: &str) {
        Self::log(LogLevel::Info, message);
    }

    /// Log a debug/operational message.
    pub fn log_debug(message: &str) {
        Self::log(LogLevel::Log, message);
    }

    /// Log a critical error message.
    pub fn log_critical(message: &str) {
        Self::log(LogLevel::Crit, message);
    }

    // ═══ Visual Formatting Functions ═══
    
    /// Log a decorated message with visual branching indicator.
    /// 
    /// Used for main status messages and important information.
    pub fn log_decorated(message: &str) {
        if !Self::is_enabled() {
            return;
        }
        println!("┣ {}", message);
    }

    /// Log an indented message for sub-items or details.
    /// 
    /// Used for secondary information under main status messages.
    pub fn log_indented(message: &str) {
        if !Self::is_enabled() {
            return;
        }
        println!("┃   {}", message);
    }

    /// Log a visual pipe separator.
    /// 
    /// Used to create visual spacing in structured output.
    pub fn log_pipe() {
        if !Self::is_enabled() {
            return;
        }
        println!("┃");
    }

    /// Log a block start message with visual separation.
    /// 
    /// Used for major state changes or new operational phases.
    pub fn log_block_start(message: &str) {
        if !Self::is_enabled() {
            return;
        }
        println!("┃");
        println!("┣ {}", message);
    }

    /// Log the application version header.
    /// 
    /// Creates the initial visual header when the application starts.
    pub fn log_version() {
        if !Self::is_enabled() {
            return;
        }
        println!("┏ sunsetr v{} ━━╸", env!("CARGO_PKG_VERSION"));
        println!("┃");
    }

    /// Log the final termination marker.
    /// 
    /// Closes the visual structure when the application ends.
    pub fn log_end() {
        if !Self::is_enabled() {
            return;
        }
        println!("╹");
    }
}
