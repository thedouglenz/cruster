//! Diagnose why a Pod is stuck in Pending state.
//!
//! Heuristics (matched in order):
//! - `unschedulable_no_nodes`: PodScheduled=False with "0/N nodes are available"
//! - `taints`: event message contains "had taint" or "toleration"
//! - `affinity`: event message contains "node affinity", "node selector", or "didn't match"
//! - `resources`: event message contains "Insufficient cpu", "Insufficient memory", or "Insufficient"
//! - `pvc_unbound`: pod references a PVC that is itself in Pending state
//! - `image_pull_failure`: container waiting with reason ImagePullBackOff or ErrImagePull
//! - `missing_serviceaccount`: event message contains "serviceaccount" and "not found"
//! - `unknown`: fallback when nothing else matched

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingReason {
    UnschedulableNoNodes,
    Taints,
    Affinity,
    Resources,
    PvcUnbound,
    ImagePullFailure,
    MissingServiceaccount,
    Unknown,
}

impl PendingReason {
    pub fn description(&self) -> &'static str {
        match self {
            Self::UnschedulableNoNodes => "No nodes available for scheduling",
            Self::Taints => "Node taints prevent scheduling",
            Self::Affinity => "Node affinity/selector constraints not satisfied",
            Self::Resources => "Insufficient CPU or memory on available nodes",
            Self::PvcUnbound => "Referenced PVC is not bound",
            Self::ImagePullFailure => "Container image pull failed",
            Self::MissingServiceaccount => "ServiceAccount not found",
            Self::Unknown => "Cause unknown from available data",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingDiagnosis {
    pub reason: PendingReason,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
    pub suggestion: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub source: EvidenceSource,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    Event,
    PodCondition,
    ContainerStatus,
    PvcStatus,
}

pub struct PodInfo {
    pub phase: Option<String>,
    pub conditions: Vec<PodCondition>,
    pub container_statuses: Vec<ContainerStatus>,
    pub volume_claims: Vec<String>,
}

pub struct PodCondition {
    pub type_: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
}

pub struct ContainerStatus {
    pub name: String,
    pub waiting_reason: Option<String>,
    pub waiting_message: Option<String>,
}

pub struct EventInfo {
    pub reason: String,
    pub message: String,
    pub type_: String,
}

pub struct PvcInfo {
    pub name: String,
    pub phase: String,
}

pub fn diagnose_pending(
    pod: &PodInfo,
    events: &[EventInfo],
    pvcs: &[PvcInfo],
) -> Result<PendingDiagnosis, DiagnoseError> {
    let phase = pod.phase.as_deref().unwrap_or("");
    if phase != "Pending" {
        return Err(DiagnoseError::NotPending {
            actual_phase: phase.to_string(),
        });
    }

    let mut evidence = Vec::new();

    if let Some(diag) = check_image_pull_failure(pod, &mut evidence) {
        return Ok(diag);
    }

    if let Some(diag) = check_pvc_unbound(pod, pvcs, &mut evidence) {
        return Ok(diag);
    }

    // Check specific scheduling reasons first, before the generic "no nodes" check
    if let Some(diag) = check_taints(events, &mut evidence) {
        return Ok(diag);
    }

    if let Some(diag) = check_affinity(events, &mut evidence) {
        return Ok(diag);
    }

    if let Some(diag) = check_resources(events, &mut evidence) {
        return Ok(diag);
    }

    if let Some(diag) = check_missing_serviceaccount(events, &mut evidence) {
        return Ok(diag);
    }

    // Generic unschedulable check comes last among event-based checks
    if let Some(diag) = check_unschedulable_no_nodes(pod, events, &mut evidence) {
        return Ok(diag);
    }

    Ok(PendingDiagnosis {
        reason: PendingReason::Unknown,
        confidence: Confidence::Low,
        evidence,
        suggestion: "Check pod events and cluster state manually".to_string(),
    })
}

fn check_image_pull_failure(
    pod: &PodInfo,
    evidence: &mut Vec<Evidence>,
) -> Option<PendingDiagnosis> {
    for cs in &pod.container_statuses {
        let reason = cs.waiting_reason.as_deref().unwrap_or("");
        if reason == "ImagePullBackOff" || reason == "ErrImagePull" {
            let detail = cs
                .waiting_message
                .clone()
                .unwrap_or_else(|| format!("Container '{}' waiting: {}", cs.name, reason));
            evidence.push(Evidence {
                source: EvidenceSource::ContainerStatus,
                detail,
            });
            return Some(PendingDiagnosis {
                reason: PendingReason::ImagePullFailure,
                confidence: Confidence::High,
                evidence: evidence.clone(),
                suggestion: "Check image name, tag, and registry credentials".to_string(),
            });
        }
    }
    None
}

fn check_pvc_unbound(
    pod: &PodInfo,
    pvcs: &[PvcInfo],
    evidence: &mut Vec<Evidence>,
) -> Option<PendingDiagnosis> {
    for claim_name in &pod.volume_claims {
        if let Some(pvc) = pvcs.iter().find(|p| &p.name == claim_name) {
            if pvc.phase == "Pending" {
                evidence.push(Evidence {
                    source: EvidenceSource::PvcStatus,
                    detail: format!("PVC '{}' is in Pending state", claim_name),
                });
                return Some(PendingDiagnosis {
                    reason: PendingReason::PvcUnbound,
                    confidence: Confidence::High,
                    evidence: evidence.clone(),
                    suggestion: format!(
                        "Check PVC '{}' - ensure a PV is available or dynamic provisioning works",
                        claim_name
                    ),
                });
            }
        }
    }
    None
}

fn check_unschedulable_no_nodes(
    pod: &PodInfo,
    events: &[EventInfo],
    evidence: &mut Vec<Evidence>,
) -> Option<PendingDiagnosis> {
    for cond in &pod.conditions {
        if cond.type_ == "PodScheduled" && cond.status == "False" {
            let msg = cond.message.as_deref().unwrap_or("");
            if msg.contains("0/") && msg.contains("nodes are available") {
                evidence.push(Evidence {
                    source: EvidenceSource::PodCondition,
                    detail: msg.to_string(),
                });
                return Some(PendingDiagnosis {
                    reason: PendingReason::UnschedulableNoNodes,
                    confidence: Confidence::High,
                    evidence: evidence.clone(),
                    suggestion: "No schedulable nodes; check node status and taints".to_string(),
                });
            }
        }
    }

    for ev in events {
        let msg_lower = ev.message.to_lowercase();
        if msg_lower.contains("0/") && msg_lower.contains("nodes are available") {
            evidence.push(Evidence {
                source: EvidenceSource::Event,
                detail: ev.message.clone(),
            });
            return Some(PendingDiagnosis {
                reason: PendingReason::UnschedulableNoNodes,
                confidence: Confidence::High,
                evidence: evidence.clone(),
                suggestion: "No schedulable nodes; check node status and taints".to_string(),
            });
        }
    }

    None
}

fn check_taints(events: &[EventInfo], evidence: &mut Vec<Evidence>) -> Option<PendingDiagnosis> {
    for ev in events {
        let msg_lower = ev.message.to_lowercase();
        if msg_lower.contains("had taint") || msg_lower.contains("node(s) had untolerated taint") {
            evidence.push(Evidence {
                source: EvidenceSource::Event,
                detail: ev.message.clone(),
            });
            return Some(PendingDiagnosis {
                reason: PendingReason::Taints,
                confidence: Confidence::High,
                evidence: evidence.clone(),
                suggestion: "Add tolerations to pod spec or remove taints from nodes".to_string(),
            });
        }
    }
    None
}

fn check_affinity(events: &[EventInfo], evidence: &mut Vec<Evidence>) -> Option<PendingDiagnosis> {
    for ev in events {
        let msg_lower = ev.message.to_lowercase();
        if msg_lower.contains("node affinity")
            || msg_lower.contains("node selector")
            || (msg_lower.contains("didn't match") && msg_lower.contains("node"))
        {
            evidence.push(Evidence {
                source: EvidenceSource::Event,
                detail: ev.message.clone(),
            });
            return Some(PendingDiagnosis {
                reason: PendingReason::Affinity,
                confidence: Confidence::High,
                evidence: evidence.clone(),
                suggestion: "Review nodeSelector/nodeAffinity rules or node labels".to_string(),
            });
        }
    }
    None
}

fn check_resources(events: &[EventInfo], evidence: &mut Vec<Evidence>) -> Option<PendingDiagnosis> {
    for ev in events {
        let msg_lower = ev.message.to_lowercase();
        if msg_lower.contains("insufficient cpu")
            || msg_lower.contains("insufficient memory")
            || (msg_lower.contains("insufficient") && ev.reason == "FailedScheduling")
        {
            evidence.push(Evidence {
                source: EvidenceSource::Event,
                detail: ev.message.clone(),
            });
            return Some(PendingDiagnosis {
                reason: PendingReason::Resources,
                confidence: Confidence::High,
                evidence: evidence.clone(),
                suggestion: "Reduce resource requests or add more capacity to the cluster"
                    .to_string(),
            });
        }
    }
    None
}

fn check_missing_serviceaccount(
    events: &[EventInfo],
    evidence: &mut Vec<Evidence>,
) -> Option<PendingDiagnosis> {
    for ev in events {
        let msg_lower = ev.message.to_lowercase();
        if (msg_lower.contains("serviceaccount") || msg_lower.contains("service account"))
            && msg_lower.contains("not found")
        {
            evidence.push(Evidence {
                source: EvidenceSource::Event,
                detail: ev.message.clone(),
            });
            return Some(PendingDiagnosis {
                reason: PendingReason::MissingServiceaccount,
                confidence: Confidence::High,
                evidence: evidence.clone(),
                suggestion: "Create the missing ServiceAccount or use an existing one".to_string(),
            });
        }
    }
    None
}

#[derive(Debug, Clone)]
pub enum DiagnoseError {
    NotPending { actual_phase: String },
}

impl std::fmt::Display for DiagnoseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotPending { actual_phase } => {
                write!(
                    f,
                    "Pod is not Pending (current phase: {}). Use a different diagnostic.",
                    actual_phase
                )
            }
        }
    }
}

