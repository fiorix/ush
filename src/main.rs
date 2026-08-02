mod cmd;

use clap::{Parser, Subcommand};
use std::process;

#[derive(Parser)]
#[command(name = "ush", version = "v2.0.0", about = "Parallel command execution")]
struct Cli {
    /// Enable verbose diagnostic output to stderr
    #[arg(short = 'v', long)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Execute parallel commands from standard input
    Exec(cmd::exec::ExecArgs),
    /// Print frequency of events from exec JSON output
    Freq(cmd::freq::FreqArgs),
    /// Dump an agent skill document for using ush
    DumpSkill,
}

fn main() {
    let cli = Cli::parse();

    if cli.verbose {
        ush::verbose::set_verbose(true);
    }

    let result = match cli.command {
        Command::Exec(args) => cmd::exec::run(&args),
        Command::Freq(args) => cmd::freq::run(&args),
        Command::DumpSkill => {
            cmd::dumpskill::run();
            return;
        }
    };

    if let Err(e) = result {
        eprintln!("{}", e);
        process::exit(1);
    }
}
