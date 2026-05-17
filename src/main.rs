//! CLI entry point for Sunsetr.
//!
//! Parses command-line arguments and dispatches to library functions.
//! All application logic lives in the library.

use std::process::ExitCode;

use anyhow::Result;

#[macro_use]
extern crate sunsetr;

use sunsetr::{
    Sunsetr,
    args::{self, CliAction, ParsedArgs},
    commands,
    common::error::{AlreadyReported, format_chain},
    config,
};

fn main() -> ExitCode {
    let parsed_args = ParsedArgs::from_env();

    if let Some(dir) = parsed_args.action.config_dir()
        && let Err(e) = config::set_config_dir(Some(dir.to_string()))
    {
        log_error_end!("{}", e);
    }

    match dispatch(parsed_args.action) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.downcast_ref::<AlreadyReported>().is_some() => ExitCode::FAILURE,
        Err(e) => {
            log_error_end!("{}", format_chain(&e));
            ExitCode::FAILURE
        }
    }
}

/// Route a parsed CLI action to its handler.
fn dispatch(action: CliAction) -> Result<()> {
    match action {
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
        CliAction::UsageHelp { command } => commands::help::show_usage(&command),
        CliAction::ShowCommandUsageDueToError {
            command,
            error_message,
        } => commands::help::show_command_usage_with_error(&command, &error_message),
        CliAction::Run {
            debug_enabled,
            background,
            ..
        } => Sunsetr::new(debug_enabled).background(background).run(),
        CliAction::Simulate {
            debug_enabled,
            start_time,
            end_time,
            multiplier,
            log_to_file,
            ..
        } => sunsetr::time::simulate::run_simulation(
            start_time,
            end_time,
            multiplier,
            debug_enabled,
            log_to_file,
        ),
        CliAction::PresetCommand {
            debug_enabled,
            subcommand,
            ..
        } => match commands::preset::handle_preset_command(&subcommand)? {
            commands::preset::PresetResult::Exit
            | commands::preset::PresetResult::TestModeActive => Ok(()),
            commands::preset::PresetResult::ContinueExecution => {
                Sunsetr::new(debug_enabled).without_headers().run()
            }
        },
        CliAction::RestartCommand {
            debug_enabled,
            instant,
            background,
            ..
        } => commands::restart::handle_restart_command(instant, debug_enabled, background),
        CliAction::StopCommand { debug_enabled, .. } => {
            commands::stop::handle_stop_command(debug_enabled)
        }
        CliAction::GeoCommand { debug_enabled, .. } => {
            commands::geo::handle_geo_command(debug_enabled)
        }
        CliAction::TestCommand {
            debug_enabled,
            temperature,
            gamma,
            ..
        } => commands::test::handle_test_command(temperature, gamma, debug_enabled),
        CliAction::StatusCommand { json, follow, .. } => {
            commands::status::handle_status_command(json, follow)
        }
        CliAction::SetCommand { fields, target, .. } => {
            commands::set::handle_set_command(fields, target.as_deref())
        }
        CliAction::GetCommand {
            fields,
            target,
            json,
            ..
        } => commands::get::handle_get_command(&fields, target.as_deref(), json),
    }
}
