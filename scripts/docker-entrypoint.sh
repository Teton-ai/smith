#!/bin/bash
set -e

# Remove any stale pid files if they exist
rm -f /run/dbus/pid

# Start dbus-daemon
dbus-daemon --system --nofork &
DBUS_PID=$!

# Keep container running and handle signals
trap "kill $DBUS_PID; exit 0" SIGTERM SIGINT
tail -f /dev/null & wait
