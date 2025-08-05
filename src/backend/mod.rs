//! Backend abstraction layer for color temperature control across multiple compositors.
//!
//! This module provides a unified interface for color temperature and gamma control
//! across different Wayland compositors through the `ColorTemperatureBackend` trait.
//! It includes automatic backend detection and supports both Hyprland-specific
//! (hyprsunset) and generic Wayland (wlr-gamma-control-unstable-v1) implementations.
//!
//! ## Supported Backends
//!
//! - **Hyprland Backend**: Uses the hyprsunset daemon for color temperature control
//! - **Wayland Backend**: Direct implementation of wlr-gamma-control-unstable-v1 protocol
//!
//! ## Backend Selection
//!
//! The backend can be selected automatically or explicitly:
//! - **Auto-detection**: Examines environment variables to determine the appropriate backend
//! - **Explicit Configuration**: Set `backend = "hyprland"` or `backend = "wayland"` in config
//!
//! Auto-detection priority: Hyprland → Wayland → error
//!
//! ## Architecture
//!
//! The backend system uses trait objects to provide a common interface while
//! allowing backend-specific optimizations and features. Each backend handles:
//! - Connection management to the underlying color control system
//! - State application with proper error handling
//! - Startup behavior and transitions
//! - Cleanup during application shutdown

use anyhow::Result;
use std::sync::atomic::AtomicBool;

use crate::Log;
use crate::config::{Backend, Config};
use crate::time_state::TransitionState;

pub mod hyprland;
pub mod wayland;

/// Enum representing different Wayland compositors that sunsetr supports
#[derive(Debug, Clone, PartialEq)]
pub enum Compositor {
    Hyprland,
    Niri,
    Sway,
    Other(String),
}

impl std::fmt::Display for Compositor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Compositor::Hyprland => write!(f, "hyprland"),
            Compositor::Niri => write!(f, "niri"),
            Compositor::Sway => write!(f, "sway"),
            Compositor::Other(name) => write!(f, "{name}"),
        }
    }
}

/// Trait for color temperature backends that can control display temperature and gamma.
///
/// This trait abstracts the differences between Hyprland (hyprsunset) and Wayland
/// (wlr-gamma-control-unstable-v1) implementations while providing a common interface
/// for the main application logic.
pub trait ColorTemperatureBackend {
    /// Apply a specific transition state with proper interpolation.
    ///
    /// This is the main method for applying color temperature and gamma changes.
    /// It handles both stable states and transitioning states with progress interpolation.
    ///
    /// # Arguments
    /// * `state` - The transition state to apply (stable or transitioning)
    /// * `config` - Configuration containing temperature and gamma values
    /// * `running` - Atomic flag to check if the application should continue
    ///
    /// # Returns
    /// - `Ok(())` if the state was applied successfully
    /// - `Err` if there was an error applying the state
    fn apply_transition_state(
        &mut self,
        state: TransitionState,
        config: &Config,
        running: &AtomicBool,
    ) -> Result<()>;

    /// Apply startup state during application initialization.
    ///
    /// This method is called during startup to set the initial display state.
    /// It may handle startup transitions differently than regular transitions.
    ///
    /// # Arguments
    /// * `state` - The initial transition state to apply
    /// * `config` - Configuration containing startup settings
    /// * `running` - Atomic flag to check if the application should continue
    ///
    /// # Returns
    /// - `Ok(())` if the startup state was applied successfully
    /// - `Err` if there was an error applying the startup state
    fn apply_startup_state(
        &mut self,
        state: TransitionState,
        config: &Config,
        running: &AtomicBool,
    ) -> Result<()>;

    /// Apply specific temperature and gamma values directly.
    ///
    /// This method is used for fine-grained control during animations like startup transitions.
    /// It bypasses the normal state-based application and sets exact values.
    ///
    /// # Arguments
    /// * `temperature` - Color temperature in Kelvin
    /// * `gamma` - Gamma value as a percentage (0.0-100.0)
    /// * `running` - Atomic flag to check if the application should continue
    ///
    /// # Returns
    /// - `Ok(())` if the values were applied successfully
    /// - `Err` if there was an error applying the values
    fn apply_temperature_gamma(
        &mut self,
        temperature: u32,
        gamma: f32,
        running: &AtomicBool,
    ) -> Result<()>;

