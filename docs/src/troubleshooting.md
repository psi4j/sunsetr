# Troubleshooting

This guide covers common issues and their solutions. If you encounter a problem not listed here, please [open an issue](https://github.com/psi4j/sunsetr/issues) on GitHub.

## Sunsetr won't start hyprsunset

- Ensure hyprsunset is installed and accessible if you're attempting to use sunsetr as a controller
- Make sure hyprsunset is not already running
- Be sure you're running on Hyprland
- Try using the Hyprland backend instead and consider removing hyprsunset as a dependancy.

## Smooth transitions aren't smooth

- Ensure `smoothing = true` in config
- Try different `startup_duration` and `shutdown_duration` settings for smoother transitions
- Adjust `adaptive_interval` for extended durations
- Check that no other color temperature tools are running

## Display doesn't change

- If using the Hyprsunset backend, verify hyprsunset works independently: `hyprctl hyprsunset temperature 4000`
- Check configuration file syntax
- Look for error messages in terminal output, follow their recommendations
- Use `"wayland"` as your backend (even on Hyprland)

---

## Still Having Issues?

If none of these solutions work:

1. **Search existing issues**: [GitHub Issues](https://github.com/psi4j/sunsetr/issues)
2. **Open a new issue**: Include debug output and system information

When reporting, please include:

- Sunsetr version
- Operating system and version
- Compositor and version
- Configuration file (redacted if needed)
- Debug output
- Steps to reproduce
