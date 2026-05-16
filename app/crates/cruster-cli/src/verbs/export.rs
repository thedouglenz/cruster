//! `cruster export <kind>/<name>` — diagnostic markdown bundle.
//!
//! Builds the same `Snapshot` the TUI uses (`E` keybind), runs it
//! through `cruster_tui::export::build_markdown`, and writes the
//! result to stdout (default) or to a file (`-o <path>`).

use std::io::Write;

use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{ConfigMap, Event, Namespace, Node, Pod, Secret, Service};
use kube::api::LogParams;
use kube::{Api, Client, Resource};
use serde::Serialize;

use cruster_core::context::{ClusterContext, EventSummary, ResourceContext, Snapshot};
use cruster_core::ResourceKey;
use cruster_tui::export::build_markdown;

use crate::args::{Cli, ExportArgs};
use crate::verbs::describe::parse_reference;
use crate::verbs::get::canonicalise_kind;

pub async fn run(_cli: &Cli, args: &ExportArgs) -> anyhow::Result<()> {
    let (raw_kind, name) = parse_reference(&args.reference)?;
    let kind_plural = canonicalise_kind(raw_kind)
        .ok_or_else(|| anyhow::anyhow!("unknown kind: {raw_kind}"))?;
    let client = Client::try_default().await?;

    let snapshot = match kind_plural {
        "pods" => build_pod_snapshot(&client, name, args).await?,
        "deployments" => {
            build_namespaced::<Deployment>(&client, "Deployment", name, args).await?
        }
        "services" => build_namespaced::<Service>(&client, "Service", name, args).await?,
        "configmaps" => build_namespaced::<ConfigMap>(&client, "ConfigMap", name, args).await?,
        "secrets" => build_namespaced::<Secret>(&client, "Secret", name, args).await?,
        "namespaces" => build_cluster::<Namespace>(&client, "Namespace", name).await?,
        "nodes" => build_cluster::<Node>(&client, "Node", name).await?,
        "events" => anyhow::bail!("cannot export an Event; export the resource it involves"),
        other => anyhow::bail!("export {other}: unrecognised kind"),
    };

    let md = build_markdown(&snapshot);

    match &args.output {
        Some(path) => std::fs::write(path, md)?,
        None => {
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(md.as_bytes())?;
        }
    }

    Ok(())
}

async fn build_pod_snapshot(
    client: &Client,
    name: &str,
    args: &ExportArgs,
) -> anyhow::Result<Snapshot> {
    let api: Api<Pod> = api_for(client, args.namespace.as_deref());
    let pod = api.get(name).await?;
    let key = ResourceKey::namespaced(
        "Pod",
        args.namespace.clone().unwrap_or_else(|| "default".into()),
        name,
    );
    let raw_yaml = serde_yaml::to_string(&pod)?;
    let status_summary = pod
        .status
        .as_ref()
        .and_then(|s| s.phase.clone())
        .unwrap_or_default();
    let age = pod
        .metadata
        .creation_timestamp
        .as_ref()
        .map(|t| t.0.to_rfc3339())
        .unwrap_or_default();
    let resource = ResourceContext {
        key,
        status_summary,
        age,
        raw_yaml,
    };

    let events = fetch_events(client, "Pod", name, args.namespace.as_deref()).await?;
    let logs = fetch_pod_logs(client, name, args).await;

    Ok(Snapshot {
        cluster: cluster_context(client),
        resource: Some(resource),
        events,
        logs,
        selection: None,
        pane: None,
    })
}

async fn build_namespaced<T>(
    client: &Client,
    kind: &str,
    name: &str,
    args: &ExportArgs,
) -> anyhow::Result<Snapshot>
where
    T: Resource<DynamicType = (), Scope = kube::core::NamespaceResourceScope>
        + Clone
        + Serialize
        + serde::de::DeserializeOwned
        + std::fmt::Debug
        + Send
        + Sync
        + 'static,
{
    let api: Api<T> = api_for(client, args.namespace.as_deref());
    let obj = api.get(name).await?;
    let key = ResourceKey::namespaced(
        kind,
        args.namespace.clone().unwrap_or_else(|| "default".into()),
        name,
    );
    let raw_yaml = serde_yaml::to_string(&obj)?;
    let events = fetch_events(client, kind, name, args.namespace.as_deref()).await?;
    Ok(Snapshot {
        cluster: cluster_context(client),
        resource: Some(ResourceContext {
            key,
            status_summary: String::new(),
            age: String::new(),
            raw_yaml,
        }),
        events,
        logs: Vec::new(),
        selection: None,
        pane: None,
    })
}

