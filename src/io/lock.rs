//! Lock file management for single-instance enforcement.
//!
//! This module handles process-level locking to ensure only one instance of sunsetr
//! runs at a time per compositor. It also manages cross-compositor switches and
//! stale lock cleanup.

use anyhow::Result;
use fs2::FileExt;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};

use crate::backend::detect_compositor;
use crate::common::utils;
use crate::config;

/// Acquire an exclusive lock on the lock file.
///
/// This function attempts to create and lock a file in the runtime directory to ensure
/// single-instance operation. The lock file contains:
/// - Process ID (PID)
/// - Compositor name
/// - Config directory (optional)
///
/// # Returns
/// - `Ok(Some((lock_file, lock_path)))` if lock was successfully acquired
/// - `Ok(None)` if another instance is running and was handled appropriately
/// - `Err(_)` if an error occurred that requires termination
pub fn acquire_lock() -> Result<Option<(File, String)>> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let lock_path = format!("{runtime_dir}/sunsetr.lock");

    // Open lock file without truncating to preserve existing content
    let mut lock_file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;

    // Try to acquire exclusive lock (non-blocking)
    match lock_file.try_lock_exclusive() {
        Ok(()) => {
            // Lock acquired successfully - clean up any existing content
            lock_file.set_len(0)?;
            lock_file.seek(SeekFrom::Start(0))?;

            // Write our PID, compositor, and config dir to the lock file
            let pid = std::process::id();
            let compositor = detect_compositor().to_string();
            writeln!(&lock_file, "{pid}")?;
            writeln!(&lock_file, "{compositor}")?;
            // Write config directory (empty line if using default)
            if let Some(ref dir) = config::get_custom_config_dir() {
                writeln!(&lock_file, "{}", dir.display())?;
            } else {
                writeln!(&lock_file)?;
            }
            lock_file.flush()?;

            Ok(Some((lock_file, lock_path)))
        }
        Err(_) => {
            // Lock file exists and is locked - another instance may be running
            // Check if it's stale or a cross-compositor switch
            // handle_lock_conflict either returns Ok(()) or exits the process
            handle_lock_conflict(&lock_path)?;

            // Conflict was resolved (stale lock or cross-compositor), retry
            let mut retry_lock_file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path)?;

            match retry_lock_file.try_lock_exclusive() {
                Ok(()) => {
                    // Successfully acquired lock after cleanup
                    retry_lock_file.set_len(0)?;
                    retry_lock_file.seek(SeekFrom::Start(0))?;

                    // Write our PID, compositor, and config dir to the lock file
                    let pid = std::process::id();
                    let compositor = detect_compositor().to_string();
                    writeln!(&retry_lock_file, "{pid}")?;
                    writeln!(&retry_lock_file, "{compositor}")?;
                    // Write config directory (empty line if using default)
                    if let Some(ref dir) = config::get_custom_config_dir() {
                        writeln!(&retry_lock_file, "{}", dir.display())?;
                    } else {
                        writeln!(&retry_lock_file)?;
                    }
                    retry_lock_file.flush()?;

                    Ok(Some((retry_lock_file, lock_path)))
                }
                Err(e) => {
                    // Still failed after cleanup attempt
                    log_error_exit!("Failed to acquire lock after cleanup attempt: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Handle lock file conflicts intelligently.
///
/// This function validates and cleans up lock files in the following scenarios:
/// - Stale lock files (process no longer running)
/// - Cross-compositor switches (e.g., switching from Hyprland to Sway)
/// - Providing helpful suggestions when instance is already running
///
/// # Returns
/// - `Ok(())` if the conflict was resolved (stale lock or cross-compositor switch)
/// - Never returns if another instance is running (calls std::process::exit)
pub fn handle_lock_conflict(lock_path: &str) -> Result<()> {
    // Read the lock file to get PID and compositor info
    let lock_content = match std::fs::read_to_string(lock_path) {
        Ok(content) => content,
        Err(_) => {
            // Lock file doesn't exist or can't be read - assume it was cleaned up
            return Ok(());
        }
    };

    let lines: Vec<&str> = lock_content.trim().lines().collect();

    // Lock file format: PID (line 1), compositor (line 2), config_dir (line 3, optional)
    if lines.len() < 2 || lines.len() > 3 {
        // Invalid lock file format
        log_warning!("Lock file format invalid, removing");
        let _ = std::fs::remove_file(lock_path);
        return Ok(());
    }

    let pid = match lines[0].parse::<u32>() {
        Ok(pid) => pid,
        Err(_) => {
            log_warning!("Lock file contains invalid PID, removing stale lock");
            let _ = std::fs::remove_file(lock_path);
            return Ok(());
        }
    };

    let existing_compositor = lines[1].to_string();

    // Check if the process is actually running
    if !utils::is_process_running(pid) {
        log_warning!("Removing stale lock file (process {pid} no longer running)");
        let _ = std::fs::remove_file(lock_path);
        return Ok(());
    }

    // Process is running - check if this is a cross-compositor switch scenario
    let current_compositor = detect_compositor().to_string();

    if existing_compositor != current_compositor {
        // Cross-compositor switch detected - force cleanup
        log_pipe!();
        log_warning!(
            "Cross-compositor switch detected: {existing_compositor} → {current_compositor}"
        );
        log_indented!("Terminating existing sunsetr process (PID: {pid})");

        if utils::kill_process(pid) {
            // Wait for process to fully exit
            std::thread::sleep(std::time::Duration::from_millis(500));

            // Clean up lock file
            let _ = std::fs::remove_file(lock_path);

            log_indented!("Cross-compositor cleanup completed");
            return Ok(());
        } else {
            log_pipe!();
            log_error!("Failed to terminate existing process");
            log_indented!("Cannot force cleanup - existing process could not be terminated");
            log_end!();
            std::process::exit(1)
        }
    }

    // Same compositor - respect single instance enforcement
    log_pipe!();
    log_error!("sunsetr is already running (PID: {pid})");
    log_block_start!("Did you mean to:");
    log_indented!("• Reload configuration: sunsetr reload");
    log_indented!("• Test new values: sunsetr test <temp> <gamma>");
    log_indented!("• Switch to a preset: sunsetr preset <preset>");
    log_indented!("• Switch geolocation: sunsetr geo");
    log_block_start!("Cannot start - another sunsetr instance is running");
    log_end!();
    std::process::exit(1)
}

/// Get the path for the test mode lock file.
///
/// This returns the path to a special lock file used during test mode to prevent
/// configuration reloads while testing color temperatures.
pub fn get_test_lock_path() -> String {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    format!("{runtime_dir}/sunsetr-test.lock")
}
