#!/bin/bash
# Mock iw command for simulating WiFi in containers

# Generate some randomness per container
RANDOM_SEED=$(hostname | md5sum | cut -c1-8)
SIGNAL=$((-(40 + (0x$RANDOM_SEED % 40))))  # -40 to -80 dBm
FREQ=$((5180 + (0x$RANDOM_SEED % 8) * 20))  # 5180-5320 MHz
MCS=$((0x$RANDOM_SEED % 9))  # 0-8
NSS=$((1 + (0x$RANDOM_SEED % 2)))  # 1-2

if [[ "$1" == "dev" && "$3" == "link" ]]; then
    cat << EOF
Connected to 00:11:22:33:44:55 (on wlan0)
	SSID: SimulatedOffice-5G
	freq: $FREQ
	signal: $SIGNAL dBm
	rx bitrate: 866.7 MBit/s VHT-MCS $MCS VHT-NSS $NSS 80MHz
	tx bitrate: 866.7 MBit/s VHT-MCS $MCS VHT-NSS $NSS 80MHz
EOF
elif [[ "$1" == "dev" && -z "$2" ]]; then
    cat << EOF
phy#0
	Interface wlan0
		ifindex 3
		wdev 0x1
		addr 02:00:00:00:00:01
		type managed
		channel 36 (5180 MHz), width: 80 MHz, center1: 5210 MHz
EOF
else
    echo "Usage: iw dev [interface] link"
    exit 1
fi
