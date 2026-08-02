# MessagePack as the binary wire format

`ush exec` speaks JSON on local stdout by default so `freq`, `jq`, and shell pipelines stay readable. For the jump-host SSH link we use MessagePack instead: it is compact, self-describing, has mature serde support via `rmp-serde`, and can fall back to JSON by swapping the serializer. Frames are length-prefixed with a 4-byte big-endian u32 so the stream is self-delimiting.

The local default remains JSON. MessagePack is available for local output via `--format=msgpack`, and the outer `ush` always requests MessagePack from remote `ush` processes in jump mode.
