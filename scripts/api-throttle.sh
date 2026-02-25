#!/bin/bash
# Apply global bandwidth limit on API container egress
# This creates a shared bandwidth pool for all device containers

set -e

BANDWIDTH="${GLOBAL_BANDWIDTH_LIMIT:-100}"

if [ "$BANDWIDTH" = "0" ] || [ "$BANDWIDTH" = "none" ]; then
    echo "[api-throttle] Global bandwidth limit disabled"
    exec "$@"
fi

# Find ALL eth interfaces and apply throttle to each
INTERFACES=$(ip -o link show | awk -F': ' '{print $2}' | grep -E '^eth[0-9]+' | sed 's/@.*//')

if [ -z "$INTERFACES" ]; then
    echo "[api-throttle] No eth interfaces found, skipping global throttle"
    exec "$@"
fi

for INTERFACE in $INTERFACES; do
    echo "[api-throttle] Applying ${BANDWIDTH}mbit limit on $INTERFACE"

    # Clear existing rules
    tc qdisc del dev "$INTERFACE" root 2>/dev/null || true

    # Apply bandwidth limit - all egress traffic shares this pool
    tc qdisc add dev "$INTERFACE" root handle 1: htb default 12
    tc class add dev "$INTERFACE" parent 1: classid 1:12 htb rate "${BANDWIDTH}mbit" ceil "${BANDWIDTH}mbit"
done

echo "[api-throttle] Global throttle applied to all interfaces"

# Execute the original command
exec "$@"
