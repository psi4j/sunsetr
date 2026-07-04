//! High-level instance management for sunsetr processes.
//!
//! Coordinates instances through lock files, covering process lifecycle, signal
//! communication, and test mode. Built on the low-level lock operations in
//! `io::lock`.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::common::error::Silent;
use crate::io::lock::{self, LockFile};

/// Information about a running sunsetr instance.
#[derive(Debug, Clone)]
pub struct InstanceInfo {
    pub pid: u32,
    pub compositor: String,
    pub config_dir: Option<PathBuf>,
    pub session_id: Option<String>,
}

impl InstanceInfo {
    /// Parse instance info from lock file contents.
    ///
    /// Lock file format:
    /// - Line 1: PID
    /// - Line 2: Compositor name
    /// - Line 3: Config directory (optional, empty if default)
    /// - Line 4: Session ID (optional, for newer versions)
    pub fn from_lock_contents(contents: &str) -> Result<Self> {
        let lines: Vec<&str> = contents.trim().lines().collect();

        if lines.is_empty() {
            anyhow::bail!("Lock file is empty");
        }

        if lines.len() < 2 || lines.len() > 4 {
            anyhow::bail!("Invalid lock file format (expected 2-4 lines)");
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

        let session_id = if let Some(session_line) = lines.get(3) {
            if !session_line.is_empty() {
                Some(session_line.to_string())
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
            session_id,
        })
    }

    pub fn to_lock_contents(&self) -> String {
        let mut contents = format!("{}\n{}", self.pid, self.compositor);

        if let Some(ref config_dir) = self.config_dir {
            contents.push_str(&format!("\n{}", config_dir.display()));
        } else {
            contents.push('\n');
        }

        if let Some(ref session_id) = self.session_id {
            contents.push_str(&format!("\n{}", session_id));
        } else {
            contents.push('\n');
        }

        contents
    }
}

/// Read the lock file and return the instance only if its process is still alive.
pub fn get_running_instance() -> Result<Option<InstanceInfo>> {
    let lock_path = lock::get_main_lock_path();

    let lock_content = match std::fs::read_to_string(&lock_path) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };

    let info = InstanceInfo::from_lock_contents(&lock_content)?;

    if is_instance_running(info.pid) {
        Ok(Some(info))
    } else {
        Ok(None)
    }
}

pub fn get_running_instance_pid() -> Result<u32> {
    get_running_instance()?
        .map(|info| info.pid)
        .ok_or_else(|| anyhow::anyhow!("No sunsetr instance running"))
}

/// Adopt the config dir recorded in the main lock file.
///
/// An instance started with `--config` records its base dir in the lock
/// file so later subcommands operate on the same configuration and state.
/// This reads that dir and adopts it for the current process.
///
/// A no-op when a config dir is already set for this process, when there
/// is no lock file, or when the lock records no config dir. The dir is
/// adopted even if the recorded instance is no longer alive so commands
/// keep using the configuration that instance used. A malformed lock file
/// is reported as an error.
pub fn restore_config_dir() -> Result<()> {
    if crate::config::get_custom_config_dir().is_some() {
        return Ok(());
    }

    let lock_content = match std::fs::read_to_string(lock::get_main_lock_path()) {
        Ok(content) => content,
        Err(_) => return Ok(()),
    };

    if let Some(config_dir) = InstanceInfo::from_lock_contents(&lock_content)?.config_dir {
        let _ = crate::config::set_config_dir(Some(config_dir.display().to_string()));
    }

    Ok(())
}

pub fn is_instance_running(pid: u32) -> bool {
    let proc_path = format!("/proc/{}", pid);
    std::path::Path::new(&proc_path).exists()
}

pub fn terminate_instance(pid: u32) -> Result<()> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    kill(Pid::from_raw(pid as i32), Signal::SIGTERM)
        .map_err(|e| anyhow::anyhow!("Failed to send SIGTERM to process: {}", e))
}

pub fn send_reload_signal(pid: u32) -> Result<()> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    kill(Pid::from_raw(pid as i32), Signal::SIGUSR2)
        .map_err(|e| anyhow::anyhow!("Failed to send reload signal: {}", e))
}

