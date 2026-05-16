//! `cruster get <kind>` — list resources of a kind.
//!
//! Output:
//! - text: a per-kind column table (mirrors the TUI views)
//! - ndjson / json / yaml: pruned k8s objects (subject to `--full`)

use std::io::Write;

use k8s_openapi::api::core::v1::Pod;
use kube::{Api, Client};
use serde_json::Value;

use crate::args::{Cli, GetArgs};
use crate::format::write_records;
use crate::output::{effective_format, stdout_is_tty};
use crate::prune::prune;

/// Normalise user-typed kind/alias to canonical plural.
pub fn canonicalise_kind(raw: &str) -> Option<&'static str> {
    match raw {
        "po" | "pod" | "pods" => Some("pods"),
        "deploy" | "deployment" | "deployments" => Some("deployments"),
        "svc" | "service" | "services" => Some("services"),
        "no" | "node" | "nodes" => Some("nodes"),
        "ev" | "event" | "events" => Some("events"),
        "cm" | "configmap" | "configmaps" => Some("configmaps"),
        "sec" | "secret" | "secrets" => Some("secrets"),
        "ns" | "namespace" | "namespaces" => Some("namespaces"),
        _ => None,
    }
}

pub async fn run(cli: &Cli, args: &GetArgs) -> anyhow::Result<()> {
    let kind = canonicalise_kind(&args.kind)
        .ok_or_else(|| anyhow::anyhow!("unknown kind: {}", args.kind))?;
    match kind {
        "pods" => run_pods(cli, args).await,
        other => {
            anyhow::bail!("get {other}: not yet implemented (see Phase 2B plan)")
        }
    }
}

async fn run_pods(cli: &Cli, args: &GetArgs) -> anyhow::Result<()> {
    let client = Client::try_default().await?;
    let api: Api<Pod> = match &args.namespace {
        Some(ns) => Api::namespaced(client, ns),
        None => Api::all(client),
    };
    let list_params = build_list_params(args);
    let mut pods = api.list(&list_params).await?.items;

    if let Some(name) = &args.name {
        pods.retain(|p| p.metadata.name.as_deref() == Some(name.as_str()));
    }

    let mut records: Vec<Value> = pods
        .iter()
        .map(|p| serde_json::to_value(p).expect("serialize"))
        .collect();
    for r in &mut records {
        prune(r, cli.full);
    }

    let format = effective_format(cli.format, cli.llm, stdout_is_tty());
    let mut stdout = std::io::stdout().lock();
    write_records(&mut stdout, format, &records, |w, _vs| {
        write_pods_text(w, &pods)
    })?;
    Ok(())
}

fn build_list_params(args: &GetArgs) -> kube::api::ListParams {
    let mut params = kube::api::ListParams::default();
    if let Some(sel) = &args.selector {
        params = params.labels(sel);
    }
    params
}

fn write_pods_text<W: Write>(out: &mut W, pods: &[Pod]) -> std::io::Result<()> {
    writeln!(out, "NAMESPACE\tNAME\tSTATUS\tREADY\tRESTARTS")?;
    for p in pods {
        let ns = p.metadata.namespace.as_deref().unwrap_or("-");
        let name = p.metadata.name.as_deref().unwrap_or("?");
        let status = pod_phase(p);
        let ready = pod_ready(p);
        let restarts = pod_restarts(p);
        writeln!(out, "{ns}\t{name}\t{status}\t{ready}\t{restarts}")?;
    }
    Ok(())
}

fn pod_phase(p: &Pod) -> String {
    p.status
        .as_ref()
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "?".into())
}

fn pod_ready(p: &Pod) -> String {
    let cs = p
        .status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .map(|c| c.as_slice())
        .unwrap_or(&[]);
    let total = cs.len();
    let ready = cs.iter().filter(|c| c.ready).count();
    format!("{ready}/{total}")
}

fn pod_restarts(p: &Pod) -> i32 {
    p.status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .map(|cs| cs.iter().map(|c| c.restart_count).sum())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalise_recognises_aliases() {
        assert_eq!(canonicalise_kind("po"), Some("pods"));
        assert_eq!(canonicalise_kind("Pods"), None);
        assert_eq!(canonicalise_kind("svc"), Some("services"));
        assert_eq!(canonicalise_kind("nope"), None);
    }

    #[test]
    fn canonicalise_handles_all_kinds() {
        assert_eq!(canonicalise_kind("pods"), Some("pods"));
        assert_eq!(canonicalise_kind("deployments"), Some("deployments"));
        assert_eq!(canonicalise_kind("services"), Some("services"));
        assert_eq!(canonicalise_kind("nodes"), Some("nodes"));
        assert_eq!(canonicalise_kind("events"), Some("events"));
        assert_eq!(canonicalise_kind("configmaps"), Some("configmaps"));
        assert_eq!(canonicalise_kind("secrets"), Some("secrets"));
        assert_eq!(canonicalise_kind("namespaces"), Some("namespaces"));
    }
}
