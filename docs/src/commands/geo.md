# geo

Choose where geographic mode takes its location from: a city picked in an interactive selector, or GeoClue.

- See [Geographic Setup](../configuration/geographic.md) for more details on location configuration

## Usage

```bash
sunsetr geo
sunsetr geo --target <PRESET>
sunsetr geo --source geoclue
```

**Flags:**

- `--target <PRESET>, -t <PRESET>`: Update a specific preset. Use `default` for the base configuration.
- `--source <SOURCE>, -s <SOURCE>`: `manual` (the default) picks a city and uses it. `geoclue` follows GeoClue, and any picked city left in the file is ignored. Both write `location_source` to the target configuration.

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

# Follow GeoClue, then show what it reports for this session
sunsetr geo --source geoclue

# Back to the picked city
sunsetr geo --source manual

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
