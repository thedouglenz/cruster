//! `cruster timeline <pod-ref>` — merged chronological event stream
//! for a pod. Combines:
//!
//! - cluster events involving the pod (message text preserved verbatim);
//! - container restarts derived from `containerStatuses` (+ OOM kills);
//! - rotation timestamps of every Secret / ConfigMap the pod references
//!   via env, envFrom, or volumes (pulled from
//!   `metadata.managedFields[].time`).
//!
//! Output is NDJSON, one record per fact, chronological ascending. A
//! final `{"summary": ...}` record carries the window, per-kind
//! counts, `kinds_absent` (negative-evidence field — the LLM should
//! not re-check these), truncation flags, and `notes` for
//! cluster-config caveats (e.g. event TTL).
//!
//! v1 scope: pod refs only. Workload-level aggregation (deployment /
//! statefulset / daemonset → owned pods) is intentionally deferred.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Write;

use anyhow::{bail, Context as _};
use chrono::{DateTime, Duration, Utc};
use k8s_openapi::api::core::v1::{
    ConfigMap, Container, ContainerStatus, Event, Pod, PodSpec, Secret, Volume,
};
use kube::api::ListParams;
use kube::{Api, Client};
use serde::Serialize;
use serde_json::{json, Value};

use crate::args::{Cli, Format, TimelineArgs};
use crate::envelope::Window;
use crate::output::{effective_format, stdout_is_tty};
use crate::verbs::describe::parse_reference;
use crate::verbs::get::canonicalise_kind;

// ---------- record types ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Kind {
    Scheduled,
    Started,
    Killing,
    Pulled,
    PullFailed,
    MountFailed,
    ProbeFailed,
    Evicted,
    Restarted,
    OomKilled,
    SecretRotated,
    ConfigmapChanged,
    GenericEvent,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Scheduled => "scheduled",
            Kind::Started => "started",
            Kind::Killing => "killing",
            Kind::Pulled => "pulled",
            Kind::PullFailed => "pull_failed",
            Kind::MountFailed => "mount_failed",
            Kind::ProbeFailed => "probe_failed",
            Kind::Evicted => "evicted",
            Kind::Restarted => "restarted",
            Kind::OomKilled => "oom_killed",
            Kind::SecretRotated => "secret_rotated",
            Kind::ConfigmapChanged => "configmap_changed",
            Kind::GenericEvent => "generic_event",
        }
    }

    /// Catalog used to compute `kinds_absent`. Excludes `GenericEvent`
    /// because it is a fallback bucket, not a probe.
    fn catalog() -> &'static [Kind] {
        &[
            Kind::Scheduled,
            Kind::Started,
            Kind::Killing,
            Kind::Pulled,
            Kind::PullFailed,
            Kind::MountFailed,
            Kind::ProbeFailed,
            Kind::Evicted,
            Kind::Restarted,
            Kind::OomKilled,
            Kind::SecretRotated,
            Kind::ConfigmapChanged,
        ]
    }

    fn from_str(s: &str) -> Option<Kind> {
        let all = [
            Kind::Scheduled,
            Kind::Started,
            Kind::Killing,
            Kind::Pulled,
            Kind::PullFailed,
            Kind::MountFailed,
            Kind::ProbeFailed,
            Kind::Evicted,
            Kind::Restarted,
            Kind::OomKilled,
            Kind::SecretRotated,
            Kind::ConfigmapChanged,
            Kind::GenericEvent,
        ];
        all.into_iter().find(|k| k.as_str() == s)
    }
}

