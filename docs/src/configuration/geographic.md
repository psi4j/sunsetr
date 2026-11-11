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
┣ Sun times for A Coruna, Spain (43.37°N, 8.40°W)
┃   Today's sunset: 18:09 (transition from 17:18 to 18:19)
┃   Tomorrow's sunrise: 08:27 (transition from 08:16 to 09:18)
┃   Sunset transition duration: 61 minutes
┃   Sunrise transition duration: 61 minutes
```

2. **Save coordinates** to your configuration
3. **Change config** to `transition_mode="geo"`
4. **Apply automatically** via [hot reload](hot-reloading.md) if sunsetr is running

The coordinates are saved to the active configuration (default or an active [preset](../presets/)):

- `~/.config/sunsetr/sunsetr.toml` (default)

## Testing other cities' coordinates

I realize we might want to test other cities' sunset/sunrise times and transition durations. Maybe we have to fly to another timezone for a special event and we want to get ahead of the jet lag and fix our sleeping schedule to their timezone.

Just run `sunsetr geo`. If you run this with `--debug`, you'll see an additional set of times in brackets `[]` to the right of the primary set of times. These times are in your autodetected local timezone. The primary set of times correspond to the selected city's coordinates' sunset/sunrise transition times. Ex:

```
┣[DEBUG] Solar calculation details for 2025-11-06:
┃               Coordinates: 35.69°N, 139.69°E
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

If the city selector (`sunsetr geo`) is not as precise as you'd like, you're welcome to manually add coordinates to `sunsetr.toml`. I recommend using https://www.geonames.org/ or Google Earth to find your coordinates. North is positive, South is negative. East is positive, West is negative.

```toml
#[Geolocation]
latitude = 29.42   # just switch these up
longitude = -98.49 # `sunsetr --debug` to see the times/duration
```

Two decimals are plenty. The second decimal is about a kilometer, which moves sunrise and sunset by seconds, so sunsetr writes and shows coordinates at that precision.

## Automatic Location with GeoClue

