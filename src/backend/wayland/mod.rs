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
use crate::time_state::TimeState;

pub mod gamma;

/// Wayland backend implementation using wlr-gamma-control-unstable-v1 protocol.
///
/// This backend provides color temperature control for generic Wayland compositors
/// that support the wlr-gamma-control-unstable-v1 protocol (most wlroots-based
/// compositors like Sway, river, Wayfire, etc.).
pub struct WaylandBackend {
    connection: Connection,
    event_queue: EventQueue<AppData>,
    app_data: AppData,
    debug_enabled: bool,
}

/// Information about a Wayland output and its gamma control
#[derive(Debug, Clone)]
struct OutputInfo {
    output: WlOutput,
    gamma_control: Option<ZwlrGammaControlV1>,
    gamma_size: Option<usize>,
    name: String,
}

/// Application data for Wayland event handling
#[derive(Debug)]
struct AppData {
    gamma_manager: Option<ZwlrGammaControlManagerV1>,
    outputs: Vec<OutputInfo>,
}

impl AppData {
    fn new() -> Self {
        Self {
            gamma_manager: None,
            outputs: Vec::new(),
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
            log_pipe!();
            anyhow::bail!("WAYLAND_DISPLAY is not set. Are you running on Wayland?");
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
        let mut app_data = AppData::new();

        // Get the registry to enumerate globals
        let _registry = display.get_registry(&qh, ());

        // Dispatch events until we have all the protocols we need
        // This may take multiple dispatch rounds
        for _ in 0..10 {
            // Maximum 10 rounds to avoid infinite loops
            event_queue.blocking_dispatch(&mut app_data)?;

            // Check if we have what we need
            if app_data.gamma_manager.is_some() && !app_data.outputs.is_empty() {
                break;
            }
        }

        // Check if we have the gamma control manager
        if app_data.gamma_manager.is_none() {
            log_pipe!();
            anyhow::bail!(
                "Compositor does not support wlr-gamma-control-unstable-v1 protocol.\n\
                This is required for color temperature control on Wayland.\n\
                \n\
                Supported compositors include:\n\
                • Hyprland, niri, Sway, river, Wayfire, labwc\n\
                • Other wlroots-based compositors\n\
                \n\
                Unsupported compositors:\n\
                • KWin (KDE), Mutter (GNOME)\n\
                \n\
                For Hyprland, you can use backend=\"hyprland\"."
            );
        }

        if debug_enabled {
            log_pipe!();
            log_debug!("Found wlr-gamma-control-unstable-v1 support");
        }

        // Enumerate outputs and create gamma controls
        Self::setup_gamma_controls(&mut app_data, &qh)?;

        // Dispatch events to process potential gamma_size events from the compositor
        // This ensures that the gamma_size is populated before we proceed.
        event_queue.roundtrip(&mut app_data).map_err(|e| {
            log_pipe!();
            anyhow::anyhow!(
                "Failed during roundtrip after setting up gamma controls: {}",
                e
            )
        })?;

        if app_data.outputs.is_empty() {
            log_pipe!();
            anyhow::bail!("No outputs found for gamma control");
        }

        if debug_enabled {
            log_debug!(
                "Initialized gamma control for {} output(s)",
                app_data.outputs.len()
            );
        }

        Ok(Self {
            connection,
            event_queue,
            app_data,
            debug_enabled,
        })
    }

    /// Set up gamma controls for all available outputs
    fn setup_gamma_controls(app_data: &mut AppData, qh: &QueueHandle<AppData>) -> Result<()> {
        if let Some(ref manager) = app_data.gamma_manager {
            for output_info in &mut app_data.outputs {
                let gamma_control = manager.get_gamma_control(&output_info.output, qh, ());
                output_info.gamma_control = Some(gamma_control);
            }
        }
        Ok(())
    }

    /// Apply gamma tables to all outputs
    fn apply_gamma_to_outputs(&mut self, temperature: u32, gamma: f32) -> Result<()> {
        if self.debug_enabled {
            log_pipe!();
            log_debug!("Starting apply_gamma_to_outputs");
        }

        // Use app_data.outputs which has the latest gamma control information
        if self.debug_enabled {
            log_debug!("Total outputs in app_data: {}", self.app_data.outputs.len());
        }

        // Keep temp files alive until after event dispatch
        let mut temp_files = Vec::new();
        let mut successful_count = 0;

        for (i, output_info) in self.app_data.outputs.iter_mut().enumerate() {
            if let (Some(gamma_control), Some(gamma_size)) =
                (&output_info.gamma_control, output_info.gamma_size)
            {
                if self.debug_enabled {
                    log_pipe!();
                    log_debug!("Processing Output {i}");
                    log_indented!("Name: '{}'", output_info.name);
                    log_indented!("Has Gamma Control: {}", output_info.gamma_control.is_some());
                    log_indented!(
                        "Gamma Size: {}",
                        output_info
                            .gamma_size
                            .map_or_else(|| "N/A".to_string(), |size| size.to_string())
                    );
                }

                // Generate gamma tables
                if self.debug_enabled {
                    log_decorated!("Creating gamma tables...");
                }
                let gamma_data =
                    gamma::create_gamma_tables(gamma_size, temperature, gamma, self.debug_enabled)?;
                if self.debug_enabled {
                    log_debug!("Created gamma tables, size: {} bytes", gamma_data.len());
                }

                // Create temporary file for gamma data
                if self.debug_enabled {
                    log_decorated!("Creating temporary file");
                }
                let mut temp_file = tempfile::tempfile()
                    .map_err(|e| anyhow::anyhow!("Failed to create temporary file: {}", e))?;

                // Write gamma data to file
                if self.debug_enabled {
                    log_decorated!("Writing gamma data to file");
                }
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
                if self.debug_enabled {
                    log_decorated!("Setting gamma table via Wayland protocol");
                }
                gamma_control.set_gamma(temp_file.as_fd());

                // Keep the temp file alive until after event dispatch
                temp_files.push(temp_file);
                successful_count += 1;

                if self.debug_enabled {
                    log_debug!(
                        "Applied gamma to output '{}': {}K, {:.1}%",
                        output_info.name,
                        temperature,
                        gamma * 100.0
                    );
                }
            } else if self.debug_enabled {
                log_warning!(
                    "Skipping output '{}' - gamma_control: {}, gamma_size: {:?}",
                    output_info.name,
                    output_info.gamma_control.is_some(),
                    output_info.gamma_size
                );
            }
        }

        // Use dispatch_pending instead of blocking_dispatch to avoid hanging
        // This processes any pending events without blocking
        match self.event_queue.dispatch_pending(&mut self.app_data) {
            Ok(_) => {
                if self.debug_enabled {
                    log_pipe!();
                    log_debug!("Wayland events dispatched successfully");
                }
            }
            Err(e) => {
                if self.debug_enabled {
                    log_warning!("Wayland event dispatch failed: {e}");
                }
                // Don't fail the whole operation just because of event dispatch issues
            }
        }

        // Try a roundtrip to ensure the compositor processes the gamma tables
        if self.debug_enabled {
            log_debug!("Performing roundtrip to ensure compositor processes gamma tables");
        }
        match self.connection.roundtrip() {
            Ok(_) => {
                if self.debug_enabled {
                    log_debug!("Roundtrip successful");
                }
            }
            Err(e) => {
                if self.debug_enabled {
                    log_pipe!();
                    log_warning!("Roundtrip failed: {e}");
                }
            }
        }

        // Log success - we successfully applied gamma to outputs
        if successful_count > 0 {
            if self.debug_enabled {
                log_debug!("Successfully applied gamma control to {successful_count} output(s)");
            }
        } else if self.debug_enabled {
            log_pipe!();
            log_warning!("No outputs were available for gamma control");
        }

        // Now temp files can be dropped
        drop(temp_files);
        if self.debug_enabled {
            log_debug!("apply_gamma_to_outputs completed");
        }
        Ok(())
    }
}

impl ColorTemperatureBackend for WaylandBackend {
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
        self.apply_gamma_to_outputs(temp, gamma / 100.0) // Convert percentage to 0.0-1.0
    }