#[derive(Debug, Clone, Serialize)]
struct Record {
    at: DateTime<Utc>,
    kind: &'static str,
    severity: Severity,
    source: Value,
    #[serde(skip_serializing_if = "Value::is_null")]
    target: Value,
    #[serde(skip_serializing_if = "Value::is_null")]
    attrs: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct Summary {
    window: Window,
    counts: BTreeMap<&'static str, usize>,
    kinds_absent: Vec<&'static str>,
    truncated: TruncationFlags,
    notes: Vec<String>,
    ms_total: u128,
}

#[derive(Debug, Serialize)]
struct TruncationFlags {
    /// True if event list was capped by the cluster's event TTL (we
    /// can't tell exactly; surfaced as a heuristic note).
    events: bool,
}

#[derive(Debug, Serialize)]
struct SummaryEnvelope {
    summary: Summary,
}

// ---------- config reference extraction (pure) ----------

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ConfigKind {
    Secret,
    ConfigMap,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ConfigRef {
    kind: ConfigKind,
    name: String,
    /// How the pod references this object: `env`, `envFrom`, `volume`,
    /// `projected`. Used to populate the record's `target.via`.
    via: &'static str,
    /// Container name where the reference appears, if applicable.
    /// `None` for volume references (volumes are pod-scoped).
    container: Option<String>,
}

fn collect_config_refs(spec: &PodSpec) -> Vec<ConfigRef> {
    let mut out: BTreeSet<ConfigRef> = BTreeSet::new();
    for c in &spec.containers {
        collect_from_container(c, &mut out);
    }
    if let Some(inits) = &spec.init_containers {
        for c in inits {
            collect_from_container(c, &mut out);
        }
    }
    for v in spec.volumes.as_deref().unwrap_or(&[]) {
        collect_from_volume(v, &mut out);
    }
    out.into_iter().collect()
}

fn collect_from_container(c: &Container, out: &mut BTreeSet<ConfigRef>) {
    if let Some(envs) = &c.env {
        for e in envs {
            if let Some(vf) = &e.value_from {
                if let Some(sref) = &vf.secret_key_ref {
                    out.insert(ConfigRef {
                        kind: ConfigKind::Secret,
                        name: sref.name.clone(),
                        via: "env",
                        container: Some(c.name.clone()),
                    });
                }
                if let Some(cref) = &vf.config_map_key_ref {
                    out.insert(ConfigRef {
                        kind: ConfigKind::ConfigMap,
                        name: cref.name.clone(),
                        via: "env",
                        container: Some(c.name.clone()),
                    });
                }
            }
        }
    }
    if let Some(efs) = &c.env_from {
        for e in efs {
            if let Some(sref) = &e.secret_ref {
                out.insert(ConfigRef {
                    kind: ConfigKind::Secret,
                    name: sref.name.clone(),
                    via: "envFrom",
                    container: Some(c.name.clone()),
                });
            }
            if let Some(cref) = &e.config_map_ref {
                out.insert(ConfigRef {
                    kind: ConfigKind::ConfigMap,
                    name: cref.name.clone(),
                    via: "envFrom",
                    container: Some(c.name.clone()),
                });
            }
        }
    }
}

fn collect_from_volume(v: &Volume, out: &mut BTreeSet<ConfigRef>) {
    if let Some(s) = &v.secret {
        if let Some(name) = &s.secret_name {
            out.insert(ConfigRef {
                kind: ConfigKind::Secret,
                name: name.clone(),
                via: "volume",
                container: None,
            });
        }
    }
    if let Some(cm) = &v.config_map {
        out.insert(ConfigRef {
            kind: ConfigKind::ConfigMap,
            name: cm.name.clone(),
            via: "volume",
            container: None,
        });
    }
    if let Some(proj) = &v.projected {
        if let Some(sources) = &proj.sources {
            for src in sources {
                if let Some(s) = &src.secret {
                    out.insert(ConfigRef {
                        kind: ConfigKind::Secret,
                        name: s.name.clone(),
                        via: "projected",
                        container: None,
                    });
                }
                if let Some(cm) = &src.config_map {
                    out.insert(ConfigRef {
                        kind: ConfigKind::ConfigMap,
                        name: cm.name.clone(),
                        via: "projected",
                        container: None,
                    });
                }
            }
        }
    }
}

// ---------- event classification (pure) ----------

fn classify_event(e: &Event) -> Kind {
    let reason = e.reason.as_deref().unwrap_or("");
    let msg = e.message.as_deref().unwrap_or("");
    match reason {
        "Scheduled" => Kind::Scheduled,
        "Started" => Kind::Started,
        "Killing" => Kind::Killing,
        "Pulled" => Kind::Pulled,
        "Failed" | "ErrImagePull" | "ImagePullBackOff" if msg.to_ascii_lowercase().contains("pull") || reason != "Failed" => {
            Kind::PullFailed
        }
        "FailedMount" => Kind::MountFailed,
        "Unhealthy" if msg.contains("probe failed") => Kind::ProbeFailed,
        "Evicted" => Kind::Evicted,
        _ => Kind::GenericEvent,
    }
}

fn event_severity(kind: Kind, type_: Option<&str>) -> Severity {
    match kind {
        Kind::PullFailed | Kind::MountFailed | Kind::Evicted | Kind::OomKilled => Severity::Error,
        Kind::Killing | Kind::Restarted | Kind::ProbeFailed => Severity::Warning,
        Kind::Scheduled
        | Kind::Started
        | Kind::Pulled
        | Kind::SecretRotated
        | Kind::ConfigmapChanged => Severity::Info,
        Kind::GenericEvent => match type_.unwrap_or("Normal") {
            "Warning" => Severity::Warning,
            _ => Severity::Info,
        },
    }
}

// ---------- derived restart records (pure) ----------

fn derive_restart_records(
    pod_name: &str,
    ns: &str,
    statuses: &[ContainerStatus],
) -> Vec<Record> {
    let mut out = Vec::new();
    for cs in statuses {
        let Some(last) = cs.last_state.as_ref().and_then(|s| s.terminated.as_ref()) else {
            continue;
        };
        let Some(finished) = last.finished_at.as_ref() else {
            continue;
        };
        let at = finished.0;
        let reason = last.reason.clone().unwrap_or_default();
        let exit_code = last.exit_code;
        let signal = last.signal;
        let started_at = last.started_at.as_ref().map(|t| t.0);
        let alive = started_at.map(|s| at.signed_duration_since(s));

        let oom = reason == "OOMKilled";
        let kind = if oom { Kind::OomKilled } else { Kind::Restarted };
        let severity = if oom { Severity::Error } else { Severity::Warning };

        let mut attrs = json!({
            "restart_count": cs.restart_count,
            "prev_exit_code": exit_code,
            "prev_reason": reason,
        });
        if let Some(sig) = signal {
            attrs["signal"] = json!(sig);
        }
        if let Some(d) = alive {
            attrs["alive_seconds"] = json!(d.num_seconds().max(0));
        }

        out.push(Record {
            at,
            kind: kind.as_str(),
            severity,
            source: json!({
                "type": "derived",
                "jsonpath": format!(
                    ".status.containerStatuses[?(@.name=='{}')]",
                    cs.name
                ),
            }),
            target: json!({ "container": cs.name }),
            attrs,
            message: None,
        });
        let _ = (pod_name, ns); // currently unused but kept for future fmt
    }
    out
}

// ---------- config rotation records (needs I/O wrapper) ----------

fn config_record_from_metadata(
    kind: ConfigKind,
    name: &str,
    namespace: &str,
    refs_for: &[ConfigRef],
    managed_fields_max_time: Option<DateTime<Utc>>,
    resource_version: Option<&str>,
    window: &Window,
) -> Option<Record> {
    let at = managed_fields_max_time?;
    if at < window.from || at > window.to {
        return None;
    }
    let kind_str = match kind {
        ConfigKind::Secret => Kind::SecretRotated.as_str(),
        ConfigKind::ConfigMap => Kind::ConfigmapChanged.as_str(),
    };
    // Aggregate every reference (via + container) into the target so
    // the LLM can see at a glance how this config is consumed.
    let consumed: Vec<Value> = refs_for
        .iter()
        .map(|r| {
            let mut o = json!({ "via": r.via });
            if let Some(c) = &r.container {
                o["container"] = json!(c);
            }
            o
        })
        .collect();
    let kind_label = match kind {
        ConfigKind::Secret => "Secret",
        ConfigKind::ConfigMap => "ConfigMap",
    };
    let mut attrs = json!({
        "consumed_by": consumed,
    });
    if let Some(rv) = resource_version {
        attrs["resource_version"] = json!(rv);
    }
    Some(Record {
        at,
        kind: kind_str,
        severity: Severity::Info,
        source: json!({
            "type": "managed_fields",
            "ref": { "kind": kind_label, "namespace": namespace, "name": name },
        }),
        target: Value::Null,
        attrs,
        message: None,
    })
}

fn max_managed_field_time(managed_fields: Option<&[k8s_openapi::apimachinery::pkg::apis::meta::v1::ManagedFieldsEntry]>) -> Option<DateTime<Utc>> {
    managed_fields?
        .iter()
        .filter_map(|m| m.time.as_ref().map(|t| t.0))
        .max()
}

// ---------- entry point ----------

pub async fn run(cli: &Cli, args: &TimelineArgs) -> anyhow::Result<()> {
    let (raw_kind, name) = parse_reference(&args.reference)?;
    let kind_plural = canonicalise_kind(raw_kind)
        .ok_or_else(|| anyhow::anyhow!("unknown kind: {raw_kind}"))?;
    if kind_plural != "pods" {
        bail!(
            "cruster timeline v1 supports pod refs only (got {}); workload-level aggregation is on the roadmap",
            kind_plural
        );
    }
    let ns = args
        .namespace
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--namespace is required for timeline"))?;

    let since_secs = parse_duration_seconds(&args.since)?;
    let now = Utc::now();
    let window = Window::new(now - Duration::seconds(since_secs), now);

    let include_filter = args
        .include
        .as_deref()
        .map(parse_include)
        .transpose()?;

    let started = std::time::Instant::now();
    let client = Client::try_default().await?;

    let pod_api: Api<Pod> = Api::namespaced(client.clone(), ns);
    let pod = pod_api.get(name).await.context("fetching pod")?;
    let spec = pod.spec.as_ref().context("pod has no spec")?;
    let status = pod.status.clone();

    let mut records: Vec<Record> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    // 1. Events involving the pod.
    let event_api: Api<Event> = Api::namespaced(client.clone(), ns);
    let events = event_api.list(&ListParams::default()).await?.items;
    let pod_uid = pod.metadata.uid.as_deref();
    for e in &events {
        let matches_pod = e.involved_object.kind.as_deref() == Some("Pod")
            && e.involved_object.name.as_deref() == Some(name)
            && (pod_uid.is_none()
                || e.involved_object.uid.as_deref() == pod_uid
                || e.involved_object.uid.is_none());
        if !matches_pod {
            continue;
        }
        let at = event_timestamp(e);
        if at < window.from || at > window.to {
            continue;
        }
        let kind = classify_event(e);
        let severity = event_severity(kind, e.type_.as_deref());
        records.push(Record {
            at,
            kind: kind.as_str(),
            severity,
            source: json!({
                "type": "event",
                "uid": e.metadata.uid,
                "reason": e.reason,
                "event_type": e.type_,
            }),
            target: e
                .involved_object
                .field_path
                .as_ref()
                .map(|fp| json!({ "field_path": fp }))
                .unwrap_or(Value::Null),
            attrs: Value::Null,
            message: e.message.clone(),
        });
    }
    // Cluster event TTL is configurable (default 1h). If the window
    // exceeds 1h, surface that as a note rather than silently returning
    // a short list.
    if since_secs > 3600 {
        notes.push(
            "kube event TTL is cluster-configurable (default 1h); records older than the TTL are not visible".into(),
        );
    }

    // 2. Derived restart / oom records from containerStatuses.
    let mut restart_records = Vec::new();
    if let Some(st) = status.as_ref() {
        if let Some(cs) = &st.container_statuses {
            restart_records.extend(derive_restart_records(name, ns, cs));
        }
        if let Some(cs) = &st.init_container_statuses {
            restart_records.extend(derive_restart_records(name, ns, cs));
        }
    }
    restart_records.retain(|r| r.at >= window.from && r.at <= window.to);
    records.extend(restart_records);

    // 3. Config rotation timestamps. For each referenced Secret /
    //    ConfigMap, GET it and look at metadata.managedFields[].time.
    let config_refs = collect_config_refs(spec);
    let secret_api: Api<Secret> = Api::namespaced(client.clone(), ns);
    let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), ns);
    // Group refs by (kind, name) so we only GET once per object.
    let mut by_target: BTreeMap<(ConfigKind, String), Vec<ConfigRef>> = BTreeMap::new();
    for r in config_refs {
        by_target
            .entry((r.kind.clone(), r.name.clone()))
            .or_default()
            .push(r);
    }
    for ((kind, name), refs) in &by_target {
        let (mft, rv) = match kind {
            ConfigKind::Secret => match secret_api.get(name).await {
                Ok(s) => (
                    max_managed_field_time(s.metadata.managed_fields.as_deref()),
                    s.metadata.resource_version,
                ),
                Err(e) => {
                    notes.push(format!(
                        "could not fetch Secret/{name} (referenced by pod): {}",
                        short_err(&e)
                    ));
                    continue;
                }
            },
            ConfigKind::ConfigMap => match cm_api.get(name).await {
                Ok(s) => (
                    max_managed_field_time(s.metadata.managed_fields.as_deref()),
                    s.metadata.resource_version,
                ),
                Err(e) => {
                    notes.push(format!(
                        "could not fetch ConfigMap/{name} (referenced by pod): {}",
                        short_err(&e)
                    ));
                    continue;
                }
            },
        };
        if let Some(rec) = config_record_from_metadata(
            kind.clone(),
            name,
            ns,
            refs,
            mft,
            rv.as_deref(),
            &window,
        ) {
            records.push(rec);
        }
    }

