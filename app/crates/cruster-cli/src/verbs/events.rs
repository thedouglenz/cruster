//! `cruster events` — recent cluster events.

use std::io::Write;

use k8s_openapi::api::core::v1::Event;
use kube::{Api, Client};
use serde_json::Value;

use crate::args::{Cli, EventsArgs};
use crate::format::write_records;
use crate::output::{effective_format, stdout_is_tty};
use crate::prune::prune;

pub async fn run(cli: &Cli, args: &EventsArgs) -> anyhow::Result<()> {
    let client = Client::try_default().await?;
    let api: Api<Event> = match &args.namespace {
        Some(ns) => Api::namespaced(client, ns),
        None => Api::all(client),
    };
    let mut events = api.list(&Default::default()).await?.items;
    let total_before_filter = events.len();

    if let Some(resource) = &args.resource {
        let (kind, name) = crate::verbs::describe::parse_reference(resource)?;
        events.retain(|e| {
            e.involved_object.kind.as_deref() == Some(kind)
                && e.involved_object.name.as_deref() == Some(name)
        });
    }

    events.sort_by(|a, b| {
        let at = a.last_timestamp.as_ref().map(|t| t.0);
        let bt = b.last_timestamp.as_ref().map(|t| t.0);
        bt.cmp(&at)
    });
    events.truncate(args.limit);

    let mut records: Vec<Value> = events
        .iter()
        .map(|e| serde_json::to_value(e).expect("serialize"))
        .collect();
    for r in &mut records {
        prune(r, cli.full);
    }

    // If a filter ate all the events, give the caller (agent or human)
    // a clear signal that the filter is the reason — not "no events
    // exist". Without this, an empty NDJSON stream is ambiguous and
    // led an agent to second-guess whether `--resource` was supported.
    let filter_dropped_all = args.resource.is_some()
        && records.is_empty()
        && total_before_filter > 0;
    if filter_dropped_all {
        let marker = serde_json::json!({
            "matched": 0,
            "filtered_from": total_before_filter,
            "note": format!(
                "no events matched --resource {}; {} event(s) exist in scope",
                args.resource.as_deref().unwrap_or(""),
                total_before_filter,
            ),
        });
        records.push(marker);
    }

    let format = effective_format(cli.format, cli.llm, stdout_is_tty());
    let mut stdout = std::io::stdout().lock();
    write_records(&mut stdout, format, &records, |w, _vs| {
        if filter_dropped_all {
            writeln!(
                w,
                "no events matched --resource {} ({} event(s) exist in scope)",
                args.resource.as_deref().unwrap_or(""),
                total_before_filter,
            )?;
            return Ok(());
        }
        writeln!(w, "NAMESPACE\tLAST_SEEN\tTYPE\tREASON\tOBJECT\tMESSAGE")?;
        for e in &events {
            let ns = e.metadata.namespace.as_deref().unwrap_or("-");
            let last = e
                .last_timestamp
                .as_ref()
                .map(|t| t.0.to_rfc3339())
                .unwrap_or_else(|| "?".into());
            let ty = e.type_.as_deref().unwrap_or("-");
            let reason = e.reason.as_deref().unwrap_or("-");
            let obj = format!(
                "{}/{}",
                e.involved_object.kind.as_deref().unwrap_or("?"),
                e.involved_object.name.as_deref().unwrap_or("?")
            );
            let msg = e.message.as_deref().unwrap_or("");
            writeln!(w, "{ns}\t{last}\t{ty}\t{reason}\t{obj}\t{msg}")?;
        }
        Ok(())
    })?;
    Ok(())
}
