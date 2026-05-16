//! `cruster get <kind>` — list resources of a kind.
//!
//! Output:
//! - text: a per-kind column table (mirrors the TUI views)
//! - ndjson / json / yaml: pruned k8s objects (subject to `--full`)

use std::io::Write;

use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{
    ConfigMap, Event, Namespace, Node, Pod, Secret, Service,
};
use kube::{Api, Client};
use serde::Serialize;
use serde_json::Value;

use crate::args::{Cli, GetArgs};
use crate::args::Format;
use crate::format::{write_records, write_records_ndjson_budgeted};
use crate::output::{effective_format, stdout_is_tty};
use crate::prune::{prune, redact_secret};

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
        "pods" => run_namespaced::<Pod, _>(cli, args, write_pods_text).await,
        "deployments" => {
            run_namespaced::<Deployment, _>(cli, args, write_deployments_text).await
        }
        "services" => run_namespaced::<Service, _>(cli, args, write_services_text).await,
        "nodes" => run_cluster::<Node, _>(cli, args, write_nodes_text).await,
        "events" => run_namespaced::<Event, _>(cli, args, write_events_text).await,
        "configmaps" => run_namespaced::<ConfigMap, _>(cli, args, write_configmaps_text).await,
        "secrets" => run_secrets(cli, args).await,
        "namespaces" => run_cluster::<Namespace, _>(cli, args, write_namespaces_text).await,
        other => anyhow::bail!("get {other}: unrecognised kind"),
    }
}

/// Generic per-kind list for namespaced resources.
async fn run_namespaced<T, F>(cli: &Cli, args: &GetArgs, text_writer: F) -> anyhow::Result<()>
where
    T: kube::Resource<DynamicType = (), Scope = kube::core::NamespaceResourceScope>
        + Clone
        + Serialize
        + serde::de::DeserializeOwned
        + Send
        + Sync
        + std::fmt::Debug
        + 'static,
    F: Fn(&mut dyn Write, &[T]) -> std::io::Result<()>,
{
    let client = Client::try_default().await?;
    let api: Api<T> = match &args.namespace {
        Some(ns) => Api::namespaced(client, ns),
        None => Api::all(client),
    };
    emit(cli, args, &api, text_writer).await
}

/// Generic per-kind list for cluster-scoped resources.
async fn run_cluster<T, F>(cli: &Cli, args: &GetArgs, text_writer: F) -> anyhow::Result<()>
where
    T: kube::Resource<DynamicType = (), Scope = kube::core::ClusterResourceScope>
        + Clone
        + Serialize
        + serde::de::DeserializeOwned
        + Send
        + Sync
        + std::fmt::Debug
        + 'static,
    F: Fn(&mut dyn Write, &[T]) -> std::io::Result<()>,
{
    let client = Client::try_default().await?;
    let api: Api<T> = Api::all(client);
    emit(cli, args, &api, text_writer).await
}

async fn emit<T, F>(cli: &Cli, args: &GetArgs, api: &Api<T>, text_writer: F) -> anyhow::Result<()>
where
    T: kube::Resource<DynamicType = ()>
        + Clone
        + Serialize
        + serde::de::DeserializeOwned
        + Send
        + Sync
        + std::fmt::Debug
        + 'static,
    F: Fn(&mut dyn Write, &[T]) -> std::io::Result<()>,
{
    let list_params = build_list_params(args);
    let mut items = api.list(&list_params).await?.items;

    if let Some(name) = &args.name {
        items.retain(|i| i.meta().name.as_deref() == Some(name.as_str()));
    }

    let mut records: Vec<Value> = items
        .iter()
        .map(|i| serde_json::to_value(i).expect("serialize"))
        .collect();
    for r in &mut records {
        prune(r, cli.full);
    }

    let format = effective_format(cli.format, cli.llm, stdout_is_tty());
    let mut stdout = std::io::stdout().lock();
    if matches!(format, Format::Ndjson) && cli.budget.is_some() {
        write_records_ndjson_budgeted(&mut stdout, &records, cli.budget)?;
    } else {
        write_records(&mut stdout, format, &records, |w, _vs| {
            text_writer(w as &mut dyn Write, &items)
        })?;
    }
    Ok(())
}

