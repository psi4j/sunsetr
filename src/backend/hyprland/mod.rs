//! Native Hyprland backend implementation using hyprland-ctm-control-v1 protocol.
//!
//! This module provides direct Color Transform Matrix (CTM) control for Hyprland
//! using the hyprland-ctm-control-v1 protocol without requiring external processes.
//!
//! ## Protocol Implementation
//!
//! The backend implements the hyprland-ctm-control-v1 Wayland protocol extension,
//! which provides direct CTM control for more precise color adjustments than
//! traditional gamma ramps.
//!
//! ## Color Science
//!
//! This backend reuses the sophisticated color science from the shared gamma module
//! (originally from wlsunset) for consistent color calculations across backends.
//!
//! ## CTM State Management
//!
//! The backend maintains both temperature and gamma state internally, combining
//! them into a single CTM matrix before any protocol communication. This ensures:
//! - Single smooth animation for all adjustments
//! - No double-commit issues
//! - Consistent visual transitions
//!
//! ## Output Management
//!
//! Similar to the Wayland backend, this implementation:
//! - Enumerates all available displays during initialization
//! - Applies CTM to all outputs simultaneously
//! - Handles dynamic output addition/removal

use anyhow::Result;
use std::sync::atomic::AtomicBool;

use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    protocol::{wl_output::WlOutput, wl_registry::WlRegistry},
};

use crate::backend::ColorTemperatureBackend;
use crate::config::Config;

use super::gamma;

// Generate the protocol bindings
pub mod protocol {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("src/backend/hyprland/hyprland-ctm-control-v1.xml");
    }

    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("src/backend/hyprland/hyprland-ctm-control-v1.xml");
}

use self::protocol::hyprland_ctm_control_manager_v1::HyprlandCtmControlManagerV1;

/// Native Hyprland backend using hyprland-ctm-control-v1 protocol.
///
/// This backend provides direct CTM control without external processes,
/// offering smooth animations via Hyprland's built-in CTM animation system.
pub struct HyprlandBackend {
    _connection: Connection,
    event_queue: EventQueue<State>,
    state: State,
    debug_enabled: bool,
    // Track current values for state management
    current_temperature: u32,
    current_gamma_percent: f32,
    // Track output count to detect hotplug changes
    last_output_count: usize,
}

/// Information about a Wayland output
#[derive(Debug, Clone)]
struct OutputInfo {
    output: WlOutput,
    name: String,
    registry_name: u32,
}

/// State for Wayland event handling
#[derive(Debug)]
struct State {
    ctm_manager: Option<HyprlandCtmControlManagerV1>,
    outputs: Vec<OutputInfo>,
    debug_enabled: bool,
    is_blocked: bool,
}

impl State {
    fn new(debug_enabled: bool) -> Self {
        Self {
            ctm_manager: None,
            outputs: Vec::new(),
            debug_enabled,
            is_blocked: false,
        }
    }
}

impl HyprlandBackend {
    /// Create a new Hyprland backend instance.
    ///
    /// This will connect to the Wayland compositor, verify CTM protocol support,
    /// and enumerate all available outputs.
    pub fn new(_config: &Config, debug_enabled: bool) -> Result<Self> {
        log_decorated!("Initializing native Hyprland CTM backend...");

        // Connect to Wayland compositor
        let connection = Connection::connect_to_env()
            .map_err(|e| anyhow::anyhow!("Failed to connect to Wayland compositor: {}", e))?;

        let mut event_queue = connection.new_event_queue();
        let qh = event_queue.handle();

        let mut state = State::new(debug_enabled);

        // Get the registry and bind to it
        let display = connection.display();
        let _registry = display.get_registry(&qh, ());

        // Initial roundtrip to receive globals
        event_queue.roundtrip(&mut state)?;

        // Check if we have the CTM manager
        if state.ctm_manager.is_none() {
            log_pipe!();
            log_error!("hyprland-ctm-control-v1 protocol not available.");
            log_indented!(
                "The native Hyprland backend requires Hyprland with CTM protocol support."
            );
            log_pipe!();
            log_block_start!("To fix this:");
            log_indented!("• Update to a newer version of Hyprland that supports CTM protocol");
            log_indented!("• Use backend=\"wayland\" for wlr-gamma-control instead");
            log_indented!("• Use backend=\"hyprsunset\" for the legacy hyprsunset backend");
            log_end!();
            std::process::exit(1);
        }

        // Check if we're blocked by another CTM manager
        if state.is_blocked {
            log_pipe!();
            log_error!("Another CTM manager is already active.");
            log_block_start!("This could be:");
            log_indented!("• hyprsunset running (check systemd services or processes)");
            log_indented!("• Another instance of sunsetr");
            log_indented!("• hyprland-ctm-vibrance or similar tools");
            log_pipe!();
            log_indented!("Please stop the conflicting tool and try again.");
            log_end!();
            std::process::exit(1);
        }

        if debug_enabled {
            log_pipe!();
            log_debug!("Found hyprland-ctm-control-v1 support");
        }

        // Do another roundtrip to get output names (they'll be logged via Name events)
        event_queue.roundtrip(&mut state)?;

        if debug_enabled {
            log_debug!(
                "Initialized CTM control for {} output(s)",
                state.outputs.len()
            );
        }

        let output_count = state.outputs.len();
        Ok(Self {
            _connection: connection,
            event_queue,
            state,
            debug_enabled,
            current_temperature: 6500,
            current_gamma_percent: 100.0,
            last_output_count: output_count,
        })
    }