sunsetr can optionally use coordinates from [GeoClue](https://gitlab.freedesktop.org/geoclue/geoclue), the freedesktop location service, so geo mode's sun times stay right when you travel without running `sunsetr geo` again.

```bash
sunsetr geo --source geoclue
```

This writes `location_source = "geoclue"` to your configuration, sets `transition_mode = "geo"` like the city picker does, and shows the sun times for the location GeoClue reports. To go back to a picked city, run `sunsetr geo` (or `sunsetr geo --source manual`), which sets the source back to manual. Manual is the default, and sunsetr never enables GeoClue on its own.

```toml
#[Geolocation]
location_source = "geoclue"
```

How it behaves:

- sunsetr asks GeoClue for city-level accuracy and rebuilds the sun times when the reported location moves more than 10 km.
- At startup it waits up to six seconds for a first fix. When none arrives in that time it uses the last fix it received, kept in the state directory as `~/.local/state/sunsetr/default/geoclue_fix`, so a start without network still has sun times. The cached fix is replaced as soon as GeoClue reports one.
- With no fix at all sunsetr exits with an error that says why. Run `sunsetr geo --source geoclue` once with a network connection to cache a fix, or `sunsetr geo` to pick a city.
- A reload that switches a running instance to GeoClue needs a fix the same way. Without one the reload is refused and the previous configuration keeps running, which the instance log reports.
- Any `latitude` and `longitude` in the file are accepted and ignored under GeoClue, and the config log says so. `sunsetr set latitude=...` still writes them for when you switch back to `manual`, but they never move the schedule while GeoClue is the source.
- Presets under one configuration directory share the cached fix, and a `--config` directory gets its own.
- `sunsetr status` shows the location in use, marked "(GeoClue)" whenever that is the configured source, with the age of the fix added while a cached one is in use. The debug log says whether a fix has arrived.

### GeoClue needs an agent

GeoClue hands out locations only to sessions that run an authorization agent, the component that decides which applications may ask where you are. Desktop shells such as GNOME and Phosh ship their own agent with a permission dialog. Hyprland, Niri, Sway and other compositors have none, and without one sunsetr logs `GeoClue has no authorization agent for this session` within five seconds of starting and exits. The cached fix is not used at startup without an agent. An agent that disappears while sunsetr is running is logged, and the location in use stays.

For those desktops GeoClue itself provides the agent to use, despite its path. The "demo" agent is a complete agent that approves every application without asking, and the package installs an XDG autostart entry for it at `/etc/xdg/autostart/geoclue-demo-agent.desktop`. Read that as what it is. With the demo agent running, any program in your session can read the same city-level location sunsetr reads. On an unsandboxed desktop every program could already look up your public IP, so this gives away little, but it is your call, which is why `location_source` defaults to `manual` and sunsetr never starts the agent for you.

The geoclue daemon needs no setup. It is started over D-Bus the moment sunsetr asks for a client. Only the agent has to be running in your session, and sunsetr accepts either order. When the agent is up first, the location is known within sunsetr's startup wait. When sunsetr is up first, it waits for the agent and switches to the reported location as soon as one registers.

If your session runs the XDG autostart directory, through `dex -a` or a similar helper, the agent starts on its own. Otherwise start it from your compositor config, ahead of sunsetr:

Hyprland (`hyprland.lua`):

```lua
hl.exec_cmd("/usr/lib/geoclue-2.0/demos/agent")
hl.exec_cmd("sunsetr")
```

Hyprland (`hyprland.conf`):

```bash
exec-once = /usr/lib/geoclue-2.0/demos/agent
exec-once = sunsetr
```

Niri (`config.kdl`):

```bash
spawn-at-startup "/usr/lib/geoclue-2.0/demos/agent"
spawn-at-startup "sunsetr"
```

Sway (`~/.config/sway/config`):

```bash
exec /usr/lib/geoclue-2.0/demos/agent
exec sunsetr
```

The path is Arch's. Other distributions install the demo agent under `libexec`, so check `geoclue-demo-agent.desktop` for yours.

If you run sunsetr from its systemd user unit, give the agent one too and order sunsetr after it:

```ini
# ~/.config/systemd/user/geoclue-agent.service
[Unit]
Description=GeoClue demo agent
PartOf=graphical-session.target
After=graphical-session.target

[Service]
ExecStart=/usr/lib/geoclue-2.0/demos/agent
Restart=on-failure

[Install]
WantedBy=graphical-session.target
```

```ini
# ~/.config/systemd/user/sunsetr.service.d/geoclue.conf
[Unit]
Wants=geoclue-agent.service
After=geoclue-agent.service
```

Then `systemctl --user enable --now geoclue-agent.service`.

### What GeoClue sends

Your location comes from GeoClue's own sources. By default that is your public IP address, looked up through BeaconDB by the geoclue daemon, plus nearby Wi-Fi networks where scanning is available. sunsetr sends nothing anywhere. It reads what the daemon reports, and an IP lookup is accurate to about 25 km, which is all the sun times need. The last fix is written to the state directory at two decimals, about a kilometer, and the debug output prints the reported coordinates, so the warning below applies to both.

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
latitude = 40.71
longitude = -74.01
```

This separation allows you to share your sunsetr configuration publicly without accidentally doxxing yourself. `geo.toml` can also serve as a temporary place to store your coordinates when travelling.

⭐ **Note**: Your debug output will still print your coordinates on startup for debugging purposes, so be extremely careful when sharing your debug output online.

```
┣ Loaded default configuration
┃   Loaded coordinates from geo.toml
┃   Backend: Auto (Wayland)
┃   Mode: Time-based (geo)
┃   Location: 41.85°N, 87.65°W <--- ⭐ Careful!
┃   Night: 3300K @ 90% gamma
┃   Day: 6500K @ 100% gamma
┃   Update interval: 60 seconds
```

## Next Steps

- **[Explore configuration options](../configuration/)**: Customize temperature and gamma values
- **[Create presets](/presets/)**: Set up location-based presets for travel
- **[Learn about commands](/commands/)**: See all available CLI commands
