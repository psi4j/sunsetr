use anyhow::{Context, Result};
use chrono::{Local, NaiveTime, Timelike};
use fs2::FileExt;
use serde::Deserialize;
use signal_hook::{
    consts::signal::{SIGINT, SIGTERM},
    iterator::Signals,
};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// Constants for configuration
const DEFAULT_NIGHT_TEMP: u32 = 4000;
const DEFAULT_NIGHT_GAMMA: f32 = 100.0;
const DEFAULT_DAY_GAMMA: f32 = 100.0;
const MINIMUM_TEMP: u32 = 1000;
const MAXIMUM_TEMP: u32 = 20000;
const MINIMUM_GAMMA: f32 = 0.0;
const MAXIMUM_GAMMA: f32 = 200.0;
const CHECK_INTERVAL: Duration = Duration::from_secs(1);

enum LogLevel {
    Log,
    Warn,
    Err,
    Info,
}

struct Logger;

impl Logger {
    fn log(level: LogLevel, message: &str) {
        match level {
            LogLevel::Log => print!("[LOG] {}", message),
            LogLevel::Warn => print!("[WARN] {}", message),
            LogLevel::Err => print!("[ERR] {}", message),
            LogLevel::Info => print!("[INFO] {}", message),
        }
        io::stdout().flush().unwrap();
    }

    fn log_error(message: &str) {
        Self::log(LogLevel::Err, &format!("{}\n", message));
    }

    fn log_warning(message: &str) {
        Self::log(LogLevel::Warn, &format!("{}\n", message));
    }

    fn log_decorated(message: &str) {
        println!("┣ {}", message);
    }

    fn log_pipe() {
        println!("┃");
    }

    fn log_block_start(message: &str) {
        println!("┃");
        println!("┣ {}", message);
    }

    fn log_version() {
        println!("┏ sunsetr v{} ━━╸", env!("CARGO_PKG_VERSION"));
        println!("┃");
    }

    fn log_end() {
        println!("╹");
    }
}

#[derive(Debug, Deserialize)]
struct Config {
    sunset: String,
    sunrise: String,
    night_temp: Option<u32>,
    night_gamma: Option<f32>,
    day_gamma: Option<f32>,
    start_hyprsunset: Option<bool>,
}

impl Config {
    fn get_config_path() -> Result<PathBuf> {
        dirs::config_dir()
            .map(|p| p.join("hypr").join("sunsetr.toml"))
            .context("Could not determine config directory")
    }

    fn create_default_config(path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Failed to create config directory")?;
        }

        let default_config = r#"# Sunsetr configuration
sunset = "19:00:00"      # Time to transition to night mode (HH:MM:SS)
sunrise = "06:00:00"     # Time to transition to day mode (HH:MM:SS)
night_temp = 4000        # Color temperature after sunset (1000-20000) Kelvin
night_gamma = 90.0       # Gamma percentage for night (0-200%)
day_gamma = 100.0        # Gamma percentage for day (0-200%)
start_hyprsunset = false # Whether to start hyprsunset automatically
                         # Set true if you're not using hyprsunset.service
"#;

