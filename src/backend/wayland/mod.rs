//! Wayland backend implementation using wlr-gamma-control-unstable-v1 protocol.
//!
//! This module provides color temperature control for generic Wayland compositors
//! that support the wlr-gamma-control-unstable-v1 protocol. This includes most
//! wlroots-based compositors like Sway, river, Wayfire, and others.
//!
//! ## Protocol Implementation
//!
//! The backend implements the wlr-gamma-control-unstable-v1 Wayland protocol extension,
//! which provides direct access to display gamma/color temperature control without
//! requiring external helper processes.
//!
//! ## Color Science
//!
//! The module includes a sophisticated color science implementation (ported from wlsunset)
//! that performs proper colorimetric calculations:
//! - CIE XYZ color space transformations
//! - Planckian locus calculations for accurate color temperatures
//! - sRGB gamma correction and display-specific adjustments
//!
//! ## Output Management
//!
//! The backend automatically discovers and manages all connected Wayland outputs:
//! - Enumerates all available displays during initialization
//! - Applies gamma adjustments to all outputs simultaneously
//! - Handles dynamic output addition/removal events
//!
//! ## Error Handling
//!
//! The Wayland backend includes comprehensive error handling:
//! - Protocol negotiation failures
//! - Compositor compatibility detection
//! - Graceful fallback when gamma control is unavailable

use anyhow::Result;
use std::os::fd::AsFd;
use std::sync::atomic::AtomicBool;

use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    protocol::{wl_output::WlOutput, wl_registry::WlRegistry},
};
use wayland_protocols_wlr::gamma_control::v1::client::{
    zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1,
    zwlr_gamma_control_v1::{Event as GammaControlEvent, ZwlrGammaControlV1},
};

use crate::backend::ColorTemperatureBackend;
use crate::config::Config;
use crate::state::period::TimeState;

use super::gamma;

/// Wayland backend implementation using wlr-gamma-control-unstable-v1 protocol.
///
/// This backend provides color temperature control for generic Wayland compositors
/// that support the wlr-gamma-control-unstable-v1 protocol (most wlroots-based
/// compositors like Sway, river, Wayfire, etc.).
pub struct WaylandBackend {
    connection: Connection,
    event_queue: EventQueue<State>,
    state: State,
    debug_enabled: bool,
    // Track current desired values so hotplugged outputs can be applied immediately
    current_temperature: u32,
    current_gamma_percent: f32,
}

/// Information about a Wayland output and its gamma control
#[derive(Debug, Clone)]
struct OutputInfo {
    output: WlOutput,
    gamma_control: Option<ZwlrGammaControlV1>,
    gamma_size: Option<usize>,
    name: String,
    // When a new output appears or becomes ready (gamma_size known), we should apply current values
    needs_apply: bool,
    // Registry name for tracking removal
    registry_name: u32,
}

/// Application data for Wayland event handling
#[derive(Debug)]
struct State {
    gamma_manager: Option<ZwlrGammaControlManagerV1>,
    outputs: Vec<OutputInfo>,
    debug_enabled: bool,
}

impl State {
    fn new(debug_enabled: bool) -> Self {
        Self {
            gamma_manager: None,
            outputs: Vec::new(),
            debug_enabled,
        }
    }
}

