//! `cruster changed -n <ns> --since <window>` — what shifted in a
//! namespace recently.
//!
//! Produces a flat NDJSON stream of changes plus a terminal `summary`
//! record. Each change is one of:
//!
//! - `secret_rotated` — Secret whose `metadata.managedFields[].time`
//!   max falls inside the window. Augmented with `consumed_by`: every
//!   pod in the namespace that references this Secret (with `via` and
//!   container hints) so the agent doesn't have to walk references.
//! - `configmap_changed` — same, for ConfigMap.
//! - `replicaset_rolled` — ReplicaSet whose `creationTimestamp` falls
//!   inside the window. Always tied to an owning Deployment.
//! - `image_changed` — when a new ReplicaSet's container image differs
//!   from the prior-revision RS for the same Deployment.
//! - `resource_created` — Deployment / StatefulSet / DaemonSet /
//!   Service whose `creationTimestamp` falls inside the window.
//!
//! v1 scope: single namespace; `--workload` filter, `replicas_scaled`,
//! `selector_changed`, `hpa_scaled`, `node_event`, and
//! `resource_deleted` are intentionally deferred — k8s doesn't preserve
//! enough history to compute them reliably without an audit log.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Write;

use anyhow::{bail, Context as _};
use chrono::{DateTime, Duration, Utc};
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, ReplicaSet, StatefulSet};
use k8s_openapi::api::core::v1::{ConfigMap, Pod, Secret, Service};
use kube::api::ListParams;
use kube::{Api, Client};
use serde::Serialize;
use serde_json::{json, Value};

use crate::args::{ChangedArgs, Cli, Format};
use crate::envelope::Window;
use crate::output::{effective_format, stdout_is_tty};
use crate::refs::{collect_config_refs, ConfigKind};

// ---------- kind enum ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Kind {
    SecretRotated,
    ConfigmapChanged,
    ReplicasetRolled,
    ImageChanged,
    ResourceCreated,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::SecretRotated => "secret_rotated",
            Kind::ConfigmapChanged => "configmap_changed",
            Kind::ReplicasetRolled => "replicaset_rolled",
            Kind::ImageChanged => "image_changed",
            Kind::ResourceCreated => "resource_created",
        }
    }

    fn all() -> &'static [Kind] {
        &[
            Kind::SecretRotated,
            Kind::ConfigmapChanged,
            Kind::ReplicasetRolled,
            Kind::ImageChanged,
            Kind::ResourceCreated,
        ]
    }

    fn from_str(s: &str) -> Option<Kind> {
        Kind::all().iter().copied().find(|k| k.as_str() == s)
    }
}

// ---------- record types ----------

#[derive(Debug, Clone, Serialize)]
struct Record {
    at: DateTime<Utc>,
    kind: &'static str,
    #[serde(rename = "ref")]
    ref_: Value,
    #[serde(skip_serializing_if = "Value::is_null")]
    before: Value,
    #[serde(skip_serializing_if = "Value::is_null")]
    after: Value,
    #[serde(skip_serializing_if = "Value::is_null")]
    attrs: Value,
}

#[derive(Debug, Serialize)]
struct Summary {
    window: Window,
    scope: Scope,
    counts: BTreeMap<&'static str, usize>,
    kinds_absent: Vec<&'static str>,
    truncated: TruncationFlags,
    notes: Vec<String>,
    ms_total: u128,
}

#[derive(Debug, Serialize)]
struct Scope {
    namespace: String,
}

#[derive(Debug, Serialize)]
struct TruncationFlags {
    /// True if Secret/ConfigMap rotation detection had to fall back to
    /// the *current* resourceVersion (the only one kube preserves) —
    /// we never see the prior rv unless cruster caches it across runs.
    managed_fields_history: bool,
}

#[derive(Debug, Serialize)]
struct SummaryEnvelope {
    summary: Summary,
}

// ---------- entry point ----------

