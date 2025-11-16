# test

Test color temperature and gamma values temporarily without modifying your configuration.

## Usage

```bash
sunsetr test <TEMPERATURE> <GAMMA>
```

## Arguments

- `TEMPERATURE`: Color temperature in Kelvin (1000-20000)
- `GAMMA`: Gamma percentage (10-200)

## Examples

```bash
# Test typical night-time settings
sunsetr test 3300 90

# Try different warmth levels
sunsetr test 4000 95   # Slightly cooler, brighter
sunsetr test 3000 85   # Warmer, dimmer
sunsetr test 2500 80   # Very warm, quite dim

# Test day-time neutral values
sunsetr test 6500 100
```

## Behavior

- **Temporarily applies** the specified temperature and gamma values
- **Works with running instance** - Sends values to the existing sunsetr process
- **Press ESC or Ctrl+C** to automatically restore previous settings
- **Does not modify** your configuration file
- **Perfect for finding** your preferred settings before committing them to config
