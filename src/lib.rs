//! ush - parallel command execution library.
//!
//! Provide a [`Spec`] describing the command template and parallelism,
//! feed targets through a crossbeam channel, and receive structured
//! [`ExecResult`] values as commands complete.

#[macro_use]
pub mod verbose;

mod exec;
pub mod freq;
pub mod strutil;
pub mod time;

pub use exec::jumpexec::jump_exec;
pub use exec::jumpexec::{JumpSpec, DEFAULT_JUMP_COMMAND};
pub use exec::{exec, read_targets, ExecError, ExecResult, Spec, SpecError};
