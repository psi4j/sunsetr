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

The IPC socket broadcasts three types of events:

**1. StateApplied:**

Sent when temperature/gamma values are applied to the display.

**JSON format:**

```json
{
  "active_preset": "default",
  "period": "sunset",
  "state": "transitioning",
  "progress": 0.4637135,
  "current_temp": 5016,
  "current_gamma": 95.36286,
  "target_temp": 3300,
  "target_gamma": 90.0,
  "next_period": "2025-11-11T17:49:25.000679991-06:00"
}
```

**2. PeriodChanged:**

Sent when transitioning between periods (Day ↔ Sunset ↔ Night ↔ Sunrise).

**Available periods:**

- `day` - Stable day period
- `sunset` - Transitioning from day to night
- `night` - Stable night period
- `sunrise` - Transitioning from night to day
- `static` - Static mode (no transitions)

**3. PresetChanged:**

Sent when switching presets.

**Includes:**

- New preset name
- Target temperature and gamma values
- Target Period

## Status Bar Integration

**Waybar Example:**

Add to `~/.config/waybar/config`:

```json
{
  "custom/sunsetr": {
    "exec": "sunsetr status --json --follow | jq --unbuffered --compact-output 'if .event_type == \"preset_changed\" then {text: \"\\(.target_temp)K\", alt: .target_period, tooltip: \"Preset: \\(.to_preset // \"default\")\\nTarget: \\(.target_temp)K @ \\(.target_gamma)%\"} elif .event_type == \"state_applied\" then {text: \"\\(.current_temp)K\", alt: .period, tooltip: \"Period: \\(.period)\\nTemp: \\(.current_temp)K @ \\(.current_gamma)%\"} else empty end'",
    "return-type": "json",
    "format": "{icon} {text}",
    "format-icons": {
      "day": "󰖨",
      "night": "",
      "sunset": "󰖛",
      "sunrise": "󰖜",
      "static": "󰋙"
    },
    "on-click": "sunsetr preset day"
  }
}
```

**Note**: This requires that you have a `day` [preset](../presets/examples.md) set in your presets directory.

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
