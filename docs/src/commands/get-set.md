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
sunsetr set <FIELD>=<VALUE>... # sets values in default config
sunsetr set --target <PRESET> <FIELD>=<VALUE>...
```

**Arguments:**

- `FIELD=VALUE`: One or more field-value pairs to set

**Flags:**

- `--target <PRESET>, -t <PRESET>`: Modify specific preset configuration (default: `default`)

**Examples:**

```bash
# Set single value
sunsetr set night_temp=3500

# Set multiple values
sunsetr set night_temp=3500 day_temp=6000

# Modify specific preset
sunsetr set --target gaming static_temp=4700
sunsetr set -t day static_gamma=110
```

**Notes:**

- Does not require sunsetr to be running
- Changes are written to configuration files
- If sunsetr is running, changes are applied immediately via hot reload
