# adjust

Adjust the running instance's temperature or gamma for now, without changing your configuration.

## Usage

```bash
sunsetr adjust [--static] <field>[+|-]=<value> [...]
sunsetr adjust [--static] <field> <[+|-]value> [...]
sunsetr adjust --reset|-r
```

## Fields

- `temp`, `current_temp`: Temperature in Kelvin
- `gamma`, `current_gamma`: Gamma in percent

## Operators

- `<field>+=<value>`: Move the field up by value
- `<field>-=<value>`: Move the field down by value
- `<field>=<value>`: Hold the field at value (needs `--static`)

The same three requests can be written as a field name followed by a separate value, where a signed value moves the field and an unsigned value holds it. `temp -100` is `temp-=100`, `temp +100` is `temp+=100`, and `temp 3300` is `temp=3300`. Pick whichever spelling matches your habits or your other keybindings.

Repeating a field on one command line folds into one change.

## Options

- `--static`, `-s`: Stop the field from following the schedule
- `--reset`, `-r`: Clear every adjustment

## Examples

```bash
# Dim by 5 percent, either spelling
sunsetr adjust gamma-=5
sunsetr adjust gamma -5

# Warmer and dimmer in one step
sunsetr adjust temp-=300 gamma-=5

# Hold 3500K until reset, then nudge the held value
sunsetr adjust --static temp=3500
sunsetr adjust --static temp-=100

# Short forms
sunsetr a gamma+=5
sunsetr a -s temp=3500
sunsetr a -r

# Back to the scheduled values
sunsetr adjust --reset
```

## Behavior

Without `--static` the amount is an offset on top of whatever the schedule computes:

```
applied = scheduled + offset
```

- **The applied value moves by exactly the amount you ask for**, at any point in a transition. The gap between your day and night values stays the same, so the transition keeps its speed and cannot invert.
- **Nothing is written** to `sunsetr.toml` or any preset.
- **Clamped** to the supported range (1000-20000K, 10-200%).
- **Kept** through period changes and config reloads.
- **Cleared** by `--reset`, a preset switch, or a restart.
- **Requires a running instance**, and refuses while [`test`](test.md) mode is active.
- **Each call sends its request** and the instance resolves it against the adjustment it holds, so calls that overlap all count.

With `--static` the result is held and the schedule no longer moves that field:

```
applied = held
```

- `<field>=<value>` holds the value given.
- `<field>+=<value>` and `<field>-=<value>` hold the value the display shows now, moved by the amount given.
- **A hold is sticky.** Later `+=` and `-=` move the held value whether or not `--static` is given again. Only `--reset`, a preset switch, or a restart returns the field to the schedule.
- Each field is held or offset on its own, so `sunsetr adjust --static temp=3500 gamma-=5` holds the temperature and offsets the gamma.

Compare [`set current_gamma+=5`](get-set.md#using-the-set-command), which edits the active period's config value and persists.

## Keybindings

Hyprland (`hyprland.conf`):

```bash
bind = $mod SHIFT, up, exec, sunsetr adjust gamma+=5
bind = $mod SHIFT, down, exec, sunsetr adjust gamma-=5
bind = $mod SHIFT, r, exec, sunsetr adjust --reset
```

Hyprland (`hyprland.lua`):

```lua
hl.bind("SUPER + SHIFT + up", hl.dsp.exec_cmd("sunsetr adjust gamma+=5"))
hl.bind("SUPER + SHIFT + down", hl.dsp.exec_cmd("sunsetr adjust gamma-=5"))
hl.bind("SUPER + SHIFT + r", hl.dsp.exec_cmd("sunsetr adjust --reset"))
```

Niri (`config.kdl`):

```bash
Mod+Shift+Up { spawn "sh" "-c" "sunsetr adjust gamma+=5"; }
Mod+Shift+Down { spawn "sh" "-c" "sunsetr adjust gamma-=5"; }
Mod+Shift+R { spawn "sh" "-c" "sunsetr adjust --reset"; }
```

## Status and IPC

`sunsetr status` shows an `Adjustment:` line while an adjustment is active, and `status --follow` prints an `ADJUST:` line on each change and appends the adjustment in brackets to every state line, as in `6200K @ 99.0% [-300K, -1.0%]`. A held field prints with a leading `=`, as in `[=3500K, -1.0%]`. `status --json` and every `state_applied` event carry an `adjustment` object, and each change emits an [`adjustment_changed`](../advanced/ipc.md#event-types) event.
