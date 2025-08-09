//! Command-line argument parsing and processing.
//!
//! This module handles parsing of command-line arguments and provides a clean
//! interface for the main application logic. It supports the standard help,
//! version, and debug flags while gracefully handling unknown options.

use crate::logger::Log;

/// Represents the parsed command-line arguments and their intended actions.
#[derive(Debug, PartialEq)]
pub enum CliAction {
    /// Run the normal application with these settings
    Run { debug_enabled: bool },
    /// Run interactive geo location selection
    RunGeoSelection { debug_enabled: bool },
    /// Reset all display gamma and reload sunsetr
    Reload { debug_enabled: bool },
    /// Test specific temperature and gamma values
    Test {
        debug_enabled: bool,
        temperature: u32,
        gamma: f32,
    },
    /// Simulate time passing for testing
    Simulate {
        debug_enabled: bool,
        start_time: String,
        end_time: String,
        multiplier: f64,
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

impl ParsedArgs {
    /// Parse command-line arguments into a structured result.
    ///
    /// This function processes the arguments and determines what action should
    /// be taken, including whether to show help, version info, or run normally.
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
        let mut unknown_arg_found = false;

        // Convert to vector for easier indexed access
        let args_vec: Vec<String> = args
            .into_iter()
            .skip(1)
            .map(|s| s.as_ref().to_string())
            .collect();

        let mut i = 0;
        while i < args_vec.len() {
            let arg_str = &args_vec[i];
            match arg_str.as_str() {
                "--help" | "-h" => display_help = true,
                "--version" | "-V" | "-v" => display_version = true,
                "--debug" | "-d" => debug_enabled = true,
                "--geo" | "-g" => run_geo_selection = true,
                "--reload" | "-r" => run_reload = true,
                "--test" | "-t" => {
                    run_test = true;
                    // Parse: --test <temperature> <gamma>
                    if i + 2 < args_vec.len() {
                        match args_vec[i + 1].parse::<u32>() {
                            Ok(temp) => test_temperature = Some(temp),
                            Err(_) => {
                                Log::log_warning(&format!(
                                    "Invalid temperature value: {}",
                                    args_vec[i + 1]
                                ));
                                unknown_arg_found = true;
                            }
                        }

                        match args_vec[i + 2].parse::<f32>() {
                            Ok(gamma) => test_gamma = Some(gamma),
                            Err(_) => {
                                Log::log_warning(&format!(
                                    "Invalid gamma value: {}",
                                    args_vec[i + 2]
                                ));
                                unknown_arg_found = true;
                            }
                        }

                        i += 2; // Skip the parsed arguments
                    } else {
                        Log::log_warning(
                            "Missing arguments for --test. Usage: --test <temperature> <gamma>",
                        );
                        unknown_arg_found = true;
                    }
                }
                "--simulate" | "-S" => {
                    run_simulate = true;
                    // Parse: --simulate <start_time> <end_time> [multiplier | --fast-forward]
                    if i + 2 < args_vec.len() {
                        simulate_start = Some(args_vec[i + 1].clone());
                        simulate_end = Some(args_vec[i + 2].clone());
                        i += 2; // Skip the parsed arguments

                        // Check for optional multiplier or --fast-forward flag
                        if i + 1 < args_vec.len() && !args_vec[i + 1].starts_with('-') {
                            if let Ok(mult) = args_vec[i + 1].parse::<f64>() {
                                simulate_multiplier = Some(mult);
                                i += 1;
                            }
                        } else if i + 1 < args_vec.len() && args_vec[i + 1] == "--fast-forward" {
                            // Use a special marker value to indicate fast-forward mode
                            simulate_multiplier = Some(-1.0);
                            i += 1;
                        }
                    } else {
                        Log::log_warning(
                            "Missing arguments for --simulate. Usage: --simulate \"YYYY-MM-DD HH:MM:SS\" \"YYYY-MM-DD HH:MM:SS\" [multiplier | --fast-forward]",
                        );
                        unknown_arg_found = true;
                    }
                }
                _ => {
                    // Check if the argument starts with a dash, indicating it's an option
                    if arg_str.starts_with('-') {
                        Log::log_warning(&format!("Unknown option: {arg_str}"));
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
            CliAction::RunGeoSelection { debug_enabled }
        } else if run_reload {
            CliAction::Reload { debug_enabled }
        } else if run_test {
            match (test_temperature, test_gamma) {
                (Some(temp), Some(gamma)) => CliAction::Test {
                    debug_enabled,
                    temperature: temp,
                    gamma,
                },
                _ => {
                    Log::log_warning("Missing temperature or gamma values for --test");
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
                },
                _ => {
                    Log::log_warning("Missing start or end time for --simulate");
                    CliAction::ShowHelpDueToError
                }
            }
        } else {
            CliAction::Run { debug_enabled }
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
    Log::log_version();
    Log::log_pipe();
    println!("┗ {}", env!("CARGO_PKG_DESCRIPTION"));
}

/// Displays custom help message using logger methods.
pub fn display_help() {
    Log::log_version();
    Log::log_block_start(env!("CARGO_PKG_DESCRIPTION"));
    Log::log_block_start("Usage: sunsetr [OPTIONS]");
    Log::log_block_start("Options:");
    Log::log_indented("-d, --debug                  Enable detailed debug output");
    Log::log_indented("-g, --geo                    Interactive city selection for geo mode");
    Log::log_indented("-h, --help                   Print help information");
    Log::log_indented("-r, --reload                 Reset all display gamma and reload sunsetr");
    Log::log_indented("-S, --simulate <start> <end> [mult|--fast-forward]");
    Log::log_indented(
        "                             Simulate time (mult: 60=1min/sec, --fast-forward: instant)",
    );
    Log::log_indented("-t, --test <temp> <gamma>    Test specific temperature and gamma values");
    Log::log_indented("-V, --version                Print version information");
    Log::log_end();
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
                debug_enabled: false
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
                debug_enabled: true
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
                debug_enabled: true
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
                debug_enabled: false
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
                debug_enabled: false
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
                debug_enabled: true
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
                debug_enabled: true
            }
        );
    }
}
