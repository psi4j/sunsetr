//! Backend abstraction for color temperature and gamma control.
//!
//! The `ColorTemperatureBackend` trait provides a common interface over three backends:
//! the Hyprland native CTM backend (hyprland-ctm-control-v1), the hyprsunset-process
//! backend, and the generic Wayland backend (wlr-gamma-control-unstable-v1, used by many
//! compositors). The backend is taken from config or auto-detected with priority
//! Hyprland -> Wayland -> error.

use anyhow::Result;
use std::sync::atomic::AtomicBool;

use crate::common::error::Silent;
use crate::config::{Backend, Config};
use crate::core::runtime_state::RuntimeState;

pub mod gamma;
pub mod hyprland;
pub mod hyprsunset;
pub mod wayland;

/// Wayland compositors sunsetr recognizes for detection and process parenting.
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

/// Common interface implemented by each color temperature and gamma backend.
pub trait ColorTemperatureBackend {
    /// Apply the color temperature and gamma for a state, interpolating during transitions.
    fn apply_transition_state(
        &mut self,
        runtime_state: &RuntimeState,
        running: &AtomicBool,
    ) -> Result<()>;

    /// Apply the initial display state at startup, which may transition differently
    /// than a regular state change.
    fn apply_startup_state(
        &mut self,
        runtime_state: &RuntimeState,
        running: &AtomicBool,
    ) -> Result<()>;

    /// Apply exact temperature (Kelvin) and gamma (percentage, 10.0-200.0) values,
    /// bypassing state-based application for fine-grained control during animations.
    fn apply_temperature_gamma(
        &mut self,
        temperature: u32,
        gamma: f64,
        running: &AtomicBool,
    ) -> Result<()>;

    fn backend_name(&self) -> &'static str;

    /// Perform a quick, non-blocking hotplug poll and apply if needed.
    /// Default no-op. Backends that support dynamic outputs can override.
    fn poll_hotplug(&mut self) -> Result<()> {
        Ok(())
    }

    /// Release backend resources at shutdown. The default is a no-op. Backends override it
    /// to perform specific cleanup such as stopping a managed process.
    fn cleanup(self: Box<Self>, debug_enabled: bool) {
        let _ = debug_enabled;
    }
}

/// Resolve the backend from the config's explicit choice or, for `auto`, from the
/// environment. Errors when the session is not Wayland or the choice is unavailable.
pub fn detect_backend(config: &Config) -> Result<BackendType> {
    if let Some(backend) = &config.backend {
        match backend {
            Backend::Auto => {
                if std::env::var("WAYLAND_DISPLAY").is_err() {
                    log_pipe!();
                    log_error!("sunsetr requires a Wayland session. WAYLAND_DISPLAY is not set.");
                    log_indented!("Please ensure you're running on a Wayland compositor.");
                    log_end!();
                    return Err(Silent.into());
                }

                if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
                    Ok(BackendType::Hyprland)
                } else {
                    Ok(BackendType::Wayland)
                }
            }
            Backend::Wayland => {
                if std::env::var("WAYLAND_DISPLAY").is_err() {
                    log_pipe!();
                    log_error!(
                        "Configuration specifies backend=\"wayland\" but WAYLAND_DISPLAY is not set."
                    );
                    log_indented!("Are you running on Wayland?");
                    log_end!();
                    return Err(Silent.into());
                }
                Ok(BackendType::Wayland)
            }
            Backend::Hyprland => {
                if std::env::var("WAYLAND_DISPLAY").is_err() {
                    log_pipe!();
                    log_error!(
                        "Configuration specifies backend=\"hyprland\" but WAYLAND_DISPLAY is not set."
                    );
                    log_indented!("Are you running on Wayland?");
                    log_end!();
                    return Err(Silent.into());
                }

                if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_err() {
                    log_pipe!();
                    log_error!(
                        "Configuration specifies backend=\"hyprland\" but you're not running on Hyprland."
                    );
                    log_block_start!("To fix this, either:");
                    log_indented!(
                        "• Switch to automatic detection: set backend=\"auto\" in sunsetr.toml"
                    );
                    log_indented!(
                        "• Use the Wayland backend: set backend=\"wayland\" in sunsetr.toml"
                    );
                    log_indented!("• Run sunsetr on Hyprland instead of your current compositor");
                    log_end!();
                    return Err(Silent.into());
                }

                Ok(BackendType::Hyprland)
            }
            Backend::Hyprsunset => {
                if std::env::var("WAYLAND_DISPLAY").is_err() {
                    log_pipe!();
                    log_error!(
                        "Configuration specifies backend=\"hyprsunset\" but WAYLAND_DISPLAY is not set."
                    );
                    log_indented!("Are you running on Wayland?");
                    log_end!();
                    return Err(Silent.into());
                }

                if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_err() {
                    log_pipe!();
                    log_error!(
                        "Configuration specifies backend=\"hyprsunset\" but you're not running on Hyprland."
                    );
                    log_block_start!("To fix this, either:");
                    log_indented!(
                        "• Switch to automatic detection: set backend=\"auto\" in sunsetr.toml"
                    );
                    log_indented!(
                        "• Use the Wayland backend: set backend=\"wayland\" in sunsetr.toml"
                    );
                    log_indented!("• Run sunsetr on Hyprland instead of your current compositor");
                    log_end!();
                    return Err(Silent.into());
                }

                Ok(BackendType::Hyprsunset)
            }
        }
    } else {
        if std::env::var("WAYLAND_DISPLAY").is_err() {
            log_pipe!();
            log_error!("sunsetr requires a Wayland session. WAYLAND_DISPLAY is not set.");
            log_indented!("Please ensure you're running on a Wayland compositor.");
            log_end!();
            return Err(Silent.into());
        }

        if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
            Ok(BackendType::Hyprland)
        } else {
            Ok(BackendType::Wayland)
        }
    }
}

