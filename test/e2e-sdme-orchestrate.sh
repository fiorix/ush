#!/bin/bash
# Host-side orchestrator for the nested SDME end-to-end tests.
# Creates an outer SDME container that runs e2e-sdme-inner.sh.
# Usage: e2e-sdme-orchestrate.sh <direct|jump> [N]
set -euo pipefail

MODE=${1:-direct}
N=${2:-10}
PREFIX="ush-outer"
OUTER_NAME="${PREFIX}-${MODE}"
OUTER_ROOTFS="ush-e2e-sdme"
KEEP=${KEEP:-0}

cleanup() {
    if [ "$KEEP" -eq 1 ]; then
        echo "KEEP=1; leaving outer container $OUTER_NAME in place."
        return
    fi
    set +e
    sudo sdme stop "$OUTER_NAME" >/dev/null 2>&1 || true
    sudo sdme rm -f "$OUTER_NAME" >/dev/null 2>&1 || true
}

trap cleanup EXIT

if ! command -v sdme >/dev/null 2>&1; then
    echo "sdme not found on host" >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Build the ush binary on the host.
echo "Building ush..."
cd "$REPO_DIR"
cargo build --release

# Build the outer rootfs if it does not already exist.
if ! sudo sdme fs ls | grep -q "^$OUTER_ROOTFS "; then
    echo "Building outer rootfs $OUTER_ROOTFS..."
    cat > /tmp/ush-e2e-sdme.sdme <<'EOF'
FROM ubuntu
RUN DEBIAN_FRONTEND=noninteractive apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y openssh-client jq systemd-container
COPY /usr/local/bin/sdme /usr/local/bin/sdme
COPY /etc/systemd/system/sdme@.service /etc/systemd/system/sdme@.service
COPY /etc/systemd/system/var-lib-sdme-pool.mount /etc/systemd/system/var-lib-sdme-pool.mount
EOF
    sudo sdme fs rm -f "$OUTER_ROOTFS" >/dev/null 2>&1 || true
    sudo sdme fs build "$OUTER_ROOTFS" /tmp/ush-e2e-sdme.sdme -f
fi

# Create and start the outer container.
echo "Creating outer container $OUTER_NAME..."
sudo sdme rm -f "$OUTER_NAME" >/dev/null 2>&1 || true
sudo sdme create --name "$OUTER_NAME" -r "$OUTER_ROOTFS" \
    --storage btrfs --network-veth --capability CAP_NET_ADMIN \
    --started --timeout 180

# Wait for the outer container to finish booting.
echo "Waiting for outer container to boot..."
for attempt in $(seq 1 90); do
    if sudo sdme exec "$OUTER_NAME" -- systemctl is-system-running >/dev/null 2>&1; then
        break
    fi
    sleep 1
done

# Ensure the outer container has a working resolver. systemd-resolved may
# point /etc/resolv.conf at its stub, which is not always reachable here.
sudo sdme exec "$OUTER_NAME" -- systemctl stop systemd-resolved.service >/dev/null 2>&1 || true
sudo sdme exec "$OUTER_NAME" -- bash -c 'echo "nameserver 8.8.8.8" > /etc/resolv.conf'
echo "Waiting for DNS in outer container..."
for attempt in $(seq 1 30); do
    if sudo sdme exec "$OUTER_NAME" -- bash -c 'getent hosts registry-1.docker.io >/dev/null 2>&1'; then
        break
    fi
    sleep 1
done

# Copy the ush binary and inner test script into the outer container.
echo "Copying test artifacts into outer container..."
sudo sdme cp "$REPO_DIR/target/release/ush" "$OUTER_NAME:/usr/local/bin/ush"
sudo sdme cp "$SCRIPT_DIR/e2e-sdme-inner.sh" "$OUTER_NAME:/usr/local/bin/e2e-sdme-inner.sh"
sudo sdme exec "$OUTER_NAME" -- chmod +x /usr/local/bin/e2e-sdme-inner.sh

# Run the test inside the outer container.
echo "Running inner test (mode=$MODE, N=$N)..."
sudo sdme exec "$OUTER_NAME" -- /usr/local/bin/e2e-sdme-inner.sh "$MODE" "$N"
