pub mod jumpexec;

use std::io::{self, BufRead, Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};

use crate::json::escape_json_string;
use crate::strutil::StringSet;
use crate::time::{format_go_duration, format_rfc3339_nano};

/// Global shutdown flag set by signal handler.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

pub fn is_shutdown() -> bool {
    SHUTDOWN.load(Ordering::SeqCst)
}

/// Install SIGINT/SIGTERM handler that sets the shutdown flag.
pub fn install_signal_handler() {
    unsafe {
        extern "C" {
            fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
        }
        extern "C" fn handler(_sig: i32) {
            SHUTDOWN.store(true, Ordering::SeqCst);
        }
        signal(2, handler); // SIGINT
        signal(15, handler); // SIGTERM
    }
    vlog!("[verbose] signal handler installed for SIGINT/SIGTERM");
}

#[derive(Debug)]
pub enum SpecError {
    NoCommand,
    NoTimeout,
    NoParallel,
    NoStdoutBytes,
    NoStderrBytes,
}

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpecError::NoCommand => write!(f, "command not set"),
            SpecError::NoTimeout => write!(f, "timeout must be greater than zero"),
            SpecError::NoParallel => write!(f, "parallel must be greater than zero"),
            SpecError::NoStdoutBytes => write!(f, "stdout_bytes must be greater than zero"),
            SpecError::NoStderrBytes => write!(f, "stderr_bytes must be greater than zero"),
        }
    }
}

impl std::error::Error for SpecError {}

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
            return Err(SpecError::NoCommand);
        }
        if self.timeout.is_zero() {
            return Err(SpecError::NoTimeout);
        }
        if self.parallel == 0 {
            return Err(SpecError::NoParallel);
        }
        if self.stdout_bytes == 0 {
            return Err(SpecError::NoStdoutBytes);
        }
        if self.stderr_bytes == 0 {
            return Err(SpecError::NoStderrBytes);
        }
        Ok(())
    }
}

pub struct ExecResult {
    pub target: String,
    pub duration: String,
    pub start_time: SystemTime,
    pub end_time: SystemTime,
    pub exit_status: i32,
    pub stdout: String,
    pub stderr: String,
    pub error: String,
}

impl ExecResult {
    pub fn to_json(&self) -> String {
        format!(
            "{{\"target\":{},\"duration\":{},\"start_time\":{},\"end_time\":{},\"exit_status\":{},\"stdout\":{},\"stderr\":{},\"error\":{}}}",
            escape_json_string(&self.target),
            escape_json_string(&self.duration),
            escape_json_string(&format_rfc3339_nano(self.start_time)),
            escape_json_string(&format_rfc3339_nano(self.end_time)),
            self.exit_status,
            escape_json_string(&self.stdout),
            escape_json_string(&self.stderr),
            escape_json_string(&self.error),
        )
    }
}

