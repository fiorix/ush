#!/bin/bash
# Runs inside the outer SDME container.
# Usage: e2e-sdme-inner.sh <direct|jump> [N]
set -euo pipefail

MODE=${1:-direct}
N=${2:-10}
PREFIX="ush-e2e"
KEY=/tmp/ush-e2e-key
TARGET_ROOTFS="inner-ubuntu"
JUMP_ROOTFS="inner-jump"
USH=/usr/local/bin/ush

expected=$N
if [ "$MODE" = "jump" ]; then
    expected=$((N * 2))
fi

cleanup() {
    if [ "${KEEP:-0}" -eq 1 ]; then
        echo "KEEP=1; leaving nested containers and rootfs in place."
        return
    fi
    set +e
    for i in $(seq 1 "$N"); do
        for j in a b; do
            sdme stop "${PREFIX}-${MODE}-target-${j}-${i}" >/dev/null 2>&1 || true
            sdme rm -f "${PREFIX}-${MODE}-target-${j}-${i}" >/dev/null 2>&1 || true
        done
    done
    for j in a b; do
        sdme stop "${PREFIX}-${MODE}-jump-${j}" >/dev/null 2>&1 || true
        sdme rm -f "${PREFIX}-${MODE}-jump-${j}" >/dev/null 2>&1 || true
    done
    sdme fs rm -f "$TARGET_ROOTFS" "$JUMP_ROOTFS" >/dev/null 2>&1 || true
    rm -f "$KEY" "$KEY.pub" /tmp/ush-e2e-*.txt /tmp/ush-e2e-*.json 2>/dev/null || true
}

trap cleanup EXIT

if ! command -v sdme >/dev/null 2>&1; then
    echo "sdme not found inside outer container" >&2
    exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
    echo "jq not found inside outer container" >&2
    exit 1
fi

# systemd-resolved may not populate /etc/resolv.conf usefully inside the outer
# container. Force a public resolver so imports and apt work.
if ! grep -qE '^nameserver' /etc/resolv.conf 2>/dev/null; then
    echo "nameserver 8.8.8.8" > /etc/resolv.conf
fi

# Ensure the base Ubuntu rootfs exists inside the outer container.
if ! sdme fs ls | grep -q '^ubuntu '; then
    echo "Importing ubuntu rootfs inside outer container..."
    sdme fs import docker.io/ubuntu -v --install-packages=yes
fi

# Generate a throwaway SSH key for this run.
ssh-keygen -t ed25519 -N "" -f "$KEY" -q

# Build the target rootfs with openssh-server and a test user whose
# authorized_keys contains the public key.
cat > /tmp/build-target.sdme <<EOF
FROM ubuntu
RUN DEBIAN_FRONTEND=noninteractive apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y openssh-server
RUN useradd -m -s /bin/bash test
COPY $KEY.pub /home/test/.ssh/authorized_keys
RUN chown -R test:test /home/test/.ssh && \
    chmod 700 /home/test/.ssh && \
    chmod 600 /home/test/.ssh/authorized_keys
EOF

echo "Building target rootfs $TARGET_ROOTFS..."
sdme fs rm -f "$TARGET_ROOTFS" >/dev/null 2>&1 || true
sdme fs build "$TARGET_ROOTFS" /tmp/build-target.sdme -f

# Build the jump-host rootfs by adding the ush binary and ssh client to the
# target rootfs. Jump hosts run ush exec and ssh-agent for target auth.
echo "Building jump-host rootfs $JUMP_ROOTFS..."
cat > /tmp/build-jump.sdme <<EOF
FROM $TARGET_ROOTFS
RUN DEBIAN_FRONTEND=noninteractive apt-get install -y openssh-client
COPY $USH /usr/local/bin/ush
EOF

sdme fs rm -f "$JUMP_ROOTFS" >/dev/null 2>&1 || true
sdme fs build "$JUMP_ROOTFS" /tmp/build-jump.sdme -f

start_ssh() {
    local name=$1
    for attempt in $(seq 1 90); do
        if sdme exec "$name" -- systemctl is-system-running >/dev/null 2>&1; then
            break
        fi
        sleep 1
    done
    sdme exec "$name" -- systemctl start ssh
}

address_of() {
    sdme ps --json | jq -r --arg name "$1" '.[] | select(.name == $name) | .addresses[0] // empty'
}

