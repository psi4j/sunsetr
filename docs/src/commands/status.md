# status

Monitor the current runtime state of sunsetr via IPC.

## Usage

```bash
sunsetr status
sunsetr status --json
sunsetr status --follow
sunsetr status --json --follow
```

## Flags

- `--json, -j`: Output in JSON format for scripting
- `--follow, -f`: Stream real-time state changes continuously

## One-Shot Mode (Default)

Displays current state once and exits:

```bash
sunsetr status
```

Output:

```
 Active preset: default
Current period: Sunset 󰖛 (32.19%)
         State: transitioning
   Temperature: 5470K → 3300K
         Gamma: 96.8% → 90.0%
   Next period: 17:49:25 (in 31m)
```

## Follow Mode

Stream real-time state changes:

```bash
sunsetr status --follow
```

Events displayed:

- **StateApplied**: Temperature/gamma updates, with the active [`adjust`](adjust.md) offset in brackets when one is set
- **PeriodChanged**: Period transitions (Day → Sunset → Night → Sunrise)
- **PresetChanged**: Preset switching
- **ConfigChanged**: Target values after a config change
- **AdjustmentChanged**: A new adjustment and the values it targets

## JSON Output

Machine-readable output for scripting:

```bash
sunsetr status --json
```

```json
{
  "active_preset": "default",
  "period": "sunset",
  "state": "transitioning",
  "progress": 0.4637135,
  "current_temp": 5016,
  "current_gamma": 95.36286,
  "adjustment": { "temp": { "offset": 0 }, "gamma": { "offset": 0.0 } },
  "target_temp": 3300,
  "target_gamma": 90.0,
  "next_period": "2025-11-11T17:49:25.000679991-06:00",
  "location_source": "manual",
  "latitude": 41.85,
  "longitude": -87.65
}
```

The location fields appear in geo mode only and carry the coordinates in use, rounded to two decimals. Under `location_source = "geoclue"` that is the last fix GeoClue delivered, so `status` reports the live location while `get latitude` reports the configured one. Three more fields describe it. `fixed_at` is when the fix arrived, `cached_fix` is present and `true` when the fix was read back from the state directory at startup, and `geoclue_agent` says whether the authorization agent was found, once that is known. The text output marks the line with "(GeoClue)" and adds the fix age for a cached fix.

## Use Cases

- **Verify sunsetr is running** correctly
- **Monitor transition progress** and timing
- **Status bar integration** (waybar, quickshell, etc.)
- **Automating changes** in your UI or other applications

## IPC Socket

The IPC can be used directly with a custom client or you can use the status command in follow and json mode (`sunsetr status -f -j`) using something like `jq`. The IPC is located at:

```bash
$XDG_RUNTIME_DIR/sunsetr-events.sock
# Typically: /run/user/1000/sunsetr-events.sock
```

## Notes

- Requires sunsetr to be running
- See [IPC Integration](../advanced/ipc.md) for advanced usage
