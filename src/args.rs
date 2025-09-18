//! Command-line argument parsing and processing.
//!
//! This module handles parsing of command-line arguments and provides a clean
//! interface for the main application logic. It supports the standard help,
//! version, and debug flags while gracefully handling unknown options.

/// Represents the parsed command-line arguments and their intended actions.
#[derive(Debug, PartialEq)]
pub enum CliAction {
    /// Run the normal application with these settings
    Run {
        debug_enabled: bool,
        config_dir: Option<String>,
        from_reload: bool, // Internal flag: true when spawned from reload command
    },

    // Subcommand-style actions (new)
    /// Reload using subcommand syntax
    ReloadCommand {
        debug_enabled: bool,
        config_dir: Option<String>,
    },
    /// Test using subcommand syntax  
    TestCommand {
        debug_enabled: bool,
        temperature: u32,
        gamma: f32,
        config_dir: Option<String>,
    },
    /// Geo using subcommand syntax
    GeoCommand {
        debug_enabled: bool,
        config_dir: Option<String>,
    },
    /// Preset subcommand
    PresetCommand {
        debug_enabled: bool,
        preset_name: String,
        config_dir: Option<String>,
    },
    /// Set configuration field subcommand
    SetCommand {
        debug_enabled: bool,
        fields: Vec<(String, String)>, // Multiple field-value pairs
        config_dir: Option<String>,
    },

    // Flag-style actions (deprecated, remove in v1.0.0)
    /// Run interactive geo location selection (deprecated --geo flag)
    RunGeoSelection {
        debug_enabled: bool,
        config_dir: Option<String>,
    },
    /// Reset all display gamma and reload sunsetr (deprecated --reload flag)
    Reload {
        debug_enabled: bool,
        config_dir: Option<String>,
    },
    /// Test specific temperature and gamma values (deprecated --test flag)
    Test {
        debug_enabled: bool,
        temperature: u32,
        gamma: f32,
        config_dir: Option<String>,
    },
    /// Simulate time passing for testing
    Simulate {
        debug_enabled: bool,
        start_time: String,
        end_time: String,
        multiplier: f64,
        log_to_file: bool,
        config_dir: Option<String>,
    },

    /// Display help information and exit
    ShowHelp,
    /// Display version information and exit
    ShowVersion,
    /// Show help due to unknown arguments and exit
    ShowHelpDueToError,
}

/// Result of parsing command-line arguments.
pub struct ParsedArgs {
    pub action: CliAction,
}

/// Show deprecation warning for old flag syntax
fn show_deprecation_warning(old_form: &str, new_form: &str) {
    log_warning!(
        "'{}' is deprecated and will be removed in v1.0.0. Please use: {}",
        old_form,
        new_form
    );
}

impl ParsedArgs {
    /// Parse command-line arguments into a structured result.
    ///
    /// This function processes the arguments and determines what action should
    /// be taken, including whether to show help, version info, or run normally.
    /// Supports both old flag-based syntax and new subcommand syntax.
    ///
    /// # Arguments
    /// * `args` - Iterator over command-line arguments (typically from std::env::args())
    ///
    /// # Returns
    /// ParsedArgs containing the determined action
    pub fn parse<I, S>(args: I) -> ParsedArgs
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut debug_enabled = false;
        let mut display_help = false;
        let mut display_version = false;
        let mut run_geo_selection = false;
        let mut run_reload = false;
        let mut run_test = false;
        let mut test_temperature: Option<u32> = None;
        let mut test_gamma: Option<f32> = None;
        let mut run_simulate = false;
        let mut simulate_start: Option<String> = None;
        let mut simulate_end: Option<String> = None;
        let mut simulate_multiplier: Option<f64> = None;
        let mut log_to_file = false;
        let mut unknown_arg_found = false;
        let mut config_dir: Option<String> = None;
        let mut from_reload = false; // Internal flag for reload-spawned processes

        // Convert to vector for easier indexed access
        let args_vec: Vec<String> = args
            .into_iter()
            .skip(1)
            .map(|s| s.as_ref().to_string())
            .collect();

