#!/bin/bash
# Apply random network throttling to simulate different network conditions
# Uses tc (traffic control) to limit bandwidth

set -e

# Find eth0 or similar real interface (not tunnels, loopback, etc)
# Docker uses names like "eth0@if299" so extract just the eth0 part
INTERFACE=$(ip -o link show | awk -F': ' '{print $2}' | grep -E '^eth[0-9]+' | sed 's/@.*//' | head -1)

if [ -z "$INTERFACE" ]; then
    echo "No eth interface found, skipping throttle"
    exit 0
fi

if [ "$NETWORK_THROTTLE" = "none" ]; then
    echo "Network throttling disabled"
    exit 0
fi

# Generate random bandwidth between 5-100 Mbps
if [ "$NETWORK_THROTTLE" = "random" ]; then
    BANDWIDTH=$((RANDOM % 96 + 5))
elif [[ "$NETWORK_THROTTLE" =~ ^[0-9]+$ ]]; then
    BANDWIDTH=$NETWORK_THROTTLE
else
    echo "Invalid NETWORK_THROTTLE value: $NETWORK_THROTTLE"
    exit 0
fi

# Add random latency between 5-50ms
LATENCY=$((RANDOM % 46 + 5))

# Add random jitter 0-10ms
JITTER=$((RANDOM % 11))

echo "Applying network throttle on $INTERFACE: ${BANDWIDTH}mbit, ${LATENCY}ms latency, ${JITTER}ms jitter"

# Clear existing rules
tc qdisc del dev "$INTERFACE" root 2>/dev/null || true

# Apply bandwidth limit with latency
tc qdisc add dev "$INTERFACE" root handle 1: htb default 12
tc class add dev "$INTERFACE" parent 1: classid 1:12 htb rate "${BANDWIDTH}mbit" ceil "${BANDWIDTH}mbit"
tc qdisc add dev "$INTERFACE" parent 1:12 netem delay "${LATENCY}ms" "${JITTER}ms"

echo "Network throttle applied successfully"
