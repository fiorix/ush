# Release and distribution pipeline

`ush` is a single-crate CLI with no web frontend or desktop app. This ADR defines how we ship it and how users install and update it. The design reuses proven patterns from `chan` and `sdme`.

## Goals

- Let a new user install `ush` with one command (domain is configurable; the site is hosted on GitHub Pages):
  ```sh
  curl -fsSL https://ush.sucks/install.sh | bash
  ```
  The script is POSIX `sh` and also works under `bash`. The default install location is `~/.local/bin/ush`.
- Let installed copies upgrade themselves with `ush upgrade`.
- Publish to the package managers we already operate for `chan`: PPA, COPR, AUR, Nix, Homebrew.
- Keep a single source of truth for the version.

## Non-goals

- A GUI, desktop app, or web service.
- Windows releases (the codebase is Unix-oriented).
- Signed binaries beyond SHA-256 checksums published with each release.

## Decisions

### 1. GitHub Releases is the artifact store

Every release is a GitHub Release created from a `v*` tag. CI builds all artifacts, uploads them, and attaches a `SHA256SUMS` file. This is the same model used by both `chan` and `sdme`.

### 2. Static musl binaries for Linux, native binary for macOS

Linux builds use `cargo-zigbuild` + Zig to produce fully static musl binaries. This lets one Linux binary run on glibc and musl systems alike, which is important for `install.sh` users.

| Host | Target triple | Artifact name |
|---|---|---|
| Linux x86_64 | `x86_64-unknown-linux-musl` | `ush-x86_64-unknown-linux-musl.tar.gz` |
| Linux aarch64 | `aarch64-unknown-linux-musl` | `ush-aarch64-unknown-linux-musl.tar.gz` |
| macOS aarch64 | `aarch64-apple-darwin` | `ush-aarch64-apple-darwin.tar.gz` |

macOS x86_64 is left out for now because Apple Silicon is our expected audience.

### 3. A small website served from GitHub Pages

