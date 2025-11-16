# Preset Examples

Ready-to-use preset configurations for common scenarios.

## Static Day Mode

Perfect for daytime work or when you want consistent, neutral lighting.

`~/.config/sunsetr/presets/day/sunsetr.toml`:

```toml
#[Backend]
backend = "auto"
transition_mode = "static" # No time-based transitions

#[Smoothing]
smoothing = true
startup_duration = 0.5     # Quick fade-in
shutdown_duration = 0.5    # Instant shutdown
adaptive_interval = 1

#[Static configuration]
static_temp = 6500         # Neutral daylight
static_gamma = 100         # Full brightness
```

**Usage:**

```bash
sunsetr preset day         # Enable day mode
sunsetr preset day         # Toggle back to default
```

## Gaming Mode

Optimized for color accuracy and maximum brightness.

`~/.config/sunsetr/presets/gaming/sunsetr.toml`:

```toml
#[Backend]
backend = "auto"
transition_mode = "static"

#[Smoothing]
smoothing = true
startup_duration = 0.5
shutdown_duration = 0.5
adaptive_interval = 1

#[Static configuration]
static_temp = 6500           # Accurate colors
static_gamma = 115           # Slightly boosted brightness
```

**Usage:**

```bash
sunsetr preset gaming        # Enable gaming mode
```

## Weekend Schedule

Different sunrise/sunset schedule for weekends.

`~/.config/sunsetr/presets/weekend/sunsetr.toml`:

```toml
#[Backend]
backend = "auto"
transition_mode = "finish_by"

#[Smoothing]
smoothing = true
startup_duration = 0.5
shutdown_duration = 0.5
adaptive_interval = 1

#[Time-based config]
night_temp = 2800            # Warmer for late nights
day_temp = 6500
night_gamma = 85             # Dimmer for comfort
day_gamma = 100
update_interval = 60

#[Manual transitions]
sunset = "22:00:00"          # Stay up later
sunrise = "09:00:00"         # Sleep in
transition_duration = 90     # Longer transitions
```

**Usage:**

```bash
# Friday night
sunsetr preset weekend

# Monday morning
sunsetr preset default
```

## Location-Based Preset (Travel)

Separate coordinates for different locations.

`~/.config/sunsetr/presets/london/sunsetr.toml`:

```toml
#[Backend]
backend = "auto"
transition_mode = "geo"      # Use London coordinates

#[Smoothing]
smoothing = true
startup_duration = 0.5
shutdown_duration = 0.5
adaptive_interval = 1

#[Time-based config]
night_temp = 3300
day_temp = 6500
night_gamma = 90
day_gamma = 100
update_interval = 60

#[Geolocation]
latitude = 51.508415
longitude = -0.125533
```

**Usage:**

```bash
# When traveling to London
sunsetr preset london

# After returning home
sunsetr preset default
```

## Reading Mode

Extra warm, dimmed for late-night reading.

`~/.config/sunsetr/presets/reading/sunsetr.toml`:

```toml
#[Backend]
backend = "auto"
transition_mode = "static"

#[Smoothing]
smoothing = true
startup_duration = 0.5       # Slow transition
shutdown_duration = 0.5
adaptive_interval = 1

#[Static configuration]
static_temp = 2333           # Very warm
static_gamma = 75            # Quite dim
```

**Usage:**

```bash
sunsetr preset reading       # Late-night reading mode
```

## Minimal Blue Light

Maximum blue light reduction for sensitive users.

`~/.config/sunsetr/presets/no-blue/sunsetr.toml`:

```toml
#[Backend]
backend = "auto"
transition_mode = "static"

#[Smoothing]
smoothing = true
startup_duration = 0.5
shutdown_duration = 0.5
adaptive_interval = 1

#[Static configuration]
static_temp = 1000           # Extreme warmth
static_gamma = 70            # Reduced brightness
```

**Usage:**

```bash
sunsetr preset no-blue       # Extreme blue light reduction
```
