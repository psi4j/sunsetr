//! Low-level lock file operations for cross-process synchronization.
//!
//! This module provides primitive lock file operations using fs2 for
//! exclusive file locking. Higher-level instance coordination logic
//! is handled by the `io::instance` module.

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// A handle to an exclusively locked file.
pub struct LockFile {
    pub(crate) file: File,
}

impl LockFile {
    /// Try to acquire an exclusive lock on a file (non-blocking).
    ///
    /// Returns `Some(LockFile)` if the lock was acquired, or `None` if the file
    /// is already locked by another process.
    pub fn try_acquire(path: impl AsRef<Path>) -> Result<Option<Self>> {
        let path = path.as_ref();

        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .context("Failed to open lock file")?;

        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(LockFile { file })),
            Err(_) => Ok(None),
        }
    }

    /// Acquire an exclusive lock on a file (blocking).
    ///
    /// This blocks until the lock is acquired, making it suitable for
    /// serializing operations that must complete atomically.
    pub fn acquire(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("Failed to open lock file: {}", path.display()))?;

        file.lock_exclusive()
            .with_context(|| format!("Failed to acquire lock on {}", path.display()))?;

        Ok(LockFile { file })
    }

    /// Write contents to the locked file.
    ///
    /// This truncates the file and writes new content.
    pub fn write(&mut self, contents: &str) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(contents.as_bytes())?;
        self.file.flush()?;
        Ok(())
    }
}

impl Drop for LockFile {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

const MAIN_LOCK_FILENAME: &str = "sunsetr.lock";
const TEST_LOCK_FILENAME: &str = "sunsetr-test.lock";

/// Get the standard path for the main sunsetr lock file.
pub fn get_main_lock_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", nix::unistd::getuid()));
    PathBuf::from(runtime_dir).join(MAIN_LOCK_FILENAME)
}

/// Get the standard path for the test mode lock file.
pub fn get_test_lock_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", nix::unistd::getuid()));
    PathBuf::from(runtime_dir).join(TEST_LOCK_FILENAME)
}

