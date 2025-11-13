# Quick Start

<!-- toc -->

This guide will help you get sunsetr running on your system in just a few minutes.

Once you've completed these steps, you can customize sunsetr with the [Configuration](configuration/) options, set your precise [Geographic location](configuration/geographic.md), or create [Presets](presets/) for different scenarios.

## First Run

On the first run, sunsetr will automatically create a default configuration file at `~/.config/sunsetr/sunsetr.toml`. The defaults are carefully tuned to provide an excellent experience out of the box:

```bash
sunsetr
```

You should see output like:

```
┣ Automatic location detection
┃   Detecting coordinates from system timezone...
┃   Detected timezone: America/Chicago
┃   Timezone mapping: Chicago, United States
┃   Coordinates: 41.8500°N, 87.6501°W
┃   Auto-detected location for new config: Chicago
┃
┣ Loaded default configuration
┃   Backend: Auto (Wayland)
┃   Mode: Time-based (geo)
┃   Location: 41.850°N, 87.650°W
┃   Night: 3300K @ 90% gamma
┃   Day: 6500K @ 100% gamma
┃   Update interval: 60 seconds
```

This will use your detected timezone to automatically populate coordinates for geolocation-based sunset and sunrise transitions. I recommend most users run the [geo command](commands/geo.md) to select more precise coordinates when using the geo transition mode.

## Compositor Setup

Sunsetr works best when started automatically via the compositor. Here's how to set it up for different compositors.

### Hyprland

Add this line near the **beginning** of your `~/.config/hyprland/hyprland.conf`:

```bash
exec-once = sunsetr
```

Starting sunsetr early during compositor initialization ensures seamless color temperature management from the moment your desktop loads.

**⚠️ WARNING:**

If selecting the Hyprland or Hyprsunset backend:

- **Do not use with hyprsunset's native config**: I recommend removing `hyprsunset.conf` entirely or backing it up. (sunsetr will need full control for smooth transition times)
- **Make sure hyprsunset isn't already running** if you want to use the Hyprland or Hyprsunset backends. You can check that a hyprsunset process isn't already running using btop or an alternative method.
- I recommend you **disable hyprsunset's systemd service** using `systemctl --user disable hyprsunset.service` and make sure to stop the process before running sunsetr.

### Niri

Add this line near the **beginning** of your `~/.config/niri/config.kdl`:

```kdl
spawn-at-startup "sunsetr"
```

### Sway

Add this line to your `~/.config/sway/config`:

```bash
exec sunsetr
```

### River

Add this line to your `~/.config/river/init`:

```bash
sunsetr &
```

### Wayfire

Add this to your `~/.config/wayfire.ini` in the `[autostart]` section:

```ini
[autostart]
sunsetr = sunsetr
```

### Other Wayland Compositors

Consult your compositor's documentation for how to start background applications on startup.

### Alternative Setup: Systemd Service

If you prefer systemd management over compositor-based startup:

```bash
systemctl --user enable --now sunsetr.service
```

**Note**: The systemd service file should be installed automatically with the listed [Installation](installation.md) methods, or you can install it using `cargo-make`:

```bash
cargo make install-service
```

## Running Sunsetr

Sunsetr runs in the **foreground by default**:

### Foreground Mode (Default)

```bash
sunsetr
```

This is the recommended way to run sunsetr when starting it from compositor configs. The process stays attached to your compositor's lifecycle.

### Background Mode

To run sunsetr as a background process:

```bash
sunsetr --background
```

This starts sunsetr in the background via the compositor. Useful if you're starting it manually from a terminal session and want to free up the terminal.

### Debug Mode

To see detailed logging including sunrise/sunset calculations for geo mode:

```bash
sunsetr --debug
```

### Verifying It's Working

After starting sunsetr, you can verify it's running and [see its current state](commands/status.md):

```bash
sunsetr status
```

You should see output showing:

```
 Active preset: default
Current period: Night
         State: stable
   Temperature: 3300K
         Gamma: 90.0%
   Next period: 06:29:21 (in 4h54m)
```

### Testing Your Setup

You may want to test various temperature and gamma settings to find your ideal values for night time using the [test](commands/test.md) command:

```bash
sunsetr test <TEMPERATURE> <GAMMA>
```

This temporarily applies temperature and gamma settings. Press **ESC** or **Ctrl+C** to restore the display and try something new.

The first value controls the color temperature (`1000-20000K`) and the second value (`10-200%`) controls the gamma of the display. Try different values to find what works best for you:

```bash
sunsetr test 4000 95   # Warm, a little dimmer
sunsetr test 3300 90   # Warmer, dimmer
sunsetr test 2333 70   # Very warm, quite dim
```

Once you've found your desired values (`night_temp`, `night_gamma`), you can set them in `~/.config/sunsetr/sunsetr.toml` or in a [custom location](advanced/custom-configs.md). You can do this by manually editing the config, or by using the [set](commands/get-set.md) command.

## Next Steps

Now that sunsetr is running, you might want to:

- **[Configure location](configuration/geographic.md)** - Use `sunsetr geo` to select your city for accurate sunrise/sunset times
- **[Customize settings](configuration/)** - Adjust temperatures, gamma values, and transition behavior
- **[Create presets](presets/)** - Set up different profiles for various scenarios
- **[Learn commands](commands/)** - Explore all available commands and options

## Troubleshooting

If sunsetr isn't working as expected, check the [Troubleshooting](troubleshooting.md) guide for solutions to common issues.
