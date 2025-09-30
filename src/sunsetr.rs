//! Application coordinator that manages the complete lifecycle of sunsetr.
//!
//! This module handles resource acquisition, initialization, and orchestration
//! of the core application logic. It manages:
//! - Terminal setup with RAII guards
//! - Configuration loading
//! - Backend detection and creation
//! - Lock file management for single-instance enforcement
//! - Signal handler setup
//! - Monitor initialization (D-Bus, config watcher)
//!
//! The `Sunsetr` struct uses a builder pattern to support different startup contexts:
//! - Normal startup: `Sunsetr::new(debug_enabled).run()`
//! - Geo restart: `Sunsetr::new(true).without_lock().with_previous_state(state).run()`
//! - Reload spawn: `Sunsetr::new(debug_enabled).with_reload().run()`
//! - Simulation mode: `Sunsetr::new(debug_enabled).without_lock().without_headers().run()`

use anyhow::{Context, Result};

use crate::{
    backend::{create_backend, detect_backend},
    common::utils::TerminalGuard,
    config::{self, Config},
    core::period::Period,
    core::{Core, CoreParams},
    geo::times::GeoTimes,
    io::signals::setup_signal_handler,
    io::{dbus, lock},
};

/// Builder for configuring and running the sunsetr application.
///
/// This builder provides a flexible way to start sunsetr with different
/// configurations depending on the context (normal startup, geo restart,
/// reload spawn, simulation mode, etc.).
///
/// # Examples
///
/// ```no_run
/// use sunsetr::Sunsetr;
/// use sunsetr::Period;
///
/// # fn main() -> anyhow::Result<()> {
/// // Normal application startup
/// let debug_enabled = false;
/// Sunsetr::new(debug_enabled).run()?;
///
/// // Restart after geo selection without creating a new lock
/// let previous_state = Some(Period::Night);
/// Sunsetr::new(true)
///     .without_lock()
///     .with_previous_state(previous_state)
///     .run()?;
///
/// // Process spawned from reload command
/// Sunsetr::new(debug_enabled)
///     .with_reload()
///     .run()?;
///
/// // Simulation mode
/// Sunsetr::new(debug_enabled)
///     .without_lock()
///     .without_headers()
///     .run()?;
/// # Ok(())
/// # }
/// ```
pub struct Sunsetr {
    debug_enabled: bool,
    create_lock: bool,
    previous_state: Option<Period>,
    show_headers: bool,
    from_reload: bool, // Process spawned from reload command
}

impl Sunsetr {
    /// Create a new runner with defaults matching normal run
    pub fn new(debug_enabled: bool) -> Self {
        Self {
            debug_enabled,
            create_lock: true,
            previous_state: None,
            show_headers: true,
            from_reload: false,
        }
    }

    /// Skip lock file creation (for geo restart)
    pub fn without_lock(mut self) -> Self {
        self.create_lock = false;
        self.show_headers = false; // Geo restarts never show headers
        self
    }

    /// Set previous state for smooth transitions
    pub fn with_previous_state(mut self, state: Option<Period>) -> Self {
        self.previous_state = state;
        self
    }

    /// Skip header display (for geo operations)
    pub fn without_headers(mut self) -> Self {
        self.show_headers = false;
        self
    }

    /// Mark this process as spawned from reload command
    pub fn with_reload(mut self) -> Self {
        self.from_reload = true;
        self
    }

