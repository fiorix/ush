# Chunked streaming output for `ush exec`

`ush exec` previously buffered all stdout/stderr for a target until the command finished, then emitted a single JSON `ExecResult`. We switched to a streaming frame protocol so output can flow back to the executor as it is produced, bounding per-target memory to the configured capture limits and removing the need to size an arbitrary output buffer.

The default mode emits `stdout_chunk`, `stderr_chunk`, and `done` frames. A `--batch` flag preserves the legacy one-line-per-target `ExecResult` output for existing consumers.
