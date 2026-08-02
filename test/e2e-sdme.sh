#!/bin/bash
# End-to-end test for ush over SSH to SDME-managed Ubuntu containers.
# Usage: ./test/e2e-sdme.sh [N]
# Requires: sdme, jq, ssh-keygen, cargo, root privileges.
set -euo pipefail

N=${1:-10}
PREFIX="ush-e2e"
ROOTFS="ush-e2e-ubuntu"
TMPDIR=$(mktemp -d)
KEEP=${KEEP:-0}

KEY="$TMPDIR/id_ed25519"
PUB="$KEY.pub"
BUILD_CONF="$TMPDIR/build.sdme"
HOSTS="$TMPDIR/hosts"
OUTPUT="$TMPDIR/output.json"

cleanup() {
    if [ "$KEEP" -eq 1 ]; then
        echo "KEEP=1 set; leaving containers and rootfs in place."
        echo "Key: $KEY"
        echo "Hosts: $HOSTS"
        return
    fi
    set +e
    for i in $(seq 1 "$N"); do
        sudo sdme stop "${PREFIX}-${i}" >/dev/null 2>&1 || true
        sudo sdme rm -f "${PREFIX}-${i}" >/dev/null 2>&1 || true
    done
    sudo sdme fs rm -f "$ROOTFS" >/dev/null 2>&1 || true
    rm -rf "$TMPDIR"
}

trap cleanup EXIT

if ! command -v sdme >/dev/null 2>&1; then
    echo "sdme not found" >&2
    exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
    echo "jq not found" >&2
    exit 1
fi

# Generate a throwaway SSH key for the test.
ssh-keygen -t ed25519 -N "" -f "$KEY" -q

# Build a rootfs with openssh-server and a test user preconfigured.
cat > "$BUILD_CONF" <<EOF
FROM ubuntu
RUN DEBIAN_FRONTEND=noninteractive apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y openssh-server
RUN useradd -m -s /bin/bash test
COPY $PUB /home/test/.ssh/authorized_keys
RUN chown -R test:test /home/test/.ssh && \
    chmod 700 /home/test/.ssh && \
    chmod 600 /home/test/.ssh/authorized_keys
EOF

echo "Building rootfs $ROOTFS..."
sudo sdme fs rm -f "$ROOTFS" >/dev/null 2>&1 || true
sudo sdme fs build "$ROOTFS" "$BUILD_CONF" -f

# Create and start hardened containers with veth networking.
echo "Creating $N containers..."
for i in $(seq 1 "$N"); do
    name="${PREFIX}-${i}"
    sudo sdme rm -f "$name" >/dev/null 2>&1 || true
    sudo sdme create --name "$name" -r "$ROOTFS" --hardened --network-veth --started --timeout 120
done

# Wait for each container to finish booting.
echo "Waiting for containers to boot..."
for i in $(seq 1 "$N"); do
    name="${PREFIX}-${i}"
    for attempt in $(seq 1 90); do
        if sudo sdme exec "$name" -- systemctl is-system-running >/dev/null 2>&1; then
            break
        fi
        sleep 1
    done
    if ! sudo sdme exec "$name" -- systemctl is-system-running >/dev/null 2>&1; then
        echo "$name did not finish booting" >&2
        exit 1
    fi
done

# Start SSH in each container.
echo "Starting SSH..."
for i in $(seq 1 "$N"); do
    sudo sdme exec "${PREFIX}-${i}" -- systemctl start ssh
done

# Collect container addresses, waiting for DHCP to assign routable IPs.
echo "Collecting container addresses..."
for attempt in $(seq 1 60); do
    sudo sdme ps --json | jq -r --arg prefix "$PREFIX" '.[] | select(.name | startswith($prefix)) | .addresses[0] // empty' | sort > "$HOSTS"
    if [ "$(wc -l < "$HOSTS")" -eq "$N" ] && ! grep -qE '^169\.254\.' "$HOSTS"; then
        break
    fi
    sleep 1
done

if [ "$(wc -l < "$HOSTS")" -ne "$N" ]; then
    echo "Expected $N addresses, got $(wc -l < "$HOSTS")" >&2
    cat "$HOSTS" >&2
    exit 1
fi

if grep -qE '^169\.254\.' "$HOSTS"; then
    echo "Some containers have link-local addresses" >&2
    cat "$HOSTS" >&2
    exit 1
fi

# Build ush.
echo "Building ush..."
cargo build --release

# Run ush over SSH.
echo "Running ush exec hostname over $N hosts..."
./target/release/ush exec -p 10 -- \
    ssh -o StrictHostKeyChecking=no \
        -o UserKnownHostsFile=/dev/null \
        -o BatchMode=yes \
        -o ConnectTimeout=5 \
        -o LogLevel=ERROR \
        -i "$KEY" \
        test@{} \
        hostname \
    < "$HOSTS" > "$OUTPUT"

TOTAL=$(wc -l < "$OUTPUT")
FAILURES=$(grep -cE '"exit_status":[1-9][0-9]*' "$OUTPUT" || true)

if [ "$TOTAL" -ne "$N" ]; then
    echo "Expected $N results, got $TOTAL" >&2
    cat "$OUTPUT" >&2
    exit 1
fi

if [ "$FAILURES" -ne 0 ]; then
    echo "$FAILURES command(s) failed" >&2
    cat "$OUTPUT" >&2
    exit 1
fi

echo "All $N hosts responded:"
jq -r '.target + ": " + (.stdout | sub("\n$"; ""))' "$OUTPUT" | sort
