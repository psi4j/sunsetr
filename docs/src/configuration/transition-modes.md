# Transition Modes

<!-- toc -->

Transition modes determine when and how sunsetr adjusts color temperature. There are 4 `time-based` modes and a `static` mode. Time-based modes use these settings since they gradually transition from day to night values and back regularly at their specified times:

```toml
#[Time-based config]
night_temp = 3300        # Color temperature during night (1000-20000) Kelvin
day_temp = 6500          # Color temperature during day (1000-20000) Kelvin
night_gamma = 90         # Gamma percentage for night (10-200%)
day_gamma = 100          # Gamma percentage for day (10-200%)
update_interval = 60     # Update frequency during transitions in seconds (10-300)
```

## 1. `geo` (Geographic) - Recommended

```toml
transition_mode = "geo"
```

Automatically calculates sunrise and sunset windows based on your geographic location. This provides the most natural transitions that change throughout the year as seasons shift.

**How it works:**

- Uses your latitude and longitude to calculate solar elevation angles
- Determines precise sunrise and sunset times for your location
- Transitions windows match the calculated solar timing closely
- Automatically recalculates daily after midnight

**Configuration:**

```toml
transition_mode = "geo"
latitude = 40.7128      # Your latitude
longitude = -74.0060    # Your longitude
```

**Note**: When using geo mode, the `sunset`, `sunrise`, and `transition_duration` settings are ignored. These values are calculated automatically from your coordinates.

See [Geographic Setup](../configuration/geographic.md) for detailed location configuration.

## Manual Transitions:

For `finish_by`, `start_at`, and `center` modes, configure these settings:

```toml
sunset = "19:00:00"           # HH:MM:SS format
sunrise = "06:00:00"          # HH:MM:SS format
transition_duration = 45      # Minutes (5-120)
```

### 2. `finish_by` (Complete By Time)

```toml
transition_mode = "finish_by"
```

Ensures transitions **complete exactly** at the configured sunset and sunrise times.

**Behavior:**

- Transition begins `transition_duration` minutes **before** the configured time
- Reaches target temperature/gamma **at** the configured time

**Example:**

```toml
transition_mode = "finish_by"
sunset = "19:00:00"
transition_duration = 45

# Transition starts: 18:15:00
# Transition ends:   19:00:00 ← Configured time
```

**When to use:** You want night mode fully active at a specific time.

### 3. `start_at` (Begin At Time)

```toml
transition_mode = "start_at"
```

Transitions **begin exactly** at the configured sunset and sunrise times.

**Behavior:**

- Transition begins **at** the configured time
- Reaches target temperature/gamma `transition_duration` minutes **after**

**Example:**

```toml
transition_mode = "start_at"
sunset = "19:00:00"
transition_duration = 45

# Transition starts: 19:00:00 ← Configured time
# Transition ends:   19:45:00
```

**When to use:** You want transitions to start at specific times.

### 4. `center` (Center On Time)

```toml
transition_mode = "center"
```

Transitions are **centered** around the configured sunset and sunrise times.

**Behavior:**

- Transition begins `transition_duration / 2` minutes **before**
- Reaches target at configured time (midpoint)
- Continues `transition_duration / 2` minutes **after**

**Example:**

```toml
transition_mode = "center"
sunset = "19:00:00"
transition_duration = 60

# Transition starts: 18:30:00
# Midpoint:          19:00:00 ← Configured time
# Transition ends:   19:30:00
```

**When to use:** You want the transition midpoint to align with specific times.

## 5. `static` (Constant Values)

```toml
transition_mode = "static"
```

Maintains constant color temperature and gamma values without any time-based transitions.

**When to use:**

- You want consistent display settings 24/7
- You prefer manual control over automatic adjustments
- You're creating a preset for specific lighting conditions
- You need color accuracy (e.g., photo editing)

**Configuration:**

```toml
transition_mode = "static"
static_temp = 6500      # Constant temperature
static_gamma = 100      # Constant gamma
```

**Note**: When using static mode, all time-based settings (`night_temp`, `day_temp`, `update_interval`, etc ) are ignored.

**Examples:**

```toml
# Always neutral (daytime)
static_temp = 6500
static_gamma = 100

# Always warm (nighttime)
static_temp = 3300
static_gamma = 90

# Gaming/color-accurate mode
static_temp = 6500
static_gamma = 100
```