        fs::write(path, default_config).context("Failed to write default config file")?;
        println!("Created default configuration file at {:?}", path);
        Ok(())
    }

    fn load() -> Result<Self> {
        let config_path = Self::get_config_path()?;

        if !config_path.exists() {
            Self::create_default_config(&config_path)?;
        }

        let content = fs::read_to_string(&config_path).context("Failed to read sunsetr.toml")?;

        let mut config: Config = toml::from_str(&content).context("Failed to parse config file")?;

        // Validate time formats
        NaiveTime::parse_from_str(&config.sunset, "%H:%M:%S")
            .context("Invalid sunset time format in config. Use HH:MM:SS format")?;
        NaiveTime::parse_from_str(&config.sunrise, "%H:%M:%S")
            .context("Invalid sunrise time format in config. Use HH:MM:SS format")?;

        // Validate temperature if specified
        if let Some(temp) = config.night_temp {
            if !(MINIMUM_TEMP..=MAXIMUM_TEMP).contains(&temp) {
                anyhow::bail!(
                    "Temperature must be between {} and {} Kelvin",
                    MINIMUM_TEMP,
                    MAXIMUM_TEMP
                );
            }
        } else {
            config.night_temp = Some(DEFAULT_NIGHT_TEMP);
        }

        // Validate night gamma if specified
        if let Some(gamma) = config.night_gamma {
            if !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma) {
                anyhow::bail!(
                    "Night gamma must be between {}% and {}%",
                    MINIMUM_GAMMA,
                    MAXIMUM_GAMMA
                );
            }
        } else {
            config.night_gamma = Some(DEFAULT_NIGHT_GAMMA);
        }

        // Validate day gamma if specified
        if let Some(gamma) = config.day_gamma {
            if !(MINIMUM_GAMMA..=MAXIMUM_GAMMA).contains(&gamma) {
                anyhow::bail!(
                    "Day gamma must be between {}% and {}%",
                    MINIMUM_GAMMA,
                    MAXIMUM_GAMMA
                );
            }
        } else {
            config.day_gamma = Some(DEFAULT_DAY_GAMMA);
        }

        // Set default for start_hyprsunset if not specified
        if config.start_hyprsunset.is_none() {
            config.start_hyprsunset = Some(false);
        }

        Ok(config)
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
enum TimeState {
    Day,
    Night,
}

fn get_current_state(config: &Config) -> TimeState {
    let now = Local::now().time();
    let sunset = NaiveTime::parse_from_str(&config.sunset, "%H:%M:%S").unwrap();
    let sunrise = NaiveTime::parse_from_str(&config.sunrise, "%H:%M:%S").unwrap();

    if (sunset < sunrise && (now >= sunset && now < sunrise))
        || (sunset >= sunrise && (now >= sunset || now < sunrise))
    {
        TimeState::Night
    } else {
        TimeState::Day
    }
}

fn time_until_next_event(config: &Config) -> Duration {
    let now = Local::now();
    let current_time = now.time();
    let sunset = NaiveTime::parse_from_str(&config.sunset, "%H:%M:%S").unwrap();
    let sunrise = NaiveTime::parse_from_str(&config.sunrise, "%H:%M:%S").unwrap();

    // Convert all times to seconds since midnight for easier comparison
    let current_secs =
        current_time.hour() * 3600 + current_time.minute() * 60 + current_time.second();
    let sunset_secs = sunset.hour() * 3600 + sunset.minute() * 60 + sunset.second();
    let sunrise_secs = sunrise.hour() * 3600 + sunrise.minute() * 60 + sunrise.second();

    let seconds_until = match get_current_state(config) {
        TimeState::Day => {
            if sunset_secs > current_secs {
                // Sunset is later today
                sunset_secs - current_secs
            } else {
                // Sunset is tomorrow
                (24 * 3600) - current_secs + sunset_secs
            }
        }
        TimeState::Night => {
            if sunrise_secs > current_secs {
                // Sunrise is later today
                sunrise_secs - current_secs
            } else {
                // Sunrise is tomorrow
                (24 * 3600) - current_secs + sunrise_secs
            }
        }
    };

    Duration::from_secs(seconds_until as u64)
}

struct HyprsunsetClient {
    socket_path: PathBuf,
    connection: Option<UnixStream>,
}

