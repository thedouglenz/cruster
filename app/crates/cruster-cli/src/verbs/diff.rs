//! `cruster diff <a> <b>` — structural diff between two resources.

use cruster_core::diff;
use kube::Client;

use crate::args::{Cli, DiffArgs};
use crate::format::write_records;
use crate::output::{effective_format, stdout_is_tty};

pub async fn run(cli: &Cli, args: &DiffArgs) -> anyhow::Result<()> {
    let client = Client::try_default().await?;
    let a_ns = args.a_namespace.as_deref().or(args.namespace.as_deref());
    let b_ns = args.b_namespace.as_deref().or(args.namespace.as_deref());
    let a = fetch_pruned(&client, &args.a, a_ns, cli.full).await?;
    let b = fetch_pruned(&client, &args.b, b_ns, cli.full).await?;
    let changes = diff::diff(&a, &b);

    let format = effective_format(cli.format, cli.llm, stdout_is_tty());
    let mut stdout = std::io::stdout().lock();
    write_records(&mut stdout, format, &changes, |w, _| {
        use std::io::Write;
        w.write_all(diff::render_text(&changes).as_bytes())
    })?;
    Ok(())
}

async fn fetch_pruned(
    client: &Client,
    reference: &str,
    ns: Option<&str>,
    full: bool,
) -> anyhow::Result<serde_json::Value> {
    let (kind, name) = crate::verbs::describe::parse_reference(reference)?;
    let canonical = crate::verbs::get::canonicalise_kind(kind)
        .ok_or_else(|| anyhow::anyhow!("unknown kind: {kind}"))?;
    let val = match canonical {
        "pods" => fetch::<k8s_openapi::api::core::v1::Pod>(client, name, ns).await?,
        "deployments" => {
            fetch::<k8s_openapi::api::apps::v1::Deployment>(client, name, ns).await?
        }
        "services" => fetch::<k8s_openapi::api::core::v1::Service>(client, name, ns).await?,
        "configmaps" => fetch::<k8s_openapi::api::core::v1::ConfigMap>(client, name, ns).await?,
        "secrets" => fetch_secret(client, name, ns).await?,
        other => anyhow::bail!("diff not supported for kind: {other}"),
    };
    let mut v = val;
    crate::prune::prune(&mut v, full);
    if canonical == "secrets" {
        crate::prune::redact_secret(&mut v);
    }
    Ok(v)
}

async fn fetch<T>(
    client: &Client,
    name: &str,
    ns: Option<&str>,
) -> anyhow::Result<serde_json::Value>
where
    T: kube::Resource<DynamicType = (), Scope = kube::core::NamespaceResourceScope>
        + Clone
        + serde::Serialize
        + serde::de::DeserializeOwned
        + std::fmt::Debug
        + Send
        + Sync
        + 'static,
{
    let ns = ns.ok_or_else(|| {
        anyhow::anyhow!("-n / --namespace is required for namespaced resources")
    })?;
    let api: kube::Api<T> = kube::Api::namespaced(client.clone(), ns);
    let obj = api.get(name).await?;
    Ok(serde_json::to_value(obj)?)
}

async fn fetch_secret(
    client: &Client,
    name: &str,
    ns: Option<&str>,
) -> anyhow::Result<serde_json::Value> {
    let ns = ns.ok_or_else(|| anyhow::anyhow!("-n / --namespace is required for secrets"))?;
    let api: kube::Api<k8s_openapi::api::core::v1::Secret> =
        kube::Api::namespaced(client.clone(), ns);
    let obj = api.get(name).await?;
    Ok(serde_json::to_value(obj)?)
}