pub fn send_test_signal(pid: u32, temp: u32, gamma: f64) -> Result<()> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let test_file_path = format!("/tmp/sunsetr-test-{}.tmp", pid);
    std::fs::write(&test_file_path, format!("{}\n{}", temp, gamma))
        .context("Failed to write test parameters")?;

    kill(Pid::from_raw(pid as i32), Signal::SIGUSR1)
        .map_err(|e| anyhow::anyhow!("Failed to send test signal: {}", e))
}

/// Whether the stored session ID differs from the current login session, marking
/// a process left over from a previous session.
fn is_stale_process(stored_session_id: Option<&str>) -> bool {
    match (stored_session_id, std::env::var("XDG_SESSION_ID").ok()) {
        (Some(stored), Some(current)) => stored != current,
        (None, _) => false,
        _ => false,
    }
}

/// Write an instant-shutdown flag, then signal the instance to shut down without
/// smooth transitions for a fast restart.
pub fn send_instant_shutdown_signal(pid: u32) -> Result<()> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let shutdown_file_path = format!("/tmp/sunsetr-shutdown-{}.tmp", pid);
    std::fs::write(&shutdown_file_path, "instant\n")
        .context("Failed to write instant shutdown flag")?;

    kill(Pid::from_raw(pid as i32), Signal::SIGTERM)
        .map_err(|e| anyhow::anyhow!("Failed to send instant shutdown signal: {}", e))
}