/// Reads targets from stdin, skipping empty lines, comments, and excluded targets.
/// Uses a bounded channel to avoid unbounded memory growth with large target lists.
pub fn read_targets(
    reader: impl io::Read + Send + 'static,
    exclude: Option<StringSet>,
) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::sync_channel(1024);

    std::thread::spawn(move || {
        let buf = io::BufReader::new(reader);
        for line in buf.lines() {
            if is_shutdown() {
                vlog!("[verbose] target reader: shutdown signal received, stopping");
                break;
            }
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

/// Executes commands in parallel, writing JSON results to writer.
pub fn exec(
    w: Arc<Mutex<Box<dyn Write + Send>>>,
    spec: &Spec,
    input: mpsc::Receiver<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    spec.validate()?;

    let input = Arc::new(Mutex::new(input));
    let mut handles = Vec::new();

    vlog!("[verbose] exec: starting {} worker(s), timeout={}, command={}", spec.parallel, format_go_duration(spec.timeout), spec.command);

    for _ in 0..spec.parallel {
        let input = input.clone();
        let w = w.clone();
        let command = spec.command.clone();
        let args = spec.args.clone();
        let timeout = spec.timeout;
        let stdout_bytes = spec.stdout_bytes;
        let stderr_bytes = spec.stderr_bytes;
        let head = spec.head;

        let handle = std::thread::spawn(move || {
            loop {
                if is_shutdown() {
                    vlog!("[verbose] worker: shutdown signal received, stopping");
                    break;
                }
                let target = {
                    let rx = input.lock().unwrap();
                    rx.recv().ok()
                };
                match target {
                    Some(target) => {
                        vlog!("[verbose] worker: executing target {}", target);
                        let result =
                            run_cmd(&command, &args, &target, timeout, stdout_bytes, stderr_bytes, head);
                        let json = result.to_json();
                        let mut w = w.lock().unwrap();
                        let _ = writeln!(w, "{}", json);
                    }
                    None => break,
                }
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.join();
    }
    Ok(())
}

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
        start_time,
        end_time: start_time,
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
            // setpgid(0, 0) — put child in its own process group
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
            result.end_time = SystemTime::now();
            result.duration = format_go_duration(result.end_time.duration_since(result.start_time).unwrap_or_default());
            return result;
        }
    };

    let pid = child.id() as i32;
    vlog!("[verbose] run_cmd: spawned pid={} pgid={} cmd={}", pid, pid, cmd_str);

    // Take ownership of stdout/stderr pipes for bounded capture
    let stdout_pipe = child.stdout.take().unwrap();
    let stderr_pipe = child.stderr.take().unwrap();

    let stdout_lim = stdout_limit;
    let stderr_lim = stderr_limit;
    let head_mode = head;

    let stdout_thread = std::thread::spawn(move || {
        bounded_read(stdout_pipe, stdout_lim, head_mode)
    });
    let stderr_thread = std::thread::spawn(move || {
        bounded_read(stderr_pipe, stderr_lim, head_mode)
    });

    // Condvar-based timeout: supports early cancellation and SIGTERM-before-SIGKILL
    let done = Arc::new((Mutex::new(false), Condvar::new()));
    let done2 = done.clone();

    let _timeout_handle = std::thread::spawn(move || {
        let (lock, cvar) = &*done2;

        // Wait for timeout or early finish
        let guard = lock.lock().unwrap();
        let (guard, _) = cvar.wait_timeout(guard, timeout).unwrap();
        if *guard {
            return; // Command finished before timeout
        }
        drop(guard);

        // Timeout reached: send SIGTERM first
        vlog!("[verbose] run_cmd: timeout reached for pid={}, sending SIGTERM", pid);
        unsafe { syscall_kill(-pid, 15) }; // SIGTERM

        // Grace period for clean shutdown
        let grace = Duration::from_secs(5);
        let guard = lock.lock().unwrap();
        let (guard, _) = cvar.wait_timeout(guard, grace).unwrap();
        if *guard {
            vlog!("[verbose] run_cmd: pid={} exited after SIGTERM", pid);
            return; // Process exited after SIGTERM
        }
        drop(guard);

        // Still alive: escalate to SIGKILL
        vlog!("[verbose] run_cmd: grace period expired for pid={}, sending SIGKILL", pid);
        unsafe { syscall_kill(-pid, 9) }; // SIGKILL
    });

    let wait_result = child.wait();

    // Signal the timeout thread that we're done (cancels sleep or prevents kill)
    {
        let (lock, cvar) = &*done;
        let mut finished = lock.lock().unwrap();
        *finished = true;
        cvar.notify_one();
    }

    // Collect bounded output from reader threads
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

    match stdout_capture {
        Ok((data, truncated)) => {
            result.stdout = bytes_to_string(&data, truncated, head);
        }
        Err(e) => {
            vlog!("[verbose] run_cmd: pid={} stdout read error: {}", pid, e);
        }
    }
    match stderr_capture {
        Ok((data, truncated)) => {
            result.stderr = bytes_to_string(&data, truncated, head);
        }
        Err(e) => {
            vlog!("[verbose] run_cmd: pid={} stderr read error: {}", pid, e);
        }
    }

    result.end_time = SystemTime::now();
    result.duration = format_go_duration(result.end_time.duration_since(result.start_time).unwrap_or_default());
    result
}

/// Read from a pipe with bounded memory usage.
/// Returns (data, truncated) where truncated indicates output exceeded limit.
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
                // Drain remaining output to avoid blocking the child
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
        // Tail mode: keep last `limit` bytes
        let mut buf = Vec::new();
        let mut total_read = 0usize;
        loop {
            let n = reader.read(&mut tmp)?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            total_read += n;
            // Trim periodically to bound memory
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

/// Convert captured bytes to a string, respecting UTF-8 boundaries.
fn bytes_to_string(data: &[u8], truncated: bool, head: bool) -> String {
    let s = if head {
        // Truncate at last valid UTF-8 boundary
        let end = match std::str::from_utf8(data) {
            Ok(_) => data.len(),
            Err(e) => e.valid_up_to(),
        };
        String::from_utf8_lossy(&data[..end]).to_string()
    } else {
        // Skip past partial UTF-8 continuation bytes at start
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

/// Find the first byte that is not a UTF-8 continuation byte (0x80..0xBF).
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
fn lossy_capture(data: &[u8], limit: usize, head: bool) -> String {
    if data.len() <= limit {
        String::from_utf8_lossy(data).to_string()
    } else if head {
        let end = match std::str::from_utf8(&data[..limit]) {
            Ok(_) => limit,
            Err(e) => e.valid_up_to(),
        };
        let truncated = String::from_utf8_lossy(&data[..end]);
        format!("{}[...]", truncated)
    } else {
        let start_offset = data.len() - limit;
        let start = start_offset + utf8_skip_continuation(&data[start_offset..]);
        let truncated = String::from_utf8_lossy(&data[start..]);
        format!("[...]{}", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_go_duration() {
        assert_eq!(format_go_duration(Duration::from_secs(0)), "0s");
        assert_eq!(format_go_duration(Duration::from_secs(1)), "1s");
        assert_eq!(format_go_duration(Duration::from_millis(100)), "100ms");
        assert_eq!(format_go_duration(Duration::from_millis(1500)), "1.5s");
        assert_eq!(
            format_go_duration(Duration::from_nanos(1_234_567)),
            "1.234567ms"
        );
        assert_eq!(format_go_duration(Duration::from_secs(90)), "1m30s");
        assert_eq!(format_go_duration(Duration::from_secs(3661)), "1h1m1s");
        assert_eq!(format_go_duration(Duration::from_micros(1)), "1µs");
        assert_eq!(format_go_duration(Duration::from_nanos(1)), "1ns");
        assert_eq!(format_go_duration(Duration::from_nanos(1500)), "1.5µs");
    }

    #[test]
    fn test_lossy_capture() {
        assert_eq!(lossy_capture(b"hello", 10, false), "hello");
        assert_eq!(lossy_capture(b"hello world", 5, true), "hello[...]");
        assert_eq!(lossy_capture(b"hello world", 5, false), "[...]world");
    }

    #[test]
    fn test_lossy_capture_utf8_boundary() {
        // "café" in UTF-8: 63 61 66 c3 a9
        let data = "café".as_bytes();
        // Truncate at 4 bytes: splits the 'é' (c3 a9)
        let result = lossy_capture(data, 4, true);
        // Should back up to before the partial UTF-8 char
        assert_eq!(result, "caf[...]");

        // Tail mode: skip continuation byte at start
        let result = lossy_capture(data, 4, false);
        // Should skip the orphaned continuation byte
        assert!(result.starts_with("[...]"));
        assert!(result.contains("afé") || result.contains("café"));
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
        use std::sync::Arc;

        let spec = Spec {
            command: "echo".to_string(),
            args: vec!["{}".to_string()],
            timeout: Duration::from_secs(1),
            parallel: 1,
            stdout_bytes: 5,
            stderr_bytes: 1024,
            head: true,
        };

        let (tx, rx) = mpsc::channel();
        tx.send("hello world".to_string()).unwrap();
        drop(tx);

        let shared_buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let shared_buf2 = shared_buf.clone();

        let w: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(SharedVecWriter(shared_buf2))));

        exec(w, &spec, rx).unwrap();

        let output = shared_buf.lock().unwrap();
        let output_str = String::from_utf8(output.clone()).unwrap();
        let line = output_str.trim();

        assert!(line.contains("\"target\":\"hello world\""), "got: {}", line);
        assert!(line.contains("\"stdout\":\"hello[...]\""), "got: {}", line);
        assert!(line.contains("\"exit_status\":0"), "got: {}", line);
    }

    struct SharedVecWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedVecWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
