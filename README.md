# sunsetr

Automatic color temperature controller for hyprsunset.

![This image was taken using a shader to simulate the effect of hyprsunset](sunsetr.png)

**sunsetr** provides seamless day/night color temperature transitions for Hyprland using hyprsunset's IPC socket. With smart defaults and animated startup transitions, it delivers a smooth, unnoticeable experience that automatically adjusts your display throughout the day.

## Features

- **Startup Transitions**: Smooth transitions when starting, no jarring changes
- **Automatic hyprsunset Management**: Handles hyprsunset startup and communication automatically
- **Smart Defaults**: Works beautifully out-of-the-box with carefully tuned settings
- **Flexible Configuration**: Extensive customization options for power users
- **Robust Error Handling**: Graceful fallback and recovery from various error condition

## Dependencies

- **Hyprland 0.49.0** (tested version)
- **hyprsunset v0.2.0** (tested version)

## Installation

### Option 1: Build from Source

```bash
git clone https://github.com/psi4j/sunsetr.git
cd sunsetr
cargo build --release
sudo cp target/release/sunsetr /usr/local/bin/
```

### Option 2: AUR (Arch Linux)

_󰣇 AUR package is here!_

```bash
paru -S sunsetr-bin
```

## Recommended Setup

For the smoothest experience, add this line near the **beginning** of your `hyprland.conf`:

```bash
exec-once = sunsetr &
```

This ensures sunsetr starts early during compositor initialization, providing seamless color temperature management from the moment your desktop loads.

## Alternative Setup: Systemd Service

If you prefer systemd management:

```bash
systemctl --user enable --now sunsetr.service
```

## ⚙️ Configuration

sunsetr creates a default configuration at `~/.config/hypr/sunsetr.toml` on first run. The defaults provide an excellent out-of-the-box experience for most users:

```toml
#[Sunsetr configuration]
start_hyprsunset = true          # Set true if you're not using hyprsunset.service
startup_transition = true        # Enable smooth transition when sunsetr starts
startup_transition_duration = 10 # Duration of startup transition in seconds (10-60)
sunset = "19:00:00"              # Time to transition to night mode (HH:MM:SS)
sunrise = "06:00:00"             # Time to transition to day mode (HH:MM:SS)
night_temp = 3300                # Color temperature after sunset (1000-20000) Kelvin
day_temp = 6500                  # Color temperature during day (1000-20000) Kelvin
night_gamma = 90                 # Gamma percentage for night (0-100%)
day_gamma = 100                  # Gamma percentage for day (0-100%)
transition_duration = 45         # Transition duration in minutes (5-120)
update_interval = 60             # Update frequency during transitions in seconds (10-300)
transition_mode = "finish_by"    # Transition timing mode:
                                 # "finish_by" - transition completes at sunset/sunrise time
                                 # "start_at" - transition starts at sunset/sunrise time
                                 # "center" - transition is centered on sunset/sunrise time
```

### Key Settings Explained

- **`start_hyprsunset = true`** (recommended): sunsetr automatically starts and manages hyprsunset, eliminating setup complexity (requires you do not enable hyprsunset.service)
- **`startup_transition = false`** (recommended): Provides transition to correct interpolated temperature when starting
- **`transition_mode = "finish_by"`**: Ensures transitions complete exactly at sunset/sunrise times for consistent daily rhythm

## Alternative Configurations

### Using External hyprsunset Management

While **not recommended** due to added complexity, you can manage hyprsunset separately:

```toml
start_hyprsunset = false
```

Then start hyprsunset via systemd:

```bash
systemctl --user enable --now hyprsunset.service
```

Or in `hyprland.conf`:

```bash
exec-once = hyprsunset &
```

**Note**: I haven't extensively tested external hyprsunset management and recommend the default integrated approach for the smoothest experience.

### Smooth Startup Transition

For smooth startup transitions that ease in to the configured temperature and gamma values:

```toml
startup_transition = true
```

## Testing Color Temperatures

To test different temperatures before configuring:

```bash
# Stop sunsetr and hyprsunset temporarily
pkill sunsetr

# Or stop sunsetr and hyprsunset
systemctl --user stop sunsetr

# Test different values
hyprctl hyprsunset temperature 4000
hyprctl hyprsunset gamma 90

# Reset to defaults
hyprctl hyprsunset identity
hyprctl hyprsunset gamma 100

# Restart sunsetr
sunsetr &
```

## Version Compatibility

This version has been tested with:

- **Hyprland 0.49.0**
- **hyprsunset v0.2.0**

Other versions may work but haven't been extensively tested.

## 🙃 Troubleshooting

### sunsetr won't start

- Ensure hyprsunset is installed and accessible
- Check that Hyprland is running
- Verify `~/.config/hypr/` directory exists

### Startup transitions aren't smooth

- Ensure `startup_transition = true` in config
- Try different `startup_transition_duration` settings for smoother transitions
- Check that no other color temperature tools are running

### Display doesn't change

- Verify hyprsunset works independently: `hyprctl hyprsunset temperature 4000`
- Check configuration file syntax
- Look for error messages in terminal output

## 🪵 Changelog

### v0.3.0

- Added smooth animated startup transitions
- Dynamic transition tracking during startup
- Automatic hyprsunset management (now default)
- Comprehensive error handling and recovery
- Improved default configuration values
- Enhanced documentation and setup instructions

## TODO

- [x] Set up AUR package
- [x] Implement gradual transitions
- [ ] Make Nix installation available

## 🙏 Thanks

Special thanks to Vaxry and the Hyprwm team for making the best Wayland experience possible for the rest of us.