/// Secrets path: always redact data/stringData regardless of `--full`.
async fn run_secrets(cli: &Cli, args: &GetArgs) -> anyhow::Result<()> {
    let client = Client::try_default().await?;
    let api: Api<Secret> = match &args.namespace {
        Some(ns) => Api::namespaced(client, ns),
        None => Api::all(client),
    };
    let list_params = build_list_params(args);
    let mut items = api.list(&list_params).await?.items;

    if let Some(name) = &args.name {
        items.retain(|i| i.metadata.name.as_deref() == Some(name.as_str()));
    }

    let mut records: Vec<Value> = items
        .iter()
        .map(|i| serde_json::to_value(i).expect("serialize"))
        .collect();
    for r in &mut records {
        prune(r, cli.full);
        redact_secret(r);
    }

    let format = effective_format(cli.format, cli.llm, stdout_is_tty());
    let mut stdout = std::io::stdout().lock();
    if matches!(format, Format::Ndjson) && cli.budget.is_some() {
        write_records_ndjson_budgeted(&mut stdout, &records, cli.budget)?;
    } else {
        write_records(&mut stdout, format, &records, |w, _vs| {
            write_secrets_text(w as &mut dyn Write, &items)
        })?;
    }
    Ok(())
}

fn build_list_params(args: &GetArgs) -> kube::api::ListParams {
    let mut params = kube::api::ListParams::default();
    if let Some(sel) = &args.selector {
        params = params.labels(sel);
    }
    params
}

// ---- Per-kind text writers --------------------------------------------

fn write_pods_text(out: &mut dyn Write, pods: &[Pod]) -> std::io::Result<()> {
    writeln!(out, "NAMESPACE\tNAME\tSTATUS\tREADY\tRESTARTS")?;
    for p in pods {
        let ns = p.metadata.namespace.as_deref().unwrap_or("-");
        let name = p.metadata.name.as_deref().unwrap_or("?");
        writeln!(
            out,
            "{ns}\t{name}\t{}\t{}\t{}",
            pod_phase(p),
            pod_ready(p),
            pod_restarts(p)
        )?;
    }
    Ok(())
}

fn write_deployments_text(out: &mut dyn Write, deps: &[Deployment]) -> std::io::Result<()> {
    writeln!(out, "NAMESPACE\tNAME\tREADY\tUP-TO-DATE\tAVAILABLE")?;
    for d in deps {
        let ns = d.metadata.namespace.as_deref().unwrap_or("-");
        let name = d.metadata.name.as_deref().unwrap_or("?");
        let desired = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);
        let ready = d
            .status
            .as_ref()
            .and_then(|s| s.ready_replicas)
            .unwrap_or(0);
        let updated = d
            .status
            .as_ref()
            .and_then(|s| s.updated_replicas)
            .unwrap_or(0);
        let available = d
            .status
            .as_ref()
            .and_then(|s| s.available_replicas)
            .unwrap_or(0);
        writeln!(out, "{ns}\t{name}\t{ready}/{desired}\t{updated}\t{available}")?;
    }
    Ok(())
}

