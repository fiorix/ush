use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::exec::jumpexec::{self, JumpSpec, DEFAULT_JUMP_COMMAND};
use crate::exec::{self as exec_mod, Spec};
use crate::strutil::StringSet;
use crate::time::parse_go_duration;

pub struct ExecArgs {
    pub timeout: Duration,
    pub parallel: usize,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
    pub head: bool,
    pub exclude_file: Option<String>,
    pub jump_hosts_file: Option<String>,
    pub jump_key: Option<String>,
    pub jump_cmd: String,
    pub command: Vec<String>,
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

pub fn parse_args(args: &[String]) -> Result<ExecArgs, String> {
    let mut result = ExecArgs::default();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        // Check for -- separator
        if arg == "--" {
            result.command = args[i + 1..].to_vec();
            break;
        }

        if let Some(val) = parse_flag_value(arg, "--timeout", "-t", args, &mut i)? {
            result.timeout =
                parse_go_duration(&val).ok_or_else(|| format!("invalid duration: {}", val))?;
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
            // First non-flag argument starts the command
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

/// Parse a flag value. Supports --flag=value, --flag value, -f value formats.
fn parse_flag_value(
    arg: &str,
    long: &str,
    short: &str,
    args: &[String],
    i: &mut usize,
) -> Result<Option<String>, String> {
    // --flag=value
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

    // -f value
    if !short.is_empty() && arg == short {
        *i += 1;
        if *i >= args.len() {
            return Err(format!("missing value for {}", short));
        }
        return Ok(Some(args[*i].clone()));
    }

    Ok(None)
}

pub fn run(args: &ExecArgs) -> Result<(), Box<dyn std::error::Error>> {
    exec_mod::install_signal_handler();

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

    let targets = exec_mod::read_targets(io::stdin(), exclude.clone());

    let w: Arc<Mutex<Box<dyn io::Write + Send>>> = Arc::new(Mutex::new(Box::new(io::stdout())));

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

        jumpexec::jump_exec(w, &jump_spec, targets)?;
    } else {
        exec_mod::exec(w, &spec, targets)?;
    }

    Ok(())
}
