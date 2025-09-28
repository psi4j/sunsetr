<p align="center">
    <img src=".github/assets/logo.png" alt="sunsetr logo" width="144" />
</p>

# sunsetr

Automatic blue light filter for Hyprland, Niri, and everything Wayland

![This image was taken using a shader to simulate the effect of sunsetr](.github/assets/sunsetr.png)

## Features

- **Multi-Compositor Support**: Works with Hyprland, Niri, Sway, River, Wayfire, and other Wayland compositors
- **Native Hyprland CTM Backend**: Direct Color Transformation Matrix support for Hyprland
- **Smarter hyprsunset Management**: Add longer, cleaner, and more precise sunset/sunrise transitions to hyprsunset (Hyprland)
- **Smooth Transitions**: Configurable fade effects with adaptive algorithm
- **Preset Management**: Quick switching between configuration profiles (e.g., day, gaming, weekend)
- **Hot Reloading**: Live updates when config files change - no restart needed
- **Geolocation-based Transitions**: Automatic sunrise/sunset calculation based on your location
- **Interactive City Selection**: Choose from 10,000+ cities worldwide for precise coordinates
- **Automatic Timezone Detection**: Falls back to system timezone for location approximation
- **Universal Wayland Support**: Direct protocol communication on Wayland compositors
- **Smart Defaults**: Works beautifully out-of-the-box with carefully tuned settings
- **Flexible Configuration**: Extensive customization options for power users
- **Comprehensive Help System**: Built-in help for all commands and features

## Dependencies

### For Hyprland Users

- **Hyprland >=0.49.0**
- **hyprsunset >=v0.2.0**

### For Other Wayland Compositors

- **Any Wayland compositor** supporting `wlr-gamma-control-unstable-v1` protocol
- **No external dependencies** - uses native Wayland protocols

## 📥 Installation

### Build from Source

```bash
git clone https://github.com/psi4j/sunsetr.git
cd sunsetr
cargo build --release
sudo cp target/release/sunsetr /usr/local/bin/
```

### AUR (Arch Linux)