    /// Execute the application with the configured settings.
    ///
    /// This method handles the complete application lifecycle including:
    /// - Terminal setup
    /// - Configuration loading
    /// - Backend detection and initialization
    /// - Lock file management (if enabled)
    /// - Signal handler setup
    /// - Main application loop
    /// - Graceful shutdown and cleanup
    pub fn run(self) -> Result<()> {
        // Show headers if requested
        if self.show_headers {
            log_version!();
        }

        // Execute the core application logic
        #[cfg(debug_assertions)]
        {
            let log_msg = format!(
                "DEBUG: Process {} startup: debug_enabled={}, create_lock={}\n",
                std::process::id(),
                self.debug_enabled,
                self.create_lock
            );
            let _ = std::fs::write(
                format!("/tmp/sunsetr-debug-{}.log", std::process::id()),
                log_msg,
            );
        }

        // Try to set up terminal features (cursor hiding, echo suppression)
        // This will gracefully handle cases where no terminal is available (e.g., systemd service)
        let _term = TerminalGuard::new().context("failed to initialize terminal features")?;

        // Note: The Hyprsunset backend uses PR_SET_PDEATHSIG for process cleanup

        // Load and validate configuration first (needed for backend detection)
        let config = match Config::load() {
            Ok(config) => config,
            Err(e) => {
                // Use the standalone error format with the full error chain
                log_error_exit!("Configuration failed");
                // Print the error chain in the default format which already looks good
                eprintln!("{:?}", e);
                std::process::exit(1);
            }
        };

        // Detect and validate the backend early (needed for lock file info)
        let backend_type = detect_backend(&config).unwrap_or_else(|_| {
            // Backend detection errors are already logged properly in detect_backend
            // Just exit since the error was already displayed
            std::process::exit(1);
        });

        // Handle lock file BEFORE any debug output from watchers
        let (lock_file, lock_path) = if self.create_lock {
            // Use the io::lock module for centralized lock management
            match lock::acquire_lock()? {
                Some((file, path)) => (Some(file), Some(path)),
                None => return Ok(()), // Lock not acquired but handled appropriately
            }
        } else {
            (None, None)
        };

        // Set up signal handling
        let signal_state = setup_signal_handler(self.debug_enabled)?;

        // Start D-Bus sleep/resume monitoring (optional - graceful degradation if D-Bus unavailable)
        if let Err(e) =
            dbus::start_sleep_resume_monitor(signal_state.signal_sender.clone(), self.debug_enabled)
        {
            log_pipe!();
            log_warning!("D-Bus sleep/resume monitoring unavailable: {}", e);
            log_indented!(
                "Sleep/resume detection will not work, but sunsetr will continue normally"
            );
            log_indented!("This is normal in environments without systemd or D-Bus");
        }

        // Start config file watcher for hot reload (optional - graceful degradation if unavailable)
        if let Err(e) =
            config::start_config_watcher(signal_state.signal_sender.clone(), self.debug_enabled)
            && self.debug_enabled
        {
            log_pipe!();
            log_warning!("Config file watching unavailable: {}", e);
            log_indented!("Hot config reload disabled, use SIGUSR2 for manual reload");
        }

        // Log configuration with resolved backend type
        config.log_config(Some(backend_type));

        // Initialize GeoTimes before backend creation if in geo mode
        // Backends need this to calculate correct initial state values
        let geo_times =
            GeoTimes::from_config(&config).context("Failed to initialize geo transition times")?;

        log_block_start!("Detected backend: {}", backend_type.name());

        // Create the backend
        let backend = create_backend(
            backend_type,
            &config,
            self.debug_enabled,
            geo_times.as_ref(),
        )?;

        // Create lock_info tuple from lock components
        let lock_info = if let (Some(lock_file), Some(lock_path)) = (lock_file, lock_path) {
            log_block_start!("Lock acquired, starting sunsetr...");
            Some((lock_file, lock_path))
        } else {
            // Skip lock creation (geo selection restart case or simulation mode)
            // Only show "Restarting" message if not in simulation mode
            if !crate::time::source::is_simulated() {
                log_block_start!("Restarting sunsetr...");
            }
            None
        };

        // Create Core with all necessary dependencies
        let core = Core::new(CoreParams {
            backend,
            config,
            signal_state,
            debug_enabled: self.debug_enabled,
            geo_times,
            lock_info,
            initial_previous_state: self.previous_state,
            from_reload: self.from_reload,
        });

        // Execute the core logic
        core.execute()?;

        Ok(())
    }
}

