//! CLI entry point for Sunsetr.
//!
//! Parses command-line arguments and dispatches to library functions.
//! All application logic lives in the library.

use anyhow::Result;

// Import log macros from the library
#[macro_use]
extern crate sunsetr;

// Import everything from the library crate
use sunsetr::{
    Sunsetr,
    args::{self, CliAction, ParsedArgs},
    commands, config,
    geo::{self},
};

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
        | CliAction::StopCommand { config_dir, .. }
        | CliAction::RestartCommand { config_dir, .. }
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
                "reload" => commands::reload::show_usage(),
                "restart" | "r" => commands::restart::show_usage(),
                "stop" | "S" => commands::stop::show_usage(),
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
        // Handle both deprecated flag and new subcommand syntax for geo
        CliAction::GeoCommand { debug_enabled, .. }
        | CliAction::RunGeoSelection { debug_enabled, .. } => {
            match commands::geo::handle_geo_command(debug_enabled)? {
                geo::GeoCommandResult::RestartInDebugMode { previous_state } => Sunsetr::new(true)
                    .without_lock()
                    .with_previous_state(previous_state)
                    .run(),
                geo::GeoCommandResult::StartNewInDebugMode => {
                    Sunsetr::new(true).without_headers().run()
                }
                geo::GeoCommandResult::Completed => Ok(()),
            }
        }
        // Handle both deprecated flag and deprecated subcommand syntax for reload
        CliAction::Reload { debug_enabled, .. }
        | CliAction::ReloadCommand { debug_enabled, .. } => {
            commands::reload::handle_reload_command(debug_enabled)
        }
        CliAction::RestartCommand {
            debug_enabled,
            instant,
            background,
            ..
        } => commands::restart::handle_restart_command(instant, debug_enabled, background),
        CliAction::StopCommand { debug_enabled, .. } => {
            commands::stop::handle_stop_command(debug_enabled)
        }
        CliAction::Run {
            debug_enabled,
            from_reload,
            background,
            ..
        } => {
            // Clean up old state directories (non-critical, ignore errors)
            let _ = sunsetr::state::preset::cleanup_orphaned_state_dirs();

            // Continue with normal application flow using builder pattern
            let sunsetr = if from_reload {
                // Process was spawned from reload
                Sunsetr::new(debug_enabled).with_reload()
            } else {
                Sunsetr::new(debug_enabled)
            };

            let sunsetr = if background {
                sunsetr.background()
            } else {
                sunsetr
            };

            sunsetr.run()
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
            let mut simulation_guards = sunsetr::time::simulate::handle_simulate_command(
                start_time,
                end_time,
                multiplier,
                debug_enabled,
                log_to_file,
            )?;

            // Run the application with simulated time
            // The output will go to stdout/stderr as normal, which the user can redirect
            Sunsetr::new(debug_enabled)
                .without_lock() // Don't interfere with real instances
                .without_headers() // Headers already shown by simulate command
                .run()?;

            // Only complete the simulation if it ran to completion (not interrupted)
            if sunsetr::time_source::simulation_ended() {
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
            commands::preset::PresetResult::TestModeActive => Ok(()),
            commands::preset::PresetResult::ContinueExecution => {
                Sunsetr::new(debug_enabled).without_headers().run()
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
