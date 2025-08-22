#!/bin/bash
#
# Installation script for sunsetr systemd sleep hook
# This hook ensures sunsetr immediately updates after system resume

set -e

HOOK_SOURCE="system-sleep/sunsetr-resume.sh"

# Detect the correct systemd sleep directory
if [ -d "/usr/lib/systemd/system-sleep" ]; then
    HOOK_DEST="/usr/lib/systemd/system-sleep/sunsetr-resume"
elif [ -d "/lib/systemd/system-sleep" ]; then
    HOOK_DEST="/lib/systemd/system-sleep/sunsetr-resume"
else
    echo "Error: Could not find systemd system-sleep directory" >&2
    echo "Checked: /usr/lib/systemd/system-sleep and /lib/systemd/system-sleep" >&2
    exit 1
fi

# Check if running as root
if [ "$EUID" -ne 0 ]; then 
    echo "This script must be run as root (use sudo)" >&2
    exit 1
fi

# Check if source file exists
if [ ! -f "$HOOK_SOURCE" ]; then
    echo "Error: Hook script not found at $HOOK_SOURCE" >&2
    echo "Make sure you're running this from the sunsetr project root" >&2
    exit 1
fi

# Create destination directory if it doesn't exist (shouldn't be needed, but just in case)
mkdir -p "$(dirname "$HOOK_DEST")"

# Install the hook
echo "Installing systemd sleep hook to: $HOOK_DEST"
cp "$HOOK_SOURCE" "$HOOK_DEST"
chmod +x "$HOOK_DEST"

echo "✓ Sleep hook installed successfully"
echo ""
echo "The hook will:"
echo "  • Detect when your system resumes from suspend/hibernate"
echo "  • Send a signal to sunsetr to immediately update the display color"
echo "  • Work regardless of how sunsetr was started"
echo ""
echo "To test: Suspend your system and resume - sunsetr should update immediately"
echo "To check logs: journalctl -t sunsetr-resume"
echo "To uninstall: sudo rm $HOOK_DEST"