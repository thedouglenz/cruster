//! CLI verb handlers (one module per verb).

pub mod describe;
pub mod events;
pub mod get;
pub mod logs;

use crate::args::{Cli, Command};

pub async fn dispatch(cli: Cli) -> anyhow::Result<()> {
    match &cli.command {
        Command::Get(args) => get::run(&cli, args).await,
        Command::Describe(args) => describe::run(&cli, args).await,
        Command::Logs(args) => logs::run(&cli, args).await,
        Command::Events(args) => events::run(&cli, args).await,
        Command::Schema(_) => anyhow::bail!("schema: not yet implemented (Task 16)"),
        Command::HelpJson => anyhow::bail!("help-json: not yet implemented (Task 16)"),
    }
}
