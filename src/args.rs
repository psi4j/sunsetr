//! Command-line argument parsing and processing.
//!
//! This module handles parsing of command-line arguments and provides a clean
//! interface for the main application logic. It supports the standard help,
//! version, and debug flags while gracefully handling unknown options.

/// Represents preset-related subcommands
#[derive(Debug, PartialEq)]
pub enum PresetSubcommand {
    /// Apply a preset configuration
    Apply { name: String },
    /// Get the currently active preset
    Active,
    /// List available presets
    List,
    // Future subcommands:
    // New { name: String, template: Option<String> },
    // Delete { name: String },
    // Export { name: String, path: PathBuf },
    // Import { path: PathBuf },
    // Validate { name: String },
}

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
    /// Preset subcommand with nested subcommands
    PresetCommand {
        debug_enabled: bool,
        subcommand: PresetSubcommand,
        config_dir: Option<String>,
    },
    /// Set configuration field subcommand
    SetCommand {
        debug_enabled: bool,
        fields: Vec<(String, String)>, // Multiple field-value pairs
        config_dir: Option<String>,
        target: Option<String>, // Target configuration (None = active, Some("default") = base, Some(name) = preset)
    },
    /// Get configuration field subcommand
    GetCommand {
        debug_enabled: bool,
        fields: Vec<String>, // Field names to retrieve
        config_dir: Option<String>,
        target: Option<String>, // Target configuration (None = active, Some("default") = base, Some(name) = preset)
        json: bool,             // Output in JSON format
    },
    /// Stop using subcommand syntax
    StopCommand {
        debug_enabled: bool,
        config_dir: Option<String>,
    },
    /// Display detailed help for a specific command or general help
    HelpCommand { command: Option<String> },
    /// Display usage help for a specific command (--help flag in command context)
    UsageHelp { command: String },

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
    /// Show command-specific usage due to error
    ShowCommandUsageDueToError {
        command: String,
        error_message: String,
    },
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
                // Check if --help flag is in context of a specific command
                if command != "help" && !command.starts_with('-') {
                    // Show usage help for this specific command (brief)
                    return ParsedArgs {
                        action: CliAction::UsageHelp {
                            command: command.clone(),
                        },
                    };
                } else {
                    return ParsedArgs {
                        action: CliAction::ShowHelp,
                    };
                }
            }

            // Handle help command first (before checking for multiple commands)
            if command == "help" || command == "h" {
                if cmd_idx + 1 < args_vec.len() && !args_vec[cmd_idx + 1].starts_with('-') {
                    // Help for specific command
                    return ParsedArgs {
                        action: CliAction::HelpCommand {
                            command: Some(args_vec[cmd_idx + 1].clone()),
                        },
                    };
                } else {
                    // General help
                    return ParsedArgs {
                        action: CliAction::HelpCommand { command: None },
                    };
                }
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
                        "get"
                            | "g"
                            | "geo"
                            | "G"
                            | "help"
                            | "h"
                            | "preset"
                            | "p"
                            | "reload"
                            | "r"
                            | "set"
                            | "s"
                            | "stop"
                            | "S"
                            | "test"
                            | "t"
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
                "geo" | "G" => {
                    // Geo takes no arguments (interactive), check immediately after
                    check_for_multiple_commands(cmd_idx + 1)
                }
                "stop" | "S" => {
                    // Stop takes no arguments, check immediately after
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
                    // Preset takes 1 argument (subcommand or preset name), check after that
                    if cmd_idx + 1 < args_vec.len() {
                        check_for_multiple_commands(cmd_idx + 2)
                    } else {
                        None
                    }
                }
                "set" | "s" | "get" | "g" => {
                    // These commands parse their own arguments including flags
                    // We can't easily determine where their arguments end
                    // without duplicating their parsing logic
                    None
                }
                _ => None,
            };

            if let Some(conflict) = conflicting_command {
                log_warning_standalone!(
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
                "stop" | "S" => {
                    return ParsedArgs {
                        action: CliAction::StopCommand {
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
                            return ParsedArgs {
                                action: CliAction::ShowCommandUsageDueToError {
                                    command: "test".to_string(),
                                    error_message: "Invalid test arguments".to_string(),
                                },
                            };
                        }
                    } else {
                        return ParsedArgs {
                            action: CliAction::ShowCommandUsageDueToError {
                                command: "test".to_string(),
                                error_message: "Missing arguments for test command".to_string(),
                            },
                        };
                    }
                }
                "geo" | "G" => {
                    return ParsedArgs {
                        action: CliAction::GeoCommand {
                            debug_enabled,
                            config_dir,
                        },
                    };
                }
                "preset" | "p" => {
                    // Parse preset subcommand: preset [active|list|<name>]
                    if cmd_idx + 1 < args_vec.len() && !args_vec[cmd_idx + 1].starts_with('-') {
                        let subcommand_or_name = &args_vec[cmd_idx + 1];

                        let subcommand = match subcommand_or_name.as_str() {
                            "active" => PresetSubcommand::Active,
                            "list" => PresetSubcommand::List,
                            // Any other string is treated as a preset name to apply
                            name => PresetSubcommand::Apply {
                                name: name.to_string(),
                            },
                        };

                        return ParsedArgs {
                            action: CliAction::PresetCommand {
                                debug_enabled,
                                subcommand,
                                config_dir,
                            },
                        };
                    } else {
                        // No arguments provided - show error with helpful info
                        return ParsedArgs {
                            action: CliAction::ShowCommandUsageDueToError {
                                command: "preset".to_string(),
                                error_message: "Missing subcommand or preset name".to_string(),
                            },
                        };
                    }
                }
                "set" | "s" => {
                    // Parse set command: set [--target <name>] <field>=<value> [<field>=<value>...] [--target <name>]
                    let mut fields = Vec::new();
                    let mut idx = cmd_idx + 1;
                    let mut target: Option<String> = None;

                    // Parse all arguments, allowing --target flag to appear anywhere
                    while idx < args_vec.len() {
                        let arg = &args_vec[idx];

                        if arg == "--target" || arg == "-t" {
                            if idx + 1 < args_vec.len() && !args_vec[idx + 1].starts_with('-') {
                                target = Some(args_vec[idx + 1].clone());
                                idx += 2; // Skip the flag and its value
                            } else {
                                return ParsedArgs {
                                    action: CliAction::ShowCommandUsageDueToError {
                                        command: "set".to_string(),
                                        error_message: "Missing target name for --target flag"
                                            .to_string(),
                                    },
                                };
                            }
                        } else if arg.starts_with('-') {
                            // Unknown flag
                            return ParsedArgs {
                                action: CliAction::ShowCommandUsageDueToError {
                                    command: "set".to_string(),
                                    error_message: format!("Unknown flag: {}", arg),
                                },
                            };
                        } else {
                            // Check for equals sign (field=value syntax)
                            if let Some(eq_pos) = arg.find('=') {
                                let field = arg[..eq_pos].to_string();
                                let value = arg[eq_pos + 1..].to_string();

                                // Validate field and value are not empty
                                if field.is_empty() || value.is_empty() {
                                    return ParsedArgs {
                                        action: CliAction::ShowCommandUsageDueToError {
                                            command: "set".to_string(),
                                            error_message: format!("Invalid syntax: '{}'", arg),
                                        },
                                    };
                                }

                                fields.push((field, value));
                            } else {
                                return ParsedArgs {
                                    action: CliAction::ShowCommandUsageDueToError {
                                        command: "set".to_string(),
                                        error_message: format!(
                                            "Invalid syntax: '{}'. Expected 'field=value' format",
                                            arg
                                        ),
                                    },
                                };
                            }
                            idx += 1;
                        }
                    }

                    if fields.is_empty() {
                        return ParsedArgs {
                            action: CliAction::ShowCommandUsageDueToError {
                                command: "set".to_string(),
                                error_message: "Missing field=value pairs".to_string(),
                            },
                        };
                    }

                    return ParsedArgs {
                        action: CliAction::SetCommand {
                            debug_enabled,
                            fields,
                            config_dir,
                            target,
                        },
                    };
                }
                "get" | "g" => {
                    // Parse get command: get [--target <name>] [--json] <field> [<field>...] [--json]
                    let mut fields = Vec::new();
                    let mut idx = cmd_idx + 1;
                    let mut target: Option<String> = None;
                    let mut json_output = false;

                    // Parse all arguments, allowing flags to appear anywhere
                    while idx < args_vec.len() {
                        let arg = &args_vec[idx];

                        if arg == "--target" || arg == "-t" {
                            if idx + 1 < args_vec.len() && !args_vec[idx + 1].starts_with('-') {
                                target = Some(args_vec[idx + 1].clone());
                                idx += 2; // Skip the flag and its value
                            } else {
                                return ParsedArgs {
                                    action: CliAction::ShowCommandUsageDueToError {
                                        command: "get".to_string(),
                                        error_message: "Missing target name".to_string(),
                                    },
                                };
                            }
                        } else if arg == "--json" || arg == "-j" {
                            json_output = true;
                            idx += 1;
                        } else if arg.starts_with('-') {
                            // Unknown flag
                            return ParsedArgs {
                                action: CliAction::ShowCommandUsageDueToError {
                                    command: "get".to_string(),
                                    error_message: format!("Unknown flag: {}", arg),
                                },
                            };
                        } else {
                            // This is a field name
                            fields.push(arg.clone());
                            idx += 1;
                        }
                    }

                    if fields.is_empty() {
                        return ParsedArgs {
                            action: CliAction::ShowCommandUsageDueToError {
                                command: "get".to_string(),
                                error_message: "Missing field names".to_string(),
                            },
                        };
                    }

                    return ParsedArgs {
                        action: CliAction::GetCommand {
                            debug_enabled,
                            fields,
                            config_dir,
                            target,
                            json: json_output,
                        },
                    };
                }
                _ => {
                    // Unknown subcommand - show error and help
                    log_warning_standalone!("Unknown command: {}", command);
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
                        log_error_standalone!(
                            "Missing directory for --config. Usage: --config <directory>"
                        );
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
                                log_error_standalone!(
                                    "Invalid temperature value: {}",
                                    args_vec[i + 1]
                                );
                                unknown_arg_found = true;
                            }
                        }

                        match args_vec[i + 2].parse::<f32>() {
                            Ok(gamma) => test_gamma = Some(gamma),
                            Err(_) => {
                                log_error_standalone!("Invalid gamma value: {}", args_vec[i + 2]);
                                unknown_arg_found = true;
                            }
                        }

                        i += 2; // Skip the parsed arguments
                    } else {
                        log_error_standalone!(
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
                            log_error_standalone!(
                                "Invalid start time format: '{}'. Use YYYY-MM-DD HH:MM:SS",
                                start_str
                            );
                            unknown_arg_found = true;
                            i += 2; // Skip the parsed arguments
                        } else if !validate_datetime(&end_str) {
                            log_error_standalone!(
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
                                        log_error_standalone!(
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
                        log_error_standalone!(
                            "Missing arguments for --simulate. Usage: --simulate \"YYYY-MM-DD HH:MM:SS\" \"YYYY-MM-DD HH:MM:SS\" [multiplier | --fast-forward] [--log]"
                        );
                        unknown_arg_found = true;
                    }
                }
                _ => {
                    // Check if the argument starts with a dash, indicating it's an option
                    if arg_str.starts_with('-') {
                        log_warning_standalone!("Unknown option: {arg_str}");
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
                    log_error_standalone!("Missing temperature or gamma values for test");
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
                    log_error_standalone!("Missing start or end time for --simulate");
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
    log_block_start!("Usage: sunsetr [OPTIONS] [COMMAND]");
    log_block_start!("Options:");
    log_indented!("-c, --config <dir>      Use custom configuration directory");
    log_indented!("-d, --debug             Enable detailed debug output");
    log_indented!("-h, --help              Print help information");
    log_indented!("-S, --simulate          Run with simulated time (for testing transitions)");
    log_indented!("                        Usage: --simulate <start> <end> [mult] [--log]");
    log_indented!("-V, --version           Print version information");
    log_block_start!("Commands:");
    log_indented!("geo, G                  Interactive city selection for geo mode");
    log_indented!("get, g <field>          Read configuration field(s)");
    log_indented!("help, h [COMMAND]       Show help for a specific command");
    log_indented!("preset, p <name>        Apply a named preset configuration");
    log_indented!("reload, r               Reset display gamma and reload configuration");
    log_indented!("set, s <field>=<value>  Update configuration field(s)");
    log_indented!("stop, S                 Cleanly terminate running sunsetr instance");
    log_indented!("test, t <temp> <gamma>  Test specific temperature and gamma values");
    log_pipe!();
    log_info!("See 'sunsetr help <command>' for more information on a specific command.");
    log_block_start!("Deprecated flags (will be removed in v1.0.0):");
    log_indented!("-r, --reload            Use 'sunsetr reload' instead");
    log_indented!("-t, --test              Use 'sunsetr test' instead");
    log_indented!("-g, --geo               Use 'sunsetr geo' instead");
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

    #[test]
    fn test_get_command_json_flag_before_field() {
        let args = vec!["sunsetr", "get", "--json", "day_temp"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::GetCommand {
                debug_enabled: false,
                fields: vec!["day_temp".to_string()],
                config_dir: None,
                target: None,
                json: true,
            }
        );
    }

    #[test]
    fn test_get_command_json_flag_after_field() {
        let args = vec!["sunsetr", "get", "day_temp", "--json"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::GetCommand {
                debug_enabled: false,
                fields: vec!["day_temp".to_string()],
                config_dir: None,
                target: None,
                json: true,
            }
        );
    }

    #[test]
    fn test_get_command_json_flag_between_fields() {
        let args = vec!["sunsetr", "get", "day_temp", "--json", "night_temp"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::GetCommand {
                debug_enabled: false,
                fields: vec!["day_temp".to_string(), "night_temp".to_string()],
                config_dir: None,
                target: None,
                json: true,
            }
        );
    }

    #[test]
    fn test_get_command_short_json_flag_after() {
        let args = vec!["sunsetr", "get", "day_temp", "-j"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::GetCommand {
                debug_enabled: false,
                fields: vec!["day_temp".to_string()],
                config_dir: None,
                target: None,
                json: true,
            }
        );
    }

    #[test]
    fn test_get_command_multiple_fields_json_at_end() {
        let args = vec![
            "sunsetr",
            "get",
            "day_temp",
            "night_temp",
            "gamma",
            "--json",
        ];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::GetCommand {
                debug_enabled: false,
                fields: vec![
                    "day_temp".to_string(),
                    "night_temp".to_string(),
                    "gamma".to_string()
                ],
                config_dir: None,
                target: None,
                json: true,
            }
        );
    }

    #[test]
    fn test_get_command_with_target_and_json_at_end() {
        let args = vec!["sunsetr", "get", "--target", "gaming", "day_temp", "--json"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::GetCommand {
                debug_enabled: false,
                fields: vec!["day_temp".to_string()],
                config_dir: None,
                target: Some("gaming".to_string()),
                json: true,
            }
        );
    }

    #[test]
    fn test_set_command_target_flag_before_fields() {
        let args = vec!["sunsetr", "set", "--target", "gaming", "day_temp=5000"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::SetCommand {
                debug_enabled: false,
                fields: vec![("day_temp".to_string(), "5000".to_string())],
                config_dir: None,
                target: Some("gaming".to_string()),
            }
        );
    }

    #[test]
    fn test_set_command_target_flag_after_fields() {
        let args = vec!["sunsetr", "set", "day_temp=5000", "--target", "gaming"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::SetCommand {
                debug_enabled: false,
                fields: vec![("day_temp".to_string(), "5000".to_string())],
                config_dir: None,
                target: Some("gaming".to_string()),
            }
        );
    }

    #[test]
    fn test_set_command_target_flag_between_fields() {
        let args = vec![
            "sunsetr",
            "set",
            "day_temp=5000",
            "--target",
            "gaming",
            "night_temp=2800",
        ];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::SetCommand {
                debug_enabled: false,
                fields: vec![
                    ("day_temp".to_string(), "5000".to_string()),
                    ("night_temp".to_string(), "2800".to_string())
                ],
                config_dir: None,
                target: Some("gaming".to_string()),
            }
        );
    }

    #[test]
    fn test_set_command_short_target_flag_after_fields() {
        let args = vec![
            "sunsetr",
            "set",
            "day_temp=5000",
            "gamma=0.9",
            "-t",
            "gaming",
        ];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::SetCommand {
                debug_enabled: false,
                fields: vec![
                    ("day_temp".to_string(), "5000".to_string()),
                    ("gamma".to_string(), "0.9".to_string())
                ],
                config_dir: None,
                target: Some("gaming".to_string()),
            }
        );
    }
}
