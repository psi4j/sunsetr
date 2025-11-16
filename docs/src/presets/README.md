# Preset System

<!-- toc -->

The preset system allows quick switching between different configuration profiles for different activities, times of day, days of the week, or locations.

## Preset Management

The preset system can be used via the CLI, added to keybinds, or bash scripts:

```bash
# Show currently active preset
sunsetr preset active

# List all available presets
sunsetr preset list

# Switch to a specific preset
sunsetr preset day
sunsetr preset gaming

# Return to default configuration
sunsetr preset default

# Or call the same preset twice to toggle back to default
sunsetr preset day
sunsetr preset day # returns to default
```

## Set up keyboard shortcuts for instant toggling:

### Hyprland (hyprland.conf)

```bash
bind = $mod, W, exec, sunsetr preset day # toggle between day preset and default config
```

### Niri (config.kdl)

```bash
Mod+W { spawn "sh" "-c" "sunsetr p day"; }
```

## Creating Presets

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
backend = "auto"           # Backend to use: "auto", "hyprland" or "wayland"
transition_mode = "static" # Select: "geo", "finish_by", "start_at", "center", "static"

#[Smoothing]
smoothing = true           # Enable smooth transitions during startup and exit
startup_duration = 0.5     # Duration of smooth startup in seconds (0.1-60 | 0 = instant)
shutdown_duration = 0.5    # Duration of smooth shutdown in seconds (0.1-60 | 0 = instant)
adaptive_interval = 1      # Adaptive interval base for smooth transition (1-1000)ms

#[Static configuration]
static_temp = 6500         # Color temperature for static mode (1000-20000) Kelvin
static_gamma = 100         # Gamma percentage for static mode (10-200%)
```

## Advanced Preset Usage

**Day-of-Week Preset Switching:**

Start sunsetr with different presets based on the day of week:

```bash
#!/bin/bash
# Start sunsetr with preset based on day of week

day=$(date +%u)  # 1=Monday, 7=Sunday

if [ $day -ge 1 ] && [ $day -le 5 ]; then
    # Weekdays: work schedule with earlier transitions
    sunsetr preset weekday
else
    # Weekends: relaxed schedule, sleep in
    sunsetr preset weekend
fi
```

Then use this script in your compositor startup:

**Hyprland:**

```bash
exec-once = ~/.config/hypr/scripts/start-sunsetr.sh
```

**Niri:**

```kdl
spawn-at-startup "~/.config/niri/scripts/start-sunsetr.sh"
```

## Next Steps

- **[See preset examples](examples.md)** - Ready-to-use preset configurations
- **[Learn about all commands](../commands/)** - Explore the full command reference
- **[Configure settings](../configuration/)** - Fine-tune your preset configurations
