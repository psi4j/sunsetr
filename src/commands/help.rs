//! Dispatch the help command to command-specific or general help.

use anyhow::Result;

/// Brief usage line for a command, shown alongside error messages.
pub fn show_command_usage(command: &str) {
    match command {
        "geo" | "G" => log_block_start!("Usage: sunsetr geo"),
        "get" | "g" => log_block_start!("Usage: sunsetr get [OPTIONS] <field> [<field>...]"),
        "preset" | "p" => log_block_start!("Usage: sunsetr preset <subcommand|name>"),
        "restart" | "r" => log_block_start!("Usage: sunsetr restart [--instant]"),
        "set" | "s" => {
            log_block_start!("Usage: sunsetr set [OPTIONS] <field>[+|-]=<value> [...]")
        }
        "status" | "S" => log_block_start!("Usage: sunsetr status [--json] [--follow]"),
        "stop" => log_block_start!("Usage: sunsetr stop"),
        "test" | "t" => log_block_start!("Usage: sunsetr test <temperature> <gamma>"),
        _ => log_block_start!("Usage: sunsetr [OPTIONS] [COMMAND]"),
    }
}

/// Show the full usage block for a command (the `--help` flag on a command).
///
/// Unknown commands fall back to the top-level help output.
pub fn show_usage(command: &str) -> Result<()> {
    match command {
        "geo" | "G" => super::geo::show_usage(),
        "get" | "g" => super::get::show_usage(),
        "preset" | "p" => super::preset::show_usage(),
        "restart" | "r" => super::restart::show_usage(),
        "set" | "s" => super::set::show_usage(),
        "status" | "S" => super::status::show_usage(),
        "stop" => super::stop::show_usage(),
        "test" | "t" => super::test::show_usage(),
        _ => {
            log_warning_standalone!("Unknown command: {}", command);
            crate::args::display_help();
        }
    }
    Ok(())
}

/// Show a parse error followed by the offending command's brief usage.
pub fn show_command_usage_with_error(command: &str, error_message: &str) -> Result<()> {
    if command == "preset" {
        super::preset::show_usage_with_context(error_message);
    } else {
        log_version!();
        log_pipe!();
        log_error!("{}", error_message);
        show_command_usage(command);
        log_block_start!("For more information, try '--help'.");
        log_end!();
    }
    Ok(())
}

/// Dispatch `sunsetr help [command]`, showing general help when `command` is `None`.
pub fn run_help_command(command: Option<&str>) -> Result<()> {
    match command {
        None => display_general_help(),
        Some("get") | Some("g") => super::get::display_help(),
        Some("geo") | Some("G") => super::geo::display_help(),
        Some("help") | Some("h") => display_help_help(),
        Some("preset") | Some("p") => super::preset::display_help(),
        Some("restart") | Some("r") => super::restart::display_help(),
        Some("set") | Some("s") => super::set::display_help(),
        Some("status") | Some("S") => super::status::display_help(),
        Some("stop") => super::stop::display_help(),
        Some("test") | Some("t") => super::test::display_help(),
        Some(unknown) => {
            log_warning_standalone!("Unknown command: {}", unknown);
            display_general_help();
        }
    }
    Ok(())
}

fn display_general_help() {
    log_version!();
    log_block_start!("Available Commands:");
    log_indented!("geo, G                  Interactive city selection for geographic mode");
    log_indented!("get, g <field>          Read configuration field(s)");
    log_indented!("help, h [COMMAND]       Show detailed help for a command");
    log_indented!("preset, p <sub|name>    Manage and apply preset configurations");
    log_indented!("restart, r [--instant]  Recreate backend and reload configuration");
    log_indented!("set, s <field>[op]=val  Update configuration field(s)");
    log_indented!("status, S               Display current runtime state");
    log_indented!("stop                    Cleanly terminate running sunsetr instance");
    log_indented!("test, t <temp> <gamma>  Test specific temperature and gamma values");
    log_pipe!();
    log_info!("Use 'sunsetr help <command>' to see detailed help for a specific command.");
    log_indented!("Use 'sunsetr --help' to see all options and general usage.");
    log_end!();
}

fn display_help_help() {
    log_version!();
    log_block_start!("Display help information");
    log_block_start!("Usage: sunsetr help [COMMAND]");
    log_block_start!("Arguments:");
    log_indented!("COMMAND  Optional command to get help for");
    log_indented!("         If omitted, shows general help");
    log_block_start!("Examples:");
    log_indented!("# Show general help");
    log_indented!("sunsetr help");
    log_pipe!();
    log_indented!("# Show help for specific commands");
    log_indented!("sunsetr help set");
    log_indented!("sunsetr help preset");
    log_indented!("sunsetr help geo");
    log_end!();
}
