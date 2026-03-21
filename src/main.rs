mod cmd;

use std::env;
use std::process;

const VERSION: &str = "ush v1.2";

const USAGE: &str = "\
Usage: ush [flags] <command> [command-flags]

Commands:
  exec    Execute parallel commands from standard input
  freq    Print frequency of events from exec JSON output

Global flags:
  -v, --verbose    Enable verbose diagnostic output to stderr
      --version    Print version and exit

Use \"ush <command> --help\" for more information about a command.";

const EXEC_USAGE: &str = "\
Usage: ush exec [flags] -- <command> [args...]

Execute parallel commands from standard input.

ush works by consuming line-oriented data from stdin, and uses that information
to compose and execute commands from a templated command line.

Each line written to standard input is a target to ush. When you compose your
command, you refer to the target by using the {} template markup.

Example running 2 echo commands:

    echo -ne 'hello\\nworld\\n' | ush exec -- echo {}

Flags:
  -t, --timeout <duration>    timeout of each command execution (default: 1m)
  -p, --parallel <n>          number of parallel commands to execute (default: 1)
      --stdout_bytes <n>      number of bytes to read from command's stdout (default: 4096)
      --stderr_bytes <n>      number of bytes to read from command's stderr (default: 4096)
      --head                  capture first N bytes instead of last N bytes
  -e, --exclude <file>        file containing target exclusion list
  -j, --jump_hosts <file>     file containing jump hosts
  -k, --jump_key <file>       ssh key file for jump hosts
      --jump_cmd <cmd>        jump command template (default: ssh -A ...)";

const FREQ_USAGE: &str = "\
Usage: ush freq <subcommand> [flags]

Print frequency of events from exec JSON output.

Subcommands:
  stdout        Print frequency of similar stdout
  stderr        Print frequency of similar stderr
  exitstatus    Print frequency of similar exit status
  duration      Print execution duration distribution

Flags:
  --json    encode output as JSON

Example:
    echo -ne 'foo\\nbar\\n' | ush exec -- echo {} | ush freq stdout";

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("{}", USAGE);
        process::exit(1);
    }

    let mut sub_start = 0;
    for (i, arg) in args.iter().enumerate() {
        match arg.as_str() {
            "--verbose" | "-v" => {
                ush::verbose::set_verbose(true);
            }
            "--help" | "-h" | "help" => {
                println!("{}", USAGE);
                return;
            }
            "--version" => {
                println!("{}", VERSION);
                return;
            }
            _ => {
                sub_start = i;
                break;
            }
        }
        sub_start = i + 1;
    }

    if sub_start >= args.len() {
        eprintln!("{}", USAGE);
        process::exit(1);
    }

    match args[sub_start].as_str() {
        "exec" => {
            let sub_args = &args[sub_start + 1..];
            if sub_args.is_empty() || sub_args.iter().any(|a| a == "--help" || a == "-h") {
                eprintln!("{}", EXEC_USAGE);
                if sub_args.is_empty() {
                    process::exit(1);
                }
                return;
            }

            let exec_args = match cmd::exec::parse_args(sub_args) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(1);
                }
            };

            if let Err(e) = cmd::exec::run(&exec_args) {
                eprintln!("{}", e);
                process::exit(1);
            }
        }
        "freq" => {
            let sub_args = &args[sub_start + 1..];
            if sub_args.is_empty() || sub_args.iter().any(|a| a == "--help" || a == "-h") {
                eprintln!("{}", FREQ_USAGE);
                if sub_args.is_empty() {
                    process::exit(1);
                }
                return;
            }

            let freq_args = match cmd::freq::parse_args(sub_args) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(1);
                }
            };

            if let Err(e) = cmd::freq::run(&freq_args) {
                eprintln!("{}", e);
                process::exit(1);
            }
        }
        other => {
            eprintln!("Unknown command: {}\n{}", other, USAGE);
            process::exit(1);
        }
    }
}
