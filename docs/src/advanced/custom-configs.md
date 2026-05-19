# Custom Config Directories

Use custom configuration directories for portable setups, testing, or multiple profiles.

## Basic Usage

```bash
sunsetr --config ~/dotfiles/sunsetr/
```

## Directory Structure

The custom directory must maintain the same structure:

```
~/dotfiles/sunsetr/
├── sunsetr.toml         # Main configuration
├── geo.toml             # Optional: geographic coordinates
└── presets/             # Optional: presets
    ├── day/
    │   └── sunsetr.toml
    └── gaming/
        └── sunsetr.toml
```

## Commands and the Custom Directory

Once started with `--config`, all subsequent commands use the custom directory:

```bash
# Start with custom config
sunsetr --config ~/dotfiles/sunsetr/

# All commands use custom directory
sunsetr preset gaming
sunsetr geo
sunsetr set night_temp=3500
sunsetr status
```

**Note:** You don't need to specify `--config` for subsequent commands during the same session.

You can also pass `--config <dir>` explicitly to any command that reads or writes config files (`run`, `restart`, `preset`, `get`, `set`, `geo`) to act on that directory without relying on a running instance. It also works with `--simulate`. The flag may appear before or after the command name.

`stop`, `test`, and `status` act on the already-running instance over the lock file and IPC. They do not take a configuration directory: `stop` and `status` use whichever directory the running instance was started with, and `test` is a transient command that restores the previous state when it exits. Passing `--config` to any of them has no effect and is ignored.

## Use Cases

**Portable Configuration:**

```bash
# Keep config with dotfiles
~/dotfiles/
└── sunsetr/
    ├── sunsetr.toml
    └── presets/
```

```bash
# Use from dotfiles
sunsetr --config ~/dotfiles/sunsetr/
```

**Testing Configuration:**

```bash
# Create test config
mkdir -p ~/test-sunsetr
cp ~/.config/sunsetr/sunsetr.toml ~/test-sunsetr/

# Test without affecting main config
sunsetr --config ~/test-sunsetr/
```

**Multiple Profiles:**

```bash
# Work profile
sunsetr --config ~/configs/sunsetr-work/

# Home profile
sunsetr --config ~/configs/sunsetr-home/

# Travel profile
sunsetr --config ~/configs/sunsetr-travel/
```

**Simulation with Custom Config:**

```bash
sunsetr --config ~/test-config/ \
    --simulate "2025-01-15 18:00:00" "2025-01-16 08:00:00" 60
```

## Configuration Precedence

When using custom directories:

1. **Custom directory files** take precedence
2. **Default directory** is never read
3. **All operations** affect custom directory only

## Lock Files and IPC

Custom directories create separate lock files and IPC sockets:

```bash
# Default instance
$XDG_RUNTIME_DIR/sunsetr/sunsetr.lock
$XDG_RUNTIME_DIR/sunsetr/ipc.sock

# Custom instance (hashed path)
$XDG_RUNTIME_DIR/sunsetr/sunsetr-<hash>.lock
$XDG_RUNTIME_DIR/sunsetr/ipc-<hash>.sock
```

This allows **multiple instances** with different config directories to run simultaneously.
