//! Help command implementation for sunsetr.
//!
//! This module provides a dispatcher for the help command that shows
//! command-specific help or general help based on the arguments provided.

use anyhow::Result;

/// Show brief usage for a command (used for error messages)
pub fn show_command_usage(command: &str) {
    match command {
        "geo" | "G" => log_block_start!("Usage: sunsetr geo"),
        "get" | "g" => log_block_start!("Usage: sunsetr get [OPTIONS] <field> [<field>...]"),
        "preset" | "p" => log_block_start!("Usage: sunsetr preset <name>"),
        "reload" | "r" => log_block_start!("Usage: sunsetr reload"),
        "set" | "s" => {
            log_block_start!("Usage: sunsetr set [OPTIONS] <field>=<value> [<field>=<value>...]")
        }
        "test" | "t" => log_block_start!("Usage: sunsetr test <temperature> <gamma>"),
        _ => log_block_start!("Usage: sunsetr [OPTIONS] [COMMAND]"),
    }
}

/// Run the help command (dispatcher)
///
/// # Arguments
/// * `command` - Optional command name to get help for (None = general help)
pub fn run_help_command(command: Option<&str>) -> Result<()> {
    match command {
        None => display_general_help(),
        Some("get") | Some("g") => super::get::display_help(),
        Some("geo") | Some("G") => super::geo::display_help(),
        Some("help") | Some("h") => display_help_help(),
        Some("preset") | Some("p") => super::preset::display_help(),
        Some("reload") | Some("r") => super::reload::display_help(),
        Some("set") | Some("s") => super::set::display_help(),
        Some("test") | Some("t") => super::test::display_help(),
        Some(unknown) => {
            log_warning_standalone!("Unknown command: {}", unknown);
            display_general_help();
        }
    }
    Ok(())
}

/// Display general help focused on commands (for the help command)
fn display_general_help() {
    log_version!();
    log_block_start!("Available Commands:");
    log_indented!("geo, G                  Interactive city selection for geographic mode");
    log_indented!("get, g <field>          Read configuration field(s)");
    log_indented!("help, h [COMMAND]       Show detailed help for a command");
    log_indented!("preset, p <name>        Apply a named preset configuration");
    log_indented!("reload, r               Reset display gamma and reload configuration");
    log_indented!("set, s <field>=<value>  Update configuration field(s)");
    log_indented!("test, t <temp> <gamma>  Test specific temperature and gamma values");
    log_pipe!();
    log_info!("Use 'sunsetr help <command>' to see detailed help for a specific command.");
    log_indented!("Use 'sunsetr --help' to see all options and general usage.");
    log_end!();
}

/// Display help for the help command itself
fn display_help_help() {
    log_version!();
    log_block_start!("help - Display help information");
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