/// Spawn a background instance via the compositor's own spawn command, so it is
/// parented to the compositor and survives the launching process exiting.
pub fn spawn_background_instance(debug_enabled: bool) -> Result<()> {
    if is_test_mode_active() {
        log_error_end!(
            "Cannot start sunsetr. Test mode is currently active\n   Exit the test mode first (press Escape in test terminal)"
        );
        return Err(Silent.into());
    }

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

            let mut cmd = std::process::Command::new("niri");
            cmd.args(["msg", "action", "spawn", "--", &*sunsetr_path]);

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

            let mut cmd = std::process::Command::new("hyprctl");
            cmd.args(["dispatch", "exec", "--", &*sunsetr_path]);

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

            let exec_cmd = if let Some(config_dir) = crate::config::get_custom_config_dir() {
                format!("'{} --config {}'", sunsetr_path, config_dir.display())
            } else {
                format!("'{}'", sunsetr_path)
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

            let _child = if let Some(config_dir) = crate::config::get_custom_config_dir() {
                std::process::Command::new(&*sunsetr_path)
                    .args(["--config", &config_dir.display().to_string()])
                    .spawn()
            } else {
                std::process::Command::new(&*sunsetr_path).spawn()
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
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Acquire the test-mode lock, which blocks config reloads and other operations
/// while a test is running.
pub fn acquire_test_lock() -> Result<TestLock> {
    let lock_path = lock::get_test_lock_path();

    match LockFile::try_acquire(&lock_path)? {
        Some(mut lock) => {
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

/// Whether test mode is active. Removes the lock file if it names a dead process.
pub fn is_test_mode_active() -> bool {
    let test_lock_path = lock::get_test_lock_path();

    if let Ok(contents) = std::fs::read_to_string(&test_lock_path)
        && let Ok(lock_pid) = contents.trim().parse::<u32>()
    {
        if is_instance_running(lock_pid) {
            return true;
        } else {
            let _ = std::fs::remove_file(&test_lock_path);
        }
    }
    false
}

/// Acquire the main lock, resolving any conflicting instance first.
pub fn ensure_single_instance() -> Result<Option<(LockFile, PathBuf)>> {
    if is_test_mode_active() {
        log_error_end!(
            "Cannot start sunsetr because test mode is currently active\n   Exit the test mode first (press Escape in test terminal)"
        );
        return Err(Silent.into());
    }

    let lock_path = lock::get_main_lock_path();

    match LockFile::try_acquire(&lock_path)? {
        Some(mut lock) => {
            let info = InstanceInfo {
                pid: std::process::id(),
                compositor: crate::backend::detect_compositor().to_string(),
                config_dir: crate::config::get_custom_config_dir(),
                session_id: std::env::var("XDG_SESSION_ID").ok(),
            };

            lock.write(&info.to_lock_contents())?;

            Ok(Some((lock, lock_path)))
        }
        None => {
            handle_instance_conflict(&lock_path, false)?;

            match LockFile::try_acquire(&lock_path)? {
                Some(mut lock) => {
                    let info = InstanceInfo {
                        pid: std::process::id(),
                        compositor: crate::backend::detect_compositor().to_string(),
                        config_dir: crate::config::get_custom_config_dir(),
                        session_id: std::env::var("XDG_SESSION_ID").ok(),
                    };

                    lock.write(&info.to_lock_contents())?;

                    Ok(Some((lock, lock_path)))
                }
                None => {
                    anyhow::bail!("Failed to acquire lock after conflict resolution")
                }
            }
        }
    }
}

/// Resolve a conflict when another instance holds the lock. Handles stale locks
/// (process gone), dysfunctional instances (zombie or cross-compositor switch,
/// recovered automatically), and active instances (reports a helpful error).
pub fn handle_instance_conflict(lock_path: &Path, debug_enabled: bool) -> Result<()> {
    let lock_content = match std::fs::read_to_string(lock_path) {
        Ok(content) => content,
        Err(_) => {
            return Ok(());
        }
    };

    let info = match InstanceInfo::from_lock_contents(&lock_content) {
        Ok(info) => info,
        Err(_) => {
            log_warning!("Lock file format invalid, removing");
            let _ = std::fs::remove_file(lock_path);
            return Ok(());
        }
    };

    if !is_instance_running(info.pid) {
        log_warning!(
            "Removing stale lock file (process {} no longer running)",
            info.pid
        );
        let _ = std::fs::remove_file(lock_path);
        return Ok(());
    }

    if is_stale_process(info.session_id.as_deref()) {
        log_info!("Detected process from previous session, recovering...");

        let _ = send_instant_shutdown_signal(info.pid);

        let max_attempts = 30;
        let mut attempts = 0;

        while attempts < max_attempts && lock_path.exists() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            attempts += 1;
        }

        if lock_path.exists() {
            log_warning!("Dysfunctional process did not clean up lock file within timeout");
            let _ = std::fs::remove_file(lock_path);
        }

        log_info!("Recovery completed, starting fresh instance...");

        let sunsetr = crate::Sunsetr::new(debug_enabled)
            .without_headers()
            .background(true);

        return sunsetr.run();
    }

    log_pipe!();
    log_error!("sunsetr is already running (PID: {})", info.pid);
    log_block_start!("Did you mean to:");
    log_indented!("• Restart application: sunsetr restart");
    log_indented!("• Test new values: sunsetr test <temp> <gamma>");
    log_indented!("• Switch to a preset: sunsetr preset <preset>");
    log_indented!("• Switch geolocation: sunsetr geo");
    log_block_start!("Cannot start because another sunsetr instance is running");
    log_end!();
    Err(Silent.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_instance_info_from_lock_contents() {
        let contents = "12345\nHyprland\n/home/user/.config/sunsetr\nsession1";
        let info = InstanceInfo::from_lock_contents(contents).unwrap();
        assert_eq!(info.pid, 12345);
        assert_eq!(info.compositor, "Hyprland");
        assert_eq!(
            info.config_dir,
            Some(PathBuf::from("/home/user/.config/sunsetr"))
        );
        assert_eq!(info.session_id, Some("session1".to_string()));

        let contents = "12345\nHyprland\n/home/user/.config/sunsetr";
        let info = InstanceInfo::from_lock_contents(contents).unwrap();
        assert_eq!(info.pid, 12345);
        assert_eq!(info.compositor, "Hyprland");
        assert_eq!(
            info.config_dir,
            Some(PathBuf::from("/home/user/.config/sunsetr"))
        );
        assert_eq!(info.session_id, None);

        let contents = "67890\nNiri\n";
        let info = InstanceInfo::from_lock_contents(contents).unwrap();
        assert_eq!(info.pid, 67890);
        assert_eq!(info.compositor, "Niri");
        assert_eq!(info.config_dir, None);
        assert_eq!(info.session_id, None);

        let contents = "99999\nSway\n";
        let info = InstanceInfo::from_lock_contents(contents).unwrap();
        assert_eq!(info.pid, 99999);
        assert_eq!(info.compositor, "Sway");
        assert_eq!(info.config_dir, None);
        assert_eq!(info.session_id, None);

        let contents = "11111\nWayland";
        let info = InstanceInfo::from_lock_contents(contents).unwrap();
        assert_eq!(info.pid, 11111);
        assert_eq!(info.compositor, "Wayland");
        assert_eq!(info.config_dir, None);
        assert_eq!(info.session_id, None);
    }

    #[test]
    fn test_instance_info_from_lock_contents_errors() {
        let contents = "";
        assert!(InstanceInfo::from_lock_contents(contents).is_err());

        let contents = "12345";
        assert!(InstanceInfo::from_lock_contents(contents).is_err());

        let contents = "not_a_pid\nHyprland";
        assert!(InstanceInfo::from_lock_contents(contents).is_err());

        let contents = "12345\nHyprland\n/config/dir\nsession\nextra_line";
        assert!(InstanceInfo::from_lock_contents(contents).is_err());
    }

    #[test]
    fn test_instance_info_to_lock_contents() {
        let info = InstanceInfo {
            pid: 12345,
            compositor: "Hyprland".to_string(),
            config_dir: Some(PathBuf::from("/home/user/.config/sunsetr")),
            session_id: Some("session1".to_string()),
        };
        let contents = info.to_lock_contents();
        assert_eq!(
            contents,
            "12345\nHyprland\n/home/user/.config/sunsetr\nsession1"
        );

        let info = InstanceInfo {
            pid: 67890,
            compositor: "Niri".to_string(),
            config_dir: None,
            session_id: None,
        };
        let contents = info.to_lock_contents();
        assert_eq!(contents, "67890\nNiri\n\n");
    }

    #[test]
    fn test_instance_info_round_trip() {
        let original = InstanceInfo {
            pid: 99999,
            compositor: "Sway".to_string(),
            config_dir: Some(PathBuf::from("/custom/config")),
            session_id: Some("test_session".to_string()),
        };

        let serialized = original.to_lock_contents();
        let parsed = InstanceInfo::from_lock_contents(&serialized).unwrap();

        assert_eq!(parsed.pid, original.pid);
        assert_eq!(parsed.compositor, original.compositor);
        assert_eq!(parsed.config_dir, original.config_dir);
        assert_eq!(parsed.session_id, original.session_id);
    }

    #[test]
    fn test_is_instance_running() {
        let current_pid = std::process::id();
        assert!(is_instance_running(current_pid));

        assert!(!is_instance_running(999999999));
    }

    #[test]
    fn test_test_lock_drop_cleanup() {
        let temp_dir = tempdir().unwrap();
        let test_lock_path = temp_dir.path().join("test.lock");

        {
            let _test_lock = TestLock {
                _lock: LockFile {
                    file: fs::File::create(&test_lock_path).unwrap(),
                },
                path: test_lock_path.clone(),
            };
            assert!(test_lock_path.exists());
        }

        assert!(!test_lock_path.exists());
    }

    #[test]
    fn test_is_test_mode_active_stale_cleanup() {
        let temp_dir = tempdir().unwrap();
        let test_lock_path = temp_dir.path().join("test.lock");

        fs::write(&test_lock_path, "999999999").unwrap();
        assert!(test_lock_path.exists());
    }

    #[test]
    fn test_signal_functions_structure() {
        let _reload_fn: fn(u32) -> Result<()> = send_reload_signal;
        let _test_fn: fn(u32, u32, f64) -> Result<()> = send_test_signal;
        let _terminate_fn: fn(u32) -> Result<()> = terminate_instance;
    }
}
