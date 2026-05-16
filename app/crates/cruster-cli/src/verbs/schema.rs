//! `cruster schema <verb>` — print the verb's structured-output schema.

use crate::args::{Cli, SchemaArgs};
use crate::schemas;

pub async fn run(_cli: &Cli, args: &SchemaArgs) -> anyhow::Result<()> {
    let body = schemas::lookup(&args.verb).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown verb '{}'. Known verbs: {}",
            args.verb,
            schemas::verb_names().join(", ")
        )
    })?;
    println!("{body}");
    Ok(())
}
