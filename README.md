# Sunsetr
Automatic color temperature controller for hyprsunset.

![This image was taken using a shader to simulate the effect of hyprsunset](sunsetr.png)

# Use
Add this line to your `hyprland.conf`
```hyprlang
exec-once = sunsetr &
```

You can also set it to refresh each time you save hyprland.conf to quickly test different temperature settings:
```hyprlang
exec = pkill sunsetr || sunsetr &
```

Alternatively, you can place and use the Systemd service and enable it:
```
systemctl --user enable --now sunsetr.service
```

# Config
`sunsetr.toml` can be found in `~/.config/hypr/sunsetr.toml`
```toml
# Sunsetr configuration
sunset = "19:00:00"   # Time to transition to night mode (HH:MM:SS)
sunrise = "06:00:00"  # Time to transition to day mode (HH:MM:SS)
temp = 4000           # Color temperature after sunset (1000-6000) Kelvin
```

# Dependencies
This controller has only been tested on these versions of Hyprland and hyprsunset:
- hyprland = "0.47.1"
- hyprsunset = "v0.1.0"

# Thanks
Thanks to Vaxry and the Hyprwm team for making the best Wayland experience possible.
