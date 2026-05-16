//! CLI verb handlers (one module per verb).

pub mod describe;
pub mod get;

use crate::args::{Cli, Command};

pub async fn dispatch(cli: Cli) -> anyhow::Result<()> {
    match &cli.command {
        Command::Get(args) => get::run(&cli, args).await,
        Command::Describe(args) => describe::run(&cli, args).await,
        Command::Logs(_) => anyhow::bail!("logs: not yet implemented (Task 13)"),
        Command::Events(_) => anyhow::bail!("events: not yet implemented (Task 14)"),
        Command::Schema(_) => anyhow::bail!("schema: not yet implemented (Task 16)"),
        Command::HelpJson => anyhow::bail!("help-json: not yet implemented (Task 16)"),
    }
}