pub async fn run(cli: &Cli, args: &ChangedArgs) -> anyhow::Result<()> {
    let since_secs = parse_duration_seconds(&args.since)?;
    let now = Utc::now();
    let window = Window::new(now - Duration::seconds(since_secs), now);
    let ns = &args.namespace;

    let include_filter = args
        .kind
        .as_deref()
        .map(parse_include)
        .transpose()?;

    let started = std::time::Instant::now();
    let client = Client::try_default().await?;

    let mut records: Vec<Record> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    // Pods first — needed to compute `consumed_by` for secret/cm changes.
    let pod_api: Api<Pod> = Api::namespaced(client.clone(), ns);
    let pods = pod_api
        .list(&ListParams::default())
        .await
        .context("listing pods")?
        .items;
    let (secret_consumers, cm_consumers) = build_consumer_index(&pods);

    // Secrets.
    let secret_api: Api<Secret> = Api::namespaced(client.clone(), ns);
    let secrets = secret_api
        .list(&ListParams::default())
        .await
        .context("listing secrets")?
        .items;
    for s in &secrets {
        let Some(at) = max_managed_field_time(s.metadata.managed_fields.as_deref()) else {
            continue;
        };
        if at < window.from || at > window.to {
            continue;
        }
        records.push(make_config_record(
            "Secret",
            ns,
            s.metadata.name.as_deref().unwrap_or(""),
            s.metadata.resource_version.as_deref(),
            latest_manager(s.metadata.managed_fields.as_deref()),
            at,
            secret_consumers
                .get(s.metadata.name.as_deref().unwrap_or(""))
                .map(|v| v.as_slice())
                .unwrap_or(&[]),
            Kind::SecretRotated,
        ));
    }

    // ConfigMaps.
    let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), ns);
    let cms = cm_api
        .list(&ListParams::default())
        .await
        .context("listing configmaps")?
        .items;
    for cm in &cms {
        let Some(at) = max_managed_field_time(cm.metadata.managed_fields.as_deref()) else {
            continue;
        };
        if at < window.from || at > window.to {
            continue;
        }
        records.push(make_config_record(
            "ConfigMap",
            ns,
            cm.metadata.name.as_deref().unwrap_or(""),
            cm.metadata.resource_version.as_deref(),
            latest_manager(cm.metadata.managed_fields.as_deref()),
            at,
            cm_consumers
                .get(cm.metadata.name.as_deref().unwrap_or(""))
                .map(|v| v.as_slice())
                .unwrap_or(&[]),
            Kind::ConfigmapChanged,
        ));
    }
    // Surface the kube-history caveat once, not per record.
    if !secrets.is_empty() || !cms.is_empty() {
        notes.push(
            "secret/configmap rotation detection uses managedFields timestamps; only the latest rotation is visible per object, and the prior resourceVersion is not preserved by kube"
                .into(),
        );
    }

    // ReplicaSets + image_changed via prior-RS lookup.
    let rs_api: Api<ReplicaSet> = Api::namespaced(client.clone(), ns);
    let all_rsets = rs_api
        .list(&ListParams::default())
        .await
        .context("listing replicasets")?
        .items;
    records.extend(rollouts_and_image_changes(&all_rsets, ns, &window));

    // Workloads + Services created in window.
    let dep_api: Api<Deployment> = Api::namespaced(client.clone(), ns);
    let sts_api: Api<StatefulSet> = Api::namespaced(client.clone(), ns);
    let ds_api: Api<DaemonSet> = Api::namespaced(client.clone(), ns);
    let svc_api: Api<Service> = Api::namespaced(client.clone(), ns);
    for d in dep_api
        .list(&ListParams::default())
        .await
        .context("listing deployments")?
        .items
    {
        if let Some(rec) = creation_record_for("Deployment", ns, &d.metadata, &window) {
            records.push(rec);
        }
    }
    for s in sts_api
        .list(&ListParams::default())
        .await
        .context("listing statefulsets")?
        .items
    {
        if let Some(rec) = creation_record_for("StatefulSet", ns, &s.metadata, &window) {
            records.push(rec);
        }
    }
    for d in ds_api
        .list(&ListParams::default())
        .await
        .context("listing daemonsets")?
        .items
    {
        if let Some(rec) = creation_record_for("DaemonSet", ns, &d.metadata, &window) {
            records.push(rec);
        }
    }
    for s in svc_api
        .list(&ListParams::default())
        .await
        .context("listing services")?
        .items
    {
        if let Some(rec) = creation_record_for("Service", ns, &s.metadata, &window) {
            records.push(rec);
        }
    }

    // Filter + sort.
    if let Some(allow) = &include_filter {
        records.retain(|r| Kind::from_str(r.kind).map(|k| allow.contains(&k)).unwrap_or(false));
    }
    records.sort_by(|a, b| b.at.cmp(&a.at)); // most-recent-first

    // Counts + kinds_absent.
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for r in &records {
        *counts.entry(r.kind).or_insert(0) += 1;
    }
    let present: HashSet<&'static str> = records.iter().map(|r| r.kind).collect();
    let mut kinds_absent: Vec<&'static str> = Kind::all()
        .iter()
        .map(|k| k.as_str())
        .filter(|k| !present.contains(k))
        .collect();
    if let Some(allow) = &include_filter {
        let allow_strs: HashSet<&'static str> = allow.iter().map(|k| k.as_str()).collect();
        kinds_absent.retain(|k| allow_strs.contains(k));
    }
    kinds_absent.sort();

    let format = effective_format(cli.format, cli.llm, stdout_is_tty());
    let mut stdout = std::io::stdout().lock();
    for r in &records {
        emit_record(&mut stdout, format, r)?;
    }
    let summary = SummaryEnvelope {
        summary: Summary {
            window,
            scope: Scope {
                namespace: ns.clone(),
            },
            counts,
            kinds_absent,
            truncated: TruncationFlags {
                managed_fields_history: true,
            },
            notes,
            ms_total: started.elapsed().as_millis(),
        },
    };
    emit_summary(&mut stdout, format, &summary)?;
    Ok(())
}

