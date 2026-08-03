#!/bin/bash
# Runs inside the outer sdme container.
# Usage: sdme-inner.sh <direct|jump> [N]
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

# Ensure the outer container has the msgpack Python module.
ensure_msgpack() {
    if python3 -c "import msgpack" 2>/dev/null; then
        return 0
    fi
    echo "Installing python3-msgpack in outer container..."
    DEBIAN_FRONTEND=noninteractive apt-get update >/dev/null 2>&1
    DEBIAN_FRONTEND=noninteractive apt-get install -y python3-msgpack >/dev/null 2>&1
}

# Decode a length-prefixed MessagePack batch stream (one ExecResult per frame) to JSON lines.
decode_msgpack_batch() {
    ensure_msgpack

    local input=$1
    local output=$2
    python3 - "$input" "$output" << 'PY'
import sys, msgpack, struct, json
inp, out = sys.argv[1], sys.argv[2]
with open(inp, "rb") as f:
    data = f.read()
pos = 0
with open(out, "w") as f:
    while pos < len(data):
        length = struct.unpack(">I", data[pos:pos+4])[0]
        pos += 4
        result = msgpack.unpackb(data[pos:pos+length])
        pos += length
        f.write(json.dumps(result) + "\n")
PY
}

# Decode a length-prefixed MessagePack streaming frame stream to JSON lines.
decode_msgpack_stream() {
    ensure_msgpack
    local input=$1
    local output=$2
    python3 - "$input" "$output" << 'PY'
import sys, msgpack, struct, json
inp, out = sys.argv[1], sys.argv[2]
with open(inp, "rb") as f:
    data = f.read()
pos = 0
with open(out, "w") as f:
    while pos < len(data):
        length = struct.unpack(">I", data[pos:pos+4])[0]
        pos += 4
        frame = msgpack.unpackb(data[pos:pos+length])
        pos += length
        f.write(json.dumps(frame) + "\n")
PY
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

    echo "Running ush exec hostname over $N hosts (MessagePack)..."
    output=/tmp/ush-e2e-output.mp
    output_json=/tmp/ush-e2e-output.json
    "$USH" exec --format=msgpack --batch -p 10 -- \
        ssh -o StrictHostKeyChecking=no \
            -o UserKnownHostsFile=/dev/null \
            -o BatchMode=yes \
            -o ConnectTimeout=5 \
            -o LogLevel=ERROR \
            -i "$KEY" \
            test@{} \
            hostname \
        < "$targets" > "$output"
    decode_msgpack_batch "$output" "$output_json"

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

    echo "Running ush exec with jump hosts over $expected targets (MessagePack / uname)..."
    output=/tmp/ush-e2e-output.mp
    output_json=/tmp/ush-e2e-output.json
    "$USH" exec --format=msgpack --batch -j "$jumps" -k "$KEY" -p 10 \
        --jump_cmd "ssh -A -oBatchMode=yes -oConnectTimeout=10 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -i $KEY -- test@{jump}" \
        -- \
        ssh -o StrictHostKeyChecking=no \
            -o UserKnownHostsFile=/dev/null \
            -o BatchMode=yes \
            -o ConnectTimeout=5 \
            -o LogLevel=ERROR \
            test@{} \
            uname \
        < "$targets" > "$output"
    decode_msgpack_batch "$output" "$output_json"

    echo "Running jump-host 8k blob streaming test (MessagePack / 1 KiB chunks)..."
    output_blob=/tmp/ush-e2e-output-blob.mp
    output_blob_json=/tmp/ush-e2e-output-blob.json
    "$USH" exec --format=msgpack -j "$jumps" -k "$KEY" -p 10 \
        --chunk_size=1024 \
        --stdout_bytes=8192 \
        --head \
        --jump_cmd "ssh -A -oBatchMode=yes -oConnectTimeout=10 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -i $KEY -- test@{jump}" \
        -- \
        ssh -o StrictHostKeyChecking=no \
            -o UserKnownHostsFile=/dev/null \
            -o BatchMode=yes \
            -o ConnectTimeout=5 \
            -o LogLevel=ERROR \
            test@{} \
            sh -c 'head -c 8192 /dev/zero | tr "\\0" "x"' \
        < "$targets" > "$output_blob"
    decode_msgpack_stream "$output_blob" "$output_blob_json"

    # Verify the blob stream: each target should emit 8 stdout_chunk frames of 1024 bytes
    # followed by a done frame with no truncation (8192 bytes captured exactly).
    chunk_count=$(grep -c '"type":"stdout_chunk"' "$output_blob_json" || true)
    expected_chunks=$((expected * 8))
    done_count=$(grep -c '"type":"done"' "$output_blob_json" || true)
    if [ "$chunk_count" -ne "$expected_chunks" ]; then
        echo "Expected $expected_chunks stdout_chunk frames, got $chunk_count" >&2
        cat "$output_blob_json" >&2
        exit 1
    fi
    if [ "$done_count" -ne "$expected" ]; then
        echo "Expected $expected done frames, got $done_count" >&2
        cat "$output_blob_json" >&2
        exit 1
    fi
    if grep -q '"stdout_truncated":true' "$output_blob_json"; then
        echo "Blob output was unexpectedly truncated" >&2
        cat "$output_blob_json" >&2
        exit 1
    fi
    echo "Blob stream verified: $chunk_count chunks across $done_count targets"
else
    echo "Unknown mode: $MODE" >&2
    exit 1
fi

total=$(wc -l < "$output_json")
failures=$(grep -cE '"exit_status":[1-9][0-9]*' "$output_json" || true)

if [ "$total" -ne "$expected" ]; then
    echo "Expected $expected results, got $total" >&2
    cat "$output_json" >&2
    exit 1
fi

if [ "$failures" -ne 0 ]; then
    echo "$failures command(s) failed" >&2
    cat "$output_json" >&2
    exit 1
fi

echo "All $expected hosts responded:"
jq -r '.target + ": " + (.stdout | sub("\n$"; ""))' "$output_json" | sort
