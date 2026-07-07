//! CLI entry point for sunsetr.
//!
//! Parses command-line arguments and dispatches to the module tree.

// IMPORTANT: `common` must be declared first so its logger macros are in
// unqualified scope for every module below (`#[macro_use]` textual scoping).
#[macro_use]
mod common;

// Entry points
mod args;
mod sunsetr;

// Core logic
mod core;

// Domain modules
mod backend;
mod commands;
mod config;
mod geo;
mod state;

// Infrastructure
mod io;
mod time;

use std::process::ExitCode;

use anyhow::Result;

use crate::args::CliAction;
use crate::common::error::{Silent, format_chain};
use crate::io::instance::restore_config_dir;
use crate::sunsetr::Sunsetr;

fn main() -> ExitCode {
    let action = CliAction::from_env();

    if let Some(dir) = action.config_dir()
        && let Err(e) = config::set_config_dir(Some(dir.to_string()))
    {
        log_error_end!("{}", e);
    } else if action.config_dir().is_none()
        && action.inherits_lock_config_dir()
        && let Err(e) = restore_config_dir()
    {
        log_error_end!("{}", format_chain(&e));
        return ExitCode::FAILURE;
    }

    match dispatch(action) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.downcast_ref::<Silent>().is_some() => ExitCode::FAILURE,
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
            pace,
            log_to_file,
            ..
        } => time::simulate::run_simulation(start_time, end_time, pace, debug_enabled, log_to_file),
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
        CliAction::StopCommand => commands::stop::handle_stop_command(),
        CliAction::GeoCommand {
            debug_enabled,
            target,
            ..
        } => commands::geo::handle_geo_command(debug_enabled, target),
        CliAction::TestCommand {
            debug_enabled,
            temperature,
            gamma,
        } => commands::test::handle_test_command(temperature, gamma, debug_enabled),
        CliAction::StatusCommand { json, follow } => {
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
