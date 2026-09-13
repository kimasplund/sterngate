#!/usr/bin/env bash
set -euo pipefail

SMB_ROOT="smb://kims-nas.local/public"
TARGET_DIR="profiles/imported"

echo "============================================================="
echo "  Sterngate NAS Diagnostic Database Scanner"
echo "  Source: ${SMB_ROOT}"
echo "  Destination: ${TARGET_DIR}"
echo "============================================================="

mkdir -p "${TARGET_DIR}"

if ! command -v gio &> /dev/null; then
    echo "[-] Error: 'gio' utility is required to browse SMB shares."
    exit 1
fi

echo "[*] Querying SMB share..."
gio list "${SMB_ROOT}" || {
    echo "[-] Could not connect to ${SMB_ROOT}. Ensure network connectivity to kims-nas.local."
    exit 1
}

echo ""
echo "[*] Cataloging DTS Projects..."
gio list "${SMB_ROOT}/DTS Projects" 2>/dev/null || echo "    No DTS Projects accessible"

echo ""
echo "[*] Cataloging Flash Calibration Files (SdFlash)..."
gio list "${SMB_ROOT}/SdFlash" 2>/dev/null || echo "    No SdFlash files accessible"

echo ""
echo "[✓] Scan completed. Available archives can be extracted into ${TARGET_DIR} for profile generation."

