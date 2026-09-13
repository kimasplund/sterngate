#!/usr/bin/env bash
set -euo pipefail

PORT="${1:-8080}"

echo "============================================================="
echo "  Starting Sterngate in Mercedes W211 Mock Simulation Mode"
echo "  URL: http://localhost:${PORT}"
echo "============================================================="

cargo run --package sterngate-cli -- mock --port "${PORT}"

