//! Frame encoding and decoding for the streaming `ush exec` protocol.
//!
//! Supports JSON (newline-delimited) and MessagePack (4-byte big-endian
//! length-prefixed) formats.

use std::io::{self, Read, Write};

use crate::exec::{ExecResult, Frame};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Json,
    Msgpack,
}

/// Encodes frames or legacy results to an output stream.
pub struct Encoder<W: Write> {
    inner: W,
    format: Format,
}

impl<W: Write> Encoder<W> {
    pub fn new(inner: W, format: Format) -> Self {
        Self { inner, format }
    }

    /// Encode a streaming frame.
    pub fn write_frame(&mut self, frame: &Frame) -> io::Result<()> {
        match self.format {
            Format::Json => {
                serde_json::to_writer(&mut self.inner, frame)?;
                self.inner.write_all(b"\n")?;
            }
            Format::Msgpack => {
                let payload =
                    rmp_serde::to_vec_named(frame).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                let len = payload.len() as u32;
                self.inner.write_all(&len.to_be_bytes())?;
                self.inner.write_all(&payload)?;
            }
        }
        Ok(())
    }

    /// Encode a legacy `ExecResult` (used by `--batch`).
    pub fn write_legacy(&mut self, result: &ExecResult) -> io::Result<()> {
        match self.format {
            Format::Json => {
                serde_json::to_writer(&mut self.inner, result)?;
                self.inner.write_all(b"\n")?;
            }
            Format::Msgpack => {
                let payload =
                    rmp_serde::to_vec_named(result).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                let len = payload.len() as u32;
                self.inner.write_all(&len.to_be_bytes())?;
                self.inner.write_all(&payload)?;
            }
        }
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Decodes MessagePack frames from a length-prefixed stream.
pub struct Decoder<R: Read> {
    inner: R,
    buf: Vec<u8>,
}

impl<R: Read> Decoder<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            buf: Vec::with_capacity(4096),
        }
    }

    /// Read the next frame. Returns `None` at EOF.
    pub fn read_frame(&mut self) -> io::Result<Option<Frame>> {
        self.read_serialized()
    }

    /// Read the next legacy `ExecResult`. Returns `None` at EOF.
    pub fn read_legacy(&mut self) -> io::Result<Option<ExecResult>> {
        self.read_serialized()
    }

    fn read_serialized<T>(&mut self) -> io::Result<Option<T>>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut len_buf = [0u8; 4];
        match self.inner.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }
        let len = u32::from_be_bytes(len_buf) as usize;
        const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;
        if len > MAX_FRAME_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("frame length {} exceeds maximum {}", len, MAX_FRAME_SIZE),
            ));
        }
        self.buf.resize(len, 0);
        self.inner.read_exact(&mut self.buf)?;
        let value = rmp_serde::from_slice(&self.buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Some(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn make_frame() -> Frame {
        Frame::StdoutChunk {
            target: "host1".to_string(),
            seq: 0,
            data: "hello".to_string(),
        }
    }

    #[test]
    fn test_json_roundtrip() {
        let frame = make_frame();
        let mut buf = Vec::new();
        Encoder::new(&mut buf, Format::Json)
            .write_frame(&frame)
            .unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("stdout_chunk"));
        assert!(s.contains("host1"));
    }

    #[test]
    fn test_msgpack_roundtrip() {
        let frame = make_frame();
        let mut buf = Vec::new();
        Encoder::new(&mut buf, Format::Msgpack)
            .write_frame(&frame)
            .unwrap();

        let mut decoder = Decoder::new(Cursor::new(&buf));
        let decoded = decoder.read_frame().unwrap().unwrap();
        match decoded {
            Frame::StdoutChunk { target, seq, data } => {
                assert_eq!(target, "host1");
                assert_eq!(seq, 0);
                assert_eq!(data, "hello");
            }
            _ => panic!("unexpected frame type"),
        }
    }

    #[test]
    fn test_msgpack_eof() {
        let mut decoder = Decoder::new(Cursor::new(&[]));
        assert!(decoder.read_frame().unwrap().is_none());
    }
}