fn write_services_text(out: &mut dyn Write, svcs: &[Service]) -> std::io::Result<()> {
    writeln!(out, "NAMESPACE\tNAME\tTYPE\tCLUSTER-IP\tPORTS")?;
    for s in svcs {
        let ns = s.metadata.namespace.as_deref().unwrap_or("-");
        let name = s.metadata.name.as_deref().unwrap_or("?");
        let type_ = s
            .spec
            .as_ref()
            .and_then(|sp| sp.type_.clone())
            .unwrap_or_else(|| "?".into());
        let cluster_ip = s
            .spec
            .as_ref()
            .and_then(|sp| sp.cluster_ip.clone())
            .unwrap_or_else(|| "-".into());
        let ports = s
            .spec
            .as_ref()
            .and_then(|sp| sp.ports.as_ref())
            .map(|ps| {
                ps.iter()
                    .map(|p| match p.protocol.as_deref() {
                        Some(proto) => format!("{}/{}", p.port, proto),
                        None => format!("{}/TCP", p.port),
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_else(|| "-".into());
        writeln!(out, "{ns}\t{name}\t{type_}\t{cluster_ip}\t{ports}")?;
    }
    Ok(())
}

fn write_nodes_text(out: &mut dyn Write, nodes: &[Node]) -> std::io::Result<()> {
    writeln!(out, "NAME\tSTATUS\tROLES\tVERSION\tOS-IMAGE")?;
    for n in nodes {
        let name = n.metadata.name.as_deref().unwrap_or("?");
        writeln!(
            out,
            "{name}\t{}\t{}\t{}\t{}",
            node_status(n),
            node_roles(n),
            node_version(n),
            node_os_image(n)
        )?;
    }
    Ok(())
}

fn write_events_text(out: &mut dyn Write, events: &[Event]) -> std::io::Result<()> {
    let mut sorted: Vec<&Event> = events.iter().collect();
    sorted.sort_by(|a, b| {
        let at = a.last_timestamp.as_ref().map(|t| t.0);
        let bt = b.last_timestamp.as_ref().map(|t| t.0);
        bt.cmp(&at)
    });
    writeln!(out, "NAMESPACE\tLAST_SEEN\tTYPE\tREASON\tOBJECT\tMESSAGE")?;
    for e in sorted {
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
        writeln!(out, "{ns}\t{last}\t{ty}\t{reason}\t{obj}\t{msg}")?;
    }
    Ok(())
}

fn write_configmaps_text(out: &mut dyn Write, cms: &[ConfigMap]) -> std::io::Result<()> {
    writeln!(out, "NAMESPACE\tNAME\tDATA\tAGE")?;
    for c in cms {
        let ns = c.metadata.namespace.as_deref().unwrap_or("-");
        let name = c.metadata.name.as_deref().unwrap_or("?");
        let count = c.data.as_ref().map(|d| d.len()).unwrap_or(0)
            + c.binary_data.as_ref().map(|d| d.len()).unwrap_or(0);
        writeln!(out, "{ns}\t{name}\t{count}\t{}", metadata_age(&c.metadata))?;
    }
    Ok(())
}

fn write_secrets_text(out: &mut dyn Write, secs: &[Secret]) -> std::io::Result<()> {
    writeln!(out, "NAMESPACE\tNAME\tTYPE\tDATA\tAGE")?;
    for s in secs {
        let ns = s.metadata.namespace.as_deref().unwrap_or("-");
        let name = s.metadata.name.as_deref().unwrap_or("?");
        let type_ = s.type_.clone().unwrap_or_else(|| "Opaque".into());
        let count = s.data.as_ref().map(|d| d.len()).unwrap_or(0)
            + s.string_data.as_ref().map(|d| d.len()).unwrap_or(0);
        writeln!(
            out,
            "{ns}\t{name}\t{type_}\t{count}\t{}",
            metadata_age(&s.metadata)
        )?;
    }
    Ok(())
}

fn write_namespaces_text(out: &mut dyn Write, nss: &[Namespace]) -> std::io::Result<()> {
    writeln!(out, "NAME\tSTATUS\tAGE")?;
    for n in nss {
        let name = n.metadata.name.as_deref().unwrap_or("?");
        let status = n
            .status
            .as_ref()
            .and_then(|s| s.phase.clone())
            .unwrap_or_else(|| "Active".into());
        writeln!(out, "{name}\t{status}\t{}", metadata_age(&n.metadata))?;
    }
    Ok(())
}

// ---- Per-kind helpers --------------------------------------------------

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

fn node_status(n: &Node) -> String {
    n.status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .and_then(|cs| cs.iter().find(|c| c.type_ == "Ready"))
        .map(|c| {
            if c.status == "True" {
                "Ready".into()
            } else {
                "NotReady".into()
            }
        })
        .unwrap_or_else(|| "?".into())
}

fn node_roles(n: &Node) -> String {
    let s = n
        .metadata
        .labels
        .as_ref()
        .map(|labels| {
            labels
                .keys()
                .filter_map(|k| k.strip_prefix("node-role.kubernetes.io/"))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    if s.is_empty() {
        "<none>".into()
    } else {
        s
    }
}

fn node_version(n: &Node) -> String {
    n.status
        .as_ref()
        .and_then(|s| s.node_info.as_ref())
        .map(|info| info.kubelet_version.clone())
        .unwrap_or_else(|| "?".into())
}

fn node_os_image(n: &Node) -> String {
    n.status
        .as_ref()
        .and_then(|s| s.node_info.as_ref())
        .map(|info| info.os_image.clone())
        .unwrap_or_else(|| "?".into())
}

fn metadata_age(m: &k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta) -> String {
    let Some(ts) = m.creation_timestamp.as_ref() else {
        return "?".into();
    };
    let delta = chrono::Utc::now().signed_duration_since(ts.0);
    let secs = delta.num_seconds().max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
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
