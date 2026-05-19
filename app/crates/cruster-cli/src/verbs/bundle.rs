//! `cruster bundle <kind>/<name>` — structured JSON diagnostic bundle.
//!
//! Same data as `cruster export`, emitted as JSON matching
//! `schemas/bundle.schema.json` instead of markdown.

use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{ConfigMap, Event, Namespace, Node, Pod, Secret, Service};
use kube::api::LogParams;
use kube::{Api, Client, Resource};
use serde::Serialize;
use serde_json::Value;

use crate::args::{BundleArgs, Cli};
use crate::verbs::describe::parse_reference;
use crate::verbs::get::canonicalise_kind;

#[derive(Debug, Serialize)]
pub struct Bundle {
    pub cluster: ClusterInfo,
    pub resource: ResourceInfo,
    pub events: Vec<EventInfo>,
    pub logs: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ClusterInfo {
    pub name: String,
    pub context: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub environment: String,
}

#[derive(Debug, Serialize)]
pub struct ResourceInfo {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age: Option<String>,
    pub raw: Value,
}

#[derive(Debug, Serialize)]
pub struct EventInfo {
    pub time: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub reason: String,
    pub message: String,
}

pub async fn run(_cli: &Cli, args: &BundleArgs) -> anyhow::Result<()> {
    let (raw_kind, name) = parse_reference(&args.reference)?;
    let kind_plural =
        canonicalise_kind(raw_kind).ok_or_else(|| anyhow::anyhow!("unknown kind: {raw_kind}"))?;
    let client = Client::try_default().await?;

    let bundle = match kind_plural {
        "pods" => build_pod_bundle(&client, name, args).await?,
        "deployments" => build_namespaced_bundle::<Deployment>(&client, "Deployment", name, args).await?,
        "services" => build_namespaced_bundle::<Service>(&client, "Service", name, args).await?,
        "configmaps" => build_namespaced_bundle::<ConfigMap>(&client, "ConfigMap", name, args).await?,
        "secrets" => build_namespaced_bundle::<Secret>(&client, "Secret", name, args).await?,
        "namespaces" => build_cluster_bundle::<Namespace>(&client, "Namespace", name).await?,
        "nodes" => build_cluster_bundle::<Node>(&client, "Node", name).await?,
        "events" => anyhow::bail!("cannot bundle an Event; bundle the resource it involves"),
        other => anyhow::bail!("bundle {other}: unrecognised kind"),
    };

    println!("{}", serde_json::to_string_pretty(&bundle)?);
    Ok(())
}

async fn build_pod_bundle(
    client: &Client,
    name: &str,
    args: &BundleArgs,
) -> anyhow::Result<Bundle> {
    let api: Api<Pod> = api_for(client, args.namespace.as_deref());
    let pod = api.get(name).await?;

    let phase = pod.status.as_ref().and_then(|s| s.phase.clone());
    let age = pod
        .metadata
        .creation_timestamp
        .as_ref()
        .map(|t| t.0.to_rfc3339());
    let raw = serde_json::to_value(&pod)?;

    let events = fetch_events(client, "Pod", name, args.namespace.as_deref()).await?;
    let logs = fetch_pod_logs(client, name, args).await;

    Ok(Bundle {
        cluster: cluster_info(),
        resource: ResourceInfo {
            kind: "Pod".to_string(),
            namespace: args.namespace.clone().or_else(|| Some("default".into())),
            name: name.to_string(),
            phase,
            age,
            raw,
        },
        events,
        logs,
    })
}

async fn build_namespaced_bundle<T>(
    client: &Client,
    kind: &str,
    name: &str,
    args: &BundleArgs,
) -> anyhow::Result<Bundle>
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
    let raw = serde_json::to_value(&obj)?;
    let events = fetch_events(client, kind, name, args.namespace.as_deref()).await?;

    Ok(Bundle {
        cluster: cluster_info(),
        resource: ResourceInfo {
            kind: kind.to_string(),
            namespace: args.namespace.clone().or_else(|| Some("default".into())),
            name: name.to_string(),
            phase: None,
            age: None,
            raw,
        },
        events,
        logs: Vec::new(),
    })
}

async fn build_cluster_bundle<T>(
    client: &Client,
    kind: &str,
    name: &str,
) -> anyhow::Result<Bundle>
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
    let raw = serde_json::to_value(&obj)?;
    let events = fetch_events(client, kind, name, None).await?;

    Ok(Bundle {
        cluster: cluster_info(),
        resource: ResourceInfo {
            kind: kind.to_string(),
            namespace: None,
            name: name.to_string(),
            phase: None,
            age: None,
            raw,
        },
        events,
        logs: Vec::new(),
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
) -> anyhow::Result<Vec<EventInfo>> {
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
        .map(|e| EventInfo {
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

async fn fetch_pod_logs(client: &Client, name: &str, args: &BundleArgs) -> Vec<String> {
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

fn cluster_info() -> ClusterInfo {
    let ctx = kube::config::Kubeconfig::read()
        .ok()
        .and_then(|c| c.current_context)
        .unwrap_or_else(|| "unknown".into());
    ClusterInfo {
        name: ctx.clone(),
        context: ctx,
        environment: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_serializes_with_required_fields() {
        let bundle = Bundle {
            cluster: ClusterInfo {
                name: "test-cluster".into(),
                context: "test-context".into(),
                environment: String::new(),
            },
            resource: ResourceInfo {
                kind: "Pod".into(),
                namespace: Some("default".into()),
                name: "nginx".into(),
                phase: Some("Running".into()),
                age: Some("2026-05-19T00:00:00Z".into()),
                raw: serde_json::json!({"kind": "Pod", "metadata": {"name": "nginx"}}),
            },
            events: vec![EventInfo {
                time: "2026-05-19T12:00:00Z".into(),
                type_: "Warning".into(),
                reason: "BackOff".into(),
                message: "Back-off restarting failed container".into(),
            }],
            logs: vec!["line 1".into(), "line 2".into()],
        };

        let json = serde_json::to_value(&bundle).unwrap();
        assert_eq!(json["cluster"]["name"], "test-cluster");
        assert_eq!(json["resource"]["kind"], "Pod");
        assert_eq!(json["resource"]["name"], "nginx");
        assert_eq!(json["resource"]["phase"], "Running");
        assert!(json["resource"]["raw"].is_object());
        assert_eq!(json["events"][0]["type"], "Warning");
        assert_eq!(json["events"][0]["reason"], "BackOff");
        assert_eq!(json["logs"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn bundle_for_namespace_has_empty_logs() {
        let bundle = Bundle {
            cluster: ClusterInfo {
                name: "test".into(),
                context: "test".into(),
                environment: String::new(),
            },
            resource: ResourceInfo {
                kind: "Namespace".into(),
                namespace: None,
                name: "kube-system".into(),
                phase: None,
                age: None,
                raw: serde_json::json!({"kind": "Namespace"}),
            },
            events: vec![],
            logs: vec![],
        };

        let json = serde_json::to_value(&bundle).unwrap();
        assert!(json["logs"].as_array().unwrap().is_empty());
        assert!(json["resource"]["namespace"].is_null());
    }

    #[test]
    fn event_info_serializes_type_correctly() {
        let event = EventInfo {
            time: "2026-05-19T12:00:00Z".into(),
            type_: "Normal".into(),
            reason: "Scheduled".into(),
            message: "Successfully assigned pod".into(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"Normal""#));
        assert!(!json.contains("type_"));
    }
}
