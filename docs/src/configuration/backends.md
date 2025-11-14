# Backend Selection

<!-- toc -->

Sunsetr supports multiple backends for different compositors. The backend determines how color temperature is applied to your display.

## Available Backends

### **`auto` (Recommended)**

```toml
backend = "auto"
```

Automatically detects your compositor and selects the best backend:

1. **Hyprland detected** → Uses native Hyprland CTM backend
2. **Other Wayland compositor** → Uses generic Wayland backend
3. **Detection fails** → Returns error with suggestions

**Recommendation**: Use `auto` unless you have a specific reason to override. This ensures optimal backend selection and makes your config portable across different compositors.

### **`hyprland` (Hyprland CTM Manager)**

```toml
backend = "hyprland"
```

Uses Hyprland's native Color Transformation Matrix protocol (`hyprland-ctm-control-v1`).

**Pros:**

- Most efficient for Hyprland
- Syncs CTM animations to display's refresh rate
- No external dependencies
- Direct protocol communication

**Cons:**

- Only works on Hyprland
- Hyprland's built-in CTM animations are not adjustable like sunsetr's [smoothing](smoothing.md) using the `wayland` backend

**Notes**:

- Hyprland's CTM animations override sunsetr's [smooth transitions](smoothing.md). To use sunsetr's smooth transitions, use `backend = "wayland"` instead
- To disable Hyprland's CTM animations for instant temperature and gamma updates, set this in `hyprland.conf`:
  ```bash
  render {
      ctm_animation = 0
  }
  ```

### **`hyprsunset` (Hypsunset Controller)**

```toml
backend = "hyprsunset"
```

Controls color temperature through Hyprland's `hyprsunset` CTM manager.

**Pros:**

- Works as Hyprland's team intends it to
- May integrate better with their other tools and ecosystem

**Cons:**

- Requires an additional dependency
- Less efficient than using the native CTM backend
- Process management overhead

### **`wayland` (WLR Gamma Control)**

```toml
backend = "wayland"
```

Uses the standard Wayland `wlr-gamma-control-unstable-v1` protocol.

**Pros:**

- Works on any Wayland compositor supporting the protocol
- Smooth transitions fully supported
- No external dependencies

**Cons:**

- Does not sync to dispay's refresh rate

**Supported compositors**: Hyprland, Niri, Sway, River, Wayfire, and most Wayland compositors.

## Backend Selection Guide

| Use Case                           | Recommended Backend      |
| ---------------------------------- | ------------------------ |
| Hyprland with smooth transitions   | `wayland`                |
| Hyprland with CTM animations       | `auto` (uses native CTM) |
| Niri, Sway, River, other Wayland   | `auto` (uses wayland)    |
| Force Hyprland CTM for preset      | `hyprland`               |
| Force WLR gamma control for preset | `wayland`                |
| Integrate with Hyprland ecosystem  | `hyprsunset`             |
| Portable config across compositors | `auto`                   |
