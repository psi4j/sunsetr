//! Integration tests for the lock file mechanism logic.
//!
//! These tests provide a higher-level view of the lock file mechanism, demonstrating
//! the complete workflow and ensuring the fix prevents the race condition that allowed
//! multiple sunsetr instances to run simultaneously.
//!
//! ## Test Strategy
//!
//! Rather than attempting to run the full sunsetr binary (which requires a Wayland
//! environment), these tests directly exercise the lock file logic using the same
//! file operations and patterns used in main.rs.
//!
//! This approach provides:
//! - Fast, reliable tests that don't depend on external environment
//! - Clear demonstration of the bug and fix
//! - Comprehensive coverage of the lock acquisition workflow

use fs2::FileExt;
use serial_test::serial;
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use tempfile::tempdir;

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
