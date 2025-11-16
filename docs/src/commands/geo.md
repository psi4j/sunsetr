# geo

Configure geographic location through an interactive city selector.

- See [Geographic Setup](../configuration/geographic.md) for more details on location configuration

## Usage

```bash
sunsetr geo
```

## Interactive Interface

The geo command launches a fuzzy search interface where you can:

- **Type to search** from 10,000+ cities worldwide
- **Navigate** with arrow keys (↑/↓)
- **Select** with Enter
- **Cancel** with Esc
- **Search by city name or country**

## Examples

```bash
# Launch city selector
sunsetr geo

# Run sunsetr in debug mode to see detailed solar calculations
sunsetr --debug
# Or
sunsetr restart -d
```

## Behavior

After selecting a city, sunsetr will:

1. **Display calculated times** for today (sunrise, sunset, transition durations)
2. **Save coordinates** to either:
   - `~/.config/sunsetr/geo.toml` (if it exists)
   - `~/.config/sunsetr/sunsetr.toml` (otherwise)
3. **Change config** to `transition_mode="geo"`
4. **Restart automatically** with the new location

## Notes

- Only works with `transition_mode = "geo"`
- See [Geographic Setup](../configuration/geographic.md) for more details on location configuration