collect_addresses() {
    local out=$1
    shift
    > "$out"
    for name in "$@"; do
        address_of "$name" >> "$out"
    done
}

wait_for_addresses() {
    local out=$1
    local expected_count=$2
    shift 2
    for attempt in $(seq 1 90); do
        collect_addresses "$out" "$@"
        if [ "$(wc -l < "$out")" -eq "$expected_count" ] && ! grep -qE '^$|^169\.254\.' "$out"; then
            return 0
        fi
        sleep 1
    done
    echo "Timed out waiting for addresses" >&2
    cat "$out" >&2
    return 1
}

if [ "$MODE" = "direct" ]; then
    echo "Creating $N target containers..."
    names=()
    for i in $(seq 1 "$N"); do
        name="${PREFIX}-${MODE}-target-a-${i}"
        sdme rm -f "$name" >/dev/null 2>&1 || true
        sdme create --name "$name" -r "$TARGET_ROOTFS" --hardened --network-veth --started --timeout 120
        names+=("$name")
    done

    echo "Starting SSH..."
    for name in "${names[@]}"; do
        start_ssh "$name"
    done

    targets=/tmp/ush-e2e-targets.txt
    wait_for_addresses "$targets" "$N" "${names[@]}"

    echo "Running ush exec hostname over $N hosts..."
    output=/tmp/ush-e2e-output.json
    "$USH" exec -p 10 -- \
        ssh -o StrictHostKeyChecking=no \
            -o UserKnownHostsFile=/dev/null \
            -o BatchMode=yes \
            -o ConnectTimeout=5 \
            -o LogLevel=ERROR \
            -i "$KEY" \
            test@{} \
            hostname \
        < "$targets" > "$output"

elif [ "$MODE" = "jump" ]; then
    echo "Creating 2 jump containers..."
    jump_names=()
    for j in a b; do
        name="${PREFIX}-${MODE}-jump-${j}"
        sdme rm -f "$name" >/dev/null 2>&1 || true
        sdme create --name "$name" -r "$JUMP_ROOTFS" --hardened --network-veth --started --timeout 120
        jump_names+=("$name")
    done

    echo "Creating $N target containers in each group (a/b)..."
    target_names=()
    for i in $(seq 1 "$N"); do
        for j in a b; do
            name="${PREFIX}-${MODE}-target-${j}-${i}"
            sdme rm -f "$name" >/dev/null 2>&1 || true
            sdme create --name "$name" -r "$TARGET_ROOTFS" --hardened --network-veth --started --timeout 120
            target_names+=("$name")
        done
    done

    echo "Starting SSH..."
    for name in "${jump_names[@]}" "${target_names[@]}"; do
        start_ssh "$name"
    done

    jumps=/tmp/ush-e2e-jumps.txt
    targets=/tmp/ush-e2e-targets.txt
    wait_for_addresses "$jumps" 2 "${jump_names[@]}"
    wait_for_addresses "$targets" "$expected" "${target_names[@]}"

    echo "Running ush exec with jump hosts over $expected targets..."
    output=/tmp/ush-e2e-output.json
    "$USH" exec -j "$jumps" -k "$KEY" -p 10 \
        --jump_cmd "ssh -A -oBatchMode=yes -oConnectTimeout=10 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -i $KEY -- test@{jump}" \
        -- \
        ssh -o StrictHostKeyChecking=no \
            -o UserKnownHostsFile=/dev/null \
            -o BatchMode=yes \
            -o ConnectTimeout=5 \
            -o LogLevel=ERROR \
            test@{} \
            hostname \
        < "$targets" > "$output"
else
    echo "Unknown mode: $MODE" >&2
    exit 1
fi

total=$(wc -l < "$output")
failures=$(grep -cE '"exit_status":[1-9][0-9]*' "$output" || true)

if [ "$total" -ne "$expected" ]; then
    echo "Expected $expected results, got $total" >&2
    cat "$output" >&2
    exit 1
fi

if [ "$failures" -ne 0 ]; then
    echo "$failures command(s) failed" >&2
    cat "$output" >&2
    exit 1
fi

echo "All $expected hosts responded:"
jq -r '.target + ": " + (.stdout | sub("\n$"; ""))' "$output" | sort
