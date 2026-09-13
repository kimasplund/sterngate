#!/usr/bin/env bash
set -euo pipefail

INTERFACE="${1:-vcan0}"

echo "[*] Setting up virtual CAN interface: ${INTERFACE}"

# Load the vcan kernel module
if ! lsmod | grep -q "^vcan "; then
    echo "[*] Loading kernel module 'vcan'..."
    sudo modprobe vcan
fi

# Check if interface already exists
if ip link show "${INTERFACE}" &>/dev/null; then
    echo "[!] Interface ${INTERFACE} already exists. Bringing it UP..."
    sudo ip link set up "${INTERFACE}"
else
    echo "[*] Creating ${INTERFACE}..."
    sudo ip link add dev "${INTERFACE}" type vcan
    sudo ip link set up "${INTERFACE}"
fi

echo "[✓] Interface ${INTERFACE} is active and ready."
ip -details link show "${INTERFACE}"

