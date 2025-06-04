# sunsetr

Automatic blue light filter for Hyprland, niri, and Wayland compositors

![This image was taken using a shader to simulate the effect of hyprsunset](sunsetr.png)

## Features

- **Multi-Compositor Support**: Works with Hyprland, niri, Sway, river, Wayfire, and other Wayland compositors
- **Automatic Backend Detection**: Intelligently detects your compositor and uses the appropriate backend
- **Startup Transitions**: Smooth transitions when starting, no jarring changes
- **Smart hyprsunset Management**: Automatically handles hyprsunset startup and communication on Hyprland
- **Universal Wayland Support**: Direct protocol communication on non-Hyprland compositors
- **Smart Defaults**: Works beautifully out-of-the-box with carefully tuned settings
- **Flexible Configuration**: Extensive customization options for power users
- **Robust Error Handling**: Graceful fallback and recovery from various error conditions

## Dependencies

### For Hyprland Users

- **Hyprland 0.49.0** (tested version)
- **hyprsunset v0.2.0** (tested version)

### For Other Wayland Compositors

- **Any Wayland compositor** supporting `wlr-gamma-control-unstable-v1` protocol
- **No external dependencies** - uses native Wayland protocols

## Installation

### Option 1: Build from Source

```bash
git clone https://github.com/psi4j/sunsetr.git
cd sunsetr
cargo build --release
sudo cp target/release/sunsetr /usr/local/bin/
```

### Option 2: AUR (Arch Linux)

```bash
paru -S sunsetr-bin
```

## Recommended Setup

### Startup

#### Hyprland

For the smoothest experience on Hyprland, add this line near the **beginning** of your `hyprland.conf`:

```bash
exec-once = sunsetr &
```

This ensures sunsetr starts early during compositor initialization, providing seamless color temperature management from the moment your desktop loads.

⚠️ WARNING: You will need to be sure you don't have hyprsunset already running if you want this to work with `start_hyprsunset = true` from the default config. I recommend disabling hyprsunset's systemd service using `systemctl --user disable hyprsunset.service` and make sure to stop the process before running sunsetr.

#### niri

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

## ⚙️ Configuration

sunsetr creates a default configuration at `~/.config/sunsetr/sunsetr.toml` on first run (legacy location `~/.config/hypr/sunsetr.toml` is still supported). The defaults provide an excellent out-of-the-box experience for most users:

```toml
#[Sunsetr configuration]
backend = "auto"                 # Backend: "auto", "hyprland", or "wayland"
start_hyprsunset = true          # Set true if you're not using hyprsunset.service (Hyprland only)
startup_transition = false       # Enable smooth transition when sunsetr starts
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

- **`backend = "auto"`** (recommended): Automatically detects your compositor and uses the appropriate backend. Use auto if you plan on using sunsetr on both Hyprland and other Wayland compositors like niri or Sway.
- **`start_hyprsunset = true`** (Hyprland only): sunsetr automatically starts and manages hyprsunset. This setting will not start hyprsunset on any non-Hyprland Wayland compositor and will be ignored. Keep this set to true and choose `auto` as your backend if you want to run sunsetr as a controller for hyprsunset on Hyprland and also plan to use other Wayland compositors. I switch between niri and Hyprland and this is the setting I use.
- **`startup_transition = false`**: Provides smooth transition to correct values when starting. This setting is useful if you have an exceptionally slow startup time when logging in for what ever reason and want the temperature change to be smooth at startup.
- **`transition_mode = "finish_by"`**: Ensures transitions complete exactly at sunset/sunrise times. Feel free to try out the other settings.

### Backend-Specific Configuration

#### Automatic Detection (Recommended)

```toml
backend = "auto"
```

sunsetr will automatically detect your compositor and configure itself appropriately.

#### Explicit Backend Selection

```toml
# For Hyprland users
backend = "hyprland"
start_hyprsunset = true

# For other Wayland compositors (Though it works on Hyprland too)
backend = "wayland"
# Ignored on non-Hyprland compositors when backend is set to auto
start_hyprsunset = false
```

## Alternative Configurations

### Using External hyprsunset Management

While **not recommended** due to added complexity, you can manage hyprsunset separately. Set this to false in `sunsetr.toml`:

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

## Hyprland

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

## Wayland

Don't try changing the time using `timedatectl` or anything like that. For now, change your `sunset` time earlier than the current time and start sunsetr. I'll be adding some simpler methods for testing real soon.

## ✓ Version Compatibility

### Hyprland

- **Hyprland >=0.49.0**
- **hyprsunset >=v0.2.0**

Other versions may work but haven't been extensively tested.

### Other Wayland Compositors

- **niri** (any version with wlr-gamma-control support)
- **Sway** (any version with wlr-gamma-control support)
- **river** (any version with wlr-gamma-control support)
- **Wayfire** (any version with wlr-gamma-control support)
- **Other wlr-based compositors** with gamma control support

## 🙃 Troubleshooting

### sunsetr won't start hyprsunset

- Ensure hyprsunset is installed and accessible if you're attempting to use sunsetr as a controller
- Be sure you're running on Hyprland

### Startup transitions aren't smooth

- Ensure `startup_transition = true` in config
- Try different `startup_transition_duration` settings for smoother transitions
- Check that no other color temperature tools are running

### Display doesn't change

- Verify hyprsunset works independently: `hyprctl hyprsunset temperature 4000` (hyprsunset has to be running)
- Check configuration file syntax
- Look for error messages in terminal output, follow their recommendations
- Use `"wayland"` as your backend and set `start_hyprsunset = false` (even on Hyprland)

## 🪵 Changelog

### v0.4.0

- **Multi-Compositor Support**: Added support for nir, Sway, river, Wayfire, and other Wayland compositors
- **Automatic Backend Detection**: Smart detection of compositor type with appropriate backend selection
- **Universal Wayland Backend**: Complete implementation of wlr-gamma-control-unstable-v1 protocol
- **Enhanced Configuration System**: New `backend` field with dual config path support
- **Zero Breaking Changes**: Full backward compatibility with existing Hyprland configurations
- **Improved Error Handling**: Better error messages with actionable guidance
- **Comprehensive Testing**: Property-based testing for all backend scenarios

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
- [x] Multi-compositor Wayland support
- [ ] Geo-location-based transitions
- [ ] Make Nix installation available

## 💛 Thanks

- to wlsunset and redshift for inspiration
- to the Hyprwm team for making Hyprland possible for the rest of us
- to the niri team for making the best Rust-based Wayland compositor
- to the Wayland community for the robust protocol ecosystem
