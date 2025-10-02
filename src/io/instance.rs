//! High-level instance management for sunsetr processes.
//!
//! This module coordinates sunsetr instances using lock files, handling process
//! lifecycle, signal communication, and test mode management. It builds on top of
//! the low-level lock file operations in `io::lock`.

use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::io::lock::{self, LockFile};

/// Information about a running sunsetr instance.
#[derive(Debug, Clone)]
pub struct InstanceInfo {
    /// Process ID of the instance
    pub pid: u32,
    /// Compositor the instance is running on
    pub compositor: String,
    /// Custom config directory if set
    pub config_dir: Option<PathBuf>,
}

impl InstanceInfo {
    /// Parse instance info from lock file contents.
    ///
    /// Lock file format:
    /// - Line 1: PID
    /// - Line 2: Compositor name
    /// - Line 3: Config directory (optional, empty if default)
    pub fn from_lock_contents(contents: &str) -> Result<Self> {
        let lines: Vec<&str> = contents.trim().lines().collect();

        if lines.is_empty() {
            anyhow::bail!("Lock file is empty");
        }

        if lines.len() < 2 || lines.len() > 3 {
            anyhow::bail!("Invalid lock file format (expected 2-3 lines)");
        }

        let pid = lines[0]
            .parse::<u32>()
            .context("Invalid PID format in lock file")?;

        let compositor = lines[1].to_string();

        let config_dir = if let Some(config_line) = lines.get(2) {
            if !config_line.is_empty() {
                Some(PathBuf::from(config_line))
            } else {
                None
            }
        } else {
            None
        };

        Ok(InstanceInfo {
            pid,
            compositor,
            config_dir,
        })
    }
}

/// Get information about the currently running sunsetr instance.
///
/// This function reads the lock file and validates that the process is still running.
/// It also restores the config directory from the lock file if present.
pub fn get_running_instance() -> Result<Option<InstanceInfo>> {
    let lock_path = lock::get_main_lock_path();

    // Read the lock file content
    let lock_content = match std::fs::read_to_string(&lock_path) {
        Ok(content) => content,
        Err(_) => return Ok(None), // No lock file means no instance running
    };

    let info = InstanceInfo::from_lock_contents(&lock_content)?;

    // If there's a config directory in the lock file, restore it for this process
    // This ensures commands like 'reload' and 'preset' use the same config dir
    if let Some(ref config_dir) = info.config_dir {
        // Try to set the config dir - ignore error if already set
        let _ = crate::config::set_config_dir(Some(config_dir.display().to_string()));
    }

    // Verify the process is still running
    if is_instance_running(info.pid) {
        Ok(Some(info))
    } else {
        Ok(None) // Process is dead, treat as no instance running
    }
}

/// Get just the PID of the running sunsetr instance.
///
/// This is a compatibility wrapper for code that only needs the PID.
pub fn get_running_instance_pid() -> Result<u32> {
    get_running_instance()?
        .map(|info| info.pid)
        .ok_or_else(|| anyhow::anyhow!("No sunsetr instance running"))
}

/// Check if a process with the given PID is still running.
pub fn is_instance_running(pid: u32) -> bool {
    // Check if /proc/{pid} exists
    let proc_path = format!("/proc/{}", pid);
    std::path::Path::new(&proc_path).exists()
}

/// Terminate a sunsetr instance by sending SIGTERM.
pub fn terminate_instance(pid: u32) -> Result<()> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    kill(Pid::from_raw(pid as i32), Signal::SIGTERM)
        .map_err(|e| anyhow::anyhow!("Failed to send SIGTERM to process: {}", e))
}

/// Send a reload signal (SIGUSR2) to a running instance.
pub fn send_reload_signal(pid: u32) -> Result<()> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    kill(Pid::from_raw(pid as i32), Signal::SIGUSR2)
        .map_err(|e| anyhow::anyhow!("Failed to send reload signal: {}", e))
}