        // Check for subcommands first (new behavior)
        // But only if we don't have flags that consume arguments
        // Find the first non-flag argument which could be a subcommand
        // We need to skip over flags and their arguments
        let mut potential_command_idx = None;
        let mut idx = 0;
        while idx < args_vec.len() {
            let arg = &args_vec[idx];
            if arg.starts_with('-') {
                // This is a flag, check if it consumes the next argument
                if matches!(arg.as_str(), "--config" | "-c") {
                    idx += 2; // Skip the flag and its argument
                } else if matches!(arg.as_str(), "--simulate" | "-S") {
                    // Simulate takes 2+ arguments, but we'll handle it specially
                    break;
                } else if matches!(arg.as_str(), "--test" | "-t") {
                    // Test takes 2 arguments, but we'll handle it specially
                    break;
                } else {
                    idx += 1; // Just skip the flag
                }
            } else {
                // Found a non-flag argument, this could be our command
                potential_command_idx = Some(idx);
                break;
            }
        }

        if let Some(cmd_idx) = potential_command_idx {
            let command = &args_vec[cmd_idx];

            // Extract debug flag and config dir from anywhere in args
            let debug_enabled = args_vec.iter().any(|arg| arg == "--debug" || arg == "-d");

            // Extract config dir if present
            let config_dir = args_vec
                .iter()
                .position(|arg| arg == "--config" || arg == "-c")
                .and_then(|idx| args_vec.get(idx + 1))
                .cloned();

            // Note: --from-reload flag is handled in the main parsing loop below

            // Check for help/version flags which take precedence
            if args_vec
                .iter()
                .any(|arg| arg == "--version" || arg == "-V" || arg == "-v")
            {
                return ParsedArgs {
                    action: CliAction::ShowVersion,
                };
            }
            if args_vec.iter().any(|arg| arg == "--help" || arg == "-h") {
                return ParsedArgs {
                    action: CliAction::ShowHelp,
                };
            }

            // Check if there are multiple commands (error condition)
            // We need to be careful with commands that take arguments
            // For example, "preset test" should be allowed (test is the preset name)
            // But "geo preset" should not be allowed
            let check_for_multiple_commands = |start_idx: usize| -> Option<String> {
                for arg in args_vec.iter().skip(start_idx) {
                    if arg.starts_with('-') {
                        continue; // Skip flags
                    }
                    if matches!(
                        arg.as_str(),
                        "reload" | "r" | "test" | "t" | "geo" | "g" | "preset" | "p"
                    ) {
                        return Some(arg.clone());
                    }
                }
                None
            };

            // Check based on the command type
            let conflicting_command = match command.as_str() {
                "reload" | "r" => {
                    // Reload takes no arguments, check immediately after
                    check_for_multiple_commands(cmd_idx + 1)
                }
                "geo" | "g" => {
                    // Geo takes no arguments (interactive), check immediately after
                    check_for_multiple_commands(cmd_idx + 1)
                }
                "test" | "t" => {
                    // Test takes 2 arguments, check after those
                    if cmd_idx + 2 < args_vec.len() {
                        check_for_multiple_commands(cmd_idx + 3)
                    } else {
                        None
                    }
                }
                "preset" | "p" => {
                    // Preset takes 1 argument, check after that
                    if cmd_idx + 1 < args_vec.len() {
                        check_for_multiple_commands(cmd_idx + 2)
                    } else {
                        None
                    }
                }
                _ => None,
            };

            if let Some(conflict) = conflicting_command {
                log_error!(
                    "Cannot use multiple commands at once: '{}' and '{}'",
                    command,
                    conflict
                );
                return ParsedArgs {
                    action: CliAction::ShowHelpDueToError,
                };
            }

            match command.as_str() {
                "reload" | "r" => {
                    return ParsedArgs {
                        action: CliAction::ReloadCommand {
                            debug_enabled,
                            config_dir,
                        },
                    };
                }
                "test" | "t" => {
                    // Parse test arguments: test <temperature> <gamma>
                    if cmd_idx + 2 < args_vec.len() {
                        if let (Ok(temp), Ok(gamma)) = (
                            args_vec[cmd_idx + 1].parse::<u32>(),
                            args_vec[cmd_idx + 2].parse::<f32>(),
                        ) {
                            return ParsedArgs {
                                action: CliAction::TestCommand {
                                    debug_enabled,
                                    temperature: temp,
                                    gamma,
                                    config_dir,
                                },
                            };
                        } else {
                            log_warning!(
                                "Invalid test arguments. Usage: sunsetr test <temperature> <gamma>"
                            );
                            return ParsedArgs {
                                action: CliAction::ShowHelpDueToError,
                            };
                        }
                    } else {
                        log_warning!(
                            "Missing arguments for test. Usage: sunsetr test <temperature> <gamma>"
                        );
                        return ParsedArgs {
                            action: CliAction::ShowHelpDueToError,
                        };
                    }
                }
                "geo" | "g" => {
                    return ParsedArgs {
                        action: CliAction::GeoCommand {
                            debug_enabled,
                            config_dir,
                        },
                    };
                }
                "preset" | "p" => {
                    // Parse preset name: preset <name>
                    if cmd_idx + 1 < args_vec.len() && !args_vec[cmd_idx + 1].starts_with('-') {
                        return ParsedArgs {
                            action: CliAction::PresetCommand {
                                debug_enabled,
                                preset_name: args_vec[cmd_idx + 1].clone(),
                                config_dir,
                            },
                        };
                    } else {
                        log_warning!("Missing preset name. Usage: sunsetr preset <name>");
                        return ParsedArgs {
                            action: CliAction::ShowHelpDueToError,
                        };
                    }
                }
                "set" | "s" => {
                    // Parse set command: set <field> <value> [<field> <value>...]
                    let mut fields = Vec::new();
                    let mut idx = cmd_idx + 1;

                    // Parse field-value pairs
                    while idx + 1 < args_vec.len() {
                        // Check if this looks like a field-value pair
                        if !args_vec[idx].starts_with('-') && !args_vec[idx + 1].starts_with('-') {
                            fields.push((args_vec[idx].clone(), args_vec[idx + 1].clone()));
                            idx += 2;
                        } else {
                            break; // Hit a flag or ran out of pairs
                        }
                    }

                    if fields.is_empty() {
                        log_warning!(
                            "Missing field or value. Usage: sunsetr set <field> <value> [<field> <value>...]"
                        );
                        log_warning!("Example: sunsetr set night_temp 4000");
                        log_warning!(
                            "Example: sunsetr set transition_mode static static_temp 5000"
                        );
                        return ParsedArgs {
                            action: CliAction::ShowHelpDueToError,
                        };
                    }

                    return ParsedArgs {
                        action: CliAction::SetCommand {
                            debug_enabled,
                            fields,
                            config_dir,
                        },
                    };
                }
                _ => {
                    // Unknown subcommand - show error and help
                    log_warning!("Unknown command: {}", command);
                    return ParsedArgs {
                        action: CliAction::ShowHelpDueToError,
                    };
                }
            }
        }

