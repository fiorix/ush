use clap::Args;

const EXAMPLES: &str = r"Examples:
  ush upgrade
  ush upgrade --check
  ush upgrade -y
";

#[derive(Args)]
#[command(after_help = EXAMPLES)]
pub(crate) struct UpgradeArgs {
    /// Only check whether an update is available; do not install it.
    #[arg(long)]
    check: bool,

    /// Skip the confirmation prompt.
    #[arg(short = 'y', long)]
    yes: bool,

    /// Install a specific version instead of the latest release.
    #[arg(long, value_name = "VERSION")]
    version: Option<String>,
}

pub(crate) fn run(args: &UpgradeArgs) -> Result<(), Box<dyn std::error::Error>> {
    ush::update::run_upgrade(ush::update::UpgradeOptions {
        assume_yes: args.yes,
        check_only: args.check,
        version_override: args.version.clone(),
        verbose: false,
    })?;
    Ok(())
}