    /// Apply the combined CTM based on current temperature and gamma
    fn apply_combined_ctm(&mut self) -> Result<()> {
        if let Some(ref manager) = self.state.ctm_manager {
            if self.debug_enabled {
                log_pipe!();
                log_debug!("Total outputs: {}", self.state.outputs.len());
                log_pipe!();
                log_debug!("Applying CTM to all outputs");
            }

            // Calculate RGB multipliers from temperature
            let (r, g, b) = gamma::temperature_to_rgb(self.current_temperature);

            // Apply gamma adjustment (convert percentage to ratio)
            let gamma_ratio = self.current_gamma_percent / 100.0;
            let r_adjusted = r * gamma_ratio;
            let g_adjusted = g * gamma_ratio;
            let b_adjusted = b * gamma_ratio;

            if self.debug_enabled {
                log_decorated!("Creating CTM matrix...");
                log_indented!(
                    "temp={}K, gamma={:.0}%, RGB factors=({:.3}, {:.3}, {:.3})",
                    self.current_temperature,
                    self.current_gamma_percent,
                    r,
                    g,
                    b
                );
                log_decorated!("CTM matrix (3x3):");
                log_indented!("[{:.3}  0.000  0.000]", r_adjusted);
                log_indented!("[0.000  {:.3}  0.000]", g_adjusted);
                log_indented!("[0.000  0.000  {:.3}]", b_adjusted);
            }

            // Create CTM matrix (diagonal matrix with RGB values)
            // CTM is row-major 3x3 matrix
            // Convert to f64 for the protocol
            let ctm = [
                r_adjusted as f64,
                0.0,
                0.0,
                0.0,
                g_adjusted as f64,
                0.0,
                0.0,
                0.0,
                b_adjusted as f64,
            ];

            // Set CTM for all outputs
            if self.debug_enabled {
                log_decorated!("Setting CTM via Hyprland protocol");
            }

            for output_info in &self.state.outputs {
                manager.set_ctm_for_output(
                    &output_info.output,
                    ctm[0],
                    ctm[1],
                    ctm[2],
                    ctm[3],
                    ctm[4],
                    ctm[5],
                    ctm[6],
                    ctm[7],
                    ctm[8],
                );
            }

            // Commit all changes atomically
            manager.commit();

            // Process events
            self.event_queue.roundtrip(&mut self.state)?;

            if self.debug_enabled {
                // Log the outputs we applied to
                let output_names: Vec<&str> =
                    self.state.outputs.iter().map(|o| o.name.as_str()).collect();
                log_debug!("Applied CTM to outputs: {}", output_names.join(", "));
            }
        }

        Ok(())
    }
}

impl ColorTemperatureBackend for HyprlandBackend {
    fn apply_transition_state(
        &mut self,
        runtime_state: &crate::core::runtime_state::RuntimeState,
        _running: &AtomicBool,
    ) -> Result<()> {
        let (temp, gamma) = runtime_state.values();
        self.current_temperature = temp;
        self.current_gamma_percent = gamma;

        if self.debug_enabled {
            log_pipe!();
            log_debug!(
                "Hyprland backend applying state: temp={}K, gamma={:.1}%",
                temp,
                gamma
            );
        }

        self.apply_combined_ctm()
    }