        // Original flag parsing (with deprecation warnings)
        let mut i = 0;
        while i < args_vec.len() {
            let arg_str = &args_vec[i];
            match arg_str.as_str() {
                "--help" | "-h" => display_help = true,
                "--version" | "-V" | "-v" => display_version = true,
                "--debug" | "-d" => debug_enabled = true,
                "--from-reload" => from_reload = true, // Internal flag
                "--config" | "-c" => {
                    // Parse: --config <directory>
                    if i + 1 < args_vec.len() && !args_vec[i + 1].starts_with('-') {
                        config_dir = Some(args_vec[i + 1].clone());
                        i += 1; // Skip the parsed argument
                    } else {
                        log_warning!("Missing directory for --config. Usage: --config <directory>");
                        unknown_arg_found = true;
                    }
                }
                "--geo" | "-g" => {
                    show_deprecation_warning(arg_str, "sunsetr geo");
                    run_geo_selection = true;
                }
                "--reload" | "-r" => {
                    show_deprecation_warning(arg_str, "sunsetr reload");
                    run_reload = true;
                }
                "--test" | "-t" => {
                    show_deprecation_warning(arg_str, "sunsetr test");
                    run_test = true;
                    // Parse: --test <temperature> <gamma>
                    if i + 2 < args_vec.len() {
                        match args_vec[i + 1].parse::<u32>() {
                            Ok(temp) => test_temperature = Some(temp),
                            Err(_) => {
                                log_warning!("Invalid temperature value: {}", args_vec[i + 1]);
                                unknown_arg_found = true;
                            }
                        }

                        match args_vec[i + 2].parse::<f32>() {
                            Ok(gamma) => test_gamma = Some(gamma),
                            Err(_) => {
                                log_warning!("Invalid gamma value: {}", args_vec[i + 2]);
                                unknown_arg_found = true;
                            }
                        }

                        i += 2; // Skip the parsed arguments
                    } else {
                        log_warning!(
                            "Missing arguments for test. Usage: test <temperature> <gamma>"
                        );
                        unknown_arg_found = true;
                    }
                }
                "--simulate" | "-S" => {
                    run_simulate = true;
                    // Parse: --simulate <start_time> <end_time> [multiplier | --fast-forward] [--log]
                    if i + 2 < args_vec.len() {
                        let start_str = args_vec[i + 1].clone();
                        let end_str = args_vec[i + 2].clone();

                        // Validate datetime format (basic check - full validation happens in simulate command)
                        // Check format is roughly "YYYY-MM-DD HH:MM:SS"
                        let validate_datetime = |s: &str| -> bool {
                            // Check length and basic structure
                            s.len() == 19
                                && s.chars().nth(4) == Some('-')
                                && s.chars().nth(7) == Some('-')
                                && s.chars().nth(10) == Some(' ')
                                && s.chars().nth(13) == Some(':')
                                && s.chars().nth(16) == Some(':')
                        };

                        if !validate_datetime(&start_str) {
                            log_error!(
                                "Invalid start time format: '{}'. Use YYYY-MM-DD HH:MM:SS",
                                start_str
                            );
                            unknown_arg_found = true;
                            i += 2; // Skip the parsed arguments
                        } else if !validate_datetime(&end_str) {
                            log_error!(
                                "Invalid end time format: '{}'. Use YYYY-MM-DD HH:MM:SS",
                                end_str
                            );
                            unknown_arg_found = true;
                            i += 2; // Skip the parsed arguments
                        } else {
                            // Basic validation that times are parseable and end > start
                            // We can't do full datetime parsing here without pulling in chrono,
                            // but we can at least check the basic format is correct
                            simulate_start = Some(start_str);
                            simulate_end = Some(end_str);
                            i += 2; // Skip the parsed arguments

                            // Check for optional multiplier or --fast-forward flag
                            if i + 1 < args_vec.len() && !args_vec[i + 1].starts_with('-') {
                                if let Ok(mult) = args_vec[i + 1].parse::<f64>() {
                                    // Validate multiplier range (0.1 to 3600)
                                    if !(0.1..=3600.0).contains(&mult) {
                                        log_error!(
                                            "Invalid multiplier: {}. Must be between 0.1 and 3600.",
                                            mult
                                        );
                                        unknown_arg_found = true;
                                    } else {
                                        simulate_multiplier = Some(mult);
                                    }
                                    i += 1;
                                }
                            } else if i + 1 < args_vec.len() && args_vec[i + 1] == "--fast-forward"
                            {
                                // Use a special marker value to indicate fast-forward mode
                                simulate_multiplier = Some(-1.0);
                                i += 1;
                            }

                            // Check for optional --log flag
                            if i + 1 < args_vec.len() && args_vec[i + 1] == "--log" {
                                log_to_file = true;
                                i += 1;
                            }
                        }
                    } else {
                        log_warning!(
                            "Missing arguments for --simulate. Usage: --simulate \"YYYY-MM-DD HH:MM:SS\" \"YYYY-MM-DD HH:MM:SS\" [multiplier | --fast-forward] [--log]"
                        );
                        unknown_arg_found = true;
                    }
                }
                _ => {
                    // Check if the argument starts with a dash, indicating it's an option
                    if arg_str.starts_with('-') {
                        log_warning!("Unknown option: {arg_str}");
                        unknown_arg_found = true;
                    }
                    // Non-option arguments are currently ignored
                }
            }
            i += 1;
        }

