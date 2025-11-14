# Geographic Setup

<!-- toc -->

When using `transition_mode = "geo"`, sunsetr automatically calculates sunrise and sunset times based on your geographic location. This provides the most natural transitions that adjust throughout the year as seasons change.

## Interactive City Selection

The easiest way to configure your location is using the interactive city selector. This launches a fuzzy search interface where you can search a local database of 10,000+ cities worldwide by country or city name to populate your coordinates.

### Example Usage

```bash
$ sunsetr geo
```

You'll see this interface:

```
┣ Select the nearest city for more accurate transition times
┃   Type to search, use ↑/↓ to navigate, Enter to select, Esc to cancel
┃
┃ Search: _
┃ ▶ A Coruna, Spain
┃   Aabenraa, Denmark
┃   Aachen, Germany
┃   Aalborg, Denmark
┃   Aalst, Belgium
┃ 100 of 10592 cities
```

### After Selection

Once you select a city, sunsetr will:

1. **Show calculated times** for today:

```
┣ Sun times for A Coruna, Spain (43.3713°N, 8.3960°W)
┃   Today's sunset: 18:09 (transition from 17:18 to 18:19)
┃   Tomorrow's sunrise: 08:27 (transition from 08:16 to 09:18)
┃   Sunset transition duration: 61 minutes
┃   Sunrise transition duration: 61 minutes
```

2. **Save coordinates** to your configuration
3. **Change config** to `transition_mode="geo"`
4. **Reload automatically** with the new location

The coordinates are saved to the active configuration (default or an active [preset](../presets/)):

- `~/.config/sunsetr/sunsetr.toml` (default)

## Testing other cities' coordinates

I realize we might want to test other cities' sunset/sunrise times and transition durations. Maybe we have to fly to another timezone for a special event and we want to get ahead of the jet lag and fix our sleeping schedule to their timezone.

Just run `sunsetr geo`. If you run this with `--debug`, you'll see an additional set of times in brackets `[]` to the right of the primary set of times. These times are in your autodetected local timezone. The primary set of times correspond to the selected city's coordinates' sunset/sunrise transition times. Ex:

```
┣[DEBUG] Solar calculation details for 2025-11-06:
┃           Raw coordinates: 35.6895°, 139.6917°
┃               Sunrise UTC: 21:07
┃                Sunset UTC: 07:41
┃       Coordinate Timezone: Asia/Tokyo (+09:00)
┃            Local timezone: America/Chicago (-06:00)
┃     Current time (Coords): 10:39:17
┃      Current time (Local): 19:39:17
┃           Time difference: +15 hours
┃   --- Sunrise (ascending) ---
┃          Civil dawn (-6°): 05:41:07 [14:41:07]
┃    Transition start (-2°): 05:58:53 [14:58:53]
┃              Sunrise (0°): 06:07:46 [15:07:46]
┃     Golden hour end (+6°): 06:34:25 [15:34:25]
┃     Transition end (+10°): 06:52:11 [15:52:11]
┃          Sunrise duration: 53 minutes
┃              Day duration: 9 hours 5 minutes (11-06)
┃   --- Sunset (descending) ---
┃   Transition start (+10°): 15:57:30 [00:57:30]
┃   Golden hour start (+6°): 16:15:16 [01:15:16]
┃               Sunset (0°): 16:41:55 [01:41:55]
┃      Transition end (-2°): 16:50:48 [01:50:48]
┃          Civil dusk (-6°): 17:08:34 [02:08:34]
┃           Sunset duration: 53 minutes
┃            Night duration: 13 hours 8 minutes (11-06 → 11-07)
┃
┣[DEBUG] Next transition will begin at: 15:57:30 [00:57:30] Day 󰖨  → Sunset 󰖛
```

## Using Arbitrary Coordinates

If the city selector (`sunsetr geo`) is not as precise as you'd like, you're welcome manually add coordinates to `sunsetr.toml`. I recommend using https://www.geonames.org/ or Google Earth to find your coordinates. North is positive, South is negative. East is positive, West is negative.

```toml
#[Geolocation]
latitude = 29.424122   # just switch these up
longitude = -98.493629 # `sunsetr --debug` to see the times/duration
```

## Privacy-Focused Geographic Configuration

If you version control your configuration files (e.g., in a dotfiles repository), you may not want to expose your geographic location. sunsetr supports storing coordinates in a separate `geo.toml` file that you can keep private:

1. **Create the geo.toml file** in the same directory as your sunsetr.toml:

   ```bash
   touch ~/.config/sunsetr/geo.toml
   ```

2. **Add geo.toml to your .gitignore**:

   ```bash
   echo "geo.toml" >> ~/.gitignore
   ```

3. **Run `sunsetr geo`** to populate it (or enter manual coordinates)

4. **Delete or spoof coordinates in** `sunsetr.toml`

Once `geo.toml` exists, it will:

- Override any coordinates in your main `sunsetr.toml`
- Receive all coordinate updates when you run `sunsetr geo`
- Keep your location private while allowing you to version control all other settings

Example `geo.toml`:

```toml
#[Private geo coordinates]
latitude = 40.7128
longitude = -74.0060
```

This separation allows you to share your sunsetr configuration publicly without accidentally doxxing yourself. `geo.toml` can also serve as a temporary place to store your coordinates when travelling.

⭐ **Note**: Your debug output will still print your coordinates on startup for debugging purposes, so be extremely careful when sharing your debug output online.

```
┣ Loaded default configuration
┃   Loaded coordinates from geo.toml
┃   Backend: Auto (Wayland)
┃   Mode: Time-based (geo)
┃   Location: 41.850°N, 87.650°W <--- ⭐ Careful!
┃   Night: 3300K @ 90% gamma
┃   Day: 6500K @ 100% gamma
┃   Update interval: 60 seconds
```

## Next Steps

- **[Explore configuration options](../configuration/)** - Customize temperature and gamma values
- **[Create presets](/presets/)** - Set up location-based presets for travel
- **[Learn about commands](/commands/)** - See all available CLI commands