// ---------- helpers ----------

#[derive(Debug, Clone, Serialize)]
struct Consumer {
    kind: &'static str,
    name: String,
    via: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    container: Option<String>,
}

/// Returns (secret_name -> consumers, configmap_name -> consumers).
fn build_consumer_index(
    pods: &[Pod],
) -> (
    BTreeMap<String, Vec<Consumer>>,
    BTreeMap<String, Vec<Consumer>>,
) {
    let mut secrets: BTreeMap<String, Vec<Consumer>> = BTreeMap::new();
    let mut cms: BTreeMap<String, Vec<Consumer>> = BTreeMap::new();
    for pod in pods {
        let Some(spec) = pod.spec.as_ref() else {
            continue;
        };
        let pod_name = pod.metadata.name.clone().unwrap_or_default();
        for r in collect_config_refs(spec) {
            let consumer = Consumer {
                kind: "Pod",
                name: pod_name.clone(),
                via: r.via,
                container: r.container.clone(),
            };
            match r.kind {
                ConfigKind::Secret => secrets.entry(r.name).or_default().push(consumer),
                ConfigKind::ConfigMap => cms.entry(r.name).or_default().push(consumer),
            }
        }
    }
    (secrets, cms)
}

#[allow(clippy::too_many_arguments)]
fn make_config_record(
    kind_label: &'static str,
    ns: &str,
    name: &str,
    resource_version: Option<&str>,
    manager: Option<String>,
    at: DateTime<Utc>,
    consumers: &[Consumer],
    kind: Kind,
) -> Record {
    let mut attrs = json!({});
    if let Some(m) = manager {
        attrs["manager"] = json!(m);
    }
    if !consumers.is_empty() {
        attrs["consumed_by"] = json!(consumers);
    }
    let mut after = json!({});
    if let Some(rv) = resource_version {
        after["resource_version"] = json!(rv);
    }
    Record {
        at,
        kind: kind.as_str(),
        ref_: json!({ "kind": kind_label, "namespace": ns, "name": name }),
        before: Value::Null,
        after,
        attrs,
    }
}

