mod cmd;

use clap::{Parser, Subcommand};
use std::env;
use std::process;

#[derive(Parser)]
#[command(name = "ush", version = env!("CARGO_PKG_VERSION"), about = "Parallel command execution")]
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
    /// Upgrade ush to the latest release
    Upgrade(cmd::update::UpgradeArgs),
    /// Dump an agent skill document for using ush
    DumpSkill,
}

fn main() {
    // Hidden subcommand: detached background update check.
    if env::args().skip(1).any(|a| a == "__update-check") {
        if let Err(e) = ush::update::run_background_check() {
            eprintln!("update-check: {e}");
            process::exit(1);
        }
        return;
    }

    let cli = Cli::parse();

    if cli.verbose {
        ush::verbose::set_verbose(true);
    }

    let skip_probe = matches!(cli.command, Command::Upgrade(_) | Command::DumpSkill);

    let result = match cli.command {
        Command::Exec(args) => cmd::exec::run(&args),
        Command::Freq(args) => cmd::freq::run(&args),
        Command::Upgrade(args) => cmd::update::run(&args),
        Command::DumpSkill => {
            cmd::dumpskill::run();
            return;
        }
    };

    if let Err(e) = result {
        eprintln!("{}", e);
        process::exit(1);
    }

    if !skip_probe {
        ush::update::maybe_spawn_background_check(cli.verbose);
    }
    ush::update::maybe_print_banner_from_env();
}