impl WaylandBackend {
    /// Create a new Wayland backend instance.
    ///
    /// This function connects to the Wayland display server and negotiates
    /// the wlr-gamma-control-unstable-v1 protocol for gamma table control.
    ///
    /// # Arguments
    /// * `config` - Configuration containing Wayland-specific settings
    /// * `debug_enabled` - Whether to enable debug output for this backend
    ///
    /// # Returns
    /// A new WaylandBackend instance ready for use
    ///
    /// # Errors
    /// Returns an error if:
    /// - Not running on Wayland (WAYLAND_DISPLAY not set)
    /// - Compositor doesn't support wlr-gamma-control-unstable-v1
    /// - Failed to connect to Wayland display server
    /// - Permission denied for gamma control
    pub fn new(_config: &Config, debug_enabled: bool) -> Result<Self> {
        // Verify we're running on Wayland
        if std::env::var("WAYLAND_DISPLAY").is_err() {
            log_error_exit!("WAYLAND_DISPLAY is not set. Are you running on Wayland?");
            std::process::exit(1);
        }

        log_decorated!("Initializing Wayland gamma control backend...");

        // Connect to Wayland display
        let connection = Connection::connect_to_env()
            .map_err(|e| anyhow::anyhow!("Failed to connect to Wayland display: {}", e))?;

        let display = connection.display();

        // Create event queue
        let mut event_queue = connection.new_event_queue();
        let qh = event_queue.handle();

        // Initialize app data
        let mut state = State::new(debug_enabled);

        // Get the registry to enumerate globals
        let _registry = display.get_registry(&qh, ());

        // Dispatch events until we have all the protocols we need
        // This may take multiple dispatch rounds
        for _ in 0..10 {
            // Maximum 10 rounds to avoid infinite loops
            event_queue.blocking_dispatch(&mut state)?;

            // Check if we have what we need
            if state.gamma_manager.is_some() && !state.outputs.is_empty() {
                break;
            }
        }

        // Check if we have the gamma control manager
        if state.gamma_manager.is_none() {
            log_pipe!();
            log_error!("Compositor does not support wlr-gamma-control-unstable-v1 protocol.");
            log_indented!("This is required for color temperature control on Wayland.");
            log_block_start!("Supported compositors include:");
            log_indented!("• Hyprland, niri, Sway, river, Wayfire, labwc");
            log_indented!("• Other wlroots-based compositors");
            log_block_start!("Unsupported compositors:");
            log_indented!("• KWin (KDE), Mutter (GNOME)");
            log_pipe!();
            log_block_start!("For Hyprland, you can use backend=\"hyprland\".");
            log_end!();
            std::process::exit(1);
        }

        if debug_enabled {
            log_pipe!();
            log_debug!("Found wlr-gamma-control-unstable-v1 support");
        }

        // Enumerate outputs and create gamma controls
        Self::setup_gamma_controls(&mut state, &qh)?;

        // Dispatch events to process potential gamma_size events from the compositor
        // This ensures that the gamma_size is populated before we proceed.
        event_queue.roundtrip(&mut state).map_err(|e| {
            log_pipe!();
            anyhow::anyhow!(
                "Failed during roundtrip after setting up gamma controls: {}",
                e
            )
        })?;

        if state.outputs.is_empty() {
            log_pipe!();
            log_error!("No outputs found for gamma control");
            log_end!();
            std::process::exit(1);
        }

        if debug_enabled {
            log_debug!(
                "Initialized gamma control for {} output(s)",
                state.outputs.len()
            );
        }

        Ok(Self {
            connection,
            event_queue,
            state,
            debug_enabled,
            current_temperature: 6500,
            current_gamma_percent: 100.0,
        })
    }

    /// Set up gamma controls for all available outputs
    fn setup_gamma_controls(state: &mut State, qh: &QueueHandle<State>) -> Result<()> {
        if let Some(ref manager) = state.gamma_manager {
            for output_info in &mut state.outputs {
                // Only set up gamma control if it doesn't already exist
                if output_info.gamma_control.is_none() {
                    let gamma_control = manager.get_gamma_control(&output_info.output, qh, ());
                    output_info.gamma_control = Some(gamma_control);
                    // gamma_size will arrive via GammaSize event shortly
                    // Request immediate apply once size is known
                    output_info.needs_apply = true;
                }
            }
        }
        Ok(())
    }

