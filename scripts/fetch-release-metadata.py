#!/usr/bin/env python3
"""Fetch the latest ush release metadata from GitHub and write dl/cli/latest.json.

Usage:
    scripts/fetch-release-metadata.py [OUT_DIR]

OUT_DIR defaults to site/static/dl/cli.
"""

import json
import pathlib
import sys
import urllib.error
import urllib.request

REPO = "fiorix/ush"
TARGETS = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "aarch64-apple-darwin",
]


def fetch_json(url: str) -> dict:
    req = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
        },
    )
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read())


def fetch_text(url: str) -> str:
    with urllib.request.urlopen(url) as resp:
        return resp.read().decode("utf-8")


def main() -> int:
    out_dir = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else pathlib.Path("site/static/dl/cli")
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / "latest.json"

    try:
        release = fetch_json(f"https://api.github.com/repos/{REPO}/releases/latest")
    except urllib.error.HTTPError as e:
        if e.code == 404:
            print("no GitHub releases found; writing empty metadata", file=sys.stderr)
            out_path.write_text(json.dumps({"version": "", "tag": "", "url": "", "assets": []}, indent=2) + "\n")
            print(f"wrote {out_path}")
            return 0
        raise

    tag = release["tag_name"]
    version = tag.lstrip("v")

    sums_url = f"https://github.com/{REPO}/releases/download/{tag}/SHA256SUMS"
    sums_body = fetch_text(sums_url)
    checksums: dict[str, str] = {}
    for line in sums_body.strip().splitlines():
        parts = line.split()
        if len(parts) == 2:
            checksums[parts[1].lstrip("*")] = parts[0].lower()

    assets = []
    for target in TARGETS:
        asset = f"ush-{target}.tar.gz"
        if asset not in checksums:
            print(f"warning: no checksum for {asset}", file=sys.stderr)
            continue
        assets.append(
            {
                "target": target,
                "asset": asset,
                "url": f"https://github.com/{REPO}/releases/download/{tag}/{asset}",
                "sha256": checksums[asset],
            }
        )

    metadata = {
        "version": version,
        "tag": tag,
        "url": release["html_url"],
        "assets": assets,
    }

    out_path.write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"wrote {out_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
