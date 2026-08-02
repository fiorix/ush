#!/bin/sh
# Generate dl/cli/latest.json (and vX.Y.Z.json) from release tarballs.
#
# Usage:
#   scripts/generate-release-metadata.sh [VERSION] [ASSETS_DIR]
#
# VERSION defaults to the value in Cargo.toml.
# ASSETS_DIR defaults to target/release.
#
# The script expects tarballs named ush-<target>.tar.gz where <target> is one of:
#   x86_64-unknown-linux-musl
#   aarch64-unknown-linux-musl
#   aarch64-apple-darwin

set -eu

VERSION=${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}
ASSETS_DIR=${2:-target/release}
OUT_DIR=${3:-site/static/dl/cli}

if [ -z "$VERSION" ]; then
    echo "usage: $0 [VERSION] [ASSETS_DIR] [OUT_DIR]" >&2
    exit 1
fi

VERSION_BARE=$(echo "$VERSION" | sed 's/^v//')
TAG="v$VERSION_BARE"

REPO="${GITHUB_REPOSITORY:-fiorix/ush}"
RELEASE_URL="https://github.com/$REPO/releases/tag/$TAG"

mkdir -p "$OUT_DIR"

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

{
    printf '{\n'
    printf '  "version": "%s",\n' "$VERSION_BARE"
    printf '  "tag": "%s",\n' "$TAG"
    printf '  "url": "%s",\n' "$RELEASE_URL"
    printf '  "assets": [\n'

    first=1
    for target in x86_64-unknown-linux-musl aarch64-unknown-linux-musl aarch64-apple-darwin; do
        asset="ush-$target.tar.gz"
        path="$ASSETS_DIR/$asset"
        if [ ! -f "$path" ]; then
            continue
        fi

        sha=$(sha256sum "$path" | awk '{print $1}')
        url="https://github.com/$REPO/releases/download/$TAG/$asset"

        if [ "$first" -eq 0 ]; then
            printf ',\n'
        fi
        first=0

        printf '    {\n'
        printf '      "target": "%s",\n' "$target"
        printf '      "asset": "%s",\n' "$asset"
        printf '      "url": "%s",\n' "$url"
        printf '      "sha256": "%s"\n' "$sha"
        printf '    }'
    done

    printf '\n  ]\n'
    printf '}\n'
} > "$tmp"

cp "$tmp" "$OUT_DIR/latest.json"
cp "$tmp" "$OUT_DIR/v$VERSION_BARE.json"

echo "Generated:"
echo "  $OUT_DIR/latest.json"
echo "  $OUT_DIR/v$VERSION_BARE.json"