#[cfg(test)]
mod lock_tests {
    use fs2::FileExt;
    use serial_test::serial;
    use std::fs::{self, File, OpenOptions};
    use std::io::{Seek, SeekFrom, Write};
    use tempfile::tempdir;

    /// Test that demonstrates the race condition bug and validates the fix.
    ///
    /// This test shows:
    /// 1. How `File::create()` immediately truncates the file (the bug)
    /// 2. How `OpenOptions` with `truncate(false)` preserves the file content (the fix)
    ///
    /// This is the core test that ensures the race condition cannot occur.
    #[test]
    #[serial]
    fn test_lock_file_not_truncated_before_lock() {
        let temp_dir = tempdir().unwrap();
        let lock_path = temp_dir.path().join("test.lock");

        // Create and lock a file with initial content
        let mut first_file = File::create(&lock_path).unwrap();
        writeln!(first_file, "12345").unwrap();
        writeln!(first_file, "compositor").unwrap();
        first_file.flush().unwrap();

        // Lock the file
        first_file
            .try_lock_exclusive()
            .expect("Failed to lock first file");

        // Now simulate what the old code did (File::create which truncates)
        let result = File::create(&lock_path);
        assert!(
            result.is_ok(),
            "File::create should succeed even when locked"
        );

        // The file is now truncated! Let's verify
        drop(result); // Close the file handle
        let content = fs::read_to_string(&lock_path).unwrap();
        assert_eq!(content, "", "File::create truncates the file immediately!");

        // Now test the fixed approach with OpenOptions
        // First, restore the content
        first_file.set_len(0).unwrap();
        first_file.seek(SeekFrom::Start(0)).unwrap();
        writeln!(first_file, "12345").unwrap();
        writeln!(first_file, "compositor").unwrap();
        first_file.flush().unwrap();

        // Try the fixed approach
        let second_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false) // Don't truncate!
            .open(&lock_path)
            .unwrap();

        // Try to lock it (this should fail)
        let lock_result = second_file.try_lock_exclusive();
        assert!(
            lock_result.is_err(),
            "Lock should fail when file is already locked"
        );