[sunsetr-bin](https://aur.archlinux.org/packages/sunsetr-bin) is available in the AUR:

```bash
paru -S sunsetr-bin
```

### NixOS and Nix

[sunsetr](https://search.nixos.org/packages?channel=unstable&from=0&size=50&sort=relevance&type=packages&query=sunsetr) is available in nixpkgs unstable:

```bash
# For NixOS users (add to configuration.nix)
environment.systemPackages = with pkgs; [
  sunsetr
];

# Or install imperatively
nix-env -iA nixpkgs.sunsetr

# Or try it out temporarily
nix-shell -p sunsetr
```

#### Flakes

A flake is available for those wanting to use the latest version `main` without waiting for it to be added to nixpkgs.

Add to your flake inputs:

```nix
{
  inputs.sunsetr.url = "github:psi4j/sunsetr";
}
```

Then you can use it in your configuration:

```nix
{ inputs, pkgs, ... }:
{
  # Install as a system package
  environment.systemPackages = [
    inputs.sunsetr.packages.${pkgs.system}.sunsetr
  ];

  # OR with home-manager
  home.packages = [
    inputs.sunsetr.packages.${pkgs.system}.sunsetr
  ];
}
```

## Recommended Setup

### Hyprland

For the smoothest experience on Hyprland, add this line near the **beginning** of your `hyprland.conf`:

```bash
exec-once = sunsetr
```

This ensures sunsetr starts early during compositor initialization, providing seamless color temperature management from the moment your desktop loads.

⚠️ **WARNING:**

**If selecting Hyprsunset backend**:

- **Do not use with hyprsunset's native config**: I recommend removing `hyprsunset.conf` entirely or backing it up. (sunsetr will need full control for smooth transition times)
- **Make sure hyprsunset isn't already running** if you want this to work with `start_hyprsunset = true` from the default config. You can check that a hyprsunset process isn't already running using btop or an alternative method.
- I recommend you **disable hyprsunset's systemd service** using `systemctl --user disable hyprsunset.service` and make sure to stop the process before running sunsetr.

### niri

For the smoothest experience on niri, add this line near the **beginning** of your startup config in `config.kdl`:

```kdl
spawn-at-startup "sunsetr"
```

### Other Wayland compositors

If you're running on Sway, or any other alternatives, see their recommended startup methods for background applications. If you run into any trouble and need any help feel free to open up an issue or start a discussion.

## Alternative Setup: Systemd Service

If you prefer systemd management:

```bash
systemctl --user enable --now sunsetr.service
```

## 🌍 Geographic Location Setup

sunsetr can automatically calculate sunrise and sunset times based on your geographic location using `transition_mode = "geo"`. This provides more accurate and natural transitions than fixed times and gives you a few benefits when compared to using fixed times. The geo transition mode uses real-time calculations and will automatically adjust throughout the year as the seasons change.

### Interactive City Selection

For the most precise location setup, use the interactive city selector:

```bash
sunsetr geo
```

This launches an interactive fuzzy search interface where you can:

- Type to search from 10,000+ cities worldwide
- Navigate with arrow keys (↑/↓)
- Select with Enter, cancel with Esc
- Search by city name or country

The tool will show you calculated sunrise/sunset times and save the coordinates to your configuration.

### Automatic Location Detection

If you don't manually select a city, sunsetr automatically detects your approximate location using:

1. **System timezone detection** - Multiple fallback methods for robust detection
2. **Timezone-to-coordinates mapping** - 466 timezone mappings worldwide
3. **London fallback** - If timezone detection fails (just run `sunsetr geo`)

### Geographic Debug Information

To see detailed solar calculation information for your location:

```bash
sunsetr --debug
```

This shows:

- Detected/configured coordinates and timezone
- Precise sunset/sunrise timing with transition boundaries
- Calculation method used (standard or extreme latitude fallback)

To see more details when you choose your location with the city selector:

```bash
sunsetr --debug geo
```

### Testing other city's coordinates (not your current location)

I realize we might want to test other cities' sunset/sunrise times and transition durations. Maybe we have to fly to another timezone for a special event and we want to get ahead of the jet lag and fix our sleeping schedule to their timezone.

Just run `sunsetr geo`. If you run this with `--debug`, you'll see an additional set of times in brackets `[]` to the right of the primary set of times. These times are in your autodetected local timezone. The primary set of times correspond to the selected city's coordinates' sunset/sunrise transition times. Ex:

```
[DEBUG] Solar calculation details:
┃           Raw coordinates: 35.6895°, 139.6917°
┃               Sunrise UTC: 19:25
┃                Sunset UTC: 10:00
┃       Coordinate Timezone: Asia/Tokyo (+09:00)
┃            Local timezone: America/Chicago (-05:00)
┃     Current time (Coords): 12:41:47
┃      Current time (Local): 22:41:47
┃           Time difference: +14 hours
┃   --- Sunset (descending) ---
┃   Transition start (+10°): 18:10:16 [04:10:16]
┃   Golden hour start (+6°): 18:30:20 [04:30:20]
┃               Sunset (0°): 19:00:26 [05:00:26]
┃      Transition end (-2°): 19:10:28 [05:10:28]
┃          Civil dusk (-6°): 19:30:32 [05:30:32]
┃            Night duration: 9 hours 5 minutes
┃   --- Sunrise (ascending) ---
┃          Civil dawn (-6°): 03:55:43 [13:55:43]
┃    Transition start (-2°): 04:15:47 [14:15:47]
┃              Sunrise (0°): 04:25:50 [14:25:50]
┃     Golden hour end (+6°): 04:55:57 [14:55:57]
┃     Transition end (+10°): 05:16:01 [15:16:01]
┃              Day duration: 12 hours 54 minutes
┃           Sunset duration: 60 minutes
┃          Sunrise duration: 60 minutes
┃
[DEBUG] Next transition will begin at: 18:10:15 [04:10:15] Day 󰖨  → Sunset 󰖛
```

### Using Arbitrary Coordinates

If the city selector (`sunsetr geo`) is not as precise as you'd like, you're welcome manually add coordinates to `sunsetr.toml`. I recommend using https://www.geonames.org/ or Google Earth to find your coordinates. North is positive, South is negative. East is positive, West is negative.

```toml
#[Geolocation-based transitions]
latitude = 29.424122   # just switch these up
longitude = -98.493629 # `sunsetr --debug` to see the times/duration
```

### Privacy-Focused Geographic Configuration

If you version control your configuration files (e.g., in a dotfiles repository), you may not want to expose your geographic location. sunsetr supports storing coordinates in a separate `geo.toml` file that you can keep private:

1. **Create the geo.toml file** in the same directory as your sunsetr.toml:

   ```bash
   touch ~/.config/sunsetr/geo.toml
   ```

2. **Add geo.toml to your .gitignore**:

   ```bash
   echo "geo.toml" >> ~/.gitignore
   ```

3. **Run `sunsetr geo`** to populate it (or enter manual coordinates)

4. **Delete or spoof coordinates in** `sunsetr.toml`

Once `geo.toml` exists, it will:

- Override any coordinates in your main `sunsetr.toml`
- Receive all coordinate updates when you run `sunsetr geo`
- Keep your location private while allowing you to version control all other settings

Example `geo.toml`:

```toml
#[Private geo coordinates]
latitude = 40.7128
longitude = -74.0060
```

This separation allows you to share your sunsetr configuration publicly without accidentally doxxing yourself. `geo.toml` can also serve as a temporary place to store your coordinates when travelling.

⭐ **Note**: Your debug output will still print your coordinates on startup for debugging purposes, so be extremely careful when sharing your debug output online.

```
┣ Loaded configuration from ~/.config/sunsetr/sunsetr.toml
┃   Loaded geo coordinates from ~/.config/sunsetr/geo.toml
┃   Backend: wayland
┃   Auto-start hyprsunset: false
┃   Enable startup transition: true
┃   Startup transition duration: 1 seconds
┃   Location: 22.3193°N, 114.1694°E <--- ⭐ Careful!
┃   Sunset time: 19:00:00
┃   Sunrise time: 06:00:00
┃   Night temperature: 3300K
┃   Day temperature: 6500K
┃   Night gamma: 90%
┃   Day gamma: 100%
┃   Transition duration: 45 minutes
┃   Update interval: 60 seconds
┃   Transition mode: geo
```

## ⚙️ Configuration

sunsetr creates a default configuration at `~/.config/sunsetr/sunsetr.toml` on first run. The defaults provide an excellent out-of-the-box experience for most users:

```toml
#[Backend]
backend = "auto"         # Backend to use: "auto", "hyprland", "hyprsunset" or "wayland"
transition_mode = "geo"  # Select: "geo", "finish_by", "start_at", "center", "static"

#[Smoothing]
smoothing = true         # Enable smooth transitions during startup and exit
startup_duration = 0.5   # Duration of smooth startup in seconds (0.1-60 | 0 = instant)
shutdown_duration = 0.5  # Duration of smooth shutdown in seconds (0.1-60 | 0 = instant)
adaptive_interval = 1    # Adaptive interval base for smooth transitions (1-1000)ms

#[Time-based config]
night_temp = 3300        # Color temperature during night (1000-20000) Kelvin
day_temp = 6500          # Color temperature during day (1000-20000) Kelvin
night_gamma = 90         # Gamma percentage for night (10-100%)
day_gamma = 100          # Gamma percentage for day (10-100%)
update_interval = 60     # Update frequency during transitions in seconds (10-300)

#[Static config]
static_temp = 6500       # Color temperature for static mode (1000-20000) Kelvin
static_gamma = 100       # Gamma percentage for static mode (10-100%)

#[Manual transitions]
sunset = "19:00:00"      # Time for manual sunset calculations (HH:MM:SS)
sunrise = "06:00:00"     # Time for manual sunrise calculations (HH:MM:SS)
transition_duration = 45 # Transition duration in minutes (5-120)

#[Geolocation]
latitude = 41.850033     # Geographic latitude (auto-detected on first run)
longitude = -87.650055   # Geographic longitude (use 'sunsetr geo' to change)
```

### Key Settings Explained

- **`backend = "auto"`** (recommended): Automatically detects your compositor and uses the appropriate backend. Use auto if you plan on using sunsetr on both Hyprland and other Wayland compositors like niri or Sway. (⭐ **Note:** The new Hyprland backend replaces hyprsunset entirely, but you can still choose to use hyprsunset as a backend with `backend = "hyprsunset"`)
- **`smoothing = true`**: Provides smooth transitions when sunsetr starts and stops. The durations are configurable via `startup_duration` and `shutdown_duration` (0.1-60 seconds). The `adaptive_interval` controls the base update interval for the adaptive algorithm. (⭐ **Note:** Smoothing is only available using the Wayland backend. Hyprland users will experience Hyprland's built-in smooth transitions instead.)
- **`transition_mode = "geo"`** (default): Automatically calculates sunset/sunrise times based on your geographic location. Use `sunsetr geo` to select your city or let it auto-detect from your timezone. This provides the most natural transitions that change throughout the year.
- **Other transition modes**:
  - `"static"` maintains constant temperature/gamma values without any time-based transitions
  - `"finish_by"` ensures transitions complete exactly at configured times
  - `"start_at"` begins transitions at configured times
  - `"center"` centers transitions around configured times

⭐ **Note**: Manual transition modes will use the configured `sunset`, `sunrise`, and `transition_duration`. Using the geo transition mode will autocalculate these settings using the given geographic coordinates (`latitude` and `longitude`), thus these manual settings will be ignored when set to geo mode.

### Backend-Specific Configuration

#### Automatic Detection (Recommended)

```toml
backend = "auto"
```

sunsetr will automatically detect your compositor and configure itself appropriately.

#### Explicit Backend Selection

```toml
# For Hyprland users, you can use the new CTM manager with:
backend = "hyprland"
# Or you can use hyprsunset as a dependency
backend = "hyprsunset"

# For other Wayland compositors (Though it works on Hyprland too):
backend = "wayland"
```

### Smooth Transitions

For smooth startup and shutdown transitions that ease in to the configured temperature and gamma values:

```toml
#[Smoothing]
smoothing = true         # Enable/disable smooth transitions
startup_duration = 0.5   # Seconds for startup transition (0.1-60, 0=instant)
shutdown_duration = 0.5  # Seconds for shutdown transition (0.1-60, 0=instant)
adaptive_interval = 1    # Base interval for adaptive algorithm (1-1000ms)
```

The `adaptive_interval` automatically adjusts to your system's capabilities. For longer transitions, you may want to increase this value:

```toml
#[Smoothing] - Example for longer transitions
smoothing = true
startup_duration = 5     # 5 second startup
shutdown_duration = 5    # 5 second shutdown
adaptive_interval = 150  # Higher base interval for less frequent updates
```

⭐ **Note**: The Hyprland compositor has its own built-in CTM animations that conflict with our smooth transitions, so smoothing is ignored when using the Hyprland and Hyprsunset backends. You can still use these settings in Hyprland by switching to the Wayland backend. To disable Hyprland's CTM animations, add this setting to `hyprland.conf`:

```bash
render {
    ctm_animation = 0
}
```

### Static Mode

Static mode maintains constant temperature and gamma values without any time-based transitions. Perfect for when you need consistent display settings:

```toml
#[Backend]
backend = "auto"
start_hyprsunset = true
transition_mode = "static"

#[Static config]
static_temp = 5000       # Constant color temperature (1000-20000) Kelvin
static_gamma = 90        # Constant gamma percentage (10-100%)
```

When using static mode:

- Night/day temperature and gamma settings are ignored
- Sunset/sunrise times and transition durations are ignored
- The display maintains the configured `static_temp` and `static_gamma` values constantly
- Combine with presets for quick toggles between different static values

### Preset Management

The preset system allows quick switching between different configuration profiles. Perfect for different activities or times of day:

```bash
# Show currently active preset
sunsetr preset active   # or just 'sunsetr p active'

# List all available presets
sunsetr preset list     # or just 'sunsetr p list'

# Switch to a specific preset
sunsetr preset day      # Apply day preset
sunsetr preset gaming   # Apply gaming preset

# Return to default configuration
sunsetr preset default  # Return to main config
# Or call the same preset twice
sunsetr preset day      # Toggles back to default
```

Set up keyboard shortcuts for instant toggling:

```bash
# Hyprland (hyprland.conf)
bind = $mod, W, exec, sunsetr preset day # toggle between day preset and default config

# Niri (config.kdl)
Mod+W { spawn "sh" "-c" "sunsetr p day"; }
```

#### Creating Presets

Create preset files in `~/.config/sunsetr/presets/`:

```
~/.config/sunsetr/
├── sunsetr.toml         # Main/default config
├── geo.toml             # Optional: private coordinates
└── presets/
    ├── day/
    │   └── sunsetr.toml # Static day values
    ├── gaming/
    │   └── sunsetr.toml # Gaming-optimized settings
    ├── weekend/
    │   └── sunsetr.toml # Weekend schedule
    └── london/
        ├── sunsetr.toml # London timezone
        └── geo.toml     # London coordinates
```

Each preset can have:

- Its own `sunsetr.toml` with complete or partial configuration
- Optional `geo.toml` for location-specific presets
- Any valid sunsetr configuration options

Example preset for static day mode (`~/.config/sunsetr/presets/day/sunsetr.toml`):

```toml
#[Backend]
backend = "auto"         # Backend to use: "auto", "hyprland" or "wayland"
transition_mode = "static"  # Select: "geo", "finish_by", "start_at", "center", "static"

#[Smoothing]
smoothing = true         # Enable smooth transitions during startup and exit
startup_duration = 0.5   # Duration of smooth startup in seconds (0.1-60 | 0 = instant)
shutdown_duration = 0    # Duration of smooth shutdown in seconds (0.1-60 | 0 = instant)
adaptive_interval = 1    # Adaptive interval base for smooth transition (1-1000)ms

#[Static configuration]
static_temp = 6500       # Color temperature for static mode (1000-20000) Kelvin
static_gamma = 100       # Gamma percentage for static mode (10-100%)
```

### Configuration Management Commands

sunsetr provides useful CLI commands for reading and modifying configuration values without manually editing TOML files:

#### Reading Configuration (`get` command)

```bash
# Get specific configuration fields
sunsetr get night_temp          # Returns: night_temp = 3300
sunsetr get night_temp day_temp # Multiple fields at once

# Get all configuration values
sunsetr get all                 # Shows entire configuration

# Output in JSON format (for scripting)
sunsetr get night_temp --json   # Returns: {"night_temp": 3300}
sunsetr get all --json          # Full config as JSON

# Target specific configurations
sunsetr get night_temp --target default  # From base config
sunsetr get night_temp -t gaming         # From gaming preset
```

#### Modifying Configuration (`set` command)

```bash
# Set configuration values
sunsetr set night_temp=3500               # Update night temperature
sunsetr set night_temp=3500 day_temp=6000 # Multiple values at once

# Target specific configurations
sunsetr set --target default night_temp=3500 # Modify base config
sunsetr set -t gaming static_temp=4700       # Modify gaming preset
```

**Safety Features:**

- Validates all values before saving
- Shows warnings for problematic changes
- Preserves configuration structure and comments

### Custom Configuration Directories

Use custom configuration directories for portable setups or testing:

```bash
# Start with custom config directory
sunsetr --config ~/dotfiles/sunsetr/

# All commands respect the custom directory
sunsetr --config ~/dotfiles/sunsetr/ preset gaming
sunsetr --config ~/dotfiles/sunsetr/ geo
sunsetr --config ~/dotfiles/sunsetr/ reload
susnetr --config ~/dotfiles/sunsetr/ set night_temp=2333
```

The custom directory maintains the same structure:

```
~/dotfiles/sunsetr/
├── sunsetr.toml
├── geo.toml
└── presets/
    └── [your presets]
```

⭐ **Note**: Once started with `--config`, all subsequent commands during that session will use the custom directory without needing the flag again.

## 🔄 Live Configuration Reload

sunsetr now supports automatic hot reloading when configuration files change:

### Automatic Hot Reloading

Configuration changes are detected and applied automatically:

```bash
# Start sunsetr - it will auto-reload on config changes
sunsetr

# Edit your config in another terminal or editor
vim ~/.config/sunsetr/sunsetr.toml
# Changes apply immediately upon save!
```

Hot reloading works with:

- Main configuration file (`sunsetr.toml`)
- Geographic coordinates file (`geo.toml`)
- Active preset configurations
- Custom configuration directories (with `--config`)

### Manual Reload Command

You can also manually trigger a reload:

```bash
sunsetr reload
# or
sunsetr r
```

**Note**: Starting a new process with `sunsetr reload` will start in the background.

## 📖 Built-in Help System

sunsetr includes a comprehensive help system for all commands:

```bash
# General help
sunsetr help            # Show all available commands
sunsetr --help          # Show detailed usage information

# Command-specific help
sunsetr help preset     # Detailed help for preset command
sunsetr help set        # Detailed help for set command
sunsetr help get        # Detailed help for get command
sunsetr help geo        # Detailed help for geo command

# Short aliases work too
sunsetr h p             # Help for preset command
sunsetr h s             # Help for set command
```

## 🧪 Testing Color Temperatures

### Quick Testing with sunsetr

The easiest way to test color temperatures and gamma values:

```bash
# Test specific temperature and gamma values (both required)
sunsetr test 3300 90
```

This command:

- Temporarily applies the specified temperature and gamma values
- Works while sunsetr is running (sends values to the existing instance)
- Press ESC or Ctrl+C to automatically restore previous settings
- Does not affect your configuration file
- Perfect for finding your preferred night-time settings

## 🚀 Simulating Time for Testing

### Simulation Mode

Test sunsetr's behavior across arbitrary time windows without waiting:

```bash
# Simulate a specific time window with 60x speed (1 minute = 1 second)
sunsetr --simulate "2025-01-15 18:00:00" "2025-01-16 08:00:00" 60

# Fast-forward through the time window as quickly as possible
sunsetr --simulate "2025-01-15 18:00:00" "2025-01-16 08:00:00" --fast-forward

# Save simulation output to a timestamped log file for inspection
sunsetr --simulate "2025-01-15 18:00:00" "2025-01-16 08:00:00" 60 --log
```

This command:

- Simulates runtime during the specified time window with a time scalar.
- Supports time multipliers from 0.1x to 3600x speed (or --fast-forward near-instant updates)
- Faithfully reproduces actual behavior including all temperature/gamma updates and logging during the scheduled time window
- Optional `--log` flag saves output to `simulation_YYYYMMDD_HHMMSS.log` in the current working directory
- Respects active preset and custom base config dir using `--config`

⭐ **Note**: At higher end of the multiplier's range, sunsetr may take longer than theoretical time due to system and processing overhead.

## 🙃 Troubleshooting

### sunsetr won't start hyprsunset

- Ensure hyprsunset is installed and accessible if you're attempting to use sunsetr as a controller
- Make sure hyprsunset is not already running
- Be sure you're running on Hyprland
- Try using the Hyprland backend instead and consider removing hyprsunset as a dependancy.

### Smooth transitions aren't smooth

- Ensure `smoothing = true` in config
- Try different `startup_duration` and `shutdown_duration` settings for smoother transitions
- Adjust `adaptive_interval` for extended durations
- Check that no other color temperature tools are running

### Display doesn't change

- If using the Hyprsunset backend, verify hyprsunset works independently: `hyprctl hyprsunset temperature 4000`
- Check configuration file syntax
- Look for error messages in terminal output, follow their recommendations
- Use `"wayland"` as your backend (even on Hyprland)

## 🪵 Changelog

### v0.10.0

- **Configuration Management Commands**: New `get` and `set` commands for CLI-based config management
  - `get` command reads configuration values with JSON output support
  - `set` command modifies configuration fields with validation and safety features
- **Enhanced Preset System**: Improved preset command with subcommands
  - `preset active` shows the currently active preset
  - `preset list` displays all available presets
- **Native Hyprland CTM Backend**: Experimental Color Transformation Matrix support for Hyprland
- **Comprehensive Help System**: Built-in help command with detailed documentation for all features
- **XDG Base Directory Support**: Migrated state management to follow XDG specifications
- **Improved Error Handling**: Consistent error severity levels and better user guidance
- **Interactive Configuration Warnings**: Safer configuration editing with preset warnings
- **Enhanced Logger**: Colored severity levels and cleaner output formatting
- **Bug Fixes**:
  - Fixed config directory handling with `--config` flag
  - Resolved smooth transition issues during reload for Hyprland

### v0.9.0

- **Static Mode**: New transition mode for maintaining constant temperature/gamma values
- **Preset Management System**: Quick switching between configuration profiles with `sunsetr preset`
- **Hot Configuration Reloading**: Automatic detection and application of config file changes
- **Custom Config Directories**: Support for portable configurations with `--config` flag
- **Enhanced Smooth Transitions**: Configurable startup/shutdown durations with adaptive algorithm
- **Improved D-Bus Handling**: Better recovery from system sleep/resume cycles
- **Configuration Refactoring**: Modular config system with better organization and validation
- **CLI Architecture Improvements**: Subcommand-based CLI with backward compatibility

### v0.8.0

- **D-Bus Sleep/Resume Detection**: Automatically resumes from sleep using systemd-recommended D-Bus approach
- **No Root Scripts**: Sleep/resume detection now runs entirely in user space via D-Bus
- **Nix Flake Support**: Added official flake.nix with reproducible builds and development shell (Special thanks [@scottmckendry](https://https://github.com/scottmckendry))
- **Display Hotplug Detection**: Automatically detects and handles monitor connection/disconnection (Special thanks [@scottmckendry](https://https://github.com/scottmckendry))

### v0.7.0

- **Runtime Simulations**: New `--simulate` command for testing transitions and geo calculations
- **NixOS/Nix Support**: Now available in nixpkgs unstable repository (Special thanks [@DoctorDalek1963](https://github.com/DoctorDalek1963))
- **Enhanced Logging System**: Zero-cost abstraction via macros, improved performance and cleaner output formatting
- **Progress Bar Improvements**: Extracted reusable progress bar component with new EMA smoothing
- **Geo Module Refactoring**: Improved transition time calculations, fixed nanosecond precision timing bugs

### v0.6.0

- **Privacy-Focused Geo Configuration**: New optional `geo.toml` file for privately storing coordinates separately from main config
- **Smoother Startup Transitions**: New Bézier curve for startup transitions and new minimum of 1 second `startup_transition_duration`

### v0.5.0

- **Geographic Location Support**: Complete implementation of location-based sunrise/sunset calculations
- **Interactive City Selection**: Fuzzy search interface with 10,000+ cities worldwide (`sunsetr geo`)
- **Automatic Location Detection**: Smart timezone-based coordinate detection with 466 timezone mappings
- **Enhanced Transitions**: Fine-tuned sun elevation angles and Bézier curves for more natural transitions
- **Extreme Latitude Handling**: Robust polar region support with seasonal awareness
- **Comprehensive Timezone System**: Multiple detection methods with intelligent fallbacks
- **Geographic Debug Mode**: Detailed solar calculation information for location verification
- **Timezone Precision**: Automatic timezone determination from coordinates for accurate times
- **Default Geo Mode**: New installations use geographic mode by default for optimal experience
- **Live Reload Command**: New `reload` flag to reload configuration without restarting
- **Interactive Testing**: New `test` command for trying different temperature/gamma values
- **Signal-Based Architecture**: Improved process communication for reload and test commands

## TODO

- [x] Set up AUR package
- [x] Make Nix installation available
- [x] Implement gradual transitions
- [x] Multi-compositor Wayland support
- [x] Geolocation-based transitions
- [x] Implement Hyprland native CTM backend
- [ ] Make Fedora Copr installation available
- [ ] Make Debian/Ubuntu installation available

## 💛 Thanks

- to wlsunset and redshift for inspiration
- to the Hyprwm team for making Hyprland possible
- to the niri team for making the best Rust-based Wayland compositor
- to the Wayland community for the robust protocol ecosystem