/// Send a test signal (SIGUSR1) to a running instance.
///
/// The test parameters are written to a temporary file that the instance reads.
pub fn send_test_signal(pid: u32, temp: u32, gamma: f32) -> Result<()> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    // Write test parameters to temporary file
    let test_file_path = format!("/tmp/sunsetr-test-{}.tmp", pid);
    std::fs::write(&test_file_path, format!("{}\n{}", temp, gamma))
        .context("Failed to write test parameters")?;

    // Send SIGUSR1 to trigger test mode
    kill(Pid::from_raw(pid as i32), Signal::SIGUSR1)
        .map_err(|e| anyhow::anyhow!("Failed to send test signal: {}", e))
}

/// Spawn a new sunsetr instance in the background.
///
/// This function uses compositor-specific commands to spawn sunsetr properly
/// as a child of the compositor rather than the current process.
pub fn spawn_background_instance(debug_enabled: bool) -> Result<()> {
    use crate::backend::{Compositor, detect_compositor};

    #[cfg(debug_assertions)]
    eprintln!(
        "DEBUG: spawn_background_instance() entry, PID: {}",
        std::process::id()
    );

    let compositor = detect_compositor();

    #[cfg(debug_assertions)]
    eprintln!("DEBUG: Detected compositor: {compositor:?}");

    if debug_enabled {
        log_pipe!();
        log_debug!("Detected compositor: {:?}", compositor);
    }

    // Get the current executable path for the sunsetr command
    let current_exe = std::env::current_exe().context("Failed to get current executable path")?;
    let sunsetr_path = current_exe.to_string_lossy();

    #[cfg(debug_assertions)]
    {
        eprintln!("DEBUG: sunsetr_path: {}", sunsetr_path);
        if let Some(config_dir) = crate::config::get_custom_config_dir() {
            eprintln!(
                "DEBUG: Custom config dir to pass: {}",
                crate::common::utils::private_path(&config_dir)
            );
        }
    }

    match compositor {
        Compositor::Niri => {
            log_block_start!("Starting sunsetr via niri compositor...");

            // Build command with required args
            // Always include --from-reload since this is only called from reload command
            let mut cmd = std::process::Command::new("niri");
            cmd.args([
                "msg",
                "action",
                "spawn",
                "--",
                &*sunsetr_path,
                "--from-reload",
            ]);

            // Add config dir if present
            if let Some(config_dir) = crate::config::get_custom_config_dir() {
                cmd.arg("--config").arg(config_dir.display().to_string());
            }

            #[cfg(debug_assertions)]
            eprintln!("DEBUG: About to spawn via niri: {:?}", cmd);

            let output = cmd.output().context("Failed to execute niri command")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("niri spawn command failed: {}", stderr);
            }

            log_decorated!("Background process started.");
        }
        Compositor::Hyprland => {
            log_block_start!("Starting sunsetr via Hyprland compositor...");

            // For Hyprland, we use -- to separate hyprctl options from the exec command
            // Always include --from-reload
            let mut cmd = std::process::Command::new("hyprctl");
            cmd.args(["dispatch", "exec", "--", &*sunsetr_path, "--from-reload"]);

            // Add config dir if present
            if let Some(config_dir) = crate::config::get_custom_config_dir() {
                cmd.arg("--config").arg(config_dir.display().to_string());
            }

            #[cfg(debug_assertions)]
            eprintln!("DEBUG: About to spawn via Hyprland: {:?}", cmd);

            let output = cmd.output().context("Failed to execute hyprctl command")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("hyprctl dispatch exec command failed: {}", stderr);
            }

            log_decorated!("Background process started.");
        }
        Compositor::Sway => {
            log_block_start!("Starting sunsetr via Sway compositor...");

            // For Sway, we need to quote the command to preserve arguments through
            // double expansion (by swaymsg and sway)
            // Always include --from-reload
            let exec_cmd = if let Some(config_dir) = crate::config::get_custom_config_dir() {
                // Single-quote the entire command to preserve arguments
                format!(
                    "'{} --from-reload --config {}'",
                    sunsetr_path,
                    config_dir.display()
                )
            } else {
                format!("'{} --from-reload'", sunsetr_path)
            };

            #[cfg(debug_assertions)]
            eprintln!("DEBUG: About to spawn via Sway: swaymsg exec {}", exec_cmd);

            let output = std::process::Command::new("swaymsg")
                .args(["exec", &exec_cmd])
                .output()
                .context("Failed to execute swaymsg command")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("swaymsg exec command failed: {}", stderr);
            }

            log_decorated!("Background process started.");
        }
        Compositor::Other(name) => {
            log_pipe!();
            log_warning!("Unknown compositor '{}' detected", name);
            log_indented!("Starting sunsetr directly (may not have proper parent relationship)");

            // Fallback to direct spawn - not ideal but better than nothing
            // Always include --from-reload since this is only called from reload
            let _child = if let Some(config_dir) = crate::config::get_custom_config_dir() {
                std::process::Command::new(&*sunsetr_path)
                    .args([
                        "--from-reload",
                        "--config",
                        &config_dir.display().to_string(),
                    ])
                    .spawn()
            } else {
                std::process::Command::new(&*sunsetr_path)
                    .args(["--from-reload"])
                    .spawn()
            }
            .context("Failed to spawn sunsetr process directly")?;

            log_decorated!("Background process started (direct spawn).");
        }
    }

    #[cfg(debug_assertions)]
    eprintln!(
        "DEBUG: spawn_background_instance() exit, PID: {}",
        std::process::id()
    );

    Ok(())
}