        // But the content should still be intact
        drop(second_file);
        let content = fs::read_to_string(&lock_path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 2, "File should still have 2 lines");
        assert_eq!(lines[0], "12345", "PID should be preserved");
        assert_eq!(lines[1], "compositor", "Compositor should be preserved");
    }

    /// Test the correct lock file workflow as implemented in main.rs.
    ///
    /// This test validates the complete workflow:
    /// 1. First process: Opens without truncating, acquires lock, then writes
    /// 2. Second process: Opens without truncating, fails to acquire lock, reads valid content
    /// 3. Third process: After first releases, can acquire lock and update content
    ///
    /// This ensures the lock file mechanism works correctly for preventing multiple instances.
    #[test]
    #[serial]
    fn test_correct_lock_file_workflow() {
        let temp_dir = tempdir().unwrap();
        let lock_path = temp_dir.path().join("test.lock");

        // First process: open without truncating, acquire lock, then write
        let mut first_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        // Acquire lock
        first_file
            .try_lock_exclusive()
            .expect("Should acquire lock");

        // Now safe to truncate and write
        first_file.set_len(0).unwrap();
        first_file.seek(SeekFrom::Start(0)).unwrap();
        writeln!(first_file, "11111").unwrap();
        writeln!(first_file, "test-compositor").unwrap();
        first_file.flush().unwrap();

        // Second process: try to do the same
        let second_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        // Try to acquire lock (should fail)
        let lock_result = second_file.try_lock_exclusive();
        assert!(
            lock_result.is_err(),
            "Second process should fail to acquire lock"
        );

        // Content should still be valid for reading
        drop(second_file);
        let content = fs::read_to_string(&lock_path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "11111");
        assert_eq!(lines[1], "test-compositor");

        // Release first lock
        drop(first_file);

        // Now third process should be able to acquire
        let mut third_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        third_file
            .try_lock_exclusive()
            .expect("Should acquire lock after first is released");

        // Update content
        third_file.set_len(0).unwrap();
        third_file.seek(SeekFrom::Start(0)).unwrap();
        writeln!(third_file, "33333").unwrap();
        writeln!(third_file, "new-compositor").unwrap();
        third_file.flush().unwrap();

        // Verify updated content
        drop(third_file);
        let content = fs::read_to_string(&lock_path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines[0], "33333");
        assert_eq!(lines[1], "new-compositor");
    }

    /// Test stale lock detection logic.
    ///
    /// This test simulates the stale lock detection that happens in `handle_lock_conflict()`:
    /// - A lock file exists with a PID that's no longer running
    /// - The application should detect this and remove the stale lock
    ///
    /// In the real implementation, this allows sunsetr to recover from crashes or
    /// force-killed processes that couldn't clean up their lock files.
    #[test]
    #[serial]
    fn test_stale_lock_detection() {
        // This tests the logic without actually running processes
        let temp_dir = tempdir().unwrap();
        let lock_path = temp_dir.path().join("test.lock");

        // Create a lock file with a definitely non-existent PID
        let mut file = File::create(&lock_path).unwrap();
        writeln!(file, "999999").unwrap();
        writeln!(file, "test-compositor").unwrap();
        drop(file);

        // Read and parse the lock file
        let content = fs::read_to_string(&lock_path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();

        assert_eq!(lines.len(), 2, "Should have 2 lines");

        let pid: u32 = lines[0].parse().expect("Should parse PID");
        assert_eq!(pid, 999999);

        // In real code, we'd check if this process exists
        // For testing, we know 999999 doesn't exist
        let process_exists = false; // Would be: is_process_running(pid)

        if !process_exists {
            // Stale lock - remove it
            fs::remove_file(&lock_path).unwrap();
        }

        assert!(!lock_path.exists(), "Stale lock should be removed");
    }

    /// Test that demonstrates the lock file race condition fix.
    ///
    /// This test shows the correct behavior with the fix:
    /// 1. Process 1 creates lock file with `truncate(false)`, acquires lock, writes content
    /// 2. Process 2 opens with `truncate(false)`, fails to acquire lock
    /// 3. The lock file content remains intact and valid
    ///
    /// This proves that the fix prevents the race condition where the second process
    /// would truncate the file before checking the lock.
    #[test]
    #[serial]
    fn test_lock_race_condition_fix() {
        let temp_dir = tempdir().unwrap();
        let lock_path = temp_dir.path().join("test.lock");

        // Process 1: Create lock file, acquire lock, write content
        let mut process1_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false) // This is the fix - don't truncate
            .open(&lock_path)
            .unwrap();

        // Acquire exclusive lock
        process1_file
            .try_lock_exclusive()
            .expect("Process 1 should acquire lock");

        // Now safe to truncate and write
        process1_file.set_len(0).unwrap();
        process1_file.seek(SeekFrom::Start(0)).unwrap();
        writeln!(process1_file, "12345").unwrap();
        writeln!(process1_file, "process1").unwrap();
        process1_file.flush().unwrap();

        // Process 2: Try to open and lock the same file
        let process2_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false) // Don't truncate - this preserves the content
            .open(&lock_path)
            .unwrap();

        // Try to acquire lock (this should fail)
        assert!(
            process2_file.try_lock_exclusive().is_err(),
            "Process 2 should fail to acquire lock"
        );

        // Verify the lock file content is still intact
        drop(process2_file); // Close handle to read the file
        let content = fs::read_to_string(&lock_path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();

        assert_eq!(lines.len(), 2, "Lock file should have 2 lines");
        assert_eq!(lines[0], "12345", "PID should be preserved");
        assert_eq!(lines[1], "process1", "Process name should be preserved");
    }

    /// Test what happens with the old (buggy) approach.
    ///
    /// This test demonstrates the original bug for educational purposes:
    /// 1. Process 1 creates a lock file with content
    /// 2. Process 2 uses `File::create()` which immediately truncates the file
    /// 3. The file becomes empty before Process 2 even checks the lock
    ///
    /// This shows why using `File::create()` for lock files is dangerous and
    /// why we need `OpenOptions` with `truncate(false)`.
    #[test]
    #[serial]
    fn test_lock_race_condition_bug() {
        let temp_dir = tempdir().unwrap();
        let lock_path = temp_dir.path().join("test_bug.lock");

        // Process 1: Create and lock file with content
        let mut process1_file = fs::File::create(&lock_path).unwrap();
        writeln!(process1_file, "12345").unwrap();
        writeln!(process1_file, "process1").unwrap();
        process1_file.flush().unwrap();
        process1_file
            .try_lock_exclusive()
            .expect("Process 1 should acquire lock");

        // Process 2: Use File::create (which truncates!)
        let _process2_file = fs::File::create(&lock_path).unwrap();
        // At this point, the file is already truncated!

        // Check the content
        drop(_process2_file);
        let content = fs::read_to_string(&lock_path).unwrap();

        // This demonstrates the bug - file is now empty
        assert_eq!(content, "", "File::create truncates the file immediately!");
    }

    /// Test the complete workflow with proper error handling.
    ///
    /// This test simulates the exact workflow from main.rs:
    /// 1. First instance: Opens file, acquires lock, writes PID and compositor
    /// 2. Second instance: Opens file, fails to acquire lock, reads content to verify owner
    /// 3. Third instance: After first releases, successfully acquires and updates lock
    ///
    /// This comprehensive test ensures the entire lock mechanism works correctly
    /// throughout the application lifecycle, including proper cleanup and handoff
    /// between instances.
    #[test]
    #[serial]
    fn test_complete_lock_workflow() {
        let temp_dir = tempdir().unwrap();
        let lock_path = temp_dir.path().join("workflow.lock");

        // Simulate what main.rs does now

        // First instance
        let mut first_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        match first_file.try_lock_exclusive() {
            Ok(_) => {
                // Lock acquired - safe to truncate and write
                first_file.set_len(0).unwrap();
                first_file.seek(SeekFrom::Start(0)).unwrap();
                writeln!(first_file, "11111").unwrap();
                writeln!(first_file, "wayland").unwrap();
                first_file.flush().unwrap();
            }
            Err(_) => panic!("First instance should acquire lock"),
        }

        // Second instance tries while first is still holding lock
        let second_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        match second_file.try_lock_exclusive() {
            Ok(_) => panic!("Second instance should NOT acquire lock"),
            Err(_) => {
                // Expected - lock is held by first instance
                // Read the lock file to check who owns it
                drop(second_file);
                let content = fs::read_to_string(&lock_path).unwrap();
                let lines: Vec<&str> = content.trim().lines().collect();

                assert_eq!(lines.len(), 2);
                let pid = lines[0].parse::<u32>().expect("Should be valid PID");
                assert_eq!(pid, 11111);
                assert_eq!(lines[1], "wayland");
            }
        }

        // Release first lock
        drop(first_file);

        // Third instance should now succeed
        let mut third_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        match third_file.try_lock_exclusive() {
            Ok(_) => {
                // Lock acquired - update content
                third_file.set_len(0).unwrap();
                third_file.seek(SeekFrom::Start(0)).unwrap();
                writeln!(third_file, "33333").unwrap();
                writeln!(third_file, "hyprland").unwrap();
                third_file.flush().unwrap();
            }
            Err(_) => panic!("Third instance should acquire lock after first releases"),
        }
    }
}
