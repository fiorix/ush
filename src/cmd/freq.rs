use std::io;

use clap::{Args, Subcommand};
use ush::freq as freq_mod;
use ush::time::parse_duration;

fn parse_duration_clap(s: &str) -> Result<std::time::Duration, String> {
    parse_duration(s).ok_or_else(|| format!("invalid duration: {s}"))
}

const EXAMPLES: &str = r"Examples:
  ush exec -- echo {} | ush freq stdout
  ush exec -- echo {} | ush freq stdout --json
  ush exec -- true | ush freq exitstatus
  ush exec -- sleep {} | ush freq duration 1s
";

#[derive(Args)]
#[command(after_help = EXAMPLES)]
pub(crate) struct FreqArgs {
    #[command(subcommand)]
    pub(crate) command: FreqCommand,
}

#[derive(Subcommand)]
pub(crate) enum FreqCommand {
    /// Print frequency of similar stdout
    #[command(after_help = EXAMPLES)]
    Stdout {
        /// Encode output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Print frequency of similar stderr
    #[command(after_help = EXAMPLES)]
    Stderr {
        /// Encode output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Print frequency of similar exit status
    #[command(after_help = EXAMPLES)]
    Exitstatus {
        /// Encode output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Print execution duration distribution
    #[command(after_help = EXAMPLES)]
    Duration {
        /// Duration bucket size (e.g., 5s, 1m)
        #[arg(value_parser = parse_duration_clap)]
        value: std::time::Duration,
        /// Encode output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn run(args: &FreqArgs) -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();

    match &args.command {
        FreqCommand::Stdout { json } => {
            let items = freq_mod::stdout(stdin.lock())?;
            encode_items(&items, *json)?;
        }
        FreqCommand::Stderr { json } => {
            let items = freq_mod::stderr(stdin.lock())?;
            encode_items(&items, *json)?;
        }
        FreqCommand::Exitstatus { json } => {
            let items = freq_mod::exit_status(stdin.lock())?;
            encode_items(&items, *json)?;
        }
        FreqCommand::Duration { json, value } => {
            let items = freq_mod::duration(stdin.lock(), *value)?;
            encode_items(&items, *json)?;
        }
    }

    Ok(())
}

fn encode_items(items: &[freq_mod::Item], as_json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = io::stdout();
    if as_json {
        freq_mod::encode_json(&mut stdout, items)?;
    } else {
        freq_mod::encode_wide(&mut stdout, items)?;
    }
    Ok(())
}
