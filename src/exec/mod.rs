pub mod jumpexec;

use std::io::{self, BufRead, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime};

use crate::json::escape_json_string;
use crate::strutil::StringSet;
use crate::time::{format_go_duration, format_rfc3339_nano};

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
pub fn read_targets(
    reader: impl io::Read + Send + 'static,
    exclude: Option<StringSet>,
) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let buf = io::BufReader::new(reader);
        for line in buf.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(ref exc) = exclude {
                if exc.contains(&line) {
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

    for _ in 0..spec.parallel {
        let input = input.clone();
        let w = w.clone();
        let command = spec.command.clone();
        let args = spec.args.clone();
        let timeout = spec.timeout;
        let stdout_bytes = spec.stdout_bytes;
        let stderr_bytes = spec.stderr_bytes;

        let handle = std::thread::spawn(move || {
            loop {
                let target = {
                    let rx = input.lock().unwrap();
                    rx.recv().ok()
                };
                match target {
                    Some(target) => {
                        let result =
                            run_cmd(&command, &args, &target, timeout, stdout_bytes, stderr_bytes);
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

    let child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            result.error = e.to_string();
            result.end_time = SystemTime::now();
            result.duration = format_go_duration(result.end_time.duration_since(result.start_time).unwrap_or_default());
            return result;
        }
    };

    let pid = child.id() as i32;
    let should_kill = Arc::new(AtomicBool::new(true));
    let should_kill2 = should_kill.clone();

    // Spawn timeout killer thread
    let _timeout_handle = std::thread::spawn(move || {
        std::thread::sleep(timeout);
        if should_kill2.load(Ordering::SeqCst) {
            // kill(-pgid, SIGKILL)
            unsafe { syscall_kill(-pid, 9) };
        }
    });

    let output = child.wait_with_output();
    should_kill.store(false, Ordering::SeqCst);
    // Don't join timeout thread — it will exit on its own

    match output {
        Ok(output) => {
            if !output.status.success() {
                result.exit_status = output.status.code().unwrap_or(-1);
                if let Some(sig) = output.status.signal() {
                    result.error = match sig {
                        9 => "signal: killed".to_string(),
                        _ => format!("signal: {}", sig),
                    };
                    result.exit_status = -1;
                } else {
                    result.error = format!("exit status {}", result.exit_status);
                }
            }
            result.stdout = lossy_capture(&output.stdout, stdout_limit);
            result.stderr = lossy_capture(&output.stderr, stderr_limit);
        }
        Err(e) => {
            result.error = e.to_string();
        }
    }

    result.end_time = SystemTime::now();
    result.duration = format_go_duration(result.end_time.duration_since(result.start_time).unwrap_or_default());
    result
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

fn lossy_capture(data: &[u8], limit: usize) -> String {
    if data.len() <= limit {
        String::from_utf8_lossy(data).to_string()
    } else {
        let truncated = String::from_utf8_lossy(&data[..limit]);
        format!("{}[...]", truncated)
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
        assert_eq!(lossy_capture(b"hello", 10), "hello");
        assert_eq!(lossy_capture(b"hello world", 5), "hello[...]");
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
