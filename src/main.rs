//! CLI entry point for Sunsetr.
//!
//! Parses command-line arguments and dispatches to library functions.
//! All application logic lives in the library.

use anyhow::Result;

#[macro_use]
extern crate sunsetr;

use sunsetr::{
    Sunsetr,
    args::{self, CliAction, ParsedArgs},
    commands, config,
};

fn main() -> Result<()> {
    let parsed_args = ParsedArgs::from_env();

    if let Some(dir) = parsed_args.action.config_dir()
        && let Err(e) = config::set_config_dir(Some(dir.to_string()))
    {
        log_error_exit!("{}", e);
    }

    match parsed_args.action {
        CliAction::ShowVersion => {
            args::display_version_info();
            Ok(())
        }
        CliAction::ShowHelp => {
            args::display_help();
            Ok(())
        }
        CliAction::ShowHelpDueToError => {
            args::display_help();
            std::process::exit(1);
        }
        CliAction::HelpCommand { command } => commands::help::run_help_command(command.as_deref()),
        CliAction::UsageHelp { command } => {
            match command.as_str() {
                "set" | "s" => commands::set::show_usage(),
                "get" | "g" => commands::get::show_usage(),
                "preset" | "p" => commands::preset::show_usage(),
                "restart" | "r" => commands::restart::show_usage(),
                "status" | "S" => commands::status::show_usage(),
                "stop" => commands::stop::show_usage(),
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
            if command == "preset" {
                commands::preset::show_usage_with_context(&error_message);
            } else {
                log_version!();
                log_pipe!();
                log_error!("{}", error_message);
                commands::help::show_command_usage(&command);
                log_block_start!("For more information, try '--help'.");
                log_end!();
            }
            Ok(())
        }
        CliAction::GeoCommand { debug_enabled, .. } => {
            commands::geo::handle_geo_command(debug_enabled)
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
            background,
            ..
        } => Sunsetr::new(debug_enabled).background(background).run(),
        CliAction::TestCommand {
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
            let mut simulation_guards = sunsetr::time::simulate::setup_simulation(
                start_time,
                end_time,
                multiplier,
                debug_enabled,
                log_to_file,
            )?;

            Sunsetr::new(debug_enabled)
                .without_lock()
                .without_headers()
                .run()?;

            if sunsetr::time_source::simulation_ended() {
                simulation_guards.complete_simulation();
            }

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
            commands::set::handle_set_command(fields, target.as_deref())
        }
        CliAction::GetCommand {
            fields,
            target,
            json,
            ..
        } => commands::get::handle_get_command(&fields, target.as_deref(), json),
        CliAction::StatusCommand { json, follow, .. } => {
            commands::status::handle_status_command(json, follow)
        }
    }
}
