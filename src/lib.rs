//! ush - parallel command execution library.
//!
//! Provide a [`Spec`] describing the command template and parallelism,
//! feed targets through a crossbeam channel, and receive structured
//! [`ExecResult`] values as commands complete.

#[macro_use]
pub mod verbose;

pub mod codec;
mod exec;
pub mod freq;
pub mod strutil;
pub mod time;
pub mod update;

pub use codec::{Decoder as FrameDecoder, Encoder as FrameEncoder, Format as OutputFormat};
pub use exec::jumpexec::jump_exec;
pub use exec::jumpexec::{JumpSpec, DEFAULT_JUMP_COMMAND};
pub use exec::{exec, read_targets, ExecError, ExecResult, Frame, Output, Spec, SpecError};
