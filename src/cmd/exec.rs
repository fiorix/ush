use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::{Args, ValueEnum};
use ush::codec::Format;
use ush::strutil::StringSet;
use ush::time::parse_duration;
use ush::{exec, jump_exec, read_targets, FrameEncoder, JumpSpec, Output, Spec, DEFAULT_JUMP_COMMAND};

fn parse_duration_clap(s: &str) -> Result<Duration, String> {
    parse_duration(s).ok_or_else(|| format!("invalid duration: {s}"))
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub(crate) enum OutputFormat {
    #[default]
    Json,
    Msgpack,
}

const EXAMPLES: &str = r"Examples:
  echo -ne 'host1\nhost2\n' | ush exec -- echo {}
  cat hosts.txt | ush exec -p 4 -- ssh user@{} -- hostid
  cat hosts.txt | ush exec -j jumps.txt -k jump.key -- ssh user@{} -- hostid
  printf 'test\n' | ush exec -t 2s --stdout_bytes=5 -- false
";

#[derive(Args)]
#[command(after_help = EXAMPLES)]
pub(crate) struct ExecArgs {
    /// Timeout of each command execution [default: 1m]
    #[arg(short = 't', long, default_value = "1m", value_parser = parse_duration_clap)]
    pub(crate) timeout: Duration,

    /// Number of parallel commands to execute
    #[arg(short = 'p', long, default_value_t = 1)]
    pub(crate) parallel: usize,

    /// Number of bytes to read from command's stdout
    #[arg(long = "stdout_bytes", default_value_t = 4096)]
    pub(crate) stdout_bytes: usize,

    /// Number of bytes to read from command's stderr
    #[arg(long = "stderr_bytes", default_value_t = 4096)]
    pub(crate) stderr_bytes: usize,

    /// Capture first N bytes instead of last N bytes
    #[arg(long)]
    pub(crate) head: bool,

    /// Output format
    #[arg(long = "format", value_enum, default_value_t = OutputFormat::Json)]
    pub(crate) format: OutputFormat,

    /// Max bytes per chunk frame
    #[arg(long = "chunk_size", default_value_t = 4096)]
    pub(crate) chunk_size: usize,

    /// Emit legacy one-line-per-target output with no chunk frames
    #[arg(long)]
    pub(crate) batch: bool,

    /// File containing target exclusion list
    #[arg(short = 'e', long = "exclude")]
    pub(crate) exclude_file: Option<String>,

    /// File containing jump hosts
    #[arg(short = 'j', long = "jump_hosts")]
    pub(crate) jump_hosts_file: Option<String>,

    /// SSH key file for jump hosts
    #[arg(short = 'k', long = "jump_key")]
    pub(crate) jump_key: Option<String>,

    /// Jump command template. The value is split on whitespace; quoted tokens are not preserved.
    #[arg(long = "jump_cmd", default_value = DEFAULT_JUMP_COMMAND)]
    pub(crate) jump_cmd: String,

    /// Command to execute (after --)
    #[arg(last = true, required = true)]
    pub(crate) command: Vec<String>,
}

/// Install SIGINT/SIGTERM handler that sets the given shutdown flag.
/// Workers check the flag before starting a new target. A target that is already running is allowed to finish (or time out).
fn install_signal_handler(shutdown: &Arc<AtomicBool>) {
    // Store the AtomicBool pointer in an AtomicPtr so signal delivery is synchronized with the store.
    static SHUTDOWN_PTR: AtomicPtr<AtomicBool> = AtomicPtr::new(std::ptr::null_mut());

    SHUTDOWN_PTR.store(Arc::as_ptr(shutdown) as *mut _, Ordering::Release);

    extern "C" {
        fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    }
    extern "C" fn handler(_sig: i32) {
        let ptr = SHUTDOWN_PTR.load(Ordering::Acquire);
        if !ptr.is_null() {
            unsafe {
                (*ptr).store(true, Ordering::Relaxed);
            }
        }
    }
    unsafe {
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
    let (result_tx, result_rx) = crossbeam_channel::bounded::<Output>(1024);

    let format = match args.format {
        OutputFormat::Json => Format::Json,
        OutputFormat::Msgpack => Format::Msgpack,
    };

    // Spawn writer thread: receives frames or legacy results and encodes them to stdout.
    let writer_handle = std::thread::spawn(move || {
        let stdout = io::stdout();
        let mut encoder = FrameEncoder::new(stdout.lock(), format);
        for output in result_rx {
            let res = match output {
                Output::Frame(frame) => encoder.write_frame(&frame),
                Output::Legacy(result) => encoder.write_legacy(&result),
            };
            if res.is_err() {
                break;
            }
        }
        let _ = encoder.flush();
    });

    if let Some(ref jump_hosts_file) = args.jump_hosts_file {
        if args.batch {
            return Err("--batch is not supported with jump hosts".into());
        }

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
            chunk_size: args.chunk_size,
        };

        jump_exec(&jump_spec, targets, result_tx, shutdown)?;
    } else {
        exec(
            &spec,
            targets,
            result_tx,
            shutdown,
            args.chunk_size,
            args.batch,
        )?;
    }

    let _ = writer_handle.join();
    Ok(())
}
