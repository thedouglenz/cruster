//! CLI verb handlers (one module per verb).

pub mod changed;
pub mod describe;
pub mod diff;
pub mod doctor;
pub mod events;
pub mod export;
pub mod get;
pub mod help_json;
pub mod license;
pub mod logs;
pub mod schema;
pub mod theme;
pub mod timeline;
pub mod trial;
pub mod why_crashloop;
pub mod why_no_endpoints;
pub mod why_pending;

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
        Command::Export(args) => export::run(&cli, args).await,
        Command::License(args) => license::run(args).await,
        Command::Trial => trial::run().await,
        Command::Timeline(args) => timeline::run(&cli, args).await,
        Command::Changed(args) => changed::run(&cli, args).await,
        Command::WhyNoEndpoints(args) => why_no_endpoints::run(&cli, args).await,
        Command::Doctor(args) => doctor::run(args).await,
        Command::WhyCrashloop(args) => why_crashloop::run(&cli, args).await,
        Command::WhyPending(args) => why_pending::run(&cli, args).await,
    }
}