    // Apply --include filter if set.
    if let Some(allow) = &include_filter {
        records.retain(|r| {
            Kind::from_str(r.kind)
                .map(|k| allow.contains(&k))
                .unwrap_or(false)
        });
    }

    // Chronological ascending sort.
    records.sort_by_key(|r| r.at);

    // Counts + kinds_absent.
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for r in &records {
        *counts.entry(r.kind).or_insert(0) += 1;
    }
    let present: HashSet<&'static str> = records.iter().map(|r| r.kind).collect();
    let mut kinds_absent: Vec<&'static str> = Kind::catalog()
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
            counts,
            kinds_absent,
            truncated: TruncationFlags { events: false },
            notes,
            ms_total: started.elapsed().as_millis(),
        },
    };
    emit_summary(&mut stdout, format, &summary)?;
    Ok(())
}

fn emit_record<W: Write>(out: &mut W, format: Format, r: &Record) -> std::io::Result<()> {
    match format {
        Format::Text => writeln!(
            out,
            "{}  [{}] {}  {}",
            r.at.to_rfc3339(),
            r.severity_str(),
            r.kind,
            r.message.as_deref().unwrap_or("-"),
        ),
        Format::Ndjson | Format::Json | Format::Yaml => {
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
                "scanned {}..{} in {} ms",
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

impl Record {
    fn severity_str(&self) -> &'static str {
        match self.severity {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

// ---------- small helpers ----------

fn event_timestamp(e: &Event) -> DateTime<Utc> {
    // Prefer last_timestamp (most recent occurrence); fall back to
    // event_time (microtime, newer API); then creationTimestamp.
    if let Some(t) = &e.last_timestamp {
        return t.0;
    }
    if let Some(t) = &e.event_time {
        return t.0;
    }
    if let Some(t) = &e.metadata.creation_timestamp {
        return t.0;
    }
    Utc::now()
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
                "unknown --include kind '{p}'; expected one of: {}",
                Kind::catalog()
                    .iter()
                    .chain(std::iter::once(&Kind::GenericEvent))
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
    Ok(out)
}

fn short_err(e: &kube::Error) -> String {
    let s = e.to_string();
    s.lines().next().unwrap_or(&s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        ConfigMapKeySelector, ConfigMapVolumeSource, EnvFromSource, EnvVar, EnvVarSource,
        SecretKeySelector, SecretVolumeSource,
    };

    fn pod_with_refs() -> Pod {
        let c = Container {
            name: "app".into(),
            env: Some(vec![
                EnvVar {
                    name: "TOKEN".into(),
                    value_from: Some(EnvVarSource {
                        secret_key_ref: Some(SecretKeySelector {
                            name: "api-creds".into(),
                            key: "token".into(),
                            optional: None,
                        }),
                        ..Default::default()
                    }),
                    value: None,
                },
                EnvVar {
                    name: "FEATURE_FLAG".into(),
                    value_from: Some(EnvVarSource {
                        config_map_key_ref: Some(ConfigMapKeySelector {
                            name: "features".into(),
                            key: "enabled".into(),
                            optional: None,
                        }),
                        ..Default::default()
                    }),
                    value: None,
                },
            ]),
            env_from: Some(vec![EnvFromSource {
                prefix: None,
                secret_ref: Some(k8s_openapi::api::core::v1::SecretEnvSource {
                    name: "bulk-secret".into(),
                    optional: None,
                }),
                config_map_ref: None,
            }]),
            ..Default::default()
        };
        let spec = PodSpec {
            containers: vec![c],
            volumes: Some(vec![Volume {
                name: "cfg".into(),
                config_map: Some(ConfigMapVolumeSource {
                    name: "file-cfg".into(),
                    ..Default::default()
                }),
                secret: Some(SecretVolumeSource {
                    secret_name: Some("file-sec".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }]),
            ..Default::default()
        };
        Pod {
            metadata: Default::default(),
            spec: Some(spec),
            status: None,
        }
    }

    #[test]
    fn collect_config_refs_walks_env_envfrom_and_volumes() {
        let pod = pod_with_refs();
        let refs = collect_config_refs(pod.spec.as_ref().unwrap());
        let names: Vec<(&str, &str)> = refs
            .iter()
            .map(|r| {
                (
                    match r.kind {
                        ConfigKind::Secret => "Secret",
                        ConfigKind::ConfigMap => "ConfigMap",
                    },
                    r.name.as_str(),
                )
            })
            .collect();
        assert!(names.contains(&("Secret", "api-creds")));
        assert!(names.contains(&("ConfigMap", "features")));
        assert!(names.contains(&("Secret", "bulk-secret")));
        assert!(names.contains(&("ConfigMap", "file-cfg")));
        assert!(names.contains(&("Secret", "file-sec")));
        // 5 distinct refs, no duplicates.
        assert_eq!(refs.len(), 5);
    }

    #[test]
    fn classify_event_maps_known_reasons() {
        let mk = |reason: &str, msg: &str| Event {
            reason: Some(reason.into()),
            message: Some(msg.into()),
            ..Default::default()
        };
        assert_eq!(classify_event(&mk("Scheduled", "")), Kind::Scheduled);
        assert_eq!(classify_event(&mk("Started", "")), Kind::Started);
        assert_eq!(classify_event(&mk("Killing", "")), Kind::Killing);
        assert_eq!(classify_event(&mk("Pulled", "")), Kind::Pulled);
        assert_eq!(
            classify_event(&mk("ErrImagePull", "")),
            Kind::PullFailed
        );
        assert_eq!(
            classify_event(&mk("ImagePullBackOff", "")),
            Kind::PullFailed
        );
        assert_eq!(classify_event(&mk("FailedMount", "")), Kind::MountFailed);
        assert_eq!(
            classify_event(&mk("Unhealthy", "Liveness probe failed: HTTP 500")),
            Kind::ProbeFailed
        );
        // Unhealthy without "probe failed" — still generic.
        assert_eq!(
            classify_event(&mk("Unhealthy", "container restarting")),
            Kind::GenericEvent
        );
        assert_eq!(classify_event(&mk("Evicted", "")), Kind::Evicted);
        assert_eq!(
            classify_event(&mk("SomeRandomReason", "")),
            Kind::GenericEvent
        );
    }

    #[test]
    fn event_severity_lifts_warning_type_to_warning() {
        assert_eq!(
            event_severity(Kind::GenericEvent, Some("Warning")),
            Severity::Warning
        );
        assert_eq!(
            event_severity(Kind::GenericEvent, Some("Normal")),
            Severity::Info
        );
        assert_eq!(event_severity(Kind::PullFailed, None), Severity::Error);
        assert_eq!(event_severity(Kind::OomKilled, None), Severity::Error);
        assert_eq!(event_severity(Kind::Started, None), Severity::Info);
    }

    #[test]
    fn derive_restart_records_emits_oom_when_reason_matches() {
        use k8s_openapi::api::core::v1::{ContainerState, ContainerStateTerminated};
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
        let now = Utc::now();
        let started = now - Duration::seconds(60);
        let cs = ContainerStatus {
            name: "agent".into(),
            ready: false,
            restart_count: 3,
            image: "x".into(),
            image_id: "x".into(),
            container_id: None,
            started: None,
            state: None,
            last_state: Some(ContainerState {
                terminated: Some(ContainerStateTerminated {
                    exit_code: 137,
                    reason: Some("OOMKilled".into()),
                    signal: Some(9),
                    started_at: Some(Time(started)),
                    finished_at: Some(Time(now)),
                    container_id: None,
                    message: None,
                }),
                ..Default::default()
            }),
            allocated_resources: None,
            resources: None,
            volume_mounts: None,
            user: None,
            allocated_resources_status: None,
        };
        let recs = derive_restart_records("pod-x", "ns", &[cs]);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].kind, "oom_killed");
        assert!(matches!(recs[0].severity, Severity::Error));
        assert_eq!(recs[0].attrs["prev_exit_code"], 137);
        assert_eq!(recs[0].attrs["signal"], 9);
        assert_eq!(recs[0].attrs["alive_seconds"], 60);
    }

    #[test]
    fn config_record_only_emitted_inside_window() {
        let now = Utc::now();
        let window = Window::new(now - Duration::seconds(3600), now);
        // Too old → None.
        let old = now - Duration::seconds(7200);
        assert!(config_record_from_metadata(
            ConfigKind::Secret,
            "creds",
            "ns",
            &[],
            Some(old),
            Some("123"),
            &window
        )
        .is_none());
        // Inside window → Some.
        let recent = now - Duration::seconds(60);
        let r = config_record_from_metadata(
            ConfigKind::Secret,
            "creds",
            "ns",
            &[ConfigRef {
                kind: ConfigKind::Secret,
                name: "creds".into(),
                via: "env",
                container: Some("agent".into()),
            }],
            Some(recent),
            Some("123"),
            &window,
        )
        .unwrap();
        assert_eq!(r.kind, "secret_rotated");
        assert_eq!(r.attrs["resource_version"], "123");
        assert_eq!(r.attrs["consumed_by"][0]["via"], "env");
        assert_eq!(r.attrs["consumed_by"][0]["container"], "agent");
    }

    #[test]
    fn parse_include_validates_kind_names() {
        let s = parse_include("restarted,oom_killed,secret_rotated").unwrap();
        assert!(s.contains(&Kind::Restarted));
        assert!(s.contains(&Kind::OomKilled));
        assert!(s.contains(&Kind::SecretRotated));
        let err = parse_include("not_a_real_kind").unwrap_err();
        assert!(err.to_string().contains("unknown --include kind"));
    }

    #[test]
    fn kind_catalog_is_complete_and_unique() {
        let names: HashSet<&'static str> =
            Kind::catalog().iter().map(|k| k.as_str()).collect();
        assert_eq!(names.len(), Kind::catalog().len(), "catalog has duplicates");
        assert!(!names.contains("generic_event"));
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration_seconds("30s").unwrap(), 30);
        assert_eq!(parse_duration_seconds("5m").unwrap(), 300);
        assert_eq!(parse_duration_seconds("2h").unwrap(), 7200);
        assert_eq!(parse_duration_seconds("1d").unwrap(), 86400);
        assert!(parse_duration_seconds("5x").is_err());
    }
}
