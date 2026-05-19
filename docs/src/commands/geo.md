# geo

Configure geographic location through an interactive city selector.

- See [Geographic Setup](../configuration/geographic.md) for more details on location configuration

## Usage

```bash
sunsetr geo
sunsetr geo --target <PRESET>
```

**Flags:**

- `--target <PRESET>, -t <PRESET>`: Update a specific preset. Use `default` for the base configuration.

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

# Update a specific preset's coordinates
sunsetr geo --target gaming

# Update the default config in a custom directory
sunsetr --config ~/dotfiles/sunsetr/ geo --target default

# Run sunsetr in debug mode to see detailed solar calculations
sunsetr --debug
# Or
sunsetr restart -d
```

## Behavior

After selecting a city, sunsetr will:

1. **Display calculated times** for today (sunrise, sunset, transition durations)
2. **Save coordinates** to the target configuration:
   - `geo.toml` (if it exists, kept private via `.gitignore`)
   - `sunsetr.toml` (otherwise)
3. **Change config** to `transition_mode="geo"`
4. **Apply automatically** via [hot reload](../configuration/hot-reloading.md) if sunsetr is running

## Notes

- Only works with `transition_mode = "geo"`
- See [Geographic Setup](../configuration/geographic.md) for more details on location configuration
