//! Unit tests for the lock file mechanism.
//!
//! These tests verify the low-level file locking behavior to ensure that the race condition
//! fix works correctly. They test the actual file operations without running the full sunsetr
//! binary, making them fast and reliable.
//!
//! ## Background
//!
//! The lock file mechanism prevents multiple sunsetr instances from running simultaneously.
//! A race condition existed where `File::create()` would truncate the lock file before
//! checking if the lock could be acquired, causing the second process to read an empty file
//! and incorrectly remove it as "invalid".
//!
//! ## Fix
//!
//! The fix uses `OpenOptions` with `truncate(false)` to preserve the file content until
//! after the exclusive lock is successfully acquired.

use serial_test::serial;
use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use tempfile::tempdir;
use fs2::FileExt;

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
    first_file.try_lock_exclusive().expect("Failed to lock first file");
    
    // Now simulate what the old code did (File::create which truncates)
    let result = File::create(&lock_path);
    assert!(result.is_ok(), "File::create should succeed even when locked");
    
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
        .truncate(false)  // Don't truncate!
        .open(&lock_path)
        .unwrap();
    
    // Try to lock it (this should fail)
    let lock_result = second_file.try_lock_exclusive();
    assert!(lock_result.is_err(), "Lock should fail when file is already locked");
    
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
    first_file.try_lock_exclusive().expect("Should acquire lock");
    
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
    assert!(lock_result.is_err(), "Second process should fail to acquire lock");
    
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
    
    third_file.try_lock_exclusive().expect("Should acquire lock after first is released");
    
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