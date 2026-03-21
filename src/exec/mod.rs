pub mod jumpexec;

use std::io::{self, Read};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};

use crossbeam_channel as channel;
use serde::{Deserialize, Serialize};

use crate::strutil::StringSet;
use crate::time::{format_duration, format_rfc3339_nano};

// ── Error types ──────────────────────────────────────────────────────

#[derive(Debug)]
pub enum SpecError {
    MissingCommand,
    ZeroTimeout,
    ZeroParallel,
    ZeroStdoutBytes,
    ZeroStderrBytes,
}

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpecError::MissingCommand => write!(f, "command not set"),
            SpecError::ZeroTimeout => write!(f, "timeout must be greater than zero"),
            SpecError::ZeroParallel => write!(f, "parallel must be greater than zero"),
            SpecError::ZeroStdoutBytes => write!(f, "stdout_bytes must be greater than zero"),
            SpecError::ZeroStderrBytes => write!(f, "stderr_bytes must be greater than zero"),
        }
    }
}

impl std::error::Error for SpecError {}

#[derive(Debug)]
pub enum ExecError {
    InvalidSpec(SpecError),
    Io(io::Error),
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecError::InvalidSpec(e) => write!(f, "invalid spec: {}", e),
            ExecError::Io(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for ExecError {}

impl From<SpecError> for ExecError {
    fn from(e: SpecError) -> Self {
        ExecError::InvalidSpec(e)
    }
}

impl From<io::Error> for ExecError {
    fn from(e: io::Error) -> Self {
        ExecError::Io(e)
    }
}

// ── Core types ───────────────────────────────────────────────────────

pub struct Spec {
    pub command: String,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub parallel: usize,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
    pub head: bool,
}

impl Spec {
    pub fn validate(&self) -> Result<(), SpecError> {
        if self.command.is_empty() {
            return Err(SpecError::MissingCommand);
        }
        if self.timeout.is_zero() {
            return Err(SpecError::ZeroTimeout);
        }
        if self.parallel == 0 {
            return Err(SpecError::ZeroParallel);
        }
        if self.stdout_bytes == 0 {
            return Err(SpecError::ZeroStdoutBytes);
        }
        if self.stderr_bytes == 0 {
            return Err(SpecError::ZeroStderrBytes);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResult {
    pub target: String,
    pub duration: String,
    pub start_time: String,
    pub end_time: String,
    pub exit_status: i32,
    pub stdout: String,
    pub stderr: String,
    #[serde(default)]
    pub error: String,
}

// ── Public functions ─────────────────────────────────────────────────

/// Reads targets from a reader, skipping empty lines, comments, and excluded targets.
/// Returns a crossbeam channel receiver that yields target strings.
pub fn read_targets(
    reader: impl io::Read + Send + 'static,
    exclude: Option<StringSet>,
) -> channel::Receiver<String> {
    let (tx, rx) = channel::bounded(1024);

    std::thread::spawn(move || {
        let buf = io::BufReader::new(reader);
        for line in io::BufRead::lines(buf) {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(ref exc) = exclude {
                if exc.contains(&line) {
                    vlog!("[verbose] target reader: excluding {}", line);
                    continue;
                }
            }
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    rx
}

/// Executes commands in parallel for each target received from the channel.
/// Results are sent to the results channel as they complete.
/// The shutdown flag can be set to stop accepting new targets.
pub fn exec(
    spec: &Spec,
    targets: channel::Receiver<String>,
    results: channel::Sender<ExecResult>,
    shutdown: Arc<AtomicBool>,
) -> Result<(), ExecError> {
    spec.validate()?;

    let mut handles = Vec::new();

    vlog!(
        "[verbose] exec: starting {} worker(s), timeout={}, command={}",
        spec.parallel,
        format_duration(spec.timeout),
        spec.command
    );

    for _ in 0..spec.parallel {
        let targets = targets.clone();
        let results = results.clone();
        let shutdown = shutdown.clone();
        let command = spec.command.clone();
        let args = spec.args.clone();
        let timeout = spec.timeout;
        let stdout_bytes = spec.stdout_bytes;
        let stderr_bytes = spec.stderr_bytes;
        let head = spec.head;

        let handle = std::thread::spawn(move || {
            while let Ok(target) = targets.recv() {
                if shutdown.load(Ordering::Relaxed) {
                    vlog!("[verbose] worker: shutdown signal received, stopping");
                    break;
                }
                vlog!("[verbose] worker: executing target {}", target);
                let result = run_cmd(
                    &command,
                    &args,
                    &target,
                    timeout,
                    stdout_bytes,
                    stderr_bytes,
                    head,
                );
                if results.send(result).is_err() {
                    break;
                }
            }
        });
        handles.push(handle);
    }

    // Drop our copy of the senders so the results channel closes when workers finish
    drop(results);

    for handle in handles {
        let _ = handle.join();
    }
    Ok(())
}

// ── Internal ─────────────────────────────────────────────────────────

fn run_cmd(
    command: &str,
    args: &[String],
    target: &str,
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
    head: bool,
) -> ExecResult {
    let start_time = SystemTime::now();
    let cmd_str = command.replace("{}", target);
    let replaced_args: Vec<String> = args.iter().map(|a| a.replace("{}", target)).collect();

    let mut result = ExecResult {
        target: target.to_string(),
        duration: String::new(),
        start_time: format_rfc3339_nano(start_time),
        end_time: String::new(),
        exit_status: 0,
        stdout: String::new(),
        stderr: String::new(),
        error: String::new(),
    };

    let mut cmd = Command::new(&cmd_str);
    cmd.args(&replaced_args);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Set process group so we can kill the whole group on timeout
    unsafe {
        cmd.pre_exec(|| {
            let ret = syscall_setpgid(0, 0);
            if ret != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            vlog!("[verbose] run_cmd: failed to spawn {}: {}", cmd_str, e);
            result.error = e.to_string();
            let end = SystemTime::now();
            result.end_time = format_rfc3339_nano(end);
            result.duration = format_duration(end.duration_since(start_time).unwrap_or_default());
            return result;
        }
    };

    let pid = child.id() as i32;
    vlog!(
        "[verbose] run_cmd: spawned pid={} pgid={} cmd={}",
        pid,
        pid,
        cmd_str
    );

    let stdout_pipe = child.stdout.take().unwrap();
    let stderr_pipe = child.stderr.take().unwrap();

    let stdout_thread = std::thread::spawn(move || bounded_read(stdout_pipe, stdout_limit, head));
    let stderr_thread = std::thread::spawn(move || bounded_read(stderr_pipe, stderr_limit, head));

    // Condvar-based timeout: supports early cancellation and SIGTERM-before-SIGKILL
    let done = Arc::new((Mutex::new(false), Condvar::new()));
    let done2 = done.clone();

    let _timeout_handle = std::thread::spawn(move || {
        let (lock, cvar) = &*done2;

        let guard = lock.lock().unwrap();
        let (guard, _) = cvar.wait_timeout(guard, timeout).unwrap();
        if *guard {
            return;
        }
        drop(guard);

        vlog!(
            "[verbose] run_cmd: timeout reached for pid={}, sending SIGTERM",
            pid
        );
        unsafe { syscall_kill(-pid, 15) };

        let grace = Duration::from_secs(5);
        let guard = lock.lock().unwrap();
        let (guard, _) = cvar.wait_timeout(guard, grace).unwrap();
        if *guard {
            vlog!("[verbose] run_cmd: pid={} exited after SIGTERM", pid);
            return;
        }
        drop(guard);

        vlog!(
            "[verbose] run_cmd: grace period expired for pid={}, sending SIGKILL",
            pid
        );
        unsafe { syscall_kill(-pid, 9) };
    });

    let wait_result = child.wait();

    {
        let (lock, cvar) = &*done;
        let mut finished = lock.lock().unwrap();
        *finished = true;
        cvar.notify_one();
    }

    let stdout_capture = stdout_thread.join().unwrap();
    let stderr_capture = stderr_thread.join().unwrap();

    match wait_result {
        Ok(status) => {
            if !status.success() {
                result.exit_status = status.code().unwrap_or(-1);
                if let Some(sig) = status.signal() {
                    result.error = match sig {
                        9 => "signal: killed".to_string(),
                        15 => "signal: terminated".to_string(),
                        _ => format!("signal: {}", sig),
                    };
                    result.exit_status = -1;
                    vlog!("[verbose] run_cmd: pid={} {}", pid, result.error);
                } else {
                    result.error = format!("exit status {}", result.exit_status);
                    vlog!("[verbose] run_cmd: pid={} {}", pid, result.error);
                }
            }
        }
        Err(e) => {
            result.error = e.to_string();
            vlog!("[verbose] run_cmd: pid={} wait error: {}", pid, e);
        }
    }

    if let Ok((data, truncated)) = stdout_capture {
        result.stdout = bytes_to_string(&data, truncated, head);
    }
    if let Ok((data, truncated)) = stderr_capture {
        result.stderr = bytes_to_string(&data, truncated, head);
    }

    let end = SystemTime::now();
    result.end_time = format_rfc3339_nano(end);
    result.duration = format_duration(end.duration_since(start_time).unwrap_or_default());
    result
}

fn bounded_read(mut reader: impl Read, limit: usize, head: bool) -> io::Result<(Vec<u8>, bool)> {
    let mut tmp = [0u8; 8192];

    if head {
        let mut buf = Vec::with_capacity(limit);
        loop {
            let n = reader.read(&mut tmp)?;
            if n == 0 {
                break;
            }
            let remaining = limit.saturating_sub(buf.len());
            if remaining > 0 {
                let take = std::cmp::min(n, remaining);
                buf.extend_from_slice(&tmp[..take]);
            }
            if buf.len() >= limit {
                loop {
                    let n = reader.read(&mut tmp)?;
                    if n == 0 {
                        break;
                    }
                }
                return Ok((buf, true));
            }
        }
        Ok((buf, false))
    } else {
        let mut buf = Vec::new();
        let mut total_read = 0usize;
        loop {
            let n = reader.read(&mut tmp)?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            total_read += n;
            if buf.len() > limit * 2 {
                let excess = buf.len() - limit;
                buf.drain(..excess);
            }
        }
        let truncated = total_read > limit;
        if buf.len() > limit {
            let excess = buf.len() - limit;
            buf.drain(..excess);
        }
        Ok((buf, truncated))
    }
}

fn bytes_to_string(data: &[u8], truncated: bool, head: bool) -> String {
    let s = if head {
        let end = match std::str::from_utf8(data) {
            Ok(_) => data.len(),
            Err(e) => e.valid_up_to(),
        };
        String::from_utf8_lossy(&data[..end]).to_string()
    } else {
        let start = utf8_skip_continuation(data);
        String::from_utf8_lossy(&data[start..]).to_string()
    };

    if truncated {
        if head {
            format!("{}[...]", s)
        } else {
            format!("[...]{}", s)
        }
    } else {
        s
    }
}

fn utf8_skip_continuation(data: &[u8]) -> usize {
    for (i, &b) in data.iter().enumerate() {
        if b < 0x80 || (b & 0xC0) != 0x80 {
            return i;
        }
    }
    data.len()
}

unsafe fn syscall_setpgid(pid: i32, pgid: i32) -> i32 {
    extern "C" {
        fn setpgid(pid: i32, pgid: i32) -> i32;
    }
    setpgid(pid, pgid)
}

unsafe fn syscall_kill(pid: i32, sig: i32) -> i32 {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    kill(pid, sig)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes_to_string() {
        assert_eq!(bytes_to_string(b"hello", false, false), "hello");
        assert_eq!(bytes_to_string(b"hello", true, true), "hello[...]");
        assert_eq!(bytes_to_string(b"world", true, false), "[...]world");
        assert_eq!(bytes_to_string(b"", false, false), "");
    }

    #[test]
    fn test_bytes_to_string_utf8_boundary() {
        let data = &"café".as_bytes()[..4];
        let result = bytes_to_string(data, true, true);
        assert_eq!(result, "caf[...]");

        let data = &"café".as_bytes()[1..];
        let result = bytes_to_string(data, true, false);
        assert_eq!(result, "[...]afé");
    }

    #[test]
    fn test_bounded_read_head() {
        let data = b"hello world, this is a test";
        let (buf, truncated) = bounded_read(&data[..], 5, true).unwrap();
        assert_eq!(&buf, b"hello");
        assert!(truncated);
    }

    #[test]
    fn test_bounded_read_tail() {
        let data = b"hello world";
        let (buf, truncated) = bounded_read(&data[..], 5, false).unwrap();
        assert_eq!(&buf, b"world");
        assert!(truncated);
    }

    #[test]
    fn test_bounded_read_no_truncation() {
        let data = b"hi";
        let (buf, truncated) = bounded_read(&data[..], 10, false).unwrap();
        assert_eq!(&buf, b"hi");
        assert!(!truncated);
    }

    #[test]
    fn test_spec_validate() {
        assert!(Spec {
            command: String::new(),
            args: vec![],
            timeout: Duration::ZERO,
            parallel: 0,
            stdout_bytes: 0,
            stderr_bytes: 0,
            head: false,
        }
        .validate()
        .is_err());

        assert!(Spec {
            command: "a".to_string(),
            args: vec![],
            timeout: Duration::from_secs(1),
            parallel: 1,
            stdout_bytes: 1,
            stderr_bytes: 1,
            head: false,
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn test_exec() {
        let spec = Spec {
            command: "echo".to_string(),
            args: vec!["{}".to_string()],
            timeout: Duration::from_secs(1),
            parallel: 1,
            stdout_bytes: 5,
            stderr_bytes: 1024,
            head: true,
        };

        let (target_tx, target_rx) = channel::bounded(1);
        let (result_tx, result_rx) = channel::bounded(10);

        target_tx.send("hello world".to_string()).unwrap();
        drop(target_tx);

        let shutdown = Arc::new(AtomicBool::new(false));
        exec(&spec, target_rx, result_tx, shutdown).unwrap();

        let result = result_rx.recv().unwrap();
        assert_eq!(result.target, "hello world");
        assert_eq!(result.stdout, "hello[...]");
        assert_eq!(result.exit_status, 0);

        // Verify it serializes to valid JSON
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"target\":\"hello world\""));
    }
}
