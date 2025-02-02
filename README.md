# sunsetr
Automatic color temperature controller for hyprsunset.

![This image was taken using a shader to simulate the effect of hyprsunset](sunsetr.png)

# Use
```hyprlang
# in hyprland.conf
exec-once = sunsetr &
# rest of config
```

You can also set it to refresh each time you save hyprland.conf to quickly test different temperature settings:
```hyprlang
# in hyprland.conf
exec = pkill sunsetr || sunsetr &
# rest of config
```
Alternatively, you can place and use the Systemd service and enable it:
```
systemctl --user enable --now sunsetr.service
```

# Dependencies
This controller has only been tested on these versions of Hyprland and hyprsunset:
- hyprland = "0.47.1"
- hyprsunset = "v0.1.0"

# Thanks
Thanks to Vaxry and the Hyprwm team for making the best Wayland experience possible.