/// Detect the current Wayland compositor.
///
/// Used to spawn processes as direct children of the compositor so parent-death
/// monitoring works correctly.
pub fn detect_compositor() -> Compositor {
    if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
        return Compositor::Hyprland;
    }

    if std::env::var("SWAYSOCK").is_ok() {
        return Compositor::Sway;
    }

    if let Ok(desktop) = std::env::var("XDG_CURRENT_DESKTOP") {
        match desktop.to_lowercase().as_str() {
            "niri" => return Compositor::Niri,
            "sway" => return Compositor::Sway,
            "hyprland" => return Compositor::Hyprland,
            _ => {}
        }
    }

    if let Ok(output) = std::process::Command::new("pgrep")
        .arg("-x")
        .arg("niri")
        .output()
        && output.status.success()
        && !output.stdout.is_empty()
    {
        return Compositor::Niri;
    }

    if let Ok(desktop) = std::env::var("XDG_CURRENT_DESKTOP") {
        Compositor::Other(desktop)
    } else {
        Compositor::Other("unknown".to_string())
    }
}

/// Create a boxed backend for the given type, initializing its connection or process.
pub fn create_backend(
    backend_type: BackendType,
    config: &Config,
    debug_enabled: bool,
    geo_times: Option<&crate::geo::times::GeoTimes>,
    initial_values: Option<(u32, f64)>, // Optional pre-calculated (temp, gamma) for optimization
) -> Result<Box<dyn ColorTemperatureBackend>> {
    match backend_type {
        BackendType::Hyprland => Ok(
            Box::new(hyprland::HyprlandBackend::new(config, debug_enabled)?)
                as Box<dyn ColorTemperatureBackend>,
        ),
        BackendType::Hyprsunset => {
            if let Some((temp, gamma)) = initial_values {
                Ok(
                    Box::new(hyprsunset::HyprsunsetBackend::new_with_initial_values(
                        debug_enabled,
                        temp,
                        gamma,
                    )?) as Box<dyn ColorTemperatureBackend>,
                )
            } else {
                Ok(Box::new(hyprsunset::HyprsunsetBackend::new(
                    config,
                    debug_enabled,
                    geo_times,
                )?) as Box<dyn ColorTemperatureBackend>)
            }
        }
        BackendType::Wayland => Ok(
            Box::new(wayland::WaylandBackend::new(config, debug_enabled)?)
                as Box<dyn ColorTemperatureBackend>,
        ),
    }
}

/// Enumeration of available backend types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    /// Native Hyprland backend using hyprland-ctm-control-v1 protocol
    Hyprland,
    /// Hyprsunset backend using the hyprsunset process
    Hyprsunset,
    /// Generic Wayland compositor using wlr-gamma-control-unstable-v1 protocol
    Wayland,
}

impl BackendType {
    pub fn name(&self) -> &'static str {
        match self {
            BackendType::Hyprland => "Hyprland",
            BackendType::Hyprsunset => "Hyprsunset",
            BackendType::Wayland => "Wayland",
        }
    }
}
