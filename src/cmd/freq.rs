use std::io;

use clap::{Args, Subcommand};
use ush::freq as freq_mod;
use ush::time::parse_duration;

fn parse_duration_clap(s: &str) -> Result<std::time::Duration, String> {
    parse_duration(s).ok_or_else(|| format!("invalid duration: {s}"))
}

#[derive(Args)]
pub(crate) struct FreqArgs {
    #[command(subcommand)]
    pub(crate) command: FreqCommand,
}

#[derive(Subcommand)]
pub(crate) enum FreqCommand {
    /// Print frequency of similar stdout
    Stdout {
        /// Encode output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Print frequency of similar stderr
    Stderr {
        /// Encode output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Print frequency of similar exit status
    Exitstatus {
        /// Encode output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Print execution duration distribution
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
