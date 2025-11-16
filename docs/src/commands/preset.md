# preset

Switch between configuration presets or view preset information.

- See [Preset System](../presets/) for detailed preset configuration

## Usage

```bash
sunsetr preset <PRESET_NAME>
sunsetr preset active
sunsetr preset list
```

## Subcommands

### `preset <name>` - Switch to a specific preset

```bash
sunsetr preset day      # Switch to day preset
sunsetr preset gaming   # Switch to gaming preset
sunsetr preset default  # Return to default configuration
```

### `preset active` - Show which preset is currently active

```bash
sunsetr preset active
```

Output:

```
gaming
```

### `preset list` - List all available presets

```bash
sunsetr preset list
```

Output:

```
day
gaming
weekend
london
```

## Toggle Behavior

Calling the same preset twice toggles back to default:

```bash
sunsetr preset day    # Switches to day preset
sunsetr preset day    # Switches back to default
```

## Notes

- Presets are stored in `~/.config/sunsetr/presets/`
- Each preset is a directory containing `sunsetr.toml` (and optionally `geo.toml`)
- See [Preset System](../presets/) for detailed preset configuration
