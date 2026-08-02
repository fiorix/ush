# Context

ush is a command-line tool and library for parallel execution of shell commands over a stream of targets.

## Domain terms

- target: one line read from stdin. Empty lines and lines starting with `#` are skipped. The string `{}` in the command template is replaced with the target.
- command template: the base command and arguments passed after `--`. Each occurrence of `{}` is replaced with the current target.
- worker: one thread that reads targets from the target channel, spawns the command template with the target substituted, and sends `Frame` values to the result channel.
- result channel: crossbeam channel that carries `Frame` values from workers to the writer thread.
- frame: a single message in the `ush exec` output stream. Frames are length-prefixed on the binary wire and newline-delimited in JSON output.
- chunk frame: a frame carrying a piece of `stdout` or `stderr` data for a target as it is produced. Sequence numbers are per target per fd.
- done frame: a frame marking completion of a target, carrying timing, exit status, error, and truncation flags.
- streaming mode: the default output mode that emits chunk frames as output arrives and a done frame when the target finishes.
- batch mode: an opt-in output mode (`--batch`) that emits one legacy `ExecResult` line per target and no chunk frames.
- local output format: the serialization format used for local `ush exec` stdout. JSON is the default; MessagePack is available via `--format=msgpack`.
- wire format: the serialization format used between `ush` processes on the jump-host link. MessagePack with a u32 length prefix.
- shutdown flag: atomic boolean set by the SIGINT/SIGTERM handler. Workers check it before starting a new target; a target already running is not interrupted.
- bounded capture: stdout/stderr collection limited to `stdout_bytes`/`stderr_bytes`. In tail mode the last N bytes are kept; in head mode the first N bytes are kept.
- process group: each child runs in its own process group so timeout handling can signal the whole group.
- jump host: an intermediate SSH host used to scale execution. ush opens one ssh-agent per jump host, runs ush on each host, and feeds it a subset of targets.
- ssh-agent slot: per-jump-host ssh-agent process and socket used to forward the jump key without a single shared agent becoming a bottleneck.
- freq aggregation: reading the frame output of `ush exec` and grouping by stdout, stderr, exit status, or duration bucket. In streaming mode `freq` reconstructs each target's output from chunk frames before aggregating.

## Data flow

```mermaid
flowchart LR
    stdin[stdin targets] --> exec[ush exec]
    exec --> workers[worker pool]
    workers --> procs[child processes]
    workers --> frames[frames on stdout]
```

## Jump host flow

```mermaid
flowchart LR
    local[ush exec -j] --> j1[jump host 1]
    local --> j2[jump host 2]
    j1 --> t1[targets subset]
    j2 --> t2[targets subset]
    j1 -.->|MessagePack| local
    j2 -.->|MessagePack| local
    local --> out[JSON frames on stdout]
```
