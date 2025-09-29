//! Main application entry point and high-level flow coordination.
//!
//! This module orchestrates the overall application lifecycle after command-line
//! argument parsing is complete. It coordinates between different modules:
//!
//! - `args`: Command-line argument parsing and help/version display
//! - `config`: Configuration loading, validation, and hot-reload support
//! - `backend`: Multi-compositor backend detection and management (Hyprland/Wayland)
//! - `time_state`: Time-based state calculation and transition logic
//! - `time_source`: Real-time and simulated time abstraction for testing
//! - `geo`: Geographic location-based solar calculations and city selection
//! - `smooth_transitions`: Smooth color temperature transitions (Wayland backend only)
//! - `signals`: Signal handling and process management (SIGUSR1/SIGUSR2)
//! - `dbus`: System sleep/resume and display hotplug detection
//! - `commands`: One-shot CLI commands (reload, test, preset, geo)
//! - `simulate`: Time simulation for testing transitions
//! - `utils`: Shared utilities including terminal management and cleanup
//! - `logger`: Centralized logging with indentation support
//!
//! The main application flow is managed through the `Sunsetr` builder pattern:
//! - Normal startup: `Sunsetr::new(debug_enabled).run()`
//! - Geo restart: `Sunsetr::new(true).without_lock().with_previous_state(state).run()`
//! - Reload spawn: `Sunsetr::new(debug_enabled).from_reload().run()`
//! - Simulation mode: `Sunsetr::new(debug_enabled).without_lock().without_headers().run()`
//!
//! The builder pattern provides flexibility for different startup contexts while
//! maintaining a clean API. The main flow consists of:
//! 1. Argument parsing and early exit for help/version/commands
//! 2. Terminal setup (cursor hiding, echo suppression) with graceful degradation
//! 3. Configuration loading and backend detection (auto-detect or explicit)
//! 4. Lock file management with cross-compositor cleanup support
//! 5. Signal handler and optional D-Bus/config watcher setup
//! 6. Initial state application with optional smooth startup transition
//! 7. Main monitoring loop with signal-driven updates and state transitions
//! 8. Graceful cleanup on shutdown (smooth transition, gamma reset, lock release)
//!
//! This structure keeps the main function focused on high-level flow while delegating
//! specific responsibilities to appropriate modules.

use anyhow::Result;

// Import macros from logger module for use in all submodules
#[macro_use]
mod logger;

mod args;
mod backend;
mod commands;
mod config;
mod constants;
mod core;
mod dbus;
mod display_state;
mod geo;
mod signals;
mod simulate;
mod smooth_transitions;
mod state;
mod sunsetr;
mod time_source;
mod time_state;
mod utils;

use args::{CliAction, ParsedArgs};