/// Test mode lock management with RAII.
pub struct TestLock {
    _lock: LockFile,
    pub path: PathBuf,
}

impl Drop for TestLock {
    fn drop(&mut self) {
        // Clean up the lock file when dropped
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Acquire a test mode lock.
///
/// This prevents configuration reloads and other operations while testing.
pub fn acquire_test_lock() -> Result<TestLock> {
    let lock_path = lock::get_test_lock_path();

    // Try to acquire the lock
    match LockFile::try_acquire(&lock_path)? {
        Some(mut lock) => {
            // Write our PID to the lock file
            lock.write(&format!("{}", std::process::id()))?;

            Ok(TestLock {
                _lock: lock,
                path: lock_path.clone(),
            })
        }
        None => {
            anyhow::bail!("Test mode is already active in another process")
        }
    }
}

/// Check if test mode is currently active.
pub fn is_test_mode_active() -> bool {
    let test_lock_path = lock::get_test_lock_path();

    // Check if lock file exists and if the PID in it is still running
    if let Ok(contents) = std::fs::read_to_string(&test_lock_path)
        && let Ok(lock_pid) = contents.trim().parse::<u32>()
    {
        // Check if the process that created the lock is still running
        if is_instance_running(lock_pid) {
            return true;
        } else {
            // Process is dead, clean up stale lock file
            let _ = std::fs::remove_file(&test_lock_path);
        }
    }
    false
}

/// Ensure single instance enforcement.
///
/// This function acquires the main lock and handles conflicts appropriately,
/// including cross-compositor switches and stale lock cleanup.
pub fn ensure_single_instance() -> Result<Option<(LockFile, PathBuf)>> {
    let lock_path = lock::get_main_lock_path();

    // Try to acquire the lock
    match LockFile::try_acquire(&lock_path)? {
        Some(mut lock) => {
            // We got the lock - write our instance info
            let pid = std::process::id();
            let compositor = crate::backend::detect_compositor().to_string();

            // Write PID, compositor, and config dir to the lock file
            writeln!(&mut lock.file, "{}", pid)?;
            writeln!(&mut lock.file, "{}", compositor)?;
            // Write config directory (empty line if using default)
            if let Some(ref dir) = crate::config::get_custom_config_dir() {
                writeln!(&mut lock.file, "{}", dir.display())?;
            } else {
                writeln!(&mut lock.file)?;
            }
            lock.file.flush()?;

            Ok(Some((lock, lock_path)))
        }
        None => {
            // Lock is held - check for conflicts
            handle_instance_conflict(&lock_path)?;

            // If we returned from handle_instance_conflict, the lock was released
            // Try to acquire it again
            match LockFile::try_acquire(&lock_path)? {
                Some(mut lock) => {
                    let pid = std::process::id();
                    let compositor = crate::backend::detect_compositor().to_string();

                    writeln!(&mut lock.file, "{}", pid)?;
                    writeln!(&mut lock.file, "{}", compositor)?;
                    if let Some(ref dir) = crate::config::get_custom_config_dir() {
                        writeln!(&mut lock.file, "{}", dir.display())?;
                    } else {
                        writeln!(&mut lock.file)?;
                    }
                    lock.file.flush()?;

                    Ok(Some((lock, lock_path)))
                }
                None => {
                    // Still couldn't get the lock
                    anyhow::bail!("Failed to acquire lock after conflict resolution")
                }
            }
        }
    }
}

/// Handle conflicts when another instance holds the lock.
///
/// This function handles:
/// - Stale locks (process no longer running)
/// - Cross-compositor switches
/// - Active instances (shows helpful error message)
pub fn handle_instance_conflict(lock_path: &Path) -> Result<()> {
    // Read the lock file to get instance info
    let lock_content = match std::fs::read_to_string(lock_path) {
        Ok(content) => content,
        Err(_) => {
            // Lock file doesn't exist or can't be read - assume it was cleaned up
            return Ok(());
        }
    };

    let info = match InstanceInfo::from_lock_contents(&lock_content) {
        Ok(info) => info,
        Err(_) => {
            // Invalid lock file format - clean it up
            log_warning!("Lock file format invalid, removing");
            let _ = std::fs::remove_file(lock_path);
            return Ok(());
        }
    };

    // Check if the process is actually running
    if !is_instance_running(info.pid) {
        log_warning!(
            "Removing stale lock file (process {} no longer running)",
            info.pid
        );
        let _ = std::fs::remove_file(lock_path);
        return Ok(());
    }

    // Process is running - check if this is a cross-compositor switch
    let current_compositor = crate::backend::detect_compositor().to_string();

    if info.compositor != current_compositor {
        // Cross-compositor switch detected - force cleanup
        log_pipe!();
        log_warning!(
            "Cross-compositor switch detected: {} → {}",
            info.compositor,
            current_compositor
        );
        log_indented!("Terminating existing sunsetr process (PID: {})", info.pid);

        if terminate_instance(info.pid).is_ok() {
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
    log_error!("sunsetr is already running (PID: {})", info.pid);
    log_block_start!("Did you mean to:");
    log_indented!("• Reload configuration: sunsetr reload");
    log_indented!("• Test new values: sunsetr test <temp> <gamma>");
    log_indented!("• Switch to a preset: sunsetr preset <preset>");
    log_indented!("• Switch geolocation: sunsetr geo");
    log_block_start!("Cannot start - another sunsetr instance is running");
    log_end!();
    std::process::exit(1)
}

/// Clean up stale lock files.
///
/// This removes lock files where the owning process is no longer running.
pub fn cleanup_stale_locks() -> Result<()> {
    let main_lock = lock::get_main_lock_path();
    let test_lock = lock::get_test_lock_path();

    // Check main lock
    if main_lock.exists()
        && let Ok(contents) = std::fs::read_to_string(&main_lock)
        && let Ok(info) = InstanceInfo::from_lock_contents(&contents)
        && !is_instance_running(info.pid)
    {
        let _ = std::fs::remove_file(&main_lock);
    }

    // Check test lock
    if test_lock.exists()
        && let Ok(contents) = std::fs::read_to_string(&test_lock)
        && let Ok(pid) = contents.trim().parse::<u32>()
        && !is_instance_running(pid)
    {
        let _ = std::fs::remove_file(&test_lock);
    }

    Ok(())
}
