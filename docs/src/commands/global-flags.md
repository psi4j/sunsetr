# Global Flags

<!-- toc -->

These flags modify how sunsetr runs and are available on the main command.

## `--debug`

Enable detailed debug output including solar calculations and state changes.

```bash
sunsetr --debug
```

**Shows:**

- Configuration loading details
- Detected/configured coordinates and timezone when using [geo](../configuration/transition-modes.md) mode
- Precise sunset/sunrise timing with transition boundaries
- Real-time state changes and temperature updates

## `--background`

Start sunsetr in the background via the compositor. Also compatible with the [restart](../commands/restart-stop.md) command.

```bash
sunsetr --background
sunsetr restart --background
```

**Note:** Not needed when starting from compositor config (`exec-once`, `spawn-at-startup`)

## `--config`

Use a custom configuration directory instead of `~/.config/sunsetr/`.

```bash
sunsetr --config ~/dotfiles/sunsetr/
```

**Use cases:**

- Portable configuration setups
- Multiple configuration profiles
- Custom dotfiles management

**Behavior:**

- All commands respect the custom directory
- Relative directory structure remains the same:

```
~/dotfiles/sunsetr/
├── sunsetr.toml
├── geo.toml
└── presets/
    └── [your presets]
```

**Examples:**

```bash
# Start with custom config
sunsetr --config ~/dotfiles/sunsetr/
```

**Note:** Once started with `--config`, subsequent commands during that session automatically use the custom directory.

See [Custom Config Directories](../advanced/custom-configs.md) for more details.

## `--simulate`

Test sunsetr's behavior across arbitrary time windows without waiting.

```bash
sunsetr --simulate "<START>" "<END>" <MULTIPLIER>
sunsetr --simulate "<START>" "<END>" --fast-forward
sunsetr --simulate "<START>" "<END>" <MULTIPLIER> --log
```

**Arguments:**

- `START`: Start time in format "YYYY-MM-DD HH:MM:SS"
- `END`: End time in format "YYYY-MM-DD HH:MM:SS"
- `MULTIPLIER`: Time speed multiplier (0.1x to 3600x)
- `--fast-forward`: Near-instant updates (maximum speed)
- `--log`: Save output to timestamped log file

**Examples:**

```bash
# Simulate evening to morning at 60x speed
sunsetr --simulate "2025-01-15 18:00:00" "2025-01-16 08:00:00" 60

# Fast-forward through time window
sunsetr --simulate "2025-01-15 18:00:00" "2025-01-16 08:00:00" --fast-forward

# Save output to log file
sunsetr --simulate "2025-01-15 18:00:00" "2025-01-16 08:00:00" 60 --log
# Creates: sunsetr-simulation-20250115-232140.log
```

**Behavior:**

- Simulates runtime during specified time window
- Faithfully reproduces actual behavior including temperature/gamma updates
- Shows all logging and state transitions
- Respects active preset and custom config directory

**Use for:**

- Testing geo calculations for specific dates
- Verifying transition timing
- Debugging time-dependent behavior
- Generating logs for bug reports

**Note:** At higher multipliers, actual time may exceed theoretical time due to system overhead.
