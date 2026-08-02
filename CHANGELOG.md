# Changelog

## 1.2.0

- Rewrite in Rust. The Go implementation and its modules were replaced by a library crate plus binary using clap, serde, and crossbeam-channel.
- Replace manual argument parsing with clap derive.
- Add shell-based end-to-end tests.