fn main() -> Result<()> {
    // Parse command-line arguments
    let parsed_args = ParsedArgs::from_env();

    // Extract config_dir from the action and set it globally if provided
    let config_dir = match &parsed_args.action {
        CliAction::Run { config_dir, .. }
        | CliAction::ReloadCommand { config_dir, .. }
        | CliAction::TestCommand { config_dir, .. }
        | CliAction::GeoCommand { config_dir, .. }
        | CliAction::PresetCommand { config_dir, .. }
        | CliAction::SetCommand { config_dir, .. }
        | CliAction::GetCommand { config_dir, .. }
        | CliAction::RunGeoSelection { config_dir, .. }
        | CliAction::Reload { config_dir, .. }
        | CliAction::Test { config_dir, .. }
        | CliAction::Simulate { config_dir, .. } => config_dir.clone(),
        _ => None,
    };

    // Set the config directory once at startup if provided
    if let Some(dir) = config_dir
        && let Err(e) = config::set_config_dir(Some(dir))
    {
        log_error_exit!("{}", e);
    }

    match parsed_args.action {
        CliAction::ShowVersion => {
            args::display_version_info();
            Ok(())
        }
        CliAction::ShowHelp | CliAction::ShowHelpDueToError => {
            args::display_help();
            Ok(())
        }
        CliAction::HelpCommand { command } => commands::help::run_help_command(command.as_deref()),
        CliAction::UsageHelp { command } => {
            // Show brief usage help for the command
            match command.as_str() {
                "set" | "s" => commands::set::show_usage(),
                "get" | "g" => commands::get::show_usage(),
                "preset" | "p" => commands::preset::show_usage(),
                "reload" | "r" => commands::reload::show_usage(),
                "test" | "t" => commands::test::show_usage(),
                "geo" | "G" => commands::geo::show_usage(),
                _ => {
                    log_warning_standalone!("Unknown command: {}", command);
                    args::display_help();
                }
            }
            Ok(())
        }
        CliAction::ShowCommandUsageDueToError {
            command,
            error_message,
        } => {
            // Special handling for preset command to show context
            if command == "preset" {
                commands::preset::show_usage_with_context(&error_message);
            } else {
                log_version!(); // Show header for this error-only output path
                log_pipe!();
                log_error!("{}", error_message);
                commands::help::show_command_usage(&command);
                log_block_start!("For more information, try '--help'.");
                log_end!();
            }
            Ok(())
        }
        CliAction::Run {
            debug_enabled,
            from_reload,
            ..
        } => {
            // Clean up old state directories (non-critical, ignore errors)
            let _ = state::cleanup_orphaned_state_dirs();

            // Continue with normal application flow using builder pattern
            if from_reload {
                // Process was spawned from reload
                sunsetr::Sunsetr::new(debug_enabled).with_reload().run()
            } else {
                sunsetr::Sunsetr::new(debug_enabled).run()
            }
        }
        // Handle both deprecated flag and new subcommand syntax for reload
        CliAction::Reload { debug_enabled, .. }
        | CliAction::ReloadCommand { debug_enabled, .. } => {
            commands::reload::handle_reload_command(debug_enabled)
        }
        // Handle both deprecated flag and new subcommand syntax for test
        CliAction::Test {
            debug_enabled,
            temperature,
            gamma,
            ..
        }
        | CliAction::TestCommand {
            debug_enabled,
            temperature,
            gamma,
            ..
        } => commands::test::handle_test_command(temperature, gamma, debug_enabled),
        // Handle both deprecated flag and new subcommand syntax for geo
        CliAction::RunGeoSelection { debug_enabled, .. }
        | CliAction::GeoCommand { debug_enabled, .. } => {
            match commands::geo::handle_geo_command(debug_enabled)? {
                geo::GeoCommandResult::RestartInDebugMode { previous_state } => {
                    sunsetr::Sunsetr::new(true)
                        .without_lock()
                        .with_previous_state(previous_state)
                        .run()
                }
                geo::GeoCommandResult::StartNewInDebugMode => {
                    sunsetr::Sunsetr::new(true).without_headers().run()
                }
                geo::GeoCommandResult::Completed => Ok(()),
            }
        }
        CliAction::Simulate {
            debug_enabled,
            start_time,
            end_time,
            multiplier,
            log_to_file,
            ..
        } => {
            // Handle --simulate flag: set up simulated time source
            // Keep the guards alive for the duration of the simulation
            let mut simulation_guards = simulate::handle_simulate_command(
                start_time,
                end_time,
                multiplier,
                debug_enabled,
                log_to_file,
            )?;

            // Run the application with simulated time
            // The output will go to stdout/stderr as normal, which the user can redirect
            sunsetr::Sunsetr::new(debug_enabled)
                .without_lock() // Don't interfere with real instances
                .without_headers() // Headers already shown by simulate command
                .run()?;

            // Only complete the simulation if it ran to completion (not interrupted)
            if time_source::simulation_ended() {
                simulation_guards.complete_simulation();
            }
            // Otherwise, the Drop implementation will handle cleanup without the "complete" message

            Ok(())
        }
        CliAction::PresetCommand {
            debug_enabled,
            subcommand,
            ..
        } => match commands::preset::handle_preset_command(&subcommand)? {
            commands::preset::PresetResult::Exit => Ok(()),
            commands::preset::PresetResult::ContinueExecution => {
                sunsetr::Sunsetr::new(debug_enabled).without_headers().run()
            }
        },
        CliAction::SetCommand { fields, target, .. } => {
            commands::set::handle_set_command(&fields, target.as_deref())
        }
        CliAction::GetCommand {
            fields,
            target,
            json,
            ..
        } => commands::get::handle_get_command(&fields, target.as_deref(), json),
    }
}
