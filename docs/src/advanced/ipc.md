# IPC Integration

<!-- toc -->

Sunsetr provides a Unix socket-based IPC (Inter-Process Communication) system for real-time state monitoring and external integrations.
The easiest way to interact with IPC is through the [status](../commands/status.md) command.

## IPC Socket Location

The IPC socket is created at:

```bash
$XDG_RUNTIME_DIR/sunsetr-events.sock
```

Typically this resolves to:

```
/run/user/1000/sunsetr-events.sock
```

## Event Types

The IPC socket broadcasts five types of events:

**1. StateApplied:**

Sent when temperature/gamma values are applied to the display.

**JSON format:**

```json
{
  "event_type": "state_applied",
  "active_preset": "default",
  "period": "sunset",
  "state": "transitioning",
  "progress": 0.4637135,
  "current_temp": 5016,
  "current_gamma": 95.36286,
  "adjustment": { "temp": { "offset": 0 }, "gamma": { "offset": 0.0 } },
  "target_temp": 3300,
  "target_gamma": 90.0,
  "next_period": "2025-11-11T17:49:25.000679991-06:00"
}
```

**2. PeriodChanged:**

Sent when transitioning between periods (Day ↔ Sunset ↔ Night ↔ Sunrise).

**JSON format:**

```json
{
  "event_type": "period_changed",
  "from_period": "day",
  "to_period": "sunset"
}
```

**Available periods:**

- `day`: Stable day period
- `sunset`: Transitioning from day to night
- `night`: Stable night period
- `sunrise`: Transitioning from night to day
- `static`: Static mode (no transitions)

**3. PresetChanged:**

Sent when switching presets.

**JSON format:**

```json
{
  "event_type": "preset_changed",
  "from_preset": "default",
  "to_preset": "gaming",
  "target_period": "static",
  "target_temp": 4700,
  "target_gamma": 100.0
}
```

**Includes:**

- Previous and new preset names
- Target temperature and gamma values
- Target period

**4. ConfigChanged:**

Sent when configuration values change, such as from [`sunsetr set`](../commands/get-set.md) commands. It gives immediate feedback about the target values even before a smooth transition completes.

**JSON format:**

```json
{
  "event_type": "config_changed",
  "target_period": "night",
  "target_temp": 3500,
  "target_gamma": 92.5
}
```

**Includes:**

- Target period after the config change
- Target temperature and gamma values

This event enables real-time UI updates (e.g., waybar, status watchers) for config changes made via the `set` command without waiting for smooth transitions to finish.

**5. AdjustmentChanged:**

Sent when the runtime adjustment changes through [`sunsetr adjust`](../commands/adjust.md), including when it is cleared by `--reset` or a preset switch. The adjustment starts at a zero offset, so the first `adjust` is a change from zero. The event is sent before the new values reach the display, and `current_*` are the values the new adjustment produces, so a client that needs to know when the display has caught up should wait for the following `state_applied`. No `config_changed` is sent, since the config is untouched.

**JSON format:**

```json
{
  "event_type": "adjustment_changed",
  "period": "night",
  "adjustment": { "temp": { "offset": -300 }, "gamma": { "hold": 90.0 } },
  "current_temp": 4716,
  "current_gamma": 90.0
}
```

**Includes:**

- Current period, which an adjustment never changes
- The new adjustment
- The values it produces, adjustment included

Each field of `adjustment` is either `{ "offset": N }`, added to the scheduled value, or `{ "hold": N }`, replacing it until reset. Temperature is in Kelvin and gamma in percent. The event carries the same `period`, `adjustment` and `current_*` fields as `state_applied`, so a status bar can render both with one filter and show the new values as soon as the adjustment is made.

The current adjustment is also carried by every `state_applied` event as `adjustment`, so a client that connects later sees it in the initial snapshot. While the smooth transition to a new adjustment is still running, that snapshot already carries the new adjustment and the values it targets. `current_*` always include the adjustment, so for an offset field the scheduled value is `current` minus `offset` unless the sum was clamped at a limit, and for a held field `current` is the held value clamped to the supported range. `target_*` on `state_applied` are the config endpoints of the active transition without the adjustment. `target_*` on `config_changed` and `preset_changed` are the values about to be applied, adjustment included.

## Status Bar Integration

**Waybar Example:**

Add to `~/.config/waybar/config`:

```json
{
  "custom/sunsetr": {
    "exec": "sunsetr status --json --follow | jq --unbuffered --compact-output 'if .event_type == \"preset_changed\" then {text: \"\\(.target_temp)K\", alt: .target_period, tooltip: \"Preset: \\(.to_preset // \"default\")\\nTarget: \\(.target_temp)K @ \\(.target_gamma)%\"} elif .event_type == \"config_changed\" then {text: \"\\(.target_temp)K\", alt: .target_period, tooltip: \"Config changed\\nTarget: \\(.target_temp)K @ \\(.target_gamma)%\"} elif .event_type == \"state_applied\" then {text: \"\\(.current_temp)K\", alt: .period, tooltip: \"Period: \\(.period)\\nTemp: \\(.current_temp)K @ \\(.current_gamma)%\"} else empty end'",
    "return-type": "json",
    "format": "{icon} {text}",
    "format-icons": {
      "day": "󰖨",
      "night": "",
      "sunset": "󰖛",
      "sunrise": "󰖜",
      "static": "󰋙"
    },
    "on-click": "sunsetr preset day",
    "on-scroll-up": "sunsetr set current_temp+=500 current_gamma+=2",
    "on-scroll-down": "sunsetr set current_temp-=500 current_gamma-=2"
  }
}
```