fn creation_record_for(
    kind_label: &'static str,
    ns: &str,
    meta: &k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta,
    window: &Window,
) -> Option<Record> {
    let at = meta.creation_timestamp.as_ref()?.0;
    if at < window.from || at > window.to {
        return None;
    }
    Some(Record {
        at,
        kind: Kind::ResourceCreated.as_str(),
        ref_: json!({
            "kind": kind_label,
            "namespace": ns,
            "name": meta.name.clone().unwrap_or_default(),
        }),
        before: Value::Null,
        after: Value::Null,
        attrs: Value::Null,
    })
}

/// Build rollouts + image-changes from the full ReplicaSet list for
/// the namespace. Groups by owning Deployment, orders by
/// `deployment.kubernetes.io/revision`, emits `replicaset_rolled` for
/// every RS created in the window plus an `image_changed` for every
/// container whose image differs from the prior-revision RS.
fn rollouts_and_image_changes(
    all_rsets: &[ReplicaSet],
    ns: &str,
    window: &Window,
) -> Vec<Record> {
    let mut by_owner: BTreeMap<String, Vec<&ReplicaSet>> = BTreeMap::new();
    for rs in all_rsets {
        let Some(owners) = rs.metadata.owner_references.as_ref() else {
            continue;
        };
        for o in owners {
            if o.kind == "Deployment" {
                by_owner.entry(o.name.clone()).or_default().push(rs);
                break;
            }
        }
    }

    let mut records = Vec::new();
    for (owner, mut rsets) in by_owner {
        rsets.sort_by_key(|rs| revision_of(rs).unwrap_or(0));
        for (i, rs) in rsets.iter().enumerate() {
            let Some(at) = rs.metadata.creation_timestamp.as_ref().map(|t| t.0) else {
                continue;
            };
            if at < window.from || at > window.to {
                continue;
            }
            let revision = revision_of(rs);
            let rs_name = rs.metadata.name.clone().unwrap_or_default();
            // replicaset_rolled
            let mut attrs = json!({
                "owner": { "kind": "Deployment", "name": owner.clone() },
            });
            if let Some(rev) = revision {
                attrs["revision"] = json!(rev);
            }
            records.push(Record {
                at,
                kind: Kind::ReplicasetRolled.as_str(),
                ref_: json!({ "kind": "ReplicaSet", "namespace": ns, "name": rs_name.clone() }),
                before: Value::Null,
                after: Value::Null,
                attrs,
            });
            // image_changed vs prior RS (if any).
            if i > 0 {
                let prior = rsets[i - 1];
                let new_imgs = container_images(rs);
                let prior_imgs = container_images(prior);
                let containers: BTreeSet<&String> =
                    new_imgs.keys().chain(prior_imgs.keys()).collect();
                for container in containers {
                    let new_img = new_imgs.get(container);
                    let prior_img = prior_imgs.get(container);
                    if new_img != prior_img {
                        let mut before = json!({});
                        if let Some(img) = prior_img {
                            before["image"] = json!(img);
                        }
                        before["rs"] = json!(prior.metadata.name.clone().unwrap_or_default());
                        if let Some(rev) = revision_of(prior) {
                            before["revision"] = json!(rev);
                        }
                        let mut after = json!({});
                        if let Some(img) = new_img {
                            after["image"] = json!(img);
                        }
                        after["rs"] = json!(rs_name.clone());
                        if let Some(rev) = revision {
                            after["revision"] = json!(rev);
                        }
                        records.push(Record {
                            at,
                            kind: Kind::ImageChanged.as_str(),
                            ref_: json!({
                                "kind": "Deployment", "namespace": ns, "name": owner.clone(),
                            }),
                            before,
                            after,
                            attrs: json!({ "container": container.clone() }),
                        });
                    }
                }
            }
        }
    }
    records
}

