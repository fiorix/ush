//! Global verbose flag for diagnostic output to stderr.

use std::sync::atomic::{AtomicBool, Ordering};

static VERBOSE: AtomicBool = AtomicBool::new(false);

pub fn set_verbose(v: bool) {
    VERBOSE.store(v, Ordering::Relaxed);
}

pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// Print a verbose diagnostic message to stderr.
/// Usage: vlog!("message {}", value);
#[macro_export]
macro_rules! vlog {
    ($($arg:tt)*) => {
        if $crate::verbose::is_verbose() {
            eprintln!($($arg)*);
        }
    };
}
