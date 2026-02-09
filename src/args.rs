//! Command-line argument parsing and processing.
//!
//! This module handles parsing of command-line arguments and provides a clean
//! interface for the main application logic. It supports the standard help,
//! version, and debug flags while gracefully handling unknown options.

/// Represents preset-related subcommands
#[derive(Debug, PartialEq)]
pub enum PresetSubcommand {
    Apply { name: String },
    Active,
    List,
    // New { name: Option<String>, from: Option<String> },  // Interactive preset builder
}

/// Operator for set command field assignments.
///
/// Determines how the provided value is applied to the configuration field:
/// - `Assign`: Sets the field to the exact value (`field=value`)
/// - `Increment`: Adds the value to the current field value (`field+=value`)
/// - `Decrement`: Subtracts the value from the current field value (`field-=value`)
///
/// Increment and decrement operators are only supported for temperature and gamma fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SetOperator {
    Assign,
    Increment,
    Decrement,
}

/// Represents the parsed command-line arguments and their intended actions.
#[derive(Debug, PartialEq)]
pub enum CliAction {
    /// Run the normal application with these settings
    Run {
        debug_enabled: bool,
        config_dir: Option<String>,
        background: bool,
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
        fields: Vec<(String, SetOperator, String)>,
        config_dir: Option<String>,
        target: Option<String>,
    },

    /// Get configuration field subcommand
    GetCommand {
        debug_enabled: bool,
        fields: Vec<String>,
        config_dir: Option<String>,
        target: Option<String>,
        json: bool,
    },

    /// Stop using subcommand syntax
    StopCommand {
        debug_enabled: bool,
        config_dir: Option<String>,
    },

    /// Restart using subcommand syntax
    RestartCommand {
        debug_enabled: bool,
        instant: bool,
        config_dir: Option<String>,
        background: bool,
    },

    /// Status command - display current runtime state
    StatusCommand {
        debug_enabled: bool,
        config_dir: Option<String>,
        json: bool,
        follow: bool,
    },

    /// Display detailed help for a specific command or general help
    HelpCommand { command: Option<String> },

    /// Display usage help for a specific command (--help flag in command context)
    UsageHelp { command: String },

