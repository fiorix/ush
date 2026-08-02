pub(crate) const SKILL: &str = r#"# ush

Use `ush` to run shell commands in parallel across a stream of targets.

## When to use

- Run a command on many local targets in parallel.
- Run commands over SSH to many hosts.
- Run commands through jump hosts when direct access is not available or when fanning out for scale.

## Agent protocol

Agents should invoke `ush exec` with `--format=msgpack`. The output is a stream of length-prefixed MessagePack frames.

Each frame is prefixed by a 4-byte big-endian unsigned integer giving the length of the MessagePack payload that follows.

## Frame types

```rust
#[serde(tag = "type", rename_all = "snake_case")]
enum Frame {
    StdoutChunk { target: String, seq: u64, data: String },
    StderrChunk { target: String, seq: u64, data: String },
    Done {
        target: String,
        start_time: String,
        end_time: String,
        duration: String,
        exit_status: i32,
        error: String,
        stdout_truncated: bool,
        stderr_truncated: bool,
    },
}
```

- `seq` is per target per fd and increments monotonically.
- Collect `stdout_chunk` and `stderr_chunk` frames per target until the matching `done` frame arrives.
- A non-zero `exit_status` or non-empty `error` indicates failure.

## Local execution

```sh
echo -ne 'host1\nhost2\n' | ush exec --format=msgpack -- echo {}
```

## SSH execution

```sh
cat hosts.txt | ush exec -p 8 --format=msgpack -- ssh user@{} -- hostname
```

## SSH via jump hosts

```sh
cat hosts.txt | ush exec -j jumps.txt -k jump.key --format=msgpack -- ssh user@{} -- hostname
```

## Notes

- Targets are read from stdin, one per line. Empty lines and lines starting with `#` are skipped.
- `{}` in the command template is replaced with the current target.
- Use `--parallel` to control concurrency.
- Use `--timeout` to set a per-target timeout (e.g., `30s`, `5m`).
- Use `--stdout_bytes` and `--stderr_bytes` to bound captured output.
- Use `--head` to capture the first N bytes instead of the last N bytes.
- Use `--batch` for the legacy one-line-per-target JSON output.
- Use `ush freq` to aggregate results; it understands both the streaming frame protocol and legacy `ExecResult` lines.
"#;

pub(crate) fn run() {
    print!("{}", SKILL);
}