    /// Apply gamma tables to outputs that have needs_apply flag set
    /// For scheduled transitions: Set all outputs' needs_apply=true before calling
    /// For hotplug events: Only new outputs have needs_apply=true
    fn apply_gamma_to_outputs(&mut self, temperature: u32, gamma: f32) -> Result<()> {
        // Use state.outputs which has the latest gamma control information
        if self.debug_enabled {
            log_pipe!();
            log_debug!("Total outputs: {}", self.state.outputs.len());
        }

        // Collect outputs that need updating
        let outputs_to_update: Vec<_> = self
            .state
            .outputs
            .iter()
            .filter(|o| o.needs_apply)
            .map(|o| o.name.clone())
            .collect();

        if outputs_to_update.is_empty() {
            return Ok(());
        }

        // Log consolidated info about the gamma application (similar to Hyprland backend)
        if self.debug_enabled {
            log_pipe!();
            log_debug!("Applying gamma to {} output(s)", outputs_to_update.len());
            log_decorated!("Creating gamma tables...");
            log_indented!(
                "temp={}K, gamma={:.0}%, RGB factors={:?}",
                temperature,
                gamma * 100.0,
                gamma::get_rgb_factors(temperature) // We'll need to expose this function
            );
        }

        // Generate gamma tables once (they're the same for all outputs)
        let gamma_size = self
            .state
            .outputs
            .iter()
            .find(|o| o.gamma_size.is_some())
            .and_then(|o| o.gamma_size)
            .unwrap_or(1024); // Default size if somehow missing

        // Generate gamma tables with debug output passed through
        let gamma_data =
            gamma::create_gamma_tables(gamma_size, temperature, gamma, self.debug_enabled)?;

        if self.debug_enabled {
            log_decorated!("Setting gamma via Wayland protocol");
        }

        // Keep temp files alive until after event dispatch
        let mut temp_files = Vec::new();
        let mut successful_outputs = Vec::new();
        let mut failed_outputs = Vec::new();

        for output_info in self.state.outputs.iter_mut() {
            // Skip outputs that don't need updating
            if !output_info.needs_apply {
                continue;
            }

            if let (Some(gamma_control), Some(_gamma_size)) =
                (&output_info.gamma_control, output_info.gamma_size)
            {
                // Create temporary file for gamma data
                let mut temp_file = tempfile::tempfile()
                    .map_err(|e| anyhow::anyhow!("Failed to create temporary file: {}", e))?;

                // Write gamma data to file
                std::io::Write::write_all(&mut temp_file, &gamma_data)
                    .map_err(|e| anyhow::anyhow!("Failed to write gamma data: {}", e))?;

                // Flush to ensure data is written
                std::io::Write::flush(&mut temp_file)
                    .map_err(|e| anyhow::anyhow!("Failed to flush gamma data: {}", e))?;

                // CRITICAL: Reset file position to beginning before sending to compositor
                // This was the bug - compositor reads from current position, which was at EOF
                std::io::Seek::seek(&mut temp_file, std::io::SeekFrom::Start(0))
                    .map_err(|e| anyhow::anyhow!("Failed to reset file position: {}", e))?;

                // Set gamma table
                gamma_control.set_gamma(temp_file.as_fd());

                // Keep the temp file alive until after event dispatch
                temp_files.push(temp_file);
                successful_outputs.push(output_info.name.clone());
            } else {
                failed_outputs.push(output_info.name.clone());
                if self.debug_enabled {
                    log_warning!(
                        "Failed to apply gamma to '{}' - gamma_control: {}, gamma_size: {:?}",
                        output_info.name,
                        output_info.gamma_control.is_some(),
                        output_info.gamma_size
                    );
                }
            }
        }

        // Use dispatch_pending instead of blocking_dispatch to avoid hanging
        // This processes any pending events without blocking
        match self.event_queue.dispatch_pending(&mut self.state) {
            Ok(_) => {}
            Err(e) => {
                if self.debug_enabled {
                    log_warning!("Wayland event dispatch failed: {e}");
                }
                // Don't fail the whole operation just because of event dispatch issues
            }
        }

        // Try a roundtrip to ensure the compositor processes the gamma tables
        match self.connection.roundtrip() {
            Ok(_) => {
                // Mark outputs as applied after successful roundtrip
                for output in &mut self.state.outputs {
                    if output.needs_apply {
                        output.needs_apply = false;
                    }
                }
            }
            Err(e) => {
                if self.debug_enabled {
                    log_warning!("Roundtrip failed: {e}");
                }
            }
        }

        // Log consolidated success message (similar to Hyprland backend)
        if !successful_outputs.is_empty() {
            if self.debug_enabled {
                log_debug!(
                    "Applied gamma to outputs: {}",
                    successful_outputs.join(", ")
                );
            }
        } else if self.debug_enabled && !failed_outputs.is_empty() {
            log_warning!("No outputs were available for gamma control");
        }

        // Now temp files can be dropped
        drop(temp_files);
        Ok(())
    }
}