    // # Flag-style actions (deprecated, remove in v1.0.0)
    /// Run interactive geo location selection (deprecated --geo flag)
    RunGeoSelection {
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
        let mut background = false;

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
                if matches!(arg.as_str(), "--config" | "-c") {
                    idx += 2;
                } else if matches!(arg.as_str(), "--simulate" | "-S" | "--test" | "-t") {
                    break;
                } else {
                    idx += 1;
                }
            } else {
                potential_command_idx = Some(idx);
                break;
            }
        }

        if let Some(cmd_idx) = potential_command_idx {
            let command = &args_vec[cmd_idx];

            let debug_enabled = args_vec.iter().any(|arg| arg == "--debug" || arg == "-d");
            let background = args_vec
                .iter()
                .any(|arg| arg == "--background" || arg == "-b");

            let config_dir = args_vec
                .iter()
                .position(|arg| arg == "--config" || arg == "-c")
                .and_then(|idx| args_vec.get(idx + 1))
                .cloned();

            if args_vec
                .iter()
                .any(|arg| arg == "--version" || arg == "-V" || arg == "-v")
            {
                return ParsedArgs {
                    action: CliAction::ShowVersion,
                };
            }
            if args_vec.iter().any(|arg| arg == "--help" || arg == "-h") {
                if command != "help" && !command.starts_with('-') {
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

            if command == "help" || command == "h" {
                if cmd_idx + 1 < args_vec.len() && !args_vec[cmd_idx + 1].starts_with('-') {
                    return ParsedArgs {
                        action: CliAction::HelpCommand {
                            command: Some(args_vec[cmd_idx + 1].clone()),
                        },
                    };
                } else {
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
                        continue;
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
                            | "restart"
                            | "r"
                            | "set"
                            | "s"
                            | "stop"
                            | "status"
                            | "S"
                            | "test"
                            | "t"
                    ) {
                        return Some(arg.clone());
                    }
                }
                None
            };

            let conflicting_command = match command.as_str() {
                "reload" => check_for_multiple_commands(cmd_idx + 1),
                "restart" | "r" => {
                    let next_idx = if cmd_idx + 1 < args_vec.len()
                        && (args_vec[cmd_idx + 1] == "--instant" || args_vec[cmd_idx + 1] == "-i")
                    {
                        cmd_idx + 2
                    } else {
                        cmd_idx + 1
                    };
                    check_for_multiple_commands(next_idx)
                }
                "geo" | "G" => check_for_multiple_commands(cmd_idx + 1),
                "stop" => check_for_multiple_commands(cmd_idx + 1),
                "test" | "t" => {
                    if cmd_idx + 2 < args_vec.len() {
                        check_for_multiple_commands(cmd_idx + 3)
                    } else {
                        None
                    }
                }
                "preset" | "p" => {
                    if cmd_idx + 1 < args_vec.len() {
                        check_for_multiple_commands(cmd_idx + 2)
                    } else {
                        None
                    }
                }
                "set" | "s" | "get" | "g" | "status" | "S" => None,
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
                "reload" => {
                    log_warning_standalone!(
                        "'sunsetr reload' is deprecated and will be removed in v1.0.0\n\n\
                        Sunsetr now has hot reloading for configuration changes.\n\
                        Use 'sunsetr restart' when you need to re-initialize the application.\n\n\
                        Note: 'restart' runs in foreground by default (breaking change).\n\
                        Use 'sunsetr --background restart' for the old background behavior."
                    );

                    return ParsedArgs {
                        action: CliAction::RestartCommand {
                            debug_enabled,
                            instant: false,
                            config_dir,
                            background: true,
                        },
                    };
                }
                "restart" | "r" => {
                    let instant = cmd_idx + 1 < args_vec.len()
                        && (args_vec[cmd_idx + 1] == "--instant" || args_vec[cmd_idx + 1] == "-i");

                    return ParsedArgs {
                        action: CliAction::RestartCommand {
                            debug_enabled,
                            instant,
                            config_dir,
                            background,
                        },
                    };
                }
                "stop" => {
                    return ParsedArgs {
                        action: CliAction::StopCommand {
                            debug_enabled,
                            config_dir,
                        },
                    };
                }
                "test" | "t" => {
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
                    if cmd_idx + 1 < args_vec.len() && !args_vec[cmd_idx + 1].starts_with('-') {
                        let subcommand_or_name = &args_vec[cmd_idx + 1];

                        let subcommand = match subcommand_or_name.as_str() {
                            "active" => PresetSubcommand::Active,
                            "list" => PresetSubcommand::List,
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
                        return ParsedArgs {
                            action: CliAction::ShowCommandUsageDueToError {
                                command: "preset".to_string(),
                                error_message: "Missing subcommand or preset name".to_string(),
                            },
                        };
                    }
                }
                "set" | "s" => {
                    let mut fields = Vec::new();
                    let mut idx = cmd_idx + 1;
                    let mut target: Option<String> = None;

                    while idx < args_vec.len() {
                        let arg = &args_vec[idx];

                        if arg == "--target" || arg == "-t" {
                            if idx + 1 < args_vec.len() && !args_vec[idx + 1].starts_with('-') {
                                target = Some(args_vec[idx + 1].clone());
                                idx += 2;
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
                            return ParsedArgs {
                                action: CliAction::ShowCommandUsageDueToError {
                                    command: "set".to_string(),
                                    error_message: format!("Unknown flag: {}", arg),
                                },
                            };
                        } else {
                            let parsed = if let Some(pos) = arg.find("+=") {
                                Some((
                                    arg[..pos].to_string(),
                                    SetOperator::Increment,
                                    arg[pos + 2..].to_string(),
                                ))
                            } else if let Some(pos) = arg.find("-=") {
                                Some((
                                    arg[..pos].to_string(),
                                    SetOperator::Decrement,
                                    arg[pos + 2..].to_string(),
                                ))
                            } else {
                                arg.find('=').map(|pos| {
                                    (
                                        arg[..pos].to_string(),
                                        SetOperator::Assign,
                                        arg[pos + 1..].to_string(),
                                    )
                                })
                            };

                            if let Some((field, op, value)) = parsed {
                                if field.is_empty() || value.is_empty() {
                                    return ParsedArgs {
                                        action: CliAction::ShowCommandUsageDueToError {
                                            command: "set".to_string(),
                                            error_message: format!("Invalid syntax: '{}'", arg),
                                        },
                                    };
                                }

                                fields.push((field, op, value));
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
                    let mut fields = Vec::new();
                    let mut idx = cmd_idx + 1;
                    let mut target: Option<String> = None;
                    let mut json_output = false;

                    while idx < args_vec.len() {
                        let arg = &args_vec[idx];

                        if arg == "--target" || arg == "-t" {
                            if idx + 1 < args_vec.len() && !args_vec[idx + 1].starts_with('-') {
                                target = Some(args_vec[idx + 1].clone());
                                idx += 2;
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
                            return ParsedArgs {
                                action: CliAction::ShowCommandUsageDueToError {
                                    command: "get".to_string(),
                                    error_message: format!("Unknown flag: {}", arg),
                                },
                            };
                        } else {
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
                "status" | "S" => {
                    let mut json_output = false;
                    let mut follow = false;
                    let mut status_debug = debug_enabled;
                    let mut status_config_dir = config_dir.clone();

                    let mut i = 1;
                    while i < args_vec.len() {
                        match args_vec[i].as_str() {
                            "--json" | "-j" => json_output = true,
                            "--follow" | "-f" => follow = true,
                            "--debug" | "-d" => status_debug = true,
                            "--config" | "-c" => {
                                if i + 1 < args_vec.len() && !args_vec[i + 1].starts_with('-') {
                                    status_config_dir = Some(args_vec[i + 1].clone());
                                    i += 1;
                                } else {
                                    return ParsedArgs {
                                        action: CliAction::ShowCommandUsageDueToError {
                                            command: "status".to_string(),
                                            error_message: "Missing directory for --config"
                                                .to_string(),
                                        },
                                    };
                                }
                            }
                            "--help" | "-h" => {
                                return ParsedArgs {
                                    action: CliAction::UsageHelp {
                                        command: "status".to_string(),
                                    },
                                };
                            }
                            arg if arg.starts_with('-') => {
                                return ParsedArgs {
                                    action: CliAction::ShowCommandUsageDueToError {
                                        command: "status".to_string(),
                                        error_message: format!("Unknown flag: {}", arg),
                                    },
                                };
                            }
                            _ => {
                                return ParsedArgs {
                                    action: CliAction::ShowCommandUsageDueToError {
                                        command: "status".to_string(),
                                        error_message: format!(
                                            "Unexpected argument: {}",
                                            args_vec[i]
                                        ),
                                    },
                                };
                            }
                        }
                        i += 1;
                    }

                    return ParsedArgs {
                        action: CliAction::StatusCommand {
                            debug_enabled: status_debug,
                            config_dir: status_config_dir,
                            json: json_output,
                            follow,
                        },
                    };
                }
                _ => {
                    log_warning_standalone!("Unknown command: {}", command);
                    return ParsedArgs {
                        action: CliAction::ShowHelpDueToError,
                    };
                }
            }
        }

        let mut i = 0;
        while i < args_vec.len() {
            let arg_str = &args_vec[i];
            match arg_str.as_str() {
                "--help" | "-h" => display_help = true,
                "--version" | "-V" | "-v" => display_version = true,
                "--debug" | "-d" => debug_enabled = true,
                "--background" | "-b" => background = true,
                "--config" | "-c" => {
                    if i + 1 < args_vec.len() && !args_vec[i + 1].starts_with('-') {
                        config_dir = Some(args_vec[i + 1].clone());
                        i += 1;
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
                    log_warning_standalone!(
                        "'--reload' is deprecated and will be removed in v1.0.0\n\n\
                        Sunsetr now has hot reloading for configuration changes.\n\
                        Use 'sunsetr restart' when you need to re-initialize the application.\n\n\
                        Note: 'restart' runs in foreground by default (breaking change).\n\
                        Use 'sunsetr --background restart' for the old background behavior."
                    );
                    run_reload = true;
                }
                "--test" | "-t" => {
                    show_deprecation_warning(arg_str, "sunsetr test");
                    run_test = true;
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

                        i += 2;
                    } else {
                        log_error_standalone!(
                            "Missing arguments for test. Usage: test <temperature> <gamma>"
                        );
                        unknown_arg_found = true;
                    }
                }
                "--simulate" | "-S" => {
                    run_simulate = true;
                    if i + 2 < args_vec.len() {
                        let start_str = args_vec[i + 1].clone();
                        let end_str = args_vec[i + 2].clone();

                        let validate_datetime = |s: &str| -> bool {
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
                            i += 2;
                        } else if !validate_datetime(&end_str) {
                            log_error_standalone!(
                                "Invalid end time format: '{}'. Use YYYY-MM-DD HH:MM:SS",
                                end_str
                            );
                            unknown_arg_found = true;
                            i += 2;
                        } else {
                            simulate_start = Some(start_str);
                            simulate_end = Some(end_str);
                            i += 2;

                            if i + 1 < args_vec.len() && !args_vec[i + 1].starts_with('-') {
                                if let Ok(mult) = args_vec[i + 1].parse::<f64>() {
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
                                simulate_multiplier = Some(-1.0);
                                i += 1;
                            }

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
                    if arg_str.starts_with('-') {
                        log_warning_standalone!("Unknown option: {arg_str}");
                        unknown_arg_found = true;
                    }
                }
            }
            i += 1;
        }

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
            CliAction::RestartCommand {
                debug_enabled,
                instant: false,
                config_dir,
                background: true,
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
                    multiplier: simulate_multiplier.unwrap_or(0.0),
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
                background,
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
    log_indented!("-b, --background        Run process in background");
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
    log_indented!("restart, r [--instant]  Recreate backend and reload configuration");
    log_indented!("set, s <field>[op]=val  Update configuration field(s)");
    log_indented!("status, S               Display current runtime state");
    log_indented!("stop                    Cleanly terminate running sunsetr instance");
    log_indented!("test, t <temp> <gamma>  Test specific temperature and gamma values");
    log_pipe!();
    log_info!("See 'sunsetr help <command>' for more information on a specific command.");
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
                background: false,
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
                background: false,
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
                background: false,
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
                fields: vec![(
                    "day_temp".to_string(),
                    SetOperator::Assign,
                    "5000".to_string()
                )],
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
                fields: vec![(
                    "day_temp".to_string(),
                    SetOperator::Assign,
                    "5000".to_string()
                )],
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
                    (
                        "day_temp".to_string(),
                        SetOperator::Assign,
                        "5000".to_string()
                    ),
                    (
                        "night_temp".to_string(),
                        SetOperator::Assign,
                        "2800".to_string()
                    )
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
                    (
                        "day_temp".to_string(),
                        SetOperator::Assign,
                        "5000".to_string()
                    ),
                    ("gamma".to_string(), SetOperator::Assign, "0.9".to_string())
                ],
                config_dir: None,
                target: Some("gaming".to_string()),
            }
        );
    }

    #[test]
    fn test_set_command_increment_operator() {
        let args = vec!["sunsetr", "set", "night_temp+=500"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::SetCommand {
                debug_enabled: false,
                fields: vec![(
                    "night_temp".to_string(),
                    SetOperator::Increment,
                    "500".to_string()
                )],
                config_dir: None,
                target: None,
            }
        );
    }

    #[test]
    fn test_set_command_decrement_operator() {
        let args = vec!["sunsetr", "set", "static_gamma-=2"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::SetCommand {
                debug_enabled: false,
                fields: vec![(
                    "static_gamma".to_string(),
                    SetOperator::Decrement,
                    "2".to_string()
                )],
                config_dir: None,
                target: None,
            }
        );
    }

    #[test]
    fn test_set_command_mixed_operators() {
        let args = vec![
            "sunsetr",
            "set",
            "night_temp+=200",
            "day_temp=6500",
            "static_gamma-=5",
        ];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::SetCommand {
                debug_enabled: false,
                fields: vec![
                    (
                        "night_temp".to_string(),
                        SetOperator::Increment,
                        "200".to_string()
                    ),
                    (
                        "day_temp".to_string(),
                        SetOperator::Assign,
                        "6500".to_string()
                    ),
                    (
                        "static_gamma".to_string(),
                        SetOperator::Decrement,
                        "5".to_string()
                    ),
                ],
                config_dir: None,
                target: None,
            }
        );
    }

    #[test]
    fn test_set_command_increment_with_decimal() {
        let args = vec!["sunsetr", "set", "night_gamma+=5.5"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::SetCommand {
                debug_enabled: false,
                fields: vec![(
                    "night_gamma".to_string(),
                    SetOperator::Increment,
                    "5.5".to_string()
                )],
                config_dir: None,
                target: None,
            }
        );
    }

    #[test]
    fn test_set_command_increment_with_target() {
        let args = vec!["sunsetr", "set", "--target", "gaming", "night_temp+=500"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::SetCommand {
                debug_enabled: false,
                fields: vec![(
                    "night_temp".to_string(),
                    SetOperator::Increment,
                    "500".to_string()
                )],
                config_dir: None,
                target: Some("gaming".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_background_flag() {
        let args = vec!["sunsetr", "--background"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::Run {
                debug_enabled: false,
                config_dir: None,
                background: true,
            }
        );
    }

    #[test]
    fn test_parse_background_short_flag() {
        let args = vec!["sunsetr", "-b"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::Run {
                debug_enabled: false,
                config_dir: None,
                background: true,
            }
        );
    }

    #[test]
    fn test_background_with_debug() {
        let args = vec!["sunsetr", "--background", "--debug"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::Run {
                debug_enabled: true,
                config_dir: None,
                background: true,
            }
        );
    }

    #[test]
    fn test_background_restart() {
        let args = vec!["sunsetr", "--background", "restart"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::RestartCommand {
                debug_enabled: false,
                instant: false,
                config_dir: None,
                background: true,
            }
        );
    }

    #[test]
    fn test_background_restart_instant() {
        let args = vec!["sunsetr", "-b", "restart", "--instant"];
        let parsed = ParsedArgs::parse(args);
        assert_eq!(
            parsed.action,
            CliAction::RestartCommand {
                debug_enabled: false,
                instant: true,
                config_dir: None,
                background: true,
            }
        );
    }
}