fn revision_of(rs: &ReplicaSet) -> Option<u64> {
    rs.metadata
        .annotations
        .as_ref()?
        .get("deployment.kubernetes.io/revision")
        .and_then(|s| s.parse::<u64>().ok())
}

fn container_images(rs: &ReplicaSet) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(spec) = rs.spec.as_ref() else {
        return out;
    };
    let Some(tmpl) = spec.template.as_ref() else {
        return out;
    };
    let Some(pod_spec) = tmpl.spec.as_ref() else {
        return out;
    };
    for c in &pod_spec.containers {
        if let Some(img) = &c.image {
            out.insert(c.name.clone(), img.clone());
        }
    }
    out
}

fn max_managed_field_time(
    managed_fields: Option<&[k8s_openapi::apimachinery::pkg::apis::meta::v1::ManagedFieldsEntry]>,
) -> Option<DateTime<Utc>> {
    managed_fields?
        .iter()
        .filter_map(|m| m.time.as_ref().map(|t| t.0))
        .max()
}

fn latest_manager(
    managed_fields: Option<&[k8s_openapi::apimachinery::pkg::apis::meta::v1::ManagedFieldsEntry]>,
) -> Option<String> {
    managed_fields?
        .iter()
        .filter_map(|m| m.time.as_ref().map(|t| (t.0, m.manager.clone())))
        .max_by_key(|(t, _)| *t)
        .and_then(|(_, m)| m)
}

fn emit_record<W: Write>(out: &mut W, format: Format, r: &Record) -> std::io::Result<()> {
    match format {
        Format::Text => {
            let ref_label = format!(
                "{}/{}",
                r.ref_["kind"].as_str().unwrap_or("?"),
                r.ref_["name"].as_str().unwrap_or("?"),
            );
            writeln!(
                out,
                "{}  {}  {}",
                r.at.to_rfc3339(),
                r.kind,
                ref_label,
            )
        }
        _ => {
            let line = serde_json::to_string(r).expect("serialize");
            writeln!(out, "{line}")
        }
    }
}

fn emit_summary<W: Write>(
    out: &mut W,
    format: Format,
    s: &SummaryEnvelope,
) -> std::io::Result<()> {
    match format {
        Format::Text => {
            let sm = &s.summary;
            writeln!(
                out,
                "scanned ns={} window={}..{} in {} ms",
                sm.scope.namespace,
                sm.window.from.to_rfc3339(),
                sm.window.to.to_rfc3339(),
                sm.ms_total
            )?;
            for (k, v) in &sm.counts {
                writeln!(out, "  {k}: {v}")?;
            }
            if !sm.kinds_absent.is_empty() {
                writeln!(out, "absent: {}", sm.kinds_absent.join(", "))?;
            }
            for n in &sm.notes {
                writeln!(out, "note: {n}")?;
            }
            Ok(())
        }
        _ => {
            let line = serde_json::to_string(s).expect("serialize");
            writeln!(out, "{line}")
        }
    }
}

fn parse_duration_seconds(s: &str) -> anyhow::Result<i64> {
    use std::str::FromStr;
    if s.is_empty() {
        anyhow::bail!("empty duration");
    }
    let (num, suffix) = s.split_at(s.len() - 1);
    let n: i64 = i64::from_str(num).context("duration number")?;
    match suffix {
        "s" => Ok(n),
        "m" => Ok(n * 60),
        "h" => Ok(n * 3600),
        "d" => Ok(n * 86400),
        _ => anyhow::bail!("unknown duration suffix: {suffix}"),
    }
}

