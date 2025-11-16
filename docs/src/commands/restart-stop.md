# restart & stop

<!-- toc -->

Process management commands for controlling the running sunsetr instance.

## Using the `restart` command

Restart sunsetr with clean backend re-initialization.

**Usage:**

```bash
sunsetr restart
sunsetr restart --instant
sunsetr restart --background
```

**Flags:**

- `--instant`: Skip smooth transitions for immediate restart
- `--background`: Restart in background mode

**Examples:**

```bash
# Normal restart with smooth transitions
sunsetr restart

# Skip smooth transitions for faster restarts
sunsetr restart --instant

# Restart in background mode
sunsetr restart --background
```

**Behavior:**

The restart command performs a clean stop-wait-start sequence:

1. **Stops current instance** gracefully
2. **Waits for shutdown** to complete
3. **Starts new instance** with fresh backend initialization
4. **Applies smooth transitions** (unless you run with `--instant`)

**When to Use:**

- **DPMS recovery**: After manual display sleep/wake cycles on Hyprland
- **Backend issues**: If temperature/gamma stop working

## Using the `stop` command

Gracefully shutdown the running sunsetr instance.

**Usage:**

```bash
sunsetr stop
```

**Behavior:**

- **Graceful shutdown**: Applies smooth shutdown transitions if configured
- **Cleanup**: Removes lock files and IPC socket
- **Restoration**: Returns display to configured day values
- **Verification**: Confirms process has terminated
