//! `cruster help-json` — machine-readable command tree.
//!
//! Agents call this at startup to discover the verb surface. The
//! format intentionally mirrors a subset of OpenAPI: a list of
//! commands, each with a name, description, and parameter list.

use clap::CommandFactory;
use serde::Serialize;

use crate::args::Cli;

#[derive(Serialize)]
struct CommandDoc {
    name: String,
    about: String,
    flags: Vec<FlagDoc>,
}

#[derive(Serialize)]
struct FlagDoc {
    name: String,
    short: Option<String>,
    long: Option<String>,
    description: String,
    required: bool,
}

pub async fn run() -> anyhow::Result<()> {
    let cmd = Cli::command();
    let mut docs = Vec::new();
    for sub in cmd.get_subcommands() {
        let mut flags = Vec::new();
        for arg in sub.get_arguments() {
            flags.push(FlagDoc {
                name: arg.get_id().to_string(),
                short: arg.get_short().map(|c| c.to_string()),
                long: arg.get_long().map(|s| s.to_string()),
                description: arg.get_help().map(|h| h.to_string()).unwrap_or_default(),
                required: arg.is_required_set(),
            });
        }
        docs.push(CommandDoc {
            name: sub.get_name().to_string(),
            about: sub
                .get_about()
                .map(|a| a.to_string())
                .unwrap_or_default(),
            flags,
        });
    }
    println!("{}", serde_json::to_string_pretty(&docs)?);
    Ok(())
}
