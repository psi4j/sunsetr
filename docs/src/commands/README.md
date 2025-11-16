# Command Reference

<!-- toc -->

This section provides a complete reference for all sunsetr commands and global flags.

## Command Cheat Sheet

| Command                       | Purpose                 | Example                             |
| ----------------------------- | ----------------------- | ----------------------------------- |
| `sunsetr`                     | Start sunsetr           | `sunsetr`                           |
| `sunsetr --background`        | Start in background     | `sunsetr --background`              |
| `sunsetr --debug`             | Start with debug output | `sunsetr --debug`                   |
| `sunsetr test <TEMP> <GAMMA>` | Test temperature/gamma  | `sunsetr test 3300 90`              |
| `sunsetr geo`                 | Select city             | `sunsetr geo`                       |
| `sunsetr preset <NAME>`       | Switch preset           | `sunsetr preset day`                |
| `sunsetr preset active`       | Show active preset      | `sunsetr preset active`             |
| `sunsetr preset list`         | List presets            | `sunsetr preset list`               |
| `sunsetr status`              | Show current state      | `sunsetr status`                    |
| `sunsetr status --json`       | JSON output             | `sunsetr status --json`             |
| `sunsetr status --follow`     | Stream updates          | `sunsetr status --follow`           |
| `sunsetr get <FIELD>`         | Read config value       | `sunsetr get night_temp`            |
| `sunsetr set <FIELD>=<VALUE>` | Write config value      | `sunsetr set night_temp=3500`       |
| `sunsetr restart`             | Restart sunsetr         | `sunsetr restart --instant`         |
| `sunsetr stop`                | Stop sunsetr            | `sunsetr stop`                      |
| `sunsetr --simulate ...`      | Simulate time window    | `sunsetr --simulate "..." "..." 60` |

## Built-in Help

Sunsetr includes comprehensive built-in help:

```bash
# General help
sunsetr help            # Show all available commands
sunsetr --help          # Show detailed usage information

# Command-specific help
sunsetr help <COMMAND>
```

## Commands

- **[test](test.md)** - Test color temperature and gamma values temporarily
- **[geo](geo.md)** - Configure geographic location interactively
- **[preset](preset.md)** - Switch between configuration presets
- **[status](status.md)** - Monitor current runtime state
- **[get & set](get-set.md)** - Read and modify configuration values
- **[restart & stop](restart-stop.md)** - Process management commands
- **[Global Flags](global-flags.md)** - Flags available on main command

## Next Steps

- **[Explore advanced features](../advanced/)** - IPC integration, simulation mode details
- **[Configure settings](../configuration/)** - Fine-tune your configuration
- **[Create presets](../presets/)** - Set up different profiles