fn parse_include(s: &str) -> anyhow::Result<HashSet<Kind>> {
    let mut out = HashSet::new();
    for part in s.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        match Kind::from_str(p) {
            Some(k) => {
                out.insert(k);
            }
            None => bail!(
                "unknown --kind '{p}'; expected one of: {}",
                Kind::all()
                    .iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::apps::v1::{ReplicaSetSpec};
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{
        ObjectMeta, OwnerReference, Time,
    };
    use std::collections::BTreeMap as Map;

    fn rs(
        name: &str,
        owner: &str,
        revision: &str,
        created: DateTime<Utc>,
        images: &[(&str, &str)],
    ) -> ReplicaSet {
        let mut anns = Map::new();
        anns.insert(
            "deployment.kubernetes.io/revision".into(),
            revision.into(),
        );
        ReplicaSet {
            metadata: ObjectMeta {
                name: Some(name.into()),
                annotations: Some(anns),
                creation_timestamp: Some(Time(created)),
                owner_references: Some(vec![OwnerReference {
                    api_version: "apps/v1".into(),
                    kind: "Deployment".into(),
                    name: owner.into(),
                    uid: "u".into(),
                    block_owner_deletion: None,
                    controller: Some(true),
                }]),
                ..Default::default()
            },
            spec: Some(ReplicaSetSpec {
                selector: Default::default(),
                template: Some(PodTemplateSpec {
                    spec: Some(PodSpec {
                        containers: images
                            .iter()
                            .map(|(n, i)| Container {
                                name: (*n).into(),
                                image: Some((*i).into()),
                                ..Default::default()
                            })
                            .collect(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn revision_of_reads_deployment_annotation() {
        let now = Utc::now();
        let r = rs("rs1", "deploy", "7", now, &[("c", "x:1")]);
        assert_eq!(revision_of(&r), Some(7));
    }

    #[test]
    fn container_images_walks_template() {
        let now = Utc::now();
        let r = rs("rs1", "deploy", "1", now, &[("a", "img-a:1"), ("b", "img-b:1")]);
        let imgs = container_images(&r);
        assert_eq!(imgs.get("a").map(String::as_str), Some("img-a:1"));
        assert_eq!(imgs.get("b").map(String::as_str), Some("img-b:1"));
    }

    #[test]
    fn rollouts_emits_rolled_and_image_change_for_new_rs() {
        let now = Utc::now();
        let prior_at = now - Duration::seconds(7200); // outside window
        let new_at = now - Duration::seconds(60); // inside
        let window = Window::new(now - Duration::seconds(3600), now);
        let prior = rs("prior-rs", "deploy", "1", prior_at, &[("agent", "img:1")]);
        let new = rs("new-rs", "deploy", "2", new_at, &[("agent", "img:2")]);

        let recs = rollouts_and_image_changes(&[prior, new], "ns", &window);
        // Only the new RS rolled (prior is outside window) + one image_changed.
        let kinds: Vec<&str> = recs.iter().map(|r| r.kind).collect();
        assert!(kinds.contains(&"replicaset_rolled"));
        assert!(kinds.contains(&"image_changed"));
        let ic = recs.iter().find(|r| r.kind == "image_changed").unwrap();
        assert_eq!(ic.before["image"], "img:1");
        assert_eq!(ic.after["image"], "img:2");
        assert_eq!(ic.attrs["container"], "agent");
    }

    #[test]
    fn rollouts_skips_image_change_when_first_rs() {
        // No prior RS exists → emit only `replicaset_rolled`, no
        // image_changed (we have nothing to diff against).
        let now = Utc::now();
        let window = Window::new(now - Duration::seconds(3600), now);
        let only = rs("only", "deploy", "1", now - Duration::seconds(30), &[("c", "img:1")]);

        let recs = rollouts_and_image_changes(&[only], "ns", &window);
        let kinds: Vec<&str> = recs.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, vec!["replicaset_rolled"]);
    }

    #[test]
    fn rollouts_emits_image_change_when_container_added() {
        let now = Utc::now();
        let prior_at = now - Duration::seconds(7200);
        let new_at = now - Duration::seconds(60);
        let window = Window::new(now - Duration::seconds(3600), now);
        let prior = rs("prior", "d", "1", prior_at, &[("a", "img-a:1")]);
        let new = rs(
            "new",
            "d",
            "2",
            new_at,
            &[("a", "img-a:1"), ("b", "img-b:1")],
        );
        let recs = rollouts_and_image_changes(&[prior, new], "ns", &window);
        // a unchanged → no image_changed for a; b new → one image_changed.
        let img_changes: Vec<&Record> = recs.iter().filter(|r| r.kind == "image_changed").collect();
        assert_eq!(img_changes.len(), 1);
        assert_eq!(img_changes[0].attrs["container"], "b");
        // `before` has no `image` field (prior didn't have container `b`).
        assert!(img_changes[0].before.get("image").is_none());
    }

    #[test]
    fn make_config_record_attaches_consumers() {
        let now = Utc::now();
        let consumers = vec![Consumer {
            kind: "Pod",
            name: "p1".into(),
            via: "envFrom",
            container: Some("agent".into()),
        }];
        let r = make_config_record(
            "Secret",
            "ns",
            "creds",
            Some("123"),
            Some("argocd".into()),
            now,
            &consumers,
            Kind::SecretRotated,
        );
        assert_eq!(r.kind, "secret_rotated");
        assert_eq!(r.ref_["name"], "creds");
        assert_eq!(r.after["resource_version"], "123");
        assert_eq!(r.attrs["manager"], "argocd");
        assert_eq!(r.attrs["consumed_by"][0]["name"], "p1");
        assert_eq!(r.attrs["consumed_by"][0]["via"], "envFrom");
    }

    #[test]
    fn parse_include_validates_kind_names() {
        let s = parse_include("secret_rotated,image_changed").unwrap();
        assert!(s.contains(&Kind::SecretRotated));
        assert!(s.contains(&Kind::ImageChanged));
        assert!(parse_include("not_a_real_kind").is_err());
    }

    #[test]
    fn parse_duration_basics() {
        assert_eq!(parse_duration_seconds("30m").unwrap(), 1800);
        assert!(parse_duration_seconds("nope").is_err());
    }

    #[test]
    fn creation_record_only_inside_window() {
        let now = Utc::now();
        let window = Window::new(now - Duration::seconds(3600), now);
        let outside_meta = ObjectMeta {
            name: Some("old".into()),
            creation_timestamp: Some(Time(now - Duration::seconds(7200))),
            ..Default::default()
        };
        let inside_meta = ObjectMeta {
            name: Some("new".into()),
            creation_timestamp: Some(Time(now - Duration::seconds(60))),
            ..Default::default()
        };
        assert!(creation_record_for("Deployment", "ns", &outside_meta, &window).is_none());
        let rec = creation_record_for("Deployment", "ns", &inside_meta, &window).unwrap();
        assert_eq!(rec.kind, "resource_created");
        assert_eq!(rec.ref_["kind"], "Deployment");
        assert_eq!(rec.ref_["name"], "new");
    }

    #[test]
    fn build_consumer_index_collects_pods_by_secret_and_cm() {
        use k8s_openapi::api::core::v1::{EnvFromSource, SecretEnvSource};
        let pods = vec![Pod {
            metadata: ObjectMeta {
                name: Some("p1".into()),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "agent".into(),
                    env_from: Some(vec![EnvFromSource {
                        prefix: None,
                        secret_ref: Some(SecretEnvSource {
                            name: "creds".into(),
                            optional: None,
                        }),
                        config_map_ref: None,
                    }]),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            status: None,
        }];
        let (sec, cm) = build_consumer_index(&pods);
        assert_eq!(sec["creds"].len(), 1);
        assert_eq!(sec["creds"][0].name, "p1");
        assert_eq!(sec["creds"][0].container.as_deref(), Some("agent"));
        assert!(cm.is_empty());
    }
}
