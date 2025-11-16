# Hot Reloading

Sunsetr automatically detects and applies configuration changes without requiring a restart.

## How It Works

Sunsetr watches these files for changes:

- **Main configuration**: `~/.config/sunsetr/sunsetr.toml`
- **Geographic coordinates**: `~/.config/sunsetr/geo.toml` (if it exists)
- **Active preset configuration**: `~/.config/sunsetr/presets/<active>/sunsetr.toml`
- **Active preset coordinates**: `~/.config/sunsetr/presets/<active>/geo.toml` (if it exists)

When any watched file changes:

1. **Detects change**
2. **Validates new configuration** before applying
3. **Applies smooth transition** to new values (if configured)
4. **Logs reload** in debug output

## Using Hot Reload

Simply edit your configuration and save:

```bash
# Start sunsetr
sunsetr

# In another terminal, edit config
vim ~/.config/sunsetr/sunsetr.toml

# Changes apply automatically on save!
```

## What Gets Hot-Reloaded

**Applies immediately:**

- Temperature values (`night_temp`, `day_temp`, `static_temp`)
- Gamma values (`night_gamma`, `day_gamma`, `static_gamma`)
- Update interval (`update_interval`)
- Transition mode changes (`transition_mode`)
- Coordinates (`latitude`, `longitude`)
- Timing values (`sunset`, `sunrise`, `transition_duration`)
- Smoothing settings (`smoothing`, `startup_duration`, `shutdown_duration`)

**Requires [restart](../commands/restart-stop.md):**

- Backend changes (`backend`)

## Hot Reload with Custom Config

Hot reload works with custom configuration directories:

```bash
# Start with custom config
sunsetr --config ~/dotfiles/sunsetr/

# Edit custom config
vim ~/dotfiles/sunsetr/sunsetr.toml

# Changes apply automatically
```

## Debugging Hot Reload

Use debug mode to see reload events:

```bash
sunsetr --debug
```

When configuration changes, you'll see:

```
┣[DEBUG] Reload state change detection:
┃   State: Day → Night
┃   Temperature: 6500 → 3300K
┃   Gamma: 100% → 90%
┃   Smooth transition: enabled
```

## Limitations

- **Backend changes** require `sunsetr restart`
- **Invalid configurations** are rejected (keeps current config)
- **Very rapid changes** may be debounced (1-2 second window)
