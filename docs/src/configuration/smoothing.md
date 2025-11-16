# Smooth Transitions

<!-- toc -->

Smooth transitions provide gradual fade effects when sunsetr starts up, [reloads](hot-reloading.md), switches [presets](../presets/), and shuts down.

## Backend Compatibility

⚠️ **Important**: Smooth transitions are only supported on the **Wayland [backend](../configuration/backends.md)**. To use smoothing on Hyprland, set `backend = "wayland"`.

## Configuration

```toml
smoothing = true             # Enable/disable smooth transitions
startup_duration = 0.5       # Seconds (0.1-60, 0 = instant)
shutdown_duration = 0.5      # Seconds (0.1-60, 0 = instant)
adaptive_interval = 1        # Base interval in milliseconds (1-1000)
```

## How Smoothing Works

1. `startup_duration` determines the duration of the smoothing animation at startup, for preset switching, and configuration reloading.
2. `shutdown_duration` determines the duration of the smoothing animation at shutdown
3. `adaptive_interval` controls the minimum granularity of the update interval that affects the perceived smoothness of the animation

## Duration Settings

The duration determines how long transitions take:

```toml
# Fast (default)
startup_duration = 0.5       # Half second
shutdown_duration = 0.5

# Moderate
startup_duration = 5.0       # 5 seconds
shutdown_duration = 5.0
```

## Adaptive Interval

The adaptive interval controls the base update interval during transitions:

```toml
adaptive_interval = 1
```

The adaptive interval uses an algorithm designed to adapt to your particular machine's capabilities. The default `1ms` maximizes the granularity of the update interval automatically, allowing for the smoothest possible subsecond animations from current to target values. The current `wlr-gamma-control-unstable-v1` protocol used by the Wayland [backend](../configuration/backends.md) relies on each compositor's implementation for gamma control updates. Each compositor will have their own performance characteristics for each type of CPU/GPU for this protocol.

Currently, `niri` and `Hyprland` handle the default settings quite well when used with `Intel` CPUs, and `NVIDIA` and `AMD` GPUs are noticeably less smooth. The performance characteristics of the smooth transitions are a result of the interaction between the compositor, the Linux kernel, and the GPU. Refining this further is out of the scope of this application, therefore, I've opened up the `adaptive_interval` as a configuration point to the user in case they'd like to attempt to refine things further to their taste.

**When to adjust:**

If you find that your mouse is lagging when the smoothing animation is occurring, you could try adjusting the base update interval a bit higher to reduce the granularity of the updates, but you will have to accompany this with a longer `startup_duration` and `shutdown_duration` if you want this to be a bit smoother. It's important to note that it is not necessarily the number or frequency of the updates causing the lag, but rather the way the compositor has to batch updates for rendering when sent to the kernel to then be processed by the GPU. Testing the smooth transitions on an old Intel CPU shows how smoothing works quite well when the process is streamlined between the compositor, kernel, and processor.

You may find success in further smoothing the animation with your compositor and processor by using settings similar to these:

```toml
#[Smoothing]
smoothing = true         # Enable smooth transitions during startup and exit
startup_duration = 5     # Duration of smooth startup in seconds (0.1-60 | 0 = instant)
shutdown_duration = 5    # Duration of smooth shutdown in seconds (0.1-60 | 0 = instant)
adaptive_interval = 144  # Adaptive interval base for smooth transitions (1-1000)ms
```

## **Instant updates**

If the smoothing animations don't meet your expectations or you want to decrease startup time, you can always disable smoothing to achieve the same results as something like `wlsunset` by setting:

```toml
smoothing = false
```

Correspondingly, if you want to disable Hyprland's native CTM animations, you can set this in `hyprland.conf`:

```bash
render {
    ctm_animation = 0
}
```
