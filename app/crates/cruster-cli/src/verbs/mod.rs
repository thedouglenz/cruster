//! CLI verb handlers (one module per verb).

pub mod describe;
pub mod diff;
pub mod events;
pub mod get;
pub mod help_json;
pub mod logs;
pub mod schema;
pub mod theme;

use crate::args::{Cli, Command};

pub async fn dispatch(cli: Cli) -> anyhow::Result<()> {
    match &cli.command {
        Command::Get(args) => get::run(&cli, args).await,
        Command::Describe(args) => describe::run(&cli, args).await,
        Command::Logs(args) => logs::run(&cli, args).await,
        Command::Events(args) => events::run(&cli, args).await,
        Command::Schema(args) => schema::run(&cli, args).await,
        Command::HelpJson => help_json::run().await,
        Command::Diff(args) => diff::run(&cli, args).await,
        Command::Theme(args) => theme::run(&cli, args).await,
    }
}