impl ColorTemperatureBackend for WaylandBackend {
    fn poll_hotplug(&mut self) -> Result<()> {
        // Store initial state for comparison
        let initial_count = self.state.outputs.len();

        // Use roundtrip to ensure we receive and process any new output events from the compositor
        // This will:
        // 1. Send a sync request to the server
        // 2. Flush any pending requests
        // 3. Read all events from the socket
        // 4. Dispatch them (including any new output announcements and removals)
        // 5. Wait for the sync reply, ensuring all events are processed
        //
        // This is the key to receiving hotplug events - we need to actively read from the socket
        // The GlobalRemove events will be handled by our Dispatch implementation
        let _ = self.event_queue.roundtrip(&mut self.state);

        // Check if output count changed
        let current_count = self.state.outputs.len();
        if current_count != initial_count && self.debug_enabled {
            log_indented!(
                "Output count changed: {} -> {}",
                initial_count,
                current_count
            );
        }

        // Check for new outputs that need gamma controls set up
        let needs_setup = self.state.outputs.iter().any(|o| o.gamma_control.is_none());

        if needs_setup {
            if self.debug_enabled {
                let new_outputs: Vec<_> = self
                    .state
                    .outputs
                    .iter()
                    .filter(|o| o.gamma_control.is_none())
                    .map(|o| o.name.as_str())
                    .collect();
                log_debug!(
                    "Setting up gamma controls for new outputs: {:?}",
                    new_outputs
                );
            }

            // Set up gamma controls for new outputs
            let qh = self.event_queue.handle();
            Self::setup_gamma_controls(&mut self.state, &qh)?;

            // Process gamma size events
            let _ = self.event_queue.roundtrip(&mut self.state);
        }

        // Check if any outputs need gamma applied
        let needs_any_apply = self
            .state
            .outputs
            .iter()
            .any(|o| o.gamma_control.is_some() && o.gamma_size.is_some() && o.needs_apply);

        if needs_any_apply {
            if self.debug_enabled {
                log_indented!("Applying gamma to newly connected output(s)");
            }

            // Apply currently desired values to all outputs
            let temp = self.current_temperature;
            let gamma_pct = self.current_gamma_percent;
            self.apply_gamma_to_outputs(temp, gamma_pct / 100.0)?;
        }
        Ok(())
    }

    fn apply_transition_state(
        &mut self,
        state: TimeState,
        config: &Config,
        _running: &AtomicBool,
    ) -> Result<()> {
        let (temp, gamma) = state.values(config);
        if self.debug_enabled {
            log_pipe!();
            log_debug!("Wayland backend applying state: temp={temp}K, gamma={gamma:.1}%");
        }
        // Remember current desired values for hotplug handling
        self.current_temperature = temp;
        self.current_gamma_percent = gamma;

        // For scheduled transitions, update all outputs
        for output in &mut self.state.outputs {
            output.needs_apply = true;
        }

        self.apply_gamma_to_outputs(temp, gamma / 100.0) // Convert percentage to 0.0-1.0
    }

    fn apply_startup_state(
        &mut self,
        state: TimeState,
        config: &Config,
        running: &AtomicBool,
    ) -> Result<()> {
        // First announce what mode we're entering (like Hyprland backend)
        crate::state::period::log_state_announcement(state);

        if self.debug_enabled {
            log_pipe!();
            log_debug!("Applying Wayland startup state...");
        }

        // Apply the state
        self.apply_transition_state(state, config, running)
    }

