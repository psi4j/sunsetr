//! Low-level lock file operations for cross-process synchronization.
//!
//! Primitive exclusive file locking via fs2. Higher-level instance coordination
//! lives in `io::instance`.

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

    /// Truncate the locked file and write `contents`.
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

pub fn get_main_lock_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", nix::unistd::getuid()));
    PathBuf::from(runtime_dir).join(MAIN_LOCK_FILENAME)
}

pub fn get_test_lock_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", nix::unistd::getuid()));
    PathBuf::from(runtime_dir).join(TEST_LOCK_FILENAME)
}

/// Lock file path for a specific config file.
///
/// Hashes the config path so multiple config directories get distinct locks. The
/// lock lives in `$XDG_RUNTIME_DIR` to keep config directories uncluttered.
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

    /// `File::create` truncates immediately, so the lock file must be opened with
    /// `truncate(false)` to survive a concurrent open before the lock is checked.
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
                third_file.set_len(0).unwrap();
                third_file.seek(SeekFrom::Start(0)).unwrap();
                writeln!(third_file, "33333").unwrap();
                writeln!(third_file, "hyprland").unwrap();
                third_file.flush().unwrap();
            }
            Err(_) => panic!("Third instance should acquire lock after first releases"),
        }
    }

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
