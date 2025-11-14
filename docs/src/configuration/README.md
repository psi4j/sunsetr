# Configuration

<!-- toc -->

Sunsetr is highly configurable through TOML files. This section covers all configuration options and how to customize sunsetr for your needs.

## Configuration Files

Sunsetr creates its configuration at `~/.config/sunsetr/sunsetr.toml` on first run.

### Default Configuration

Here's the complete default configuration with all available options:

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
night_gamma = 90         # Gamma percentage for night (10-200%)
day_gamma = 100          # Gamma percentage for day (10-200%)
update_interval = 60     # Update frequency during transitions in seconds (10-300)

#[Static config]
static_temp = 6500       # Color temperature for static mode (1000-20000) Kelvin
static_gamma = 100       # Gamma percentage for static mode (10-200%)

#[Manual transitions]
sunset = "19:00:00"      # Time for manual sunset calculations (HH:MM:SS)
sunrise = "06:00:00"     # Time for manual sunrise calculations (HH:MM:SS)
transition_duration = 45 # Transition duration in minutes (5-120)

#[Geolocation]
latitude = 30.267153     # Geographic latitude (auto-detected on first run)
longitude = -97.743057   # Geographic longitude (use 'sunsetr geo' to change)
```

### Configuration Location

The configuration directory structure looks like this:

```
~/.config/sunsetr/
├── sunsetr.toml         # Main configuration file
├── geo.toml             # Optional: private geographic coordinates
└── presets/             # Optional: preset configurations
    ├── day/
    │   └── sunsetr.toml
    ├── gaming/
    │   └── sunsetr.toml
    └── ...
```

For more information on how to use and manage presets, please see the [preset](../commands/preset.md) command.

### Configuration Management

Sunsetr provides CLI commands for reading and modifying configuration values:

- [`sunsetr get`](../commands/get-set.md#get) - Read configuration values
- [`sunsetr set`](../commands/get-set.md#set) - Modify configuration values

### Hot Reloading

Sunsetr automatically detects and applies configuration changes without requiring a restart. Simply edit your configuration file, save it, and sunsetr will reload the new settings.

**Watched files:**

- `~/.config/sunsetr/sunsetr.toml` - Main configuration
- `~/.config/sunsetr/geo.toml` - Private geo coordinates (if it exists)
- Active preset configuration files

See [Hot Reloading](hot-reloading.md) in Advanced Features for more details.

## Next Steps

- **[Backend Selection](backends.md)** - Choose the right backend for your compositor
- **[Transition Modes](transition-modes.md)** - Configure when and how color temperature changes
- **[Temperature & Gamma](temperature-gamma.md)** - Fine-tune display settings
- **[Smooth Transitions](smoothing.md)** - Configure startup/shutdown animations