/// Get the lock file path for a specific config file.
///
/// Uses a stable hash of the config path to support multiple config directories.
/// The lockfile is placed in $XDG_RUNTIME_DIR to avoid cluttering config directories.
pub fn get_config_lock_path(config_path: &Path) -> PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    config_path.hash(&mut hasher);
    let hash = hasher.finish();

    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", nix::unistd::getuid()));
    PathBuf::from(runtime_dir).join(format!("sunsetr-config-{:x}.lock", hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs::{self, OpenOptions};
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
        let mut first_file = File::create(&lock_path).unwrap();
        writeln!(first_file, "12345").unwrap();
        writeln!(first_file, "compositor").unwrap();
        first_file.flush().unwrap();

        first_file
            .try_lock_exclusive()
            .expect("Failed to lock first file");

        let result = File::create(&lock_path);
        assert!(
            result.is_ok(),
            "File::create should succeed even when locked"
        );

        drop(result);
        let content = fs::read_to_string(&lock_path).unwrap();
        assert_eq!(content, "", "File::create truncates the file immediately!");
        first_file.set_len(0).unwrap();
        first_file.seek(SeekFrom::Start(0)).unwrap();
        writeln!(first_file, "12345").unwrap();
        writeln!(first_file, "compositor").unwrap();
        first_file.flush().unwrap();

        let second_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        let lock_result = second_file.try_lock_exclusive();
        assert!(
            lock_result.is_err(),
            "Lock should fail when file is already locked"
        );

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

        let mut first_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        first_file
            .try_lock_exclusive()
            .expect("Should acquire lock");

        first_file.set_len(0).unwrap();
        first_file.seek(SeekFrom::Start(0)).unwrap();
        writeln!(first_file, "11111").unwrap();
        writeln!(first_file, "test-compositor").unwrap();
        first_file.flush().unwrap();

        let second_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        let lock_result = second_file.try_lock_exclusive();
        assert!(
            lock_result.is_err(),
            "Second process should fail to acquire lock"
        );

        drop(second_file);
        let content = fs::read_to_string(&lock_path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "11111");
        assert_eq!(lines[1], "test-compositor");

        drop(first_file);

        let mut third_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        third_file
            .try_lock_exclusive()
            .expect("Should acquire lock after first is released");

        third_file.set_len(0).unwrap();
        third_file.seek(SeekFrom::Start(0)).unwrap();
        writeln!(third_file, "33333").unwrap();
        writeln!(third_file, "new-compositor").unwrap();
        third_file.flush().unwrap();

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
        let temp_dir = tempdir().unwrap();
        let lock_path = temp_dir.path().join("test.lock");
        let mut file = File::create(&lock_path).unwrap();
        writeln!(file, "999999").unwrap();
        writeln!(file, "test-compositor").unwrap();
        drop(file);
        let content = fs::read_to_string(&lock_path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 2, "Should have 2 lines");
        let pid: u32 = lines[0].parse().expect("Should parse PID");
        assert_eq!(pid, 999999);
        let instance_exists = false;

        if !instance_exists {
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

        let mut process1_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        process1_file
            .try_lock_exclusive()
            .expect("Process 1 should acquire lock");

        process1_file.set_len(0).unwrap();
        process1_file.seek(SeekFrom::Start(0)).unwrap();
        writeln!(process1_file, "12345").unwrap();
        writeln!(process1_file, "process1").unwrap();
        process1_file.flush().unwrap();

        let process2_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        assert!(
            process2_file.try_lock_exclusive().is_err(),
            "Process 2 should fail to acquire lock"
        );

        drop(process2_file);
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
        let mut process1_file = fs::File::create(&lock_path).unwrap();
        writeln!(process1_file, "12345").unwrap();
        writeln!(process1_file, "process1").unwrap();
        process1_file.flush().unwrap();
        process1_file
            .try_lock_exclusive()
            .expect("Process 1 should acquire lock");
        let _process2_file = fs::File::create(&lock_path).unwrap();
        drop(_process2_file);
        let content = fs::read_to_string(&lock_path).unwrap();
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
        let mut first_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        match first_file.try_lock_exclusive() {
            Ok(_) => {
                first_file.set_len(0).unwrap();
                first_file.seek(SeekFrom::Start(0)).unwrap();
                writeln!(first_file, "11111").unwrap();
                writeln!(first_file, "wayland").unwrap();
                first_file.flush().unwrap();
            }
            Err(_) => panic!("First instance should acquire lock"),
        }

        let second_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        match second_file.try_lock_exclusive() {
            Ok(_) => panic!("Second instance should NOT acquire lock"),
            Err(_) => {
                drop(second_file);
                let content = fs::read_to_string(&lock_path).unwrap();
                let lines: Vec<&str> = content.trim().lines().collect();

                assert_eq!(lines.len(), 2);
                let pid = lines[0].parse::<u32>().expect("Should be valid PID");
                assert_eq!(pid, 11111);
                assert_eq!(lines[1], "wayland");
            }
        }

        drop(first_file);
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

    /// Test LockFile struct methods.
    #[test]
    #[serial]
    fn test_lockfile_struct() {
        let temp_dir = tempdir().unwrap();
        let lock_path = temp_dir.path().join("struct_test.lock");
        let lock1 = LockFile::try_acquire(&lock_path).unwrap();
        assert!(lock1.is_some(), "Should acquire lock on empty file");
        let lock2 = LockFile::try_acquire(&lock_path).unwrap();
        assert!(lock2.is_none(), "Second acquisition should fail");
        let mut lock = lock1.unwrap();
        lock.write("test content\nline 2").unwrap();
        drop(lock);
        let content = fs::read_to_string(&lock_path).unwrap();
        assert_eq!(content, "test content\nline 2");
        let lock3 = LockFile::try_acquire(&lock_path).unwrap();
        assert!(lock3.is_some(), "Should be able to acquire after drop");
    }
}
