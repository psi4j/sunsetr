# Temperature and Gamma Settings

<!-- toc -->

Color temperature and gamma control how your display looks during different periods.

## Color Temperature

Color temperature is measured in Kelvin (K) and affects the color "warmth" of your display:

- 2000-3000K: Very warm, orange-yellow (candlelight to incandescent)
- 3000-4000K: Warm white to neutral warm (halogen, sunrise/sunset)
- 4000-5000K: Neutral white to cool white (fluorescent)
- 5000-6500K: Daylight white (direct sunlight, overcast sky)
- 6500-8000K: Cool white to blue-white (bright daylight)
- 8000+K: Very cool, ice blue (this will wake you up)

**Valid range**: 1000-20000K

## Gamma

Gamma scales the overall brightness of your display:

- **10-80%**: Very dim
- **80-90%**: Dim, comfortable for night
- **90-100%**: Normal brightness
- **100-200%**: Brighter than the display's native white

**Valid range**: 10-200%

**Note**: Gamma values above 100% clip the brightest tones to white, so detail in highlights is lost. Values below 80% may make the display difficult to read.

## Day and Night Configuration

Configure separate temperature and gamma values for day and night periods:

```toml
# Daytime values (during day period)
day_temp = 6500          # Neutral, natural light
day_gamma = 100          # Full brightness

# Nighttime values (during night period)
night_temp = 3300        # Warm, reduced blue light
night_gamma = 90         # Slightly dimmed
```

## Update Interval

Controls how frequently sunsetr updates color temperature and gamma during sunset/sunrise transitions.

### Adaptive Mode (Default)

```toml
update_interval = "auto"
```

The default `"auto"` mode dynamically calculates the optimal interval at each point in the transition. It uses the configured temperature/gamma ranges and transition duration to keep every step below the **just-noticeable difference (JND)** threshold for human perception.

### Fixed Mode

```toml
update_interval = 60     # Seconds (10-300)
```

You can override adaptive mode with a fixed integer interval in seconds:

**Note**: `update_interval` only affects updates during sunset/sunrise transitions.

## Testing Values

Use the [test](../commands/test.md) command to temporarily try different temperature and gamma values:

```bash
# Test typical night-time settings
sunsetr test 3300 90

# Try different warmth levels
sunsetr test 4000 95   # Slightly cooler, brighter
sunsetr test 3000 85   # Warmer, dimmer
sunsetr test 2333 70   # Very warm, quite dim
```

Press **ESC** or **Ctrl+C** to restore previous settings.