The site lives in `site/` and is built with a static-site generator (Zola is recommended to match `sdme`, but a tiny Node script like `chan`'s marketing site also works). It contains:

- `index.html` with the install one-liner and links to package instructions.
- `install.sh`, a POSIX shell script that:
  - Defaults `PREFIX` to `$HOME/.local` (falling back from `XDG_BIN_HOME`).
  - Maps `uname -s` / `uname -m` to the artifact name.
  - Downloads the tarball from the matching GitHub Release.
  - Verifies the SHA-256 checksum from `SHA256SUMS`.
  - Extracts `ush` into `$PREFIX/bin/ush`.
  - Warns if `$PREFIX/bin` is not on `PATH`.
- `dl/cli/latest.json`, generated after each release, describing the latest version, tag, release URL, and an array of per-target assets. Each asset has `target`, `asset`, `url`, and `sha256`. Both `install.sh` and `ush upgrade` read this file.

We use `~/.local/bin` as the default (per the request) rather than `/usr/local/bin`, which `sdme` uses.

### 4. Hand-rolled self-upgrade copied from `sdme`

We do not use the `self_update` crate. Instead, we copy the approach in `sdme/src/update.rs`:

- `ush upgrade` fetches the latest release metadata, picks the asset for the current platform, downloads the tarball, verifies the tarball SHA-256, extracts the `ush` binary, and atomically renames it over `/proc/self/exe`.
- A background `ush __update-check` subprocess writes a small state file; the next normal invocation prints an update hint banner on stderr.
- `USH_UPDATE_CHECK=0` disables probing.
- `USH_UPDATE_METADATA_URL` overrides the metadata URL.
- `USH_UPDATE_INSECURE=1` allows `http://` and `file://` metadata/download URLs for local testing only.
- Packaged builds disable self-upgrade (see decision 5).

The main adaptation is replacing `sdme` URLs and asset names with `ush` equivalents and making the background probe optional because `ush` currently has no config file.

### 5. Distro packages stamp the binary to disable self-upgrade

Following `chan`'s `CHAN_PACKAGED` and `sdme`'s `SDME_CHANNEL`, `build.rs` bakes `USH_PACKAGED` into the binary when an environment variable is set:

```rust
let packaged = std::env::var("USH_PACKAGED").unwrap_or_else(|_| "source".into());
println!("cargo:rustc-env=USH_PACKAGED={packaged}");
```

When `USH_PACKAGED` is not `source`, `ush upgrade` refuses to run and tells the user to use the package manager. Each package build sets the appropriate value:

| Package | `USH_PACKAGED` value |
|---|---|
| tarball / `install.sh` | `source` |
| Debian / PPA | `deb` |
| Fedora / COPR | `rpm` |
| Arch / AUR | `aur` |
| Nix | `nix` |
| Homebrew | `brew` |

### 6. Single version source of truth

The version lives only in `Cargo.toml`. All packaging scripts read it from there, matching both `chan` and `sdme`.

```sh
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
```

### 7. Downstream packaging

We prepare packaging for the same targets `chan` already supports, adapted for a single CLI binary.

#### Debian / Ubuntu (PPA)

- Files: `packaging/debian/debian/control`, `rules`, `changelog.in`, plus `packaging/debian/build-source.sh` and `upload.sh`.
- Source package built from a vendored or upstream tarball.
- `rules` sets `USH_PACKAGED=deb`.

#### Fedora / RPM (COPR)

- Files: `.copr/Makefile`, `packaging/fedora/ush.spec`, `packaging/fedora/make-srpm.sh`.
- COPR's `make srpm` entry point builds a vendored source tarball.
- The spec sets `USH_PACKAGED=rpm` during `%build`.

#### Arch (AUR)

- Files: `packaging/arch/aur/ush/PKGBUILD.in` and `make-aur-package.sh`.
- Source package uses the GitHub archive tarball.
- `PKGBUILD` sets `USH_PACKAGED=aur`.

#### Nix

- Files: `flake.nix`, `packaging/nix/ush.nix`.
- `buildRustPackage` builds from the local source.
- `USH_PACKAGED=nix` is set in the derivation.

#### Homebrew

- Files: `packaging/homebrew/Formula/ush.rb.in`, `make-homebrew-package.sh`.
- Formula points at the macOS tarball.
- Rendered formula is pushed to `fiorix/homebrew-ush`.

## File changes

### New files

- `build.rs` — bakes `USH_PACKAGED`.
- `src/update.rs` — self-upgrade engine, adapted from `sdme/src/update.rs`.
- `src/cmd/update.rs` — `ush upgrade` CLI wiring.
- `site/config.toml`, `site/templates/base.html`, `site/templates/index.html`, `site/static/install.sh`, `site/static/css/style.css`, `site/static/js/version-badge.js`.
- `scripts/generate-release-metadata.sh` — generate `dl/cli/latest.json` from local release tarballs.
- `scripts/fetch-release-metadata.py` — regenerate `dl/cli/latest.json` from the latest GitHub Release (used by the Pages workflow).
- `.github/workflows/release.yml` — tag-driven release and site deploy.
- `.github/workflows/pages.yml` — build and deploy the site on every push to main.
- `packaging/linux/Makefile` — standalone tarball builds.
- `Makefile` — convenience targets.
- `packaging/debian/`, `packaging/fedora/`, `packaging/arch/`, `packaging/nix/`, `packaging/homebrew/` (phase 2).
- `.copr/Makefile` (phase 2).

### Modified files

- `Cargo.toml` — add `ureq`, `sha2`, `anyhow`, `libc`, `tar`, `flate2`.
- `src/main.rs` — add `Upgrade` subcommand, early `__update-check` dispatcher, background probe spawn, and banner print.
- `src/cmd/mod.rs` — expose the new `update` module.
- `src/lib.rs` — expose the `update` module for the binary to use.

## Release flow

1. Developer bumps `Cargo.toml` version and pushes a `vX.Y.Z` tag.
2. `.github/workflows/release.yml` triggers.
3. CI builds static binaries for Linux x86_64/aarch64 and a native macOS aarch64 binary.
4. CI creates a GitHub Release and uploads tarballs + `SHA256SUMS`.
5. A post-release step generates `dl/cli/latest.json` from the local assets, builds the site with Zola, and deploys it to GitHub Pages.
6. On every push to `main`, `.github/workflows/pages.yml` fetches the latest release metadata from GitHub and rebuilds/deploys the site so non-release site changes do not erase the metadata.
7. A separate downstream workflow (or manual step) publishes to PPA, COPR, AUR, Nix cache, and Homebrew.

## Security notes

- `install.sh` and `ush upgrade` verify SHA-256 checksums before writing the binary.
- Downloads happen over HTTPS only.
- The atomic `fs::rename` after verification means a failed or partial download never leaves a broken `ush` binary in place.
- Distro packages disable self-upgrade so users are not tempted to bypass the package manager.

## Future work

- Add code signing or notarization for macOS if Gatekeeper becomes an issue.
- Add an `x86_64-apple-darwin` build if requested.
- Add a Windows build if the codebase is ported.
