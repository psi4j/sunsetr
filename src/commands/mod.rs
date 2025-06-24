//! Command-line command handlers for sunsetr.
//!
//! This module contains implementations for one-shot CLI commands like --reload and --test.
//! Each command is implemented in its own submodule to keep the code organized and maintainable.

pub mod reload;
pub mod test;

use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;

use crate::backend::ColorTemperatureBackend;
use crate::config::Config;

// Re-export from signals for backward compatibility (used by signals module)
// pub use crate::signals::TestModeParams;

/// Apply specific temperature and gamma values across all available backends in parallel.
/// This is a reusable function that can work with any temperature/gamma combination.
/// Only attempts to use backends that are compatible with the current compositor.
pub fn apply_gamma_all_backends(
    temperature: u32,
    gamma: f32,
    debug_enabled: bool,
) -> (Result<()>, Result<()>) {
    use crate::backend::{detect_compositor, Compositor};
    
    // Load config for backend initialization
    let config = match Config::load() {
        Ok(cfg) => cfg,
        Err(e) => {
            return (
                Err(anyhow::anyhow!("Failed to load config: {}", e)),
                Err(anyhow::anyhow!("Failed to load config: {}", e)),
            );
        }
    };
    
    let running = Arc::new(AtomicBool::new(true));
    let current_compositor = detect_compositor();
    
    // Always try Wayland backend (works on all compositors)
    let wayland_handle = thread::spawn({
        let cfg = config.clone();
        let run = running.clone();
        move || {
            match crate::backend::wayland::WaylandBackend::new(&cfg, debug_enabled) {
                Ok(mut backend) => {
                    backend.apply_temperature_gamma(temperature, gamma, &run)
                }
                Err(e) => Err(e),
            }
        }
    });
    
    // Only try Hyprland backend if we're running on Hyprland
    let hyprland_result = if current_compositor == Compositor::Hyprland {
        let hyprland_handle = thread::spawn({
            let cfg = config.clone();
            let run = running.clone();
            move || {
                match crate::backend::hyprland::HyprlandBackend::new(&cfg, debug_enabled) {
                    Ok(mut backend) => {
                        backend.apply_temperature_gamma(temperature, gamma, &run)
                    }
                    Err(e) => Err(e),
                }
            }
        });
        hyprland_handle.join().unwrap_or_else(|_| Err(anyhow::anyhow!("Hyprland thread panicked")))
    } else {
        // Skip Hyprland backend on non-Hyprland compositors
        Err(anyhow::anyhow!("Skipped - Hyprland backend only works on Hyprland compositor"))
    };
    
    // Wait for Wayland thread and return both results
    let wayland_result = wayland_handle.join().unwrap_or_else(|_| Err(anyhow::anyhow!("Wayland thread panicked")));
    
    (wayland_result, hyprland_result)
}

/// Reset gamma to defaults (6500K, 100%) across all backends.
pub fn reset_all_gamma_parallel(debug_enabled: bool) -> (Result<()>, Result<()>) {
    apply_gamma_all_backends(6500, 100.0, debug_enabled)
}