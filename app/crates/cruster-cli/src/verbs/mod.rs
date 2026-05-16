//! CLI verb handlers (one module per verb).

use crate::args::Cli;

pub async fn dispatch(_cli: Cli) -> anyhow::Result<()> {
    anyhow::bail!("no verbs registered yet (Task 4)")
}