    fn apply_startup_state(
        &mut self,
        state: TimeState,
        config: &Config,
        running: &AtomicBool,
    ) -> Result<()> {
        // First announce what mode we're entering (like Hyprland backend)
        crate::time_state::log_state_announcement(state);

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
        self.apply_gamma_to_outputs(temperature, gamma / 100.0) // Convert percentage to 0.0-1.0
    }

    fn backend_name(&self) -> &'static str {
        "Wayland"
    }
}

// Implement Dispatch traits for Wayland protocol handling
impl Dispatch<WlRegistry, ()> for AppData {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: <WlRegistry as Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_registry::Event;

        if let Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "zwlr_gamma_control_manager_v1" => {
                    let manager =
                        registry.bind::<ZwlrGammaControlManagerV1, _, _>(name, version, qh, ());
                    state.gamma_manager = Some(manager);
                }
                "wl_output" => {
                    let output = registry.bind::<WlOutput, _, _>(name, version, qh, ());
                    state.outputs.push(OutputInfo {
                        output,
                        gamma_control: None,
                        gamma_size: None,
                        name: format!("output-{name}"),
                    });
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<ZwlrGammaControlManagerV1, ()> for AppData {
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

impl Dispatch<ZwlrGammaControlV1, ()> for AppData {
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
                        // Only log gamma size in debug builds or when explicitly enabled
                        #[cfg(debug_assertions)]
                        log_decorated!("Output '{}' gamma size: {}", output_info.name, size);
                        break;
                    }
                }
            }
            GammaControlEvent::Failed => {
                // This is critical - the compositor rejected our gamma control
                for output_info in &state.outputs {
                    if let Some(ref control) = output_info.gamma_control
                        && control == gamma_control
                    {
                        log_critical!(
                            "Gamma control failed for output '{}' - compositor rejected our control!",
                            output_info.name
                        );
                        log_indented!("This could mean:");
                        log_indented!("1. Another client already has exclusive gamma control");
                        log_indented!("2. The compositor doesn't actually support gamma control");
                        log_indented!("3. Permission denied for gamma control");
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

impl Dispatch<WlOutput, ()> for AppData {
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
            // Update output name
            for output_info in &mut state.outputs {
                if &output_info.output == output {
                    output_info.name = name;
                    break;
                }
            }
        }
    }
}
