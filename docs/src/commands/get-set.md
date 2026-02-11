# get & set

<!-- toc -->

Read and modify configuration values.

## Using the `get` command

Read configuration values from sunsetr configuration files.

**Usage:**

```bash
sunsetr get <FIELD>...
sunsetr get all
sunsetr get <FIELD> --json
sunsetr get <FIELD> --target <PRESET>
```

**Arguments:**

- `FIELD`: One or more configuration field names
- `all`: Special keyword to retrieve all configuration values

**Flags:**

- `--json`: Output in JSON format
- `--target <PRESET>, -t <PRESET>`: Read from specific preset (default: `default`)

**Examples:**

```bash
# Get specific field
sunsetr get night_temp
# Output: night_temp = 3300

# Get multiple fields
sunsetr get night_temp day_temp
# Output:
# night_temp = 3300
# day_temp = 6500

# Get all configuration
sunsetr get all

# JSON output for scripting
sunsetr get night_temp --json
# Output: {"night_temp": 3300}

# Full config as JSON
sunsetr get all --json

# Read from specific preset
sunsetr get night_temp --target gaming
sunsetr get night_temp -t day
```

**Available Fields:**

All fields from `sunsetr.toml`:

- `backend`
- `transition_mode`
- `smoothing`
- `startup_duration`
- `shutdown_duration`
- `adaptive_interval`
- `night_temp`
- `day_temp`
- `night_gamma`
- `day_gamma`
- `update_interval`
- `static_temp`
- `static_gamma`
- `sunset`
- `sunrise`
- `transition_duration`
- `latitude`
- `longitude`

**Notes:**

- Does not require sunsetr to be running
- Reads from configuration files directly
- Useful for scripting and automation

## Using the `set` command

Modify configuration values with validation and safety features.

**Usage:**

```bash
sunsetr set <FIELD>=<VALUE>...            # assign values
sunsetr set <FIELD>+=<VALUE>...           # increment values
sunsetr set <FIELD>-=<VALUE>...           # decrement values
sunsetr set --target <PRESET> <FIELD>=<VALUE>...
```

**Operators:**

| Operator | Description        | Example           |
| -------- | ------------------ | ----------------- |
| `=`      | Assign a value     | `night_temp=3500` |
| `+=`     | Increment by value | `night_temp+=500` |
| `-=`     | Decrement by value | `night_temp-=500` |

The `+=` and `-=` operators are supported on temperature and gamma fields only (`night_temp`, `day_temp`, `static_temp`, `night_gamma`, `day_gamma`, `static_gamma`). They read the current value from the config file, compute the new absolute value, and pass it through the normal validation pipeline.

**Arguments:**

- `FIELD[+|-]=VALUE`: One or more field-operator-value triples to set

**Flags:**

- `--target <PRESET>, -t <PRESET>`: Modify specific preset configuration (default: `default`)

**Virtual Aliases:**

The `current_temp` and `current_gamma` aliases resolve to the concrete field matching the running instance's active period via IPC:

| Active Period | `current_temp` resolves to | `current_gamma` resolves to |
| ------------- | -------------------------- | --------------------------- |
| Day           | `day_temp`                 | `day_gamma`                 |
| Sunrise       | `day_temp`                 | `day_gamma`                 |
| Night         | `night_temp`               | `night_gamma`               |
| Sunset        | `night_temp`               | `night_gamma`               |
| Static        | `static_temp`              | `static_gamma`              |

These aliases require a running sunsetr instance (they use IPC to determine the current period). They cannot be combined with the `--target` flag.

**Examples:**

```bash
# Set single value
sunsetr set night_temp=3500

# Set multiple values
sunsetr set night_temp=3500 day_temp=6000

# Increment/decrement values
sunsetr set night_temp+=500               # increase night temp by 500K
sunsetr set night_temp-=200               # decrease night temp by 200K
sunsetr set static_gamma+=5               # increase static gamma by 5%
sunsetr set night_temp+=500 day_gamma-=10 # mix operators in one command

# Adjust the active period's temperature without knowing the period
sunsetr set current_temp+=500
sunsetr set current_temp=3500 current_gamma=90

# Modify specific preset
sunsetr set --target gaming static_temp=4700
sunsetr set -t day static_gamma=110
```

**Keybinding Examples:**

The `current_temp` alias combined with `+=`/`-=` is ideal for keybindings where you want to adjust the active temperature on the fly:

Hyprland (`hyprland.conf`):

```bash
bind = $mod, SHIFT, up, exec, sunsetr set current_temp+=500
bind = $mod, SHIFT, down, exec, sunsetr set current_temp-=500
```

Niri (`config.kdl`):

```bash
Mod+Shift+Up { spawn "sh" "-c" "sunsetr set current_temp+=500"; }
Mod+Shift+Down { spawn "sh" "-c" "sunsetr set current_temp-=500"; }
```

**Notes:**

- Does not require sunsetr to be running (except when using `current_temp`/`current_gamma` aliases)
- Changes are written to configuration files using atomic file replacement
- If sunsetr is running, changes are applied immediately via hot reload and a `config_changed` [IPC event](../advanced/ipc.md#event-types) is emitted
