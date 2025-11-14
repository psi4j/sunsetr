# Changelog

<!-- toc -->

### v0.11.0

- **Process Management Commands**: New `status`, `stop`, and `restart` commands
  - `status` command displays current runtime state with JSON output support
  - `stop` command cleanly terminates running instances with verification
  - `restart` command recreates backend with clean stop-wait-start sequence
- **Background Operation**: New `--background` flag for daemon-like operation
- **Extended Gamma Range**: Gamma now supports 10-200% (previously 10-100%) for enhanced brightness control
- **IPC Foundation**: Unix socket-based IPC for real-time state broadcasting to external applications
- **Critical Timing Fixes**:
  - Eliminated period transition boundary delays (transitions now occur exactly on time)
  - Fixed time jump handling for NTP sync, sleep/resume, and manual time adjustments
  - Corrected DST boundary handling in status output and transition schedules
  - Fixed geo mode timezone mismatch causing delayed transition updates
- **Reliability Improvements**:
  - Session-aware zombie process detection with automatic recovery after logout/reboot
  - Test command instance isolation to prevent concurrent instance conflicts
  - Multiple preset switching fixes and edge case improvements
- **Geographic Data Improvements**:
  - Added Asia/Kolkata timezone support (Special thanks [@acagastya](https://github.com/acagastya))
  - Fixed country/coordinate data accuracy (Special thanks [@acagastya](https://github.com/acagastya))
- **Breaking Changes**:
  - `reload` command deprecated and removed (use `restart` or rely on automatic hot reloading)

### v0.10.0

- **Configuration Management Commands**: New `get` and `set` commands for CLI-based config management
  - `get` command reads configuration values with JSON output support
  - `set` command modifies configuration fields with validation and safety features
- **Enhanced Preset System**: Improved preset command with subcommands
  - `preset active` shows the currently active preset
  - `preset list` displays all available presets
- **Native Hyprland CTM Backend**: Experimental Color Transformation Matrix support for Hyprland
- **Comprehensive Help System**: Built-in help command with detailed documentation for all features
- **XDG Base Directory Support**: Migrated state management to follow XDG specifications
- **Improved Error Handling**: Consistent error severity levels and better user guidance
- **Interactive Configuration Warnings**: Safer configuration editing with preset warnings
- **Enhanced Logger**: Colored severity levels and cleaner output formatting
- **Bug Fixes**:
  - Fixed config directory handling with `--config` flag
  - Resolved smooth transition issues during reload for Hyprland

### v0.9.0

- **Static Mode**: New transition mode for maintaining constant temperature/gamma values
- **Preset Management System**: Quick switching between configuration profiles with `sunsetr preset`
- **Hot Configuration Reloading**: Automatic detection and application of config file changes
- **Custom Config Directories**: Support for portable configurations with `--config` flag
- **Enhanced Smooth Transitions**: Configurable startup/shutdown durations with adaptive algorithm
- **Improved D-Bus Handling**: Better recovery from system sleep/resume cycles
- **Configuration Refactoring**: Modular config system with better organization and validation
- **CLI Architecture Improvements**: Subcommand-based CLI with backward compatibility

### v0.8.0

- **D-Bus Sleep/Resume Detection**: Automatically resumes from sleep using systemd-recommended D-Bus approach
- **No Root Scripts**: Sleep/resume detection now runs entirely in user space via D-Bus
- **Nix Flake Support**: Added official flake.nix with reproducible builds and development shell (Special thanks [@scottmckendry](https://github.com/scottmckendry))
- **Display Hotplug Detection**: Automatically detects and handles monitor connection/disconnection (Special thanks [@scottmckendry](https://github.com/scottmckendry))

### v0.7.0

- **Runtime Simulations**: New `--simulate` command for testing transitions and geo calculations
- **NixOS/Nix Support**: Now available in nixpkgs unstable repository (Special thanks [@DoctorDalek1963](https://github.com/DoctorDalek1963))
- **Enhanced Logging System**: Zero-cost abstraction via macros, improved performance and cleaner output formatting
- **Progress Bar Improvements**: Extracted reusable progress bar component with new EMA smoothing
- **Geo Module Refactoring**: Improved transition time calculations, fixed nanosecond precision timing bugs

### v0.6.0

- **Privacy-Focused Geo Configuration**: New optional `geo.toml` file for privately storing coordinates separately from main config
- **Smoother Startup Transitions**: New Bézier curve for startup transitions and new minimum of 1 second `startup_transition_duration`

### v0.5.0

- **Geographic Location Support**: Complete implementation of location-based sunrise/sunset calculations
- **Interactive City Selection**: Fuzzy search interface with 10,000+ cities worldwide (`sunsetr geo`)
- **Automatic Location Detection**: Smart timezone-based coordinate detection with 466 timezone mappings
- **Enhanced Transitions**: Fine-tuned sun elevation angles and Bézier curves for more natural transitions
- **Extreme Latitude Handling**: Robust polar region support with seasonal awareness
- **Comprehensive Timezone System**: Multiple detection methods with intelligent fallbacks
- **Geographic Debug Mode**: Detailed solar calculation information for location verification
- **Timezone Precision**: Automatic timezone determination from coordinates for accurate times
- **Default Geo Mode**: New installations use geographic mode by default for optimal experience
- **Live Reload Command**: New `reload` flag to reload configuration without restarting
- **Interactive Testing**: New `test` command for trying different temperature/gamma values
- **Signal-Based Architecture**: Improved process communication for reload and test commands

---

## Version History

For the complete version history with all minor releases and patches, see the [GitHub Releases](https://github.com/psi4j/sunsetr/releases) page.
