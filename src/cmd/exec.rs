use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ush::strutil::StringSet;
use ush::time::parse_duration;
use ush::{exec, jump_exec, read_targets, ExecResult, JumpSpec, Spec, DEFAULT_JUMP_COMMAND};

pub(crate) struct ExecArgs {
    pub(crate) timeout: Duration,
    pub(crate) parallel: usize,
    pub(crate) stdout_bytes: usize,
    pub(crate) stderr_bytes: usize,
    pub(crate) head: bool,
    pub(crate) exclude_file: Option<String>,
    pub(crate) jump_hosts_file: Option<String>,
    pub(crate) jump_key: Option<String>,
    pub(crate) jump_cmd: String,
    pub(crate) command: Vec<String>,
}

impl Default for ExecArgs {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(60),
            parallel: 1,
            stdout_bytes: 4096,
            stderr_bytes: 4096,
            head: false,
            exclude_file: None,
            jump_hosts_file: None,
            jump_key: None,
            jump_cmd: DEFAULT_JUMP_COMMAND.to_string(),
            command: Vec::new(),
        }
    }
}

pub(crate) fn parse_args(args: &[String]) -> Result<ExecArgs, String> {
    let mut result = ExecArgs::default();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        if arg == "--" {
            result.command = args[i + 1..].to_vec();
            break;
        }

        if let Some(val) = parse_flag_value(arg, "--timeout", "-t", args, &mut i)? {
            result.timeout =
                parse_duration(&val).ok_or_else(|| format!("invalid duration: {}", val))?;
        } else if let Some(val) = parse_flag_value(arg, "--parallel", "-p", args, &mut i)? {
            result.parallel = val
                .parse()
                .map_err(|_| format!("invalid parallel: {}", val))?;
        } else if let Some(val) = parse_flag_value(arg, "--stdout_bytes", "", args, &mut i)? {
            result.stdout_bytes = val
                .parse()
                .map_err(|_| format!("invalid stdout_bytes: {}", val))?;
        } else if let Some(val) = parse_flag_value(arg, "--stderr_bytes", "", args, &mut i)? {
            result.stderr_bytes = val
                .parse()
                .map_err(|_| format!("invalid stderr_bytes: {}", val))?;
        } else if let Some(val) = parse_flag_value(arg, "--exclude", "-e", args, &mut i)? {
            result.exclude_file = Some(val);
        } else if let Some(val) = parse_flag_value(arg, "--jump_hosts", "-j", args, &mut i)? {
            result.jump_hosts_file = Some(val);
        } else if let Some(val) = parse_flag_value(arg, "--jump_key", "-k", args, &mut i)? {
            result.jump_key = Some(val);
        } else if let Some(val) = parse_flag_value(arg, "--jump_cmd", "", args, &mut i)? {
            result.jump_cmd = val;
        } else if arg == "--head" {
            result.head = true;
        } else {
            result.command = args[i..].to_vec();
            break;
        }

        i += 1;
    }

    if result.command.is_empty() {
        return Err("exec requires a command".to_string());
    }

    Ok(result)
}

fn parse_flag_value(
    arg: &str,
    long: &str,
    short: &str,
    args: &[String],
    i: &mut usize,
) -> Result<Option<String>, String> {
    if !long.is_empty() {
        if let Some(val) = arg.strip_prefix(&format!("{}=", long)) {
            return Ok(Some(val.to_string()));
        }
        if arg == long {
            *i += 1;
            if *i >= args.len() {
                return Err(format!("missing value for {}", long));
            }
            return Ok(Some(args[*i].clone()));
        }
    }

    if !short.is_empty() && arg == short {
        *i += 1;
        if *i >= args.len() {
            return Err(format!("missing value for {}", short));
        }
        return Ok(Some(args[*i].clone()));
    }

    Ok(None)
}

/// Install SIGINT/SIGTERM handler that sets the given shutdown flag.
fn install_signal_handler(shutdown: &Arc<AtomicBool>) {
    // Store a raw pointer to the AtomicBool for the signal handler.
    // Safe because the AtomicBool lives in an Arc that outlives the process.
    static mut SHUTDOWN_PTR: *const AtomicBool = std::ptr::null();

    unsafe {
        SHUTDOWN_PTR = Arc::as_ptr(shutdown);

        extern "C" {
            fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
        }
        extern "C" fn handler(_sig: i32) {
            unsafe {
                if !SHUTDOWN_PTR.is_null() {
                    (*SHUTDOWN_PTR).store(true, Ordering::Relaxed);
                }
            }
        }
        signal(2, handler); // SIGINT
        signal(15, handler); // SIGTERM
    }
}

pub(crate) fn run(args: &ExecArgs) -> Result<(), Box<dyn std::error::Error>> {
    let shutdown = Arc::new(AtomicBool::new(false));
    install_signal_handler(&shutdown);

    let exclude = args
        .exclude_file
        .as_deref()
        .map(|f| StringSet::from_file(Path::new(f)))
        .transpose()?;

    let command = args.command[0].clone();
    let cmd_args: Vec<String> = args.command[1..].to_vec();

    let spec = Spec {
        command,
        args: cmd_args,
        timeout: args.timeout,
        parallel: args.parallel,
        stdout_bytes: args.stdout_bytes,
        stderr_bytes: args.stderr_bytes,
        head: args.head,
    };

    let targets = read_targets(io::stdin(), exclude.clone());
    let (result_tx, result_rx) = crossbeam_channel::bounded::<ExecResult>(1024);

    // Spawn writer thread: receives results, serializes to JSON, prints to stdout
    let writer_handle = std::thread::spawn(move || {
        let stdout = io::stdout();
        for result in result_rx {
            if let Ok(json) = serde_json::to_string(&result) {
                let mut out = stdout.lock();
                let _ = writeln!(out, "{}", json);
            }
        }
    });

    if let Some(ref jump_hosts_file) = args.jump_hosts_file {
        let mut hosts = StringSet::from_file(Path::new(jump_hosts_file))?;
        if let Some(ref exc) = exclude {
            for h in exc.sorted_strings() {
                hosts.remove(&h);
            }
        }

        let jump_spec = JumpSpec {
            spec,
            jump_hosts_key_file: args.jump_key.clone().unwrap_or_default(),
            jump_command: args.jump_cmd.clone(),
            jump_hosts: hosts.sorted_strings(),
        };

        jump_exec(&jump_spec, targets, result_tx, shutdown)?;
    } else {
        exec(&spec, targets, result_tx, shutdown)?;
    }

    let _ = writer_handle.join();
    Ok(())
}

use std::io::Write;
