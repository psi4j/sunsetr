#!/bin/bash
#
# Systemd sleep hook for sunsetr
# Install to: /usr/lib/systemd/system-sleep/sunsetr-resume
#
# This script is automatically called by systemd-sleep(8) with two arguments:
# $1: "pre" (before sleep) or "post" (after resume)  
# $2: Sleep type (suspend, hibernate, hybrid-sleep, suspend-then-hibernate)
#
# This hook works regardless of how sunsetr was started (systemd service,
# manual execution, etc.) by checking for any running sunsetr processes.

case "$1" in
    post)
        # System is resuming from sleep
        # Give the system a moment to stabilize after resume
        sleep 0.5
        
        # Find all sunsetr processes and send SIGUSR2 to each
        # This triggers immediate state recalculation
        pids=$(pgrep -x sunsetr 2>/dev/null)
        
        if [ -n "$pids" ]; then
            for pid in $pids; do
                # Verify the process still exists before sending signal
                if [ -d "/proc/$pid" ]; then
                    kill -USR2 "$pid" 2>/dev/null && \
                        logger -t sunsetr-resume "Sent SIGUSR2 to sunsetr (PID $pid) after resume from $2"
                fi
            done
        else
            # No sunsetr processes found - this is normal if sunsetr isn't running
            logger -t sunsetr-resume "No sunsetr processes found after resume from $2"
        fi
        ;;
    pre)
        # System is going to sleep - nothing to do
        logger -t sunsetr-resume "System entering $2"
        ;;
esac

exit 0
