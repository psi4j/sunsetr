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
    /// Try to acquire an exclusive lock on a file.
    ///
    /// Returns `Some(LockFile)` if the lock was acquired, or `None` if the file
    /// is already locked by another process.
    pub fn try_acquire(path: impl AsRef<Path>) -> Result<Option<Self>> {
        let path = path.as_ref();

        // Open file without truncating to preserve existing content
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .context("Failed to open lock file")?;

        // Try to acquire exclusive lock (non-blocking)
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(LockFile { file })),
            Err(_) => {
                // File is already locked
                Ok(None)
            }
        }
    }

    /// Write contents to the locked file.
    ///
    /// This truncates the file and writes new content.
    pub fn write(&mut self, contents: &str) -> Result<()> {
        // Truncate and rewind
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;

        // Write new contents
        self.file.write_all(contents.as_bytes())?;
        self.file.flush()?;

        Ok(())
    }
}

impl Drop for LockFile {
    fn drop(&mut self) {
        // Unlock is automatic when file handle is dropped
        // but we can explicitly unlock for clarity
        let _ = self.file.unlock();
    }
}

// Lock file name constants
const MAIN_LOCK_FILENAME: &str = "sunsetr.lock";
const TEST_LOCK_FILENAME: &str = "sunsetr-test.lock";

/// Get the standard path for the main sunsetr lock file.
pub fn get_main_lock_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime_dir).join(MAIN_LOCK_FILENAME)
}

/// Get the standard path for the test mode lock file.
pub fn get_test_lock_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime_dir).join(TEST_LOCK_FILENAME)
}