impl HyprsunsetClient {
    fn new() -> Result<Self> {
        // Determine socket path (similar to how hyprsunset does it)
        let his_env = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));

        let user_dir = format!("{}/hypr/", runtime_dir);

        let socket_path = if let Some(his) = his_env {
            PathBuf::from(format!("{}{}/.hyprsunset.sock", user_dir, his))
        } else {
            PathBuf::from(format!("{}/.hyprsunset.sock", user_dir))
        };

        Logger::log_decorated(&format!("Socket path: {:?}", socket_path));

        // Verify socket exists
        if socket_path.exists() {
            Logger::log_decorated("Socket file exists");
        } else {
            Logger::log_warning(&format!(
                "Warning: Socket file doesn't exist at {:?}",
                socket_path
            ));
        }

        Ok(Self {
            socket_path,
            connection: None,
        })
    }

    // fn ensure_connected(&mut self) -> Result<&mut UnixStream> {
    //     if self.connection.is_none() {
    //         Logger::log_decorated("Opening new connection to hyprsunset...");
    //         self.connection = Some(UnixStream::connect(&self.socket_path).context(format!(
    //             "Failed to connect to socket at {:?}",
    //             self.socket_path
    //         ))?);
    //     }
    //
    //     Ok(self.connection.as_mut().unwrap())
    // }

    fn close_connection(&mut self) {
        if self.connection.is_some() {
            Logger::log_decorated("Closing existing connection");
            self.connection = None;
        }
    }

    fn send_command(&mut self, command: &str) -> Result<()> {
        Logger::log(
            LogLevel::Log,
            &format!("Sending command to hyprsunset: {}\n", command),
        );

        // Try to connect
        let mut stream = match UnixStream::connect(&self.socket_path) {
            Ok(s) => s,
            Err(e) => {
                return Err(e).context(format!(
                    "Failed to connect to socket at {:?}",
                    self.socket_path
                ))
            }
        };

        // Set a short timeout to prevent hanging
        if let Err(e) = stream.set_read_timeout(Some(Duration::from_millis(500))) {
            Logger::log_decorated(&format!(
                "Failed to set read timeout: {}. Continuing anyway.",
                e
            ));
        }

        // Send the command
        match stream.write_all(command.as_bytes()) {
            Ok(_) => {
                Logger::log_decorated("Command sent successfully");

                // Try to read a response, but don't fail if we can't
                let mut buffer = [0; 1024];
                match stream.read(&mut buffer) {
                    Ok(bytes_read) => {
                        if bytes_read > 0 {
                            let response = String::from_utf8_lossy(&buffer[0..bytes_read]);
                            Logger::log_decorated(&format!("Received response: {}", response));
                        } else {
                            Logger::log_decorated(
                                "Connection closed by hyprsunset without response",
                            );
                        }
                    }
                    Err(e) => {
                        Logger::log_decorated(&format!(
                            "Could not read response: {}. This is expected with hyprsunset.",
                            e
                        ));
                    }
                }

                // Explicitly close the stream
                drop(stream);
                Ok(())
            }
            Err(e) => {
                // Explicitly close the stream
                drop(stream);
                Err(e).context("Failed to write command to socket")
            }
        }
    }

    fn set_identity_mode(&mut self) -> Result<()> {
        self.send_command("identity")
    }

    fn set_temperature(&mut self, temp: u32) -> Result<()> {
        self.send_command(&format!("temperature {}", temp))
    }

    fn set_gamma(&mut self, gamma: f32) -> Result<()> {
        self.send_command(&format!("gamma {}", gamma))
    }

    // fn get_temperature(&mut self) -> Result<u32> {
    //     Logger::log_decorated("Note: get_temperature is using a default value as hyprsunset doesn't provide a readable response");
    //     // Try to send the command but don't worry about the response
    //     let _ = self.send_command("temperature");
    //     Ok(6000) // Default value from hyprsunset
    // }
    //
    // fn get_gamma(&mut self) -> Result<f32> {
    //     Logger::log_decorated("Note: get_gamma is using a default value as hyprsunset doesn't provide a readable response");
    //     // Try to send the command but don't worry about the response
    //     let _ = self.send_command("gamma");
    //     Ok(100.0) // Default value from hyprsunset
    // }

    fn test_connection(&mut self) -> bool {
        Logger::log_decorated("Testing connection to hyprsunset...");
        match UnixStream::connect(&self.socket_path) {
            Ok(_) => {
                Logger::log_decorated("Successfully connected to hyprsunset socket");
                true
            }
            Err(e) => {
                Logger::log_decorated(&format!("Connection test failed: {}", e));
                false
            }
        }
    }
}

fn start_hyprsunset() -> Result<()> {
    Logger::log_decorated("Starting hyprsunset process...");

    std::process::Command::new("hyprsunset")
        .spawn()
        .context("Failed to start hyprsunset")?;

    // Give hyprsunset time to initialize
    Logger::log_decorated("Waiting for hyprsunset to initialize...");
    thread::sleep(Duration::from_secs(2));

    Ok(())
}

