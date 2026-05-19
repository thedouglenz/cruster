//! `cruster why-pending <pod>` — diagnose why a Pod is stuck in Pending.
//!
//! Performs structural inspection of the pod status, events, and
//! referenced PVCs to determine the most likely cause.

use std::io::Write;

use k8s_openapi::api::core::v1::{Event, PersistentVolumeClaim, Pod};
use kube::{Api, Client};
use serde::Serialize;

use cruster_core::diagnose::pending::{
    diagnose_pending, Confidence, ContainerStatus, DiagnoseError, EventInfo, Evidence,
    PendingReason, PodCondition, PodInfo, PvcInfo,
};

use crate::args::{Cli, WhyPendingArgs};
use crate::format::write_records;
use crate::output::{effective_format, stdout_is_tty};
use crate::verbs::describe::parse_reference;
use crate::verbs::get::canonicalise_kind;

#[derive(Debug, Serialize)]
struct DiagnosisRecord {
    pod: String,
    namespace: String,
    reason: PendingReason,
    confidence: Confidence,
    description: String,
    evidence: Vec<Evidence>,
    suggestion: String,
}

#[derive(Debug, Serialize)]
struct ErrorRecord {
    error: String,
    pod: String,
    namespace: String,
}

pub async fn run(cli: &Cli, args: &WhyPendingArgs) -> anyhow::Result<()> {
    let (raw_kind, name) = parse_reference(&args.reference)?;
    let kind_plural =
        canonicalise_kind(raw_kind).ok_or_else(|| anyhow::anyhow!("unknown kind: {raw_kind}"))?;

    if kind_plural != "pods" {
        anyhow::bail!("why-pending only supports pods, got: {}", raw_kind);
    }

    let client = Client::try_default().await?;
    let ns = args.namespace.as_deref().unwrap_or("default");

    let pod_api: Api<Pod> = Api::namespaced(client.clone(), ns);
    let pod = pod_api.get(name).await?;

    let pod_info = extract_pod_info(&pod);
    let events = fetch_pod_events(&client, name, ns).await?;
    let pvcs = fetch_pvcs(&client, ns, &pod_info.volume_claims).await?;

    let format = effective_format(cli.format, cli.llm, stdout_is_tty());
    let mut stdout = std::io::stdout().lock();

    match diagnose_pending(&pod_info, &events, &pvcs) {
        Ok(diag) => {
            let record = DiagnosisRecord {
                pod: name.to_string(),
                namespace: ns.to_string(),
                reason: diag.reason,
                confidence: diag.confidence,
                description: diag.reason.description().to_string(),
                evidence: diag.evidence,
                suggestion: diag.suggestion,
            };
            write_records(&mut stdout, format, &[record], |w, records| {
                for r in records {
                    writeln!(w, "Pod: {}/{}", r.namespace, r.pod)?;
                    writeln!(w, "Reason: {:?}", r.reason)?;
                    writeln!(w, "Confidence: {:?}", r.confidence)?;
                    writeln!(w, "Description: {}", r.description)?;
                    writeln!(w)?;
                    if !r.evidence.is_empty() {
                        writeln!(w, "Evidence:")?;
                        for ev in &r.evidence {
                            writeln!(w, "  [{:?}] {}", ev.source, ev.detail)?;
                        }
                        writeln!(w)?;
                    }
                    writeln!(w, "Suggestion: {}", r.suggestion)?;
                }
                Ok(())
            })?;
        }
        Err(DiagnoseError::NotPending { actual_phase }) => {
            let record = ErrorRecord {
                error: format!(
                    "Pod is not Pending (current phase: {}). Use a different diagnostic.",
                    actual_phase
                ),
                pod: name.to_string(),
                namespace: ns.to_string(),
            };
            write_records(&mut stdout, format, &[record], |w, records| {
                for r in records {
                    writeln!(w, "Error: {}", r.error)?;
                }
                Ok(())
            })?;
            std::process::exit(1);
        }
    }

    Ok(())
}

fn extract_pod_info(pod: &Pod) -> PodInfo {
    let status = pod.status.as_ref();
    let phase = status.and_then(|s| s.phase.clone());

    let conditions = status
        .map(|s| {
            s.conditions
                .as_ref()
                .map(|conds| {
                    conds
                        .iter()
                        .map(|c| PodCondition {
                            type_: c.type_.clone(),
                            status: c.status.clone(),
                            reason: c.reason.clone(),
                            message: c.message.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();

    let container_statuses = status
        .map(|s| {
            let mut statuses = Vec::new();
            if let Some(css) = &s.container_statuses {
                for cs in css {
                    if let Some(waiting) = cs.state.as_ref().and_then(|s| s.waiting.as_ref()) {
                        statuses.push(ContainerStatus {
                            name: cs.name.clone(),
                            waiting_reason: waiting.reason.clone(),
                            waiting_message: waiting.message.clone(),
                        });
                    }
                }
            }
            if let Some(ics) = &s.init_container_statuses {
                for cs in ics {
                    if let Some(waiting) = cs.state.as_ref().and_then(|s| s.waiting.as_ref()) {
                        statuses.push(ContainerStatus {
                            name: cs.name.clone(),
                            waiting_reason: waiting.reason.clone(),
                            waiting_message: waiting.message.clone(),
                        });
                    }
                }
            }
            statuses
        })
        .unwrap_or_default();

    let volume_claims = pod
        .spec
        .as_ref()
        .map(|spec| {
            spec.volumes
                .as_ref()
                .map(|vols| {
                    vols.iter()
                        .filter_map(|v| {
                            v.persistent_volume_claim
                                .as_ref()
                                .map(|pvc| pvc.claim_name.clone())
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();

    PodInfo {
        phase,
        conditions,
        container_statuses,
        volume_claims,
    }
}

async fn fetch_pod_events(
    client: &Client,
    pod_name: &str,
    namespace: &str,
) -> anyhow::Result<Vec<EventInfo>> {
    let api: Api<Event> = Api::namespaced(client.clone(), namespace);
    let events = api.list(&Default::default()).await?.items;

    Ok(events
        .into_iter()
        .filter(|e| {
            e.involved_object.kind.as_deref() == Some("Pod")
                && e.involved_object.name.as_deref() == Some(pod_name)
        })
        .map(|e| EventInfo {
            reason: e.reason.unwrap_or_default(),
            message: e.message.unwrap_or_default(),
            type_: e.type_.unwrap_or_default(),
        })
        .collect())
}

async fn fetch_pvcs(
    client: &Client,
    namespace: &str,
    claim_names: &[String],
) -> anyhow::Result<Vec<PvcInfo>> {
    if claim_names.is_empty() {
        return Ok(vec![]);
    }

    let api: Api<PersistentVolumeClaim> = Api::namespaced(client.clone(), namespace);
    let mut pvcs = Vec::new();

    for name in claim_names {
        if let Ok(pvc) = api.get(name).await {
            let phase = pvc
                .status
                .as_ref()
                .and_then(|s| s.phase.clone())
                .unwrap_or_default();
            pvcs.push(PvcInfo {
                name: name.clone(),
                phase,
            });
        }
    }

    Ok(pvcs)
}
