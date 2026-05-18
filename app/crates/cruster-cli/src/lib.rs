//! Cruster command-line interface.
//!
//! Entry point for non-TUI invocations. The `cruster` binary calls
//! `run(args)` when invoked with subcommands; with no args it stays
//! in TUI mode.

use std::ffi::OsString;

pub mod args;
pub mod budget;
pub mod envelope;
pub mod format;
pub mod output;
pub mod prune;
pub mod schemas;
pub mod verbs;

/// Run cruster in CLI mode. Returns a process exit code.
///
/// `argv` is the full process argv (including argv[0]). Caller is
/// responsible for routing this only when subcommands are present.
pub async fn run<I, T>(argv: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match args::Cli::try_parse(argv) {
        Ok(c) => c,
        Err(e) => {
            // clap prints the error/help itself; preserve its exit code.
            e.exit();
        }
    };

    match verbs::dispatch(cli).await {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("cruster: {e:#}");
            1
        }
    }
}