        // Determine the action based on parsed flags
        let action = if display_version {
            CliAction::ShowVersion
        } else if display_help || unknown_arg_found {
            if unknown_arg_found {
                CliAction::ShowHelpDueToError
            } else {
                CliAction::ShowHelp
            }
        } else if run_geo_selection {
            CliAction::RunGeoSelection {
                debug_enabled,
                config_dir,
            }
        } else if run_reload {
            CliAction::Reload {
                debug_enabled,
                config_dir,
            }
        } else if run_test {
            match (test_temperature, test_gamma) {
                (Some(temp), Some(gamma)) => CliAction::Test {
                    debug_enabled,
                    temperature: temp,
                    gamma,
                    config_dir,
                },
                _ => {
                    log_warning!("Missing temperature or gamma values for test");
                    CliAction::ShowHelpDueToError
                }
            }
        } else if run_simulate {
            match (simulate_start, simulate_end) {
                (Some(start), Some(end)) => CliAction::Simulate {
                    debug_enabled,
                    start_time: start,
                    end_time: end,
                    multiplier: simulate_multiplier.unwrap_or(0.0), // 0 = default 3600x
                    log_to_file,
                    config_dir,
                },
                _ => {
                    log_warning!("Missing start or end time for --simulate");
                    CliAction::ShowHelpDueToError
                }
            }
        } else {
            CliAction::Run {
                debug_enabled,
                config_dir,
                from_reload,
            }
        };