impl std::error::Error for DiagnoseError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_pod() -> PodInfo {
        PodInfo {
            phase: Some("Pending".to_string()),
            conditions: vec![],
            container_statuses: vec![],
            volume_claims: vec![],
        }
    }

    #[test]
    fn rejects_non_pending_pod() {
        let pod = PodInfo {
            phase: Some("Running".to_string()),
            ..empty_pod()
        };
        let result = diagnose_pending(&pod, &[], &[]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, DiagnoseError::NotPending { .. }));
    }

    #[test]
    fn detects_image_pull_backoff() {
        let pod = PodInfo {
            container_statuses: vec![ContainerStatus {
                name: "main".to_string(),
                waiting_reason: Some("ImagePullBackOff".to_string()),
                waiting_message: Some("Back-off pulling image".to_string()),
            }],
            ..empty_pod()
        };
        let diag = diagnose_pending(&pod, &[], &[]).unwrap();
        assert_eq!(diag.reason, PendingReason::ImagePullFailure);
        assert_eq!(diag.confidence, Confidence::High);
    }

    #[test]
    fn detects_err_image_pull() {
        let pod = PodInfo {
            container_statuses: vec![ContainerStatus {
                name: "main".to_string(),
                waiting_reason: Some("ErrImagePull".to_string()),
                waiting_message: None,
            }],
            ..empty_pod()
        };
        let diag = diagnose_pending(&pod, &[], &[]).unwrap();
        assert_eq!(diag.reason, PendingReason::ImagePullFailure);
    }

    #[test]
    fn detects_pvc_unbound() {
        let pod = PodInfo {
            volume_claims: vec!["data-pvc".to_string()],
            ..empty_pod()
        };
        let pvcs = vec![PvcInfo {
            name: "data-pvc".to_string(),
            phase: "Pending".to_string(),
        }];
        let diag = diagnose_pending(&pod, &[], &pvcs).unwrap();
        assert_eq!(diag.reason, PendingReason::PvcUnbound);
        assert_eq!(diag.confidence, Confidence::High);
    }

    #[test]
    fn detects_unschedulable_no_nodes_from_condition() {
        let pod = PodInfo {
            conditions: vec![PodCondition {
                type_: "PodScheduled".to_string(),
                status: "False".to_string(),
                reason: Some("Unschedulable".to_string()),
                message: Some("0/3 nodes are available: 3 node(s) had taint".to_string()),
            }],
            ..empty_pod()
        };
        let diag = diagnose_pending(&pod, &[], &[]).unwrap();
        assert_eq!(diag.reason, PendingReason::UnschedulableNoNodes);
    }

    #[test]
    fn detects_unschedulable_no_nodes_from_event() {
        let pod = empty_pod();
        let events = vec![EventInfo {
            reason: "FailedScheduling".to_string(),
            message: "0/0 nodes are available".to_string(),
            type_: "Warning".to_string(),
        }];
        let diag = diagnose_pending(&pod, &events, &[]).unwrap();
        assert_eq!(diag.reason, PendingReason::UnschedulableNoNodes);
    }

    #[test]
    fn detects_taints() {
        let pod = empty_pod();
        let events = vec![EventInfo {
            reason: "FailedScheduling".to_string(),
            message:
                "0/1 nodes are available: 1 node(s) had untolerated taint {key=value:NoSchedule}"
                    .to_string(),
            type_: "Warning".to_string(),
        }];
        let diag = diagnose_pending(&pod, &events, &[]).unwrap();
        assert_eq!(diag.reason, PendingReason::Taints);
    }

    #[test]
    fn detects_affinity() {
        let pod = empty_pod();
        let events = vec![EventInfo {
            reason: "FailedScheduling".to_string(),
            message: "0/3 nodes are available: 3 node(s) didn't match Pod's node affinity/selector"
                .to_string(),
            type_: "Warning".to_string(),
        }];
        let diag = diagnose_pending(&pod, &events, &[]).unwrap();
        assert_eq!(diag.reason, PendingReason::Affinity);
    }

    #[test]
    fn detects_resources() {
        let pod = empty_pod();
        let events = vec![EventInfo {
            reason: "FailedScheduling".to_string(),
            message: "0/3 nodes are available: 3 Insufficient cpu".to_string(),
            type_: "Warning".to_string(),
        }];
        let diag = diagnose_pending(&pod, &events, &[]).unwrap();
        assert_eq!(diag.reason, PendingReason::Resources);
    }

    #[test]
    fn detects_missing_serviceaccount() {
        let pod = empty_pod();
        let events = vec![EventInfo {
            reason: "FailedCreate".to_string(),
            message: "error creating pod: pods \"test\" is forbidden: error looking up service account default/nonexistent: serviceaccount \"nonexistent\" not found".to_string(),
            type_: "Warning".to_string(),
        }];
        let diag = diagnose_pending(&pod, &events, &[]).unwrap();
        assert_eq!(diag.reason, PendingReason::MissingServiceaccount);
    }

    #[test]
    fn returns_unknown_when_no_match() {
        let pod = empty_pod();
        let diag = diagnose_pending(&pod, &[], &[]).unwrap();
        assert_eq!(diag.reason, PendingReason::Unknown);
        assert_eq!(diag.confidence, Confidence::Low);
    }

    #[test]
    fn image_pull_takes_priority_over_pvc() {
        let pod = PodInfo {
            volume_claims: vec!["data-pvc".to_string()],
            container_statuses: vec![ContainerStatus {
                name: "main".to_string(),
                waiting_reason: Some("ImagePullBackOff".to_string()),
                waiting_message: None,
            }],
            ..empty_pod()
        };
        let pvcs = vec![PvcInfo {
            name: "data-pvc".to_string(),
            phase: "Pending".to_string(),
        }];
        let diag = diagnose_pending(&pod, &[], &pvcs).unwrap();
        assert_eq!(diag.reason, PendingReason::ImagePullFailure);
    }
}
