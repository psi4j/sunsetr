# adjust

Offset the running instance's temperature or gamma for now, without changing your configuration.

## Usage

```bash
sunsetr adjust <temp|gamma> <+N|-N> [...]
sunsetr adjust --reset|-r
```

## Arguments

- `temp <+N|-N>`: Add N Kelvin to the current offset
- `gamma <+N|-N>`: Add N percent to the current offset
- `--reset`, `-r`: Clear the offset

The sign is required. Repeating a field accumulates.

## Examples

```bash
# Dim by 5 percent
sunsetr adjust gamma -5

# Warmer and dimmer in one step
sunsetr adjust temp -300 gamma -5

# Short forms
sunsetr a gamma +5
sunsetr a -r

# Back to the scheduled values
sunsetr adjust --reset
```

## Behavior

The offset is added on top of whatever the schedule computes:

```
applied = scheduled + offset
```

- **The applied value moves by exactly the amount you ask for**, at any point in a transition. The gap between your day and night values stays the same, so the transition keeps its speed and cannot invert.
- **Nothing is written** to `sunsetr.toml` or any preset.
- **Clamped** to the supported range (1000-20000K, 10-200%).
- **Kept** through period changes and config reloads.
- **Cleared** by `--reset`, a preset switch, or a restart.
- **Requires a running instance**, and refuses while [`test`](test.md) mode is active.
- **Each call sends its delta** and the instance adds it to the offset it holds, so calls that overlap all count.

Compare [`set current_gamma+=5`](get-set.md#using-the-set-command), which edits the active period's config value and persists.

## Keybindings

Hyprland (`hyprland.conf`):

```bash
bind = $mod SHIFT, up, exec, sunsetr adjust gamma +5
bind = $mod SHIFT, down, exec, sunsetr adjust gamma -5
bind = $mod SHIFT, r, exec, sunsetr adjust --reset
```

Hyprland (`hyprland.lua`):

```lua
hl.bind("SUPER + SHIFT + up", hl.dsp.exec_cmd("sunsetr adjust gamma +5"))
hl.bind("SUPER + SHIFT + down", hl.dsp.exec_cmd("sunsetr adjust gamma -5"))
hl.bind("SUPER + SHIFT + r", hl.dsp.exec_cmd("sunsetr adjust --reset"))
```

Niri (`config.kdl`):

```bash
Mod+Shift+Up { spawn "sh" "-c" "sunsetr adjust gamma +5"; }
Mod+Shift+Down { spawn "sh" "-c" "sunsetr adjust gamma -5"; }
Mod+Shift+R { spawn "sh" "-c" "sunsetr adjust --reset"; }
```

## Status and IPC

`sunsetr status` shows an `Adjustment:` line while an offset is active, and `status --follow` prints an `ADJUST:` line on each change and appends the offset in brackets to every state line, as in `6200K @ 99.0% [-300K, -1.0%]`. `status --json` and every `state_applied` event carry `temp_offset` and `gamma_offset`, and each change emits an [`adjustment_changed`](../advanced/ipc.md#event-types) event.
