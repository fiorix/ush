# Changelog

## 2.1.1

- Consolidate test scripts under `tests/`.
- Normalize `sdme` references to lowercase.

## 2.1.0

- Add `ush upgrade` with automatic update checks.
- New website at https://ush.sucks/ with `curl | bash` installer.
- GitHub Actions release pipeline: static musl Linux binaries, macOS binary,
  SHA256SUMS, and release metadata.

## 2.0.0

- Streaming output by default: `stdout_chunk`, `stderr_chunk`, and `done` frames.
- Add `--format {json|msgpack}`, `--chunk-size`, and `--batch` flags.
- MessagePack wire format for jump-host links.
- Add `ush dump-skill` for agent usage documentation.

## 1.2.0

- Rewrite in Rust. The Go implementation and its modules were replaced by a library crate plus binary using clap, serde, and crossbeam-channel.
- Replace manual argument parsing with clap derive.
- Add shell-based end-to-end tests.