fn verify_hyprsunset_installed() -> Result<()> {
    match std::process::Command::new("which")
        .arg("hyprsunset")
        .output()
    {
        Ok(output) => {
            if !output.status.success() {
                anyhow::bail!("hyprsunset is not installed on the system");
            }
            Logger::log_decorated("hyprsunset is installed");
            Ok(())
        }
        Err(_) => anyhow::bail!("Failed to check if hyprsunset is installed"),
    }
}

fn apply_state(
    client: &mut HyprsunsetClient,
    state: TimeState,
    config: &Config,
    running: &AtomicBool,
) -> Result<()> {
    // Don't try to apply state if we're shutting down
    if !running.load(Ordering::SeqCst) {
        Logger::log_decorated("Skipping state application during shutdown");
        return Ok(());
    }

    match state {
        TimeState::Day => {
            // In day mode, special handling for identity command
            Logger::log_decorated("Applying day mode settings");

            // First close existing connection to ensure clean state
            client.close_connection();

            // Send identity command
            match client.set_identity_mode() {
                Ok(_) => {
                    Logger::log_decorated("Identity mode set successfully");
                }
                Err(e) => {
                    Logger::log_error(&format!("Error setting identity mode: {}", e));
                    return Err(e);
                }
            }

            // Give hyprsunset time to process
            thread::sleep(Duration::from_secs(1));

            // Verify socket still exists
            if !client.socket_path.exists() {
                Logger::log_warning("Warning: Socket file disappeared after identity command");
                return Err(anyhow::anyhow!(
                    "Socket file disappeared after identity command"
                ));
            }

            // Set gamma
            match client.set_gamma(config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA)) {
                Ok(_) => {
                    Logger::log_decorated("Gamma set successfully");
                }
                Err(e) => {
                    Logger::log_error(&format!("Error setting gamma: {}", e));
                    return Err(e);
                }
            }
        }
        TimeState::Night => {
            // In night mode, set temperature and gamma
            Logger::log_decorated("Applying night mode settings");

            // First close existing connection to ensure clean state
            client.close_connection();

            // Send temperature command
            match client.set_temperature(config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP)) {
                Ok(_) => {
                    Logger::log_decorated("Temperature set successfully");
                }
                Err(e) => {
                    Logger::log_error(&format!("Error setting temperature: {}", e));
                    return Err(e);
                }
            }

            // Give hyprsunset time to process
            thread::sleep(Duration::from_secs(1));

            // Verify socket still exists
            if !client.socket_path.exists() {
                Logger::log_decorated("Warning: Socket file disappeared after temperature command");
                return Err(anyhow::anyhow!(
                    "Socket file disappeared after temperature command"
                ));
            }

            // Set gamma
            match client.set_gamma(config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA)) {
                Ok(_) => {
                    Logger::log_decorated("Gamma set successfully");
                }
                Err(e) => {
                    Logger::log_error(&format!("Error setting gamma: {}", e));
                    return Err(e);
                }
            }
        }
    }
    Ok(())
}

// Function to perform cleanup on shutdown
fn cleanup(client: &mut HyprsunsetClient, lock_file: File, lock_path: &str) {
    Logger::log_decorated("Performing cleanup...");

    // Close any open connection
    client.close_connection();

    // Drop the lock file handle
    drop(lock_file);

    // Remove the lock file
    if let Err(e) = fs::remove_file(lock_path) {
        Logger::log_decorated(&format!("Warning: Failed to remove lock file: {}", e));
    } else {
        Logger::log_decorated("Lock file removed successfully");
    }

    Logger::log_decorated("Cleanup complete");
}

