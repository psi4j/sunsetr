# sunsetr

Automatic color temperature controller for hyprsunset.

![This image was taken using a shader to simulate the effect of hyprsunset](sunsetr.png)

# Use

Add this line to your `hyprland.conf`

```bash
exec-once = sunsetr &
```

Alternatively, you can place and use the Systemd service and enable it:

```bash
systemctl --user enable --now sunsetr.service
```

## Testing different temperatures

If you want to test different temperatures before setting your sunset temp in the config, I recommend using hyprsunset directly:

```bash
pkill sunsetr
```

then:

```bash
hyprsunset -t 4000
```

# Config

A default config will be generated on the first run.
`sunsetr.toml` can be found in `~/.config/hypr/sunsetr.toml`

```toml
# Sunsetr configuration
sunset = "19:00:00"   # Time to transition to night mode (HH:MM:SS)
sunrise = "06:00:00"  # Time to transition to day mode (HH:MM:SS)
temp = 4000           # Color temperature after sunset (1000-6500) Kelvin
```

# Installation

## Build from source:

You will need to have Rust version 1.78.0 or greater installed. Clone the repo, cd into sunsetr, then:

```bash
cargo build --release
```

You can find the `sunsetr` binary in the `./target/release` directory and move it to `/usr/local/bin` or where ever you place your custom binaries.

# Dependencies

This controller has only been tested on these versions of Hyprland and hyprsunset:

- Hyprland 0.47.2
- hyprsunset v0.1.0

# TODO

- [] Set up AUR package
- [] Implement gradual transitions
- [] Make Nix installation available

# Thanks

Thanks to Vaxry and the Hyprwm team for making the best Wayland experience possible for the rest of us.