**Note**: This requires that you have a `day` [preset](../presets/examples.md) set in your presets directory. The scroll actions use the `current_temp`/`current_gamma` [virtual aliases](../commands/get-set.md#using-the-set-command) to adjust whichever period is currently active.

**Waybar with a transient adjustment:**

The same module with scroll actions that use [`sunsetr adjust`](../commands/adjust.md), so scrolling never writes to your config. The filter shows the adjustment in the tooltip, as `+300K` for an offset or `=3500K` for a held value. The bar text updates on each `adjustment_changed`, which arrives as soon as the adjustment is made, and again on the `state_applied` that follows once the display has caught up. Both events carry the same fields, so one branch handles them.

```json
{
  "custom/sunsetr": {
    "exec": "sunsetr status --json --follow | jq --unbuffered --compact-output 'def adj(u): if .offset != null then \"\\(if .offset >= 0 then \"+\" else \"\" end)\\(.offset)\\(u)\" else \"=\\(.hold)\\(u)\" end; if .event_type == \"state_applied\" or .event_type == \"adjustment_changed\" then {text: \"\\(.current_temp)K\", alt: .period, tooltip: \"Period: \\(.period)\\nTemp: \\(.current_temp)K @ \\(.current_gamma)%\\nAdjustment: \\(.adjustment.temp | adj(\"K\")) @ \\(.adjustment.gamma | adj(\"%\"))\"} else empty end'",
    "return-type": "json",
    "format": "{icon} {text}",
    "format-icons": {
      "day": "󰖨",
      "night": "",
      "sunset": "󰖛",
      "sunrise": "󰖜",
      "static": "󰋙"
    },
    "on-click": "sunsetr adjust --reset",
    "on-scroll-up": "sunsetr adjust temp+=500 gamma+=2",
    "on-scroll-down": "sunsetr adjust temp-=500 gamma-=2"
  }
}
```

## Custom IPC Clients

You can write custom clients that connect to the IPC socket directly.

### Python Example

```python
import socket
import json

def monitor_sunsetr():
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect("/run/user/1000/sunsetr-events.sock")

    buffer = ""
    try:
        while True:
            data = sock.recv(4096).decode('utf-8')
            buffer += data

            while '\n' in buffer:
                line, buffer = buffer.split('\n', 1)
                if line.strip():
                    event = json.loads(line)
                    event_type = event.get('event_type', 'unknown')

                    if event_type == 'state_applied':
                        period = event.get('period', 'unknown')
                        temp = event.get('current_temp', 0)
                        gamma = event.get('current_gamma', 0)
                        print(f"Period: {period}")
                        print(f"Temp: {temp}K")
                        print(f"Gamma: {gamma}%")
                    elif event_type == 'preset_changed':
                        preset = event.get('to_preset') or 'default'
                        target_period = event.get('target_period', 'unknown')
                        target_temp = event.get('target_temp', 0)
                        target_gamma = event.get('target_gamma', 0)
                        print(f"Preset changed to: {preset}")
                        print(f"Period: {target_period}")
                        print(f"Target: {target_temp}K @ {target_gamma}%")
                    elif event_type == 'config_changed':
                        target_period = event.get('target_period', 'unknown')
                        target_temp = event.get('target_temp', 0)
                        target_gamma = event.get('target_gamma', 0)
                        print(f"Config changed")
                        print(f"Period: {target_period}")
                        print(f"Target: {target_temp}K @ {target_gamma}%")
                    elif event_type == 'period_changed':
                        to_period = event.get('to_period', event.get('period', 'unknown'))
                        from_period = event.get('from_period', 'unknown')
                        print(f"Period: {from_period} → {to_period}")
                    else:
                        # Unknown event type, print full event for debugging
                        print(f"Unknown event: {json.dumps(event, indent=2)}")

                    print("---")
    except KeyboardInterrupt:
        print("\nExiting...")
    finally:
        sock.close()

if __name__ == "__main__":
    monitor_sunsetr()
```

### Bash Example with `socat`

```bash
# Stream IPC events (all events, raw JSON)
socat UNIX-CONNECT:/run/user/1000/sunsetr-events.sock -

# Parse state_applied events with jq
socat UNIX-CONNECT:/run/user/1000/sunsetr-events.sock - | \
    while read -r line; do
        echo "$line" | jq -r 'select(.event_type == "state_applied") | "Period: \(.period) | Temp: \(.current_temp)K"'
    done
```
