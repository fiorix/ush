use std::io::Cursor;
use std::process::Command;

use ush::codec::Decoder;
use ush::Frame;

fn ush() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ush"));
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    cmd
}

#[test]
fn msgpack_batch_roundtrip() {
    let mut cmd = ush();
    cmd.args(["exec", "--format=msgpack", "--batch", "--", "echo", "{}"]);

    let mut child = cmd.spawn().unwrap();
    {
        let stdin = child.stdin.take().unwrap();
        std::io::Write::write_all(&mut { stdin }, b"alpha\nbeta\n").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    let mut decoder = Decoder::new(Cursor::new(&output.stdout));
    let mut targets = Vec::new();
    while let Some(result) = decoder.read_legacy().unwrap() {
        targets.push(result.target);
    }
    targets.sort();
    assert_eq!(targets, vec!["alpha", "beta"]);
}

#[test]
fn msgpack_streaming_chunks() {
    let mut cmd = ush();
    cmd.args([
        "exec",
        "--format=msgpack",
        "--chunk_size=16",
        "--stdout_bytes=64",
        "--head",
        "--",
        "sh",
        "-c",
        "printf '%0.sx' $(seq 1 100)",
    ]);

    let mut child = cmd.spawn().unwrap();
    {
        let stdin = child.stdin.take().unwrap();
        std::io::Write::write_all(&mut { stdin }, b"t\n").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    let mut decoder = Decoder::new(Cursor::new(&output.stdout));
    let mut chunks = 0;
    let mut saw_done = false;
    while let Some(frame) = decoder.read_frame().unwrap() {
        match frame {
            Frame::StdoutChunk { data, .. } => {
                assert!(!data.is_empty());
                chunks += 1;
            }
            Frame::Done { stdout_truncated, .. } => {
                assert!(stdout_truncated);
                saw_done = true;
            }
            _ => {}
        }
    }
    assert!(saw_done);
    assert!(chunks >= 2, "expected multiple chunks, got {}", chunks);
}
