//! Native Hyprland backend implementation using hyprland-ctm-control-v1 protocol.
//!
//! Provides direct Color Transform Matrix (CTM) control for Hyprland without an external
//! process, reusing the shared gamma module's color science. Temperature and gamma are
//! combined into a single CTM matrix before each commit, which keeps all adjustments to one
//! smooth animation and avoids double-commit artifacts. Like the Wayland backend, it applies
//! the CTM to every output and handles outputs being added or removed.

use anyhow::Result;
use std::sync::atomic::AtomicBool;

use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    protocol::{wl_output::WlOutput, wl_registry::WlRegistry},
};

use crate::backend::ColorTemperatureBackend;
use crate::common::error::Silent;
use crate::config::Config;

use super::gamma;

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

/// Native Hyprland backend using hyprland-ctm-control-v1, driving Hyprland's built-in
/// CTM animation instead of an external process.
pub struct HyprlandBackend {
    _connection: Connection,
    event_queue: EventQueue<State>,
    state: State,
    debug_enabled: bool,
    current_temperature: u32,
    current_gamma_percent: f64,
    last_output_count: usize,
}

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
    /// Connect to the compositor, verify CTM protocol support, and enumerate outputs.
    pub fn new(_config: &Config, debug_enabled: bool) -> Result<Self> {
        log_decorated!("Initializing native Hyprland CTM backend...");

        let connection = Connection::connect_to_env()
            .map_err(|e| anyhow::anyhow!("Failed to connect to Wayland compositor: {}", e))?;

        let mut event_queue = connection.new_event_queue();
        let qh = event_queue.handle();

        let mut state = State::new(debug_enabled);

        let display = connection.display();
        let _registry = display.get_registry(&qh, ());

        // Initial roundtrip to receive globals
        event_queue.roundtrip(&mut state)?;

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
            return Err(Silent.into());
        }

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
            return Err(Silent.into());
        }

        if debug_enabled {
            log_pipe!();
            log_debug!("Found hyprland-ctm-control-v1 support");
        }

        // Second roundtrip: output names arrive via Name events
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

            let (r, g, b) = gamma::temperature_to_rgb(self.current_temperature);

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

            // Row-major 3x3 diagonal CTM: RGB scale factors on the diagonal
            let ctm = [
                r_adjusted, 0.0, 0.0, 0.0, g_adjusted, 0.0, 0.0, 0.0, b_adjusted,
            ];

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

            self.event_queue.roundtrip(&mut self.state)?;

            if self.debug_enabled {
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
        crate::core::period::log_state_announcement(runtime_state.period());

        if self.debug_enabled {
            log_pipe!();
            log_debug!("Applying Hyprland startup state...");
        }

        self.apply_transition_state(runtime_state, running)
    }

    fn apply_temperature_gamma(
        &mut self,
        temperature: u32,
        gamma: f64,
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
        self.event_queue.roundtrip(&mut self.state)?;
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

            if !self.state.outputs.is_empty() {
                self.apply_combined_ctm()?;
            }
        }

        Ok(())
    }

    fn cleanup(mut self: Box<Self>, debug_enabled: bool) {
        if debug_enabled {
            log_debug!("Native Hyprland backend shutting down");
        }

        // Destroy the CTM manager while the connection is alive so Hyprland animates back to identity
        self.state.ctm_manager = None;

        // Flush the destruction to the compositor
        let _ = self.event_queue.roundtrip(&mut self.state);

        // Let Hyprland start the identity animation before the connection closes
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

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
                    // Cap at v2: v2 adds the Blocked event for conflict detection
                    let manager_version = version.min(2);
                    let manager = registry.bind::<HyprlandCtmControlManagerV1, _, _>(
                        name,
                        manager_version,
                        qh,
                        (),
                    );
                    state.ctm_manager = Some(manager);
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
            // data is the registry name, used to match the right output
            if let Some(info) = state.outputs.iter_mut().find(|o| o.registry_name == *data) {
                let old_name = info.name.clone();
                info.name = name.clone();
                if old_name.starts_with("output-") && state.debug_enabled {
                    log_debug!("Output identified: {}", name);
                }
            }
        }
    }
}