fn main() -> Result<()> {
    Logger::log_version();

    // Verify hyprsunset is installed
    verify_hyprsunset_installed()?;

    // Set up signal handling
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    let mut signals = Signals::new([SIGTERM, SIGINT])?;
    thread::spawn(move || {
        for signal in signals.forever() {
            Logger::log(
                LogLevel::Info,
                &format!("Shutdown signal received: {:?}\n", signal),
            );
            r.store(false, Ordering::SeqCst);
            Logger::log(LogLevel::Info, "Set running flag to false\n");
        }
    });

    // Create and acquire lock file
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let lock_path = format!("{}/sunsetr.lock", runtime_dir);
    let lock_file = File::create(&lock_path)?;

    // Try to acquire exclusive lock
    match lock_file.try_lock_exclusive() {
        Ok(_) => {
            Logger::log_decorated("Lock acquired, starting sunsetr...");

            let config = Config::load()?;

            // Start hyprsunset if configured to do so
            if config.start_hyprsunset.unwrap_or(false) {
                start_hyprsunset()?;
            } else {
                Logger::log_decorated(
                    "Not starting hyprsunset (assuming it's managed by hyprsunset.service)",
                );
            }

            // Initialize hyprsunset client
            let mut client = HyprsunsetClient::new()?;

            // Test connection to hyprsunset
            if !client.test_connection() {
                Logger::log_warning(" Could not establish initial connection to hyprsunset.");
                Logger::log_decorated("Make sure hyprsunset is running before continuing.");
                Logger::log_decorated("Continuing anyway, will retry during operation...");
            }

            // Log configuration
            Logger::log_block_start("Loaded configuration");
            println!("┃   Sunset time: {}", config.sunset);
            println!("┃   Sunrise time: {}", config.sunrise);
            println!(
                "┃   Night temperature: {}K",
                config.night_temp.unwrap_or(DEFAULT_NIGHT_TEMP)
            );
            println!(
                "┃   Night gamma: {}%",
                config.night_gamma.unwrap_or(DEFAULT_NIGHT_GAMMA)
            );
            println!(
                "┃   Day gamma: {}%",
                config.day_gamma.unwrap_or(DEFAULT_DAY_GAMMA)
            );
            Logger::log_decorated(&format!(
                "Auto-start hyprsunset: {}",
                config.start_hyprsunset.unwrap_or(false)
            ));
            Logger::log_pipe();

            let mut current_state = get_current_state(&config);
            Logger::log_decorated(&format!("Initial state: {:?}", current_state));

            // Add a pipe before state application
            Logger::log_pipe();

            // Apply initial settings
            if running.load(Ordering::SeqCst) {
                match apply_state(&mut client, current_state, &config, &running) {
                    Ok(_) => {} // Success message is handled in apply_state
                    Err(e) => {
                        Logger::log(
                            LogLevel::Err,
                            &format!(
                                "Error applying initial state: {}. Will retry on next cycle.\n",
                                e
                            ),
                        );
                    }
                }
            }

            // Log pipe before timing info
            Logger::log_pipe();

            // Main loop
            while running.load(Ordering::SeqCst) {
                let sleep_duration = time_until_next_event(&config);
                Logger::log_decorated(&format!(
                    "Current time: {}, Next transition in {} minutes {} seconds",
                    Local::now().format("%H:%M:%S"),
                    sleep_duration.as_secs() / 60,
                    sleep_duration.as_secs() % 60
                ));

                // Sleep in smaller intervals to check running status
                let mut slept = Duration::from_secs(0);
                while slept < sleep_duration && running.load(Ordering::SeqCst) {
                    let sleep_chunk = CHECK_INTERVAL.min(sleep_duration - slept);
                    thread::sleep(sleep_chunk);
                    slept += sleep_chunk;
                }

                if !running.load(Ordering::SeqCst) {
                    break;
                }

                let new_state = get_current_state(&config);
                if new_state != current_state {
                    Logger::log_block_start(&format!(
                        "State transition at {}: {:?} -> {:?}",
                        Local::now().format("%H:%M:%S%.3f"),
                        current_state,
                        new_state
                    ));
                    Logger::log_pipe();

                    if running.load(Ordering::SeqCst) {
                        match apply_state(&mut client, new_state, &config, &running) {
                            Ok(_) => {
                                // Success message is handled in apply_state
                                current_state = new_state;
                            }
                            Err(e) => {
                                Logger::log(
                                    LogLevel::Err,
                                    &format!(
                                        "Error applying new state: {}. Will retry on next cycle.\n",
                                        e
                                    ),
                                );
                            }
                        }
                    }
                }
            }

            // Ensure proper cleanup on shutdown
            Logger::log_block_start("Shutting down sunsetr...");
            cleanup(&mut client, lock_file, &lock_path);
            Logger::log_end();
        }
        Err(_) => {
            Logger::log_decorated(
                "Another instance of sunsetr is already running. Kill sunsetr before restarting.",
            );
            Logger::log_end();
            std::process::exit(1);
        }
    }

    Ok(())
}
