use std::io;

use ush::freq as freq_mod;
use ush::time::parse_duration;

pub(crate) struct FreqArgs {
    pub(crate) command: FreqCommand,
}

pub(crate) enum FreqCommand {
    Stdout {
        json: bool,
    },
    Stderr {
        json: bool,
    },
    Exitstatus {
        json: bool,
    },
    Duration {
        json: bool,
        value: std::time::Duration,
    },
}

pub(crate) fn parse_args(args: &[String]) -> Result<FreqArgs, String> {
    if args.is_empty() {
        return Err("freq requires a subcommand: stdout, stderr, exitstatus, duration".to_string());
    }

    let subcmd = &args[0];
    let sub_args = &args[1..];

    match subcmd.as_str() {
        "stdout" => {
            let json = has_json_flag(sub_args);
            Ok(FreqArgs {
                command: FreqCommand::Stdout { json },
            })
        }
        "stderr" => {
            let json = has_json_flag(sub_args);
            Ok(FreqArgs {
                command: FreqCommand::Stderr { json },
            })
        }
        "exitstatus" => {
            let json = has_json_flag(sub_args);
            Ok(FreqArgs {
                command: FreqCommand::Exitstatus { json },
            })
        }
        "duration" => {
            let json = has_json_flag(sub_args);
            let value_str = sub_args
                .iter()
                .find(|a| *a != "--json")
                .ok_or_else(|| "duration requires a value argument, e.g. 5s".to_string())?;
            let value = parse_duration(value_str)
                .ok_or_else(|| format!("invalid duration: {}", value_str))?;
            Ok(FreqArgs {
                command: FreqCommand::Duration { json, value },
            })
        }
        _ => Err(format!("unknown freq subcommand: {}", subcmd)),
    }
}

fn has_json_flag(args: &[String]) -> bool {
    args.iter().any(|a| a == "--json")
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