    /// Get a human-readable name for this backend.
    ///
    /// # Returns
    /// A string identifying the backend (e.g., "Hyprland", "Wayland")
    fn backend_name(&self) -> &'static str;

    /// Perform backend-specific cleanup operations.
    ///
    /// This method is called during application shutdown to clean up any
    /// resources or processes managed by the backend.
    ///
    /// # Arguments
    /// * `debug_enabled` - Whether to show detailed cleanup logging
    ///
    /// The default implementation does nothing, but backends can override
    /// this to perform specific cleanup (e.g., stopping managed processes).
    fn cleanup(self: Box<Self>, debug_enabled: bool) {
        // Default implementation does nothing
        // Backends can override this for specific cleanup needs
        let _ = debug_enabled; // Suppress unused parameter warning
    }
}

/// Detect the appropriate backend based on the current environment and configuration.
///
/// This function examines environment variables and system state to determine
/// whether to use the Hyprland or Wayland backend.
///
/// # Arguments
/// * `config` - Configuration that may explicitly specify backend preference
///
/// # Returns
/// - `BackendType::Hyprland` if running on Hyprland or explicitly configured
/// - `BackendType::Wayland` if running on other Wayland compositors
///
/// # Errors
/// Returns an error if no suitable backend can be determined or if the
/// environment is not supported (e.g., not running on Wayland).
pub fn detect_backend(config: &Config) -> Result<BackendType> {
    // Check explicit configuration first
    if let Some(backend) = &config.backend {
        match backend {
            Backend::Auto => {
                // Auto-detect based on environment
                if std::env::var("WAYLAND_DISPLAY").is_err() {
                    Log::log_pipe();
                    anyhow::bail!(
                        "sunsetr requires a Wayland session. WAYLAND_DISPLAY is not set.\n\
                        Please ensure you're running on a Wayland compositor."
                    );
                }

                // Check if we're running on Hyprland
                if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
                    Ok(BackendType::Hyprland)
                } else {
                    Ok(BackendType::Wayland)
                }
            }
            Backend::Wayland => {
                // Verify we're actually on Wayland
                if std::env::var("WAYLAND_DISPLAY").is_err() {
                    Log::log_pipe();
                    anyhow::bail!(
                        "Configuration specifies backend=\"wayland\" but WAYLAND_DISPLAY is not set.\n\
                        Are you running on Wayland?"
                    );
                }
                Ok(BackendType::Wayland)
            }
            Backend::Hyprland => {
                // Verify we're actually running on Hyprland when explicitly configured
                if std::env::var("WAYLAND_DISPLAY").is_err() {
                    Log::log_pipe();
                    anyhow::bail!(
                        "Configuration specifies backend=\"hyprland\" but WAYLAND_DISPLAY is not set.\n\
                        Are you running on Wayland?"
                    );
                }

                if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_err() {
                    Log::log_pipe();
                    anyhow::bail!(
                        "Configuration specifies backend=\"hyprland\" but you're not running on Hyprland.\n\
                        \n\
                        To fix this, either:\n\
                        • Switch to automatic detection: set backend=\"auto\" in sunsetr.toml\n\
                        • Use the Wayland backend: set backend=\"wayland\" in sunsetr.toml\n\
                        • Run sunsetr on Hyprland instead of your current compositor"
                    );
                }

                Ok(BackendType::Hyprland)
            }
        }
    } else {
        // Fallback to auto-detection when backend is not specified
        if std::env::var("WAYLAND_DISPLAY").is_err() {
            Log::log_pipe();
            anyhow::bail!(
                "sunsetr requires a Wayland session. WAYLAND_DISPLAY is not set.\n\
                Please ensure you're running on a Wayland compositor."
            );
        }

        // Check if we're running on Hyprland
        if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
            Ok(BackendType::Hyprland)
        } else {
            Ok(BackendType::Wayland)
        }
    }
}

