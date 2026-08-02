#!/bin/sh
# ush installer for macOS and Linux.
#
#   curl -fsSL https://ush.sucks/install.sh | bash
#
# Downloads release metadata, selects the matching standalone tarball,
# verifies SHA256, and installs `ush` into PREFIX/bin.
# Defaults:
#
#   METADATA_URL=https://ush.sucks/dl/cli/latest.json
#   PREFIX=$HOME/.local

set -eu

DEFAULT_METADATA_BASE="https://ush.sucks/dl/cli"
PREFIX="${PREFIX:-$HOME/.local}"

err() { printf 'install: %s\n' "$1" >&2; exit 1; }

validate_version() {
    version=$1
    case "$version" in
        ""|v*|*[!0123456789.]*|*.*.*.*) err "VERSION must be a bare X.Y.Z version." ;;
    esac
    old_ifs=$IFS
    IFS=.
    # shellcheck disable=SC2086
    set -- $version
    IFS=$old_ifs
    [ "$#" -eq 3 ] || err "VERSION must be a bare X.Y.Z version."
    [ -n "$1" ] && [ -n "$2" ] && [ -n "$3" ] || err "VERSION must be a bare X.Y.Z version."
}

if [ "${VERSION:-}" ]; then
    validate_version "$VERSION"
fi

if [ "${METADATA_URL:-}" ]; then
    :
elif [ "${BASE:-}" ]; then
    BASE="${BASE%/}"
    if [ "${VERSION:-}" ]; then
        METADATA_URL="$BASE/v$VERSION.json"
    else
        METADATA_URL="$BASE/latest.json"
    fi
elif [ "${VERSION:-}" ]; then
    METADATA_URL="$DEFAULT_METADATA_BASE/v$VERSION.json"
else
    METADATA_URL="$DEFAULT_METADATA_BASE/latest.json"
fi

os=$(uname -s)
arch=$(uname -m)

case "$os" in
    Darwin)
        case "$arch" in
            arm64|aarch64) target="aarch64-apple-darwin" ;;
            *) err "macOS on $arch is not published. arm64 only for now." ;;
        esac
        ;;
    Linux)
        case "$arch" in
            x86_64|amd64)  target="x86_64-unknown-linux-musl" ;;
            aarch64|arm64) target="aarch64-unknown-linux-musl" ;;
            *) err "Linux on $arch is not published." ;;
        esac
        ;;
    *) err "Unsupported OS: $os." ;;
esac

fetch_url() {
    url=$1
    out=$2
    case "$url" in
        file://*) cp "${url#file://}" "$out" ;;
        /*|./*|../*) cp "$url" "$out" ;;
        *)
            if command -v curl >/dev/null 2>&1; then
                curl -fsSL "$url" -o "$out"
            elif command -v wget >/dev/null 2>&1; then
                wget -qO "$out" "$url"
            else
                printf 'install: need curl or wget on PATH.\n' >&2
                return 1
            fi
            ;;
    esac
}

json_field() {
    file=$1
    key=$2
    # shellcheck disable=SC2020
    tr '{}[],' '\n\n\n\n\n' < "$file" | awk -v key="$key" '
        function trim(s) {
            sub(/^[[:space:]]+/, "", s)
            sub(/[[:space:]]+$/, "", s)
            return s
        }
        function value(s) {
            sub(/^[^:]*:[[:space:]]*"/, "", s)
            sub(/"[[:space:]]*$/, "", s)
            return s
        }
        {
            line = trim($0)
            if (index(line, "\"" key "\"") == 1) {
                print value(line)
                exit
            }
        }
    '
}

target_metadata() {
    file=$1
    wanted=$2
    # shellcheck disable=SC2020
    tr '{}[],' '\n\n\n\n\n' < "$file" | awk -v wanted="$wanted" '
        function trim(s) {
            sub(/^[[:space:]]+/, "", s)
            sub(/[[:space:]]+$/, "", s)
            return s
        }
        function value(s) {
            sub(/^[^:]*:[[:space:]]*"/, "", s)
            sub(/"[[:space:]]*$/, "", s)
            return s
        }
        BEGIN { in_target = 0; asset = ""; url = ""; sha = "" }
        {
            line = trim($0)
            if (index(line, "\"target\"") == 1) {
                in_target = (value(line) == wanted)
                asset = ""; url = ""; sha = ""
                next
            }
            if (!in_target) {
                next
            }
            if (index(line, "\"asset\"") == 1) {
                asset = value(line)
            } else if (index(line, "\"url\"") == 1) {
                url = value(line)
            } else if (index(line, "\"sha256\"") == 1) {
                sha = value(line)
            }
            if (asset != "" && url != "" && sha != "") {
                print asset
                print url
                print sha
                exit
            }
        }
    '
}

sha256_file() {
    file=$1
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print tolower($1)}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print tolower($1)}'
    else
        err "need sha256sum or shasum on PATH."
    fi
}

bindir="$PREFIX/bin"
mkdir -p "$bindir"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

printf 'install: reading %s\n' "$METADATA_URL"
if ! fetch_url "$METADATA_URL" "$tmp/ush-release.json"; then
    printf 'install: release metadata unavailable: %s\n' "$METADATA_URL" >&2
    printf 'install: manual downloads: https://github.com/fiorix/ush/releases\n' >&2
    exit 1
fi

version=$(json_field "$tmp/ush-release.json" version)
[ -n "$version" ] || err "metadata is missing version."

info=$(target_metadata "$tmp/ush-release.json" "$target")
[ -n "$info" ] || err "metadata has no asset for $target."

asset=$(printf '%s\n' "$info" | sed -n '1p')
url=$(printf '%s\n' "$info" | sed -n '2p')
expected_sha=$(printf '%s\n' "$info" | sed -n '3p' | tr 'A-F' 'a-f')

expected_asset="ush-$target.tar.gz"
[ "$asset" = "$expected_asset" ] || err "metadata asset mismatch for $target: $asset"

case "$expected_sha" in
    ""|*[!0123456789abcdef]*) err "metadata has invalid SHA256 for $asset." ;;
esac
[ "${#expected_sha}" -eq 64 ] || err "metadata has invalid SHA256 for $asset."

printf 'install: downloading %s\n' "$url"
fetch_url "$url" "$tmp/ush.tar.gz" || err "download failed: $url"

actual_sha=$(sha256_file "$tmp/ush.tar.gz")
[ "$actual_sha" = "$expected_sha" ] || err "SHA256 mismatch for $asset."

tar -xzf "$tmp/ush.tar.gz" -C "$tmp"

bin=$(find "$tmp" -type f -name ush -perm -u+x | head -n1 || true)
[ -n "$bin" ] || err "binary 'ush' not found inside $asset"

install -m 0755 "$bin" "$bindir/ush"
printf 'install: installed ush %s to %s\n' "$version" "$bindir/ush"

case ":$PATH:" in
    *":$bindir:") ;;
    *)
        printf 'install: note: %s is not on your PATH.\n' "$bindir" >&2
        printf '  add this to your shell rc:  export PATH="%s:$PATH"\n' "$bindir" >&2
        ;;
esac