async fn build_cluster<T>(
    client: &Client,
    kind: &str,
    name: &str,
) -> anyhow::Result<Snapshot>
where
    T: Resource<DynamicType = (), Scope = kube::core::ClusterResourceScope>
        + Clone
        + Serialize
        + serde::de::DeserializeOwned
        + std::fmt::Debug
        + Send
        + Sync
        + 'static,
{
    let api: Api<T> = Api::all(client.clone());
    let obj = api.get(name).await?;
    let raw_yaml = serde_yaml::to_string(&obj)?;
    let key = ResourceKey::cluster_scoped(kind, name);
    let events = fetch_events(client, kind, name, None).await?;
    Ok(Snapshot {
        cluster: cluster_context(client),
        resource: Some(ResourceContext {
            key,
            status_summary: String::new(),
            age: String::new(),
            raw_yaml,
        }),
        events,
        logs: Vec::new(),
        selection: None,
        pane: None,
    })
}

fn api_for<T>(client: &Client, namespace: Option<&str>) -> Api<T>
where
    T: Resource<DynamicType = (), Scope = kube::core::NamespaceResourceScope>,
{
    match namespace {
        Some(ns) => Api::namespaced(client.clone(), ns),
        None => Api::default_namespaced(client.clone()),
    }
}

async fn fetch_events(
    client: &Client,
    kind: &str,
    name: &str,
    namespace: Option<&str>,
) -> anyhow::Result<Vec<EventSummary>> {
    let api: Api<Event> = match namespace {
        Some(ns) => Api::namespaced(client.clone(), ns),
        None => Api::all(client.clone()),
    };
    let mut events = api.list(&Default::default()).await?.items;
    events.retain(|e| {
        e.involved_object.kind.as_deref() == Some(kind)
            && e.involved_object.name.as_deref() == Some(name)
    });
    events.sort_by(|a, b| {
        let at = a.last_timestamp.as_ref().map(|t| t.0);
        let bt = b.last_timestamp.as_ref().map(|t| t.0);
        bt.cmp(&at)
    });
    Ok(events
        .into_iter()
        .map(|e| EventSummary {
            time: e
                .last_timestamp
                .as_ref()
                .map(|t| t.0.to_rfc3339())
                .unwrap_or_default(),
            type_: e.type_.unwrap_or_default(),
            reason: e.reason.unwrap_or_default(),
            message: e.message.unwrap_or_default(),
        })
        .collect())
}

/// Fetch log tail. Silently returns an empty Vec on failure so a pod
/// that's still pending (no logs yet) doesn't fail the entire export.
async fn fetch_pod_logs(client: &Client, name: &str, args: &ExportArgs) -> Vec<String> {
    let api: Api<Pod> = api_for(client, args.namespace.as_deref());
    let params = LogParams {
        tail_lines: Some(args.tail),
        ..Default::default()
    };
    let Ok(text) = api.logs(name, &params).await else {
        return Vec::new();
    };
    text.lines().map(|s| s.to_string()).collect()
}

fn cluster_context(_client: &Client) -> ClusterContext {
    let ctx = kube::config::Kubeconfig::read()
        .ok()
        .and_then(|c| c.current_context)
        .unwrap_or_else(|| "unknown".into());
    ClusterContext {
        name: ctx.clone(),
        context: ctx,
        // Environment classification is a TUI-side concern (driven by
        // ~/.config/cruster/safety.toml). The CLI is one-shot and
        // doesn't load that config; leave it blank for v1.
        environment: String::new(),
    }
}