/// Detect the current Wayland compositor
///
/// This function determines which compositor is currently running, which is used
/// to spawn processes as direct children of the compositor for proper parent death
/// monitoring.
///
/// # Returns
/// - `Compositor::Hyprland` if running on Hyprland
/// - `Compositor::Niri` if running on niri
/// - `Compositor::Sway` if running on Sway
/// - `Compositor::Other(name)` for unknown compositors
pub fn detect_compositor() -> Compositor {
    // Check for Hyprland first (it has specific env var)
    if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
        return Compositor::Hyprland;
    }

    // Check for Sway
    if std::env::var("SWAYSOCK").is_ok() {
        return Compositor::Sway;
    }

    // Try to detect niri or other compositors via XDG_CURRENT_DESKTOP or other methods
    if let Ok(desktop) = std::env::var("XDG_CURRENT_DESKTOP") {
        match desktop.to_lowercase().as_str() {
            "niri" => return Compositor::Niri,
            "sway" => return Compositor::Sway,
            "hyprland" => return Compositor::Hyprland,
            _ => {}
        }
    }

    // Try to detect via running processes
    if let Ok(output) = std::process::Command::new("pgrep")
        .arg("-x")
        .arg("niri")
        .output()
    {
        if output.status.success() && !output.stdout.is_empty() {
            return Compositor::Niri;
        }
    }

    // Default to Other with the desktop name if available
    if let Ok(desktop) = std::env::var("XDG_CURRENT_DESKTOP") {
        Compositor::Other(desktop)
    } else {
        Compositor::Other("unknown".to_string())
    }
}

/// Create a backend instance based on the detected or configured backend type.
///
/// # Arguments
/// * `backend_type` - The type of backend to create
/// * `config` - Configuration for backend initialization
/// * `debug_enabled` - Whether debug output should be enabled for this backend
///
/// # Returns
/// A boxed backend implementation ready for use
///
/// # Errors
/// Returns an error if the backend cannot be initialized or if required
/// dependencies are missing.
pub fn create_backend(
    backend_type: BackendType,
    config: &Config,
    debug_enabled: bool,
) -> Result<Box<dyn ColorTemperatureBackend>> {
    match backend_type {
        BackendType::Hyprland => Ok(
            Box::new(hyprland::HyprlandBackend::new(config, debug_enabled)?)
                as Box<dyn ColorTemperatureBackend>,
        ),
        BackendType::Wayland => Ok(
            Box::new(wayland::WaylandBackend::new(config, debug_enabled)?)
                as Box<dyn ColorTemperatureBackend>,
        ),
    }
}

/// Enumeration of available backend types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    /// Hyprland compositor using hyprsunset for color temperature control
    Hyprland,
    /// Generic Wayland compositor using wlr-gamma-control-unstable-v1 protocol
    Wayland,
}

impl BackendType {
    /// Get the human-readable name for this backend type.
    pub fn name(&self) -> &'static str {
        match self {
            BackendType::Hyprland => "Hyprland",
            BackendType::Wayland => "Wayland",
        }
    }

    /// Get the default configuration values for this backend type.
    ///
    /// # Returns
    /// Tuple of (start_hyprsunset, backend) defaults for this backend
    #[allow(dead_code)]
    pub fn default_config_values(&self) -> (bool, Backend) {
        match self {
            BackendType::Hyprland => (true, Backend::Hyprland), // Start hyprsunset, use hyprland backend
            BackendType::Wayland => (false, Backend::Wayland), // Don't start hyprsunset, use wayland backend
        }
    }

    /// Get the default configuration values for auto-detection.
    ///
    /// # Returns
    /// Tuple of (start_hyprsunset, backend) defaults based on environment detection
    #[allow(dead_code)]
    pub fn auto_config_values() -> (bool, Backend) {
        // Check if we're running on Hyprland
        if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
            (true, Backend::Hyprland) // Start hyprsunset on Hyprland
        } else {
            (false, Backend::Wayland) // Don't start hyprsunset on other compositors
        }
    }
}