    fn apply_temperature_gamma(
        &mut self,
        temperature: u32,
        gamma: f32,
        _running: &AtomicBool,
    ) -> Result<()> {
        // Remember current desired values for hotplug handling
        self.current_temperature = temperature;
        self.current_gamma_percent = gamma;

        // For test mode, update all outputs
        for output in &mut self.state.outputs {
            output.needs_apply = true;
        }

        self.apply_gamma_to_outputs(temperature, gamma / 100.0) // Convert percentage to 0.0-1.0
    }

    fn backend_name(&self) -> &'static str {
        "Wayland"
    }
}

// Implement Dispatch traits for Wayland protocol handling
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
                match interface.as_str() {
                    "zwlr_gamma_control_manager_v1" => {
                        let manager =
                            registry.bind::<ZwlrGammaControlManagerV1, _, _>(name, version, qh, ());
                        state.gamma_manager = Some(manager);
                    }
                    "wl_output" => {
                        let output = registry.bind::<WlOutput, _, _>(name, version, qh, ());
                        // Use a placeholder name until we get the real name from the Name event
                        let output_name = format!("output-{name}");
                        state.outputs.push(OutputInfo {
                            output,
                            gamma_control: None,
                            gamma_size: None,
                            name: output_name,
                            needs_apply: true,
                            registry_name: name,
                        });
                    }
                    _ => {}
                }
            }
            Event::GlobalRemove { name } => {
                // Remove the output that was unplugged
                let before_count = state.outputs.len();
                state.outputs.retain(|output_info| {
                    if output_info.registry_name == name {
                        if state.debug_enabled {
                            log_debug!("Output removed: {}", output_info.name);
                        }
                        false
                    } else {
                        true
                    }
                });
                if state.outputs.len() != before_count {
                    #[cfg(debug_assertions)]
                    log_debug!(
                        "Removed output with registry name {name}, {} outputs remaining",
                        state.outputs.len()
                    );
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrGammaControlManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwlrGammaControlManagerV1,
        _: <ZwlrGammaControlManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // No events for the manager
    }
}

impl Dispatch<ZwlrGammaControlV1, ()> for State {
    fn event(
        state: &mut Self,
        gamma_control: &ZwlrGammaControlV1,
        event: GammaControlEvent,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            GammaControlEvent::GammaSize { size } => {
                // Find the output this belongs to and set the gamma size
                for output_info in &mut state.outputs {
                    if let Some(ref control) = output_info.gamma_control
                        && control == gamma_control
                    {
                        output_info.gamma_size = Some(size as usize);
                        // Mark that this output is now ready for an initial apply
                        output_info.needs_apply = true;
                        // Only log gamma size in debug builds or when explicitly enabled
                        #[cfg(debug_assertions)]
                        log_decorated!("Output '{}' gamma size: {}", output_info.name, size);
                        break;
                    }
                }
            }
            GammaControlEvent::Failed => {
                // This is critical - the compositor rejected our gamma control
                // Mark the gamma control as failed by removing it
                for output_info in &mut state.outputs {
                    if let Some(ref control) = output_info.gamma_control
                        && control == gamma_control
                    {
                        if state.debug_enabled {
                            log_pipe!();
                            log_warning!(
                                "Gamma control failed for output '{}' - removing stale control",
                                output_info.name
                            );
                        }
                        // Clear the failed gamma control so we can try to recreate it
                        output_info.gamma_control = None;
                        output_info.gamma_size = None;
                        output_info.needs_apply = true;
                        break;
                    }
                }
            }
            _ => {
                log_decorated!("Received unknown gamma control event: {event:?}");
            }
        }
    }
}

impl Dispatch<WlOutput, ()> for State {
    fn event(
        state: &mut Self,
        output: &WlOutput,
        event: <WlOutput as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_output::Event;

        if let Event::Name { name } = event {
            // Update output name with the real name
            for output_info in &mut state.outputs {
                if &output_info.output == output {
                    let old_name = output_info.name.clone();
                    output_info.name = name.clone();
                    // Log when we discover a new output (not during initialization)
                    if old_name.starts_with("output-") && state.debug_enabled {
                        log_debug!("Output identified: {}", name);
                    }
                    break;
                }
            }
        }
    }
}