        ParsedArgs { action }
    }

    /// Convenience method to parse from std::env::args()
    pub fn from_env() -> ParsedArgs {
        Self::parse(std::env::args())
    }
}

/// Displays version information using custom logging style.
pub fn display_version_info() {
    log_version!();
    log_pipe!();
    println!("┗ {}", env!("CARGO_PKG_DESCRIPTION"));
}

/// Displays custom help message using logger methods.
pub fn display_help() {
    log_version!();
    log_block_start!(env!("CARGO_PKG_DESCRIPTION"));
    log_block_start!("Usage:");
    log_indented!("sunsetr [OPTIONS] [COMMAND]");
    log_block_start!("Options:");
    log_indented!("-c, --config <dir>     Use custom configuration directory");
    log_indented!("-d, --debug            Enable detailed debug output");
    log_indented!("-h, --help             Print help information");
    log_indented!("-S, --simulate         Run with simulated time (for testing transitions)");
    log_indented!("                       Usage: --simulate <start> <end> [multiplier] [--log]");
    log_indented!("-V, --version          Print version information");
    log_block_start!("Commands:");
    log_indented!("geo, g                 Interactive city selection for geo mode");
    log_indented!("preset, p <name>       Apply a named preset configuration");
    log_indented!("reload, r              Reset display gamma and reload configuration");
    log_indented!("set, s <field> <value> [...] Update configuration field(s)");
    log_indented!("test, t <temp> <gamma> Test specific temperature and gamma values");
    log_block_start!("Deprecated flags (will be removed in v1.0.0):");
    log_indented!("-r, --reload           Use 'sunsetr reload' instead");
    log_indented!("-t, --test             Use 'sunsetr test' instead");
    log_indented!("-g, --geo              Use 'sunsetr geo' instead");
    log_end!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_no_args() {
        let args = vec!["sunsetr"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::Run {
                debug_enabled: false,
                config_dir: None,
                from_reload: false,
            }
        );
    }

    #[test]
    fn test_parse_debug_flag() {
        let args = vec!["sunsetr", "--debug"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::Run {
                debug_enabled: true,
                config_dir: None,
                from_reload: false,
            }
        );
    }

    #[test]
    fn test_parse_debug_short_flag() {
        let args = vec!["sunsetr", "-d"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::Run {
                debug_enabled: true,
                config_dir: None,
                from_reload: false,
            }
        );
    }

    #[test]
    fn test_parse_help_flag() {
        let args = vec!["sunsetr", "--help"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(parsed.action, CliAction::ShowHelp);
    }

    #[test]
    fn test_parse_help_short_flag() {
        let args = vec!["sunsetr", "-h"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(parsed.action, CliAction::ShowHelp);
    }

    #[test]
    fn test_parse_version_flag() {
        let args = vec!["sunsetr", "--version"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(parsed.action, CliAction::ShowVersion);
    }

    #[test]
    fn test_parse_version_short_flags() {
        let args1 = vec!["sunsetr", "-V"];
        let parsed1 = ParsedArgs::parse(args1);
        assert_eq!(parsed1.action, CliAction::ShowVersion);

        let args2 = vec!["sunsetr", "-v"];
        let parsed2 = ParsedArgs::parse(args2);
        assert_eq!(parsed2.action, CliAction::ShowVersion);
    }

    #[test]
    fn test_parse_multiple_flags() {
        let args = vec!["sunsetr", "--debug", "--help"];
        let parsed = ParsedArgs::parse(args);
        // Help takes precedence
        assert_eq!(parsed.action, CliAction::ShowHelp);
    }

    #[test]
    fn test_parse_unknown_flag() {
        let args = vec!["sunsetr", "--unknown"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(parsed.action, CliAction::ShowHelpDueToError);
    }

    #[test]
    fn test_parse_mixed_valid_and_invalid() {
        let args = vec!["sunsetr", "--debug", "--invalid"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(parsed.action, CliAction::ShowHelpDueToError);
    }

    #[test]
    fn test_version_takes_precedence() {
        let args = vec!["sunsetr", "--version", "--help", "--debug"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(parsed.action, CliAction::ShowVersion);
    }

    #[test]
    fn test_parse_geo_flag() {
        let args = vec!["sunsetr", "--geo"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::RunGeoSelection {
                debug_enabled: false,
                config_dir: None
            }
        );
    }

    #[test]
    fn test_parse_geo_short_flag() {
        let args = vec!["sunsetr", "-g"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::RunGeoSelection {
                debug_enabled: false,
                config_dir: None
            }
        );
    }

    #[test]
    fn test_geo_with_debug() {
        let args = vec!["sunsetr", "--geo", "--debug"];
        let parsed = ParsedArgs::parse(args);
        // Geo selection with debug output enabled
        assert_eq!(
            parsed.action,
            CliAction::RunGeoSelection {
                debug_enabled: true,
                config_dir: None
            }
        );
    }

    #[test]
    fn test_debug_with_geo() {
        let args = vec!["sunsetr", "--debug", "--geo"];
        let parsed = ParsedArgs::parse(args);
        // Order doesn't matter
        assert_eq!(
            parsed.action,
            CliAction::RunGeoSelection {
                debug_enabled: true,
                config_dir: None
            }
        );
    }

    #[test]
    fn test_debug_with_test_subcommand() {
        let args = vec!["sunsetr", "-d", "test", "2333", "70"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::TestCommand {
                debug_enabled: true,
                temperature: 2333,
                gamma: 70.0,
                config_dir: None
            }
        );
    }

    #[test]
    fn test_test_subcommand_with_debug_after() {
        let args = vec!["sunsetr", "test", "2333", "70", "-d"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::TestCommand {
                debug_enabled: true,
                temperature: 2333,
                gamma: 70.0,
                config_dir: None
            }
        );
    }
}
