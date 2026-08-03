#!/bin/bash
# End-to-end jump-host test for ush over SSH to hardened sdme containers.
# Runs inside an outer sdme container so the hardened targets are nested.
# Usage: ./tests/e2e/sdme-jump.sh [N]
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$SCRIPT_DIR/sdme-orchestrate.sh" jump "${1:-10}"
