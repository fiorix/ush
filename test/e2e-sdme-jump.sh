#!/bin/bash
# End-to-end jump-host test for ush over SSH to hardened SDME containers.
# Runs inside an outer SDME container so the hardened targets are nested.
# Usage: ./test/e2e-sdme-jump.sh [N]
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$SCRIPT_DIR/e2e-sdme-orchestrate.sh" jump "${1:-10}"