    fn apply_startup_state(
        &mut self,
        runtime_state: &crate::core::runtime_state::RuntimeState,
        running: &AtomicBool,
    ) -> Result<()> {
        // First announce what mode we're entering (like Wayland backend)
        crate::core::period::log_state_announcement(runtime_state.period());

        if self.debug_enabled {
            log_pipe!();
            log_debug!("Applying Hyprland startup state...");
        }

        // Apply the state (Hyprland's CTM animation will handle smoothness if configured)
        self.apply_transition_state(runtime_state, running)
    }

    fn apply_temperature_gamma(
        &mut self,
        temperature: u32,
        gamma: f32,
        _running: &AtomicBool,
    ) -> Result<()> {
        self.current_temperature = temperature;
        self.current_gamma_percent = gamma;
        self.apply_combined_ctm()
    }

    fn backend_name(&self) -> &'static str {
        "Hyprland"
    }

    fn poll_hotplug(&mut self) -> Result<()> {
        // Re-enumerate outputs
        self.event_queue.roundtrip(&mut self.state)?;

        // Only reapply CTM if output count changed (hotplug event)
        let current_output_count = self.state.outputs.len();
        if current_output_count != self.last_output_count {
            if self.debug_enabled {
                log_debug!(
                    "Output count changed: {} -> {}",
                    self.last_output_count,
                    current_output_count
                );
            }
            self.last_output_count = current_output_count;

            // Apply current CTM to all outputs (including new ones)
            if !self.state.outputs.is_empty() {
                self.apply_combined_ctm()?;
            }
        }

        Ok(())
    }

    fn cleanup(mut self: Box<Self>, debug_enabled: bool) {
        // Destroy the CTM manager explicitly before closing the connection
        // This ensures Hyprland can properly animate the transition back to identity
        if debug_enabled {
            log_debug!("Native Hyprland backend shutting down");
        }

        // Explicitly destroy the CTM manager while the connection is still alive
        // This triggers Hyprland's animation to identity matrix
        self.state.ctm_manager = None;

        // Flush and wait for the destruction to be processed
        let _ = self.event_queue.roundtrip(&mut self.state);

        // Give Hyprland a moment to start the animation before closing the connection
        // This mimics the slight delay that happens when hyprsunset process terminates
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Now the connection can be closed
    }
}

// Wayland protocol event handling

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: <WlRegistry as Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_registry::Event;

        match event {
            Event::Global {
                name,
                interface,
                version,
            } => {
                if interface == "hyprland_ctm_control_manager_v1" {
                    // Bind to the CTM manager, prefer v2 for conflict detection
                    let manager_version = version.min(2);
                    let manager = registry.bind::<HyprlandCtmControlManagerV1, _, _>(
                        name,
                        manager_version,
                        qh,
                        (),
                    );
                    state.ctm_manager = Some(manager);
                    // Don't log binding details - we'll announce support later
                } else if interface == "wl_output" {
                    let output = registry.bind::<WlOutput, _, _>(name, version.min(4), qh, name);

                    state.outputs.push(OutputInfo {
                        output,
                        name: format!("output-{}", name),
                        registry_name: name,
                    });
                }
            }
            Event::GlobalRemove { name } => {
                // Remove output if it was unregistered
                state.outputs.retain(|o| o.registry_name != name);
            }
            _ => {}
        }
    }
}

impl Dispatch<HyprlandCtmControlManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &HyprlandCtmControlManagerV1,
        event: <HyprlandCtmControlManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use self::protocol::hyprland_ctm_control_manager_v1::Event;

        match event {
            Event::Blocked => {
                state.is_blocked = true;
                if state.debug_enabled {
                    log_warning!("CTM manager blocked by another instance");
                }
            }
        }
    }
}

impl Dispatch<WlOutput, u32> for State {
    fn event(
        state: &mut Self,
        _output: &WlOutput,
        event: <WlOutput as Proxy>::Event,
        data: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_output::Event;

        if let Event::Name { name } = event {
            // Update output name if we receive it
            // Use the registry name (data) to find the right output
            if let Some(info) = state.outputs.iter_mut().find(|o| o.registry_name == *data) {
                let old_name = info.name.clone();
                info.name = name.clone();
                // Log when we discover a new output (matching Wayland backend pattern)
                if old_name.starts_with("output-") && state.debug_enabled {
                    log_debug!("Output identified: {}", name);
                }
            }
        }
    }
}
