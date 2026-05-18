//! Pure diagnosis functions for CrashLoopBackOff pods.
//!
//! All functions take raw Kubernetes data structures and return a
//! structured diagnosis. No I/O — callers fetch the pod and logs.

use k8s_openapi::api::core::v1::Pod;
use serde::{Deserialize, Serialize};

use super::log_signals::{detect_signal_in_lines, LogSignal};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashloopReason {
    OomKilled,
    AuthFailure,
    ConnectivityFailure,
    PermissionDenied,
    MissingPath,
    ConfigMissing,
    ImagePullFailure,
    Unknown,
}

impl CrashloopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            CrashloopReason::OomKilled => "oom_killed",
            CrashloopReason::AuthFailure => "auth_failure",
            CrashloopReason::ConnectivityFailure => "connectivity_failure",
            CrashloopReason::PermissionDenied => "permission_denied",
            CrashloopReason::MissingPath => "missing_path",
            CrashloopReason::ConfigMissing => "config_missing",
            CrashloopReason::ImagePullFailure => "image_pull_failure",
            CrashloopReason::Unknown => "unknown",
        }
    }

    pub fn suggested_fix(self) -> &'static str {
        match self {
            CrashloopReason::OomKilled => {
                "Increase memory limit in pod spec or investigate memory leak"
            }
            CrashloopReason::AuthFailure => {
                "Check API key/token in referenced Secret; verify credentials are valid"
            }
            CrashloopReason::ConnectivityFailure => {
                "Verify network policies, service endpoints, and DNS resolution"
            }
            CrashloopReason::PermissionDenied => {
                "Check securityContext, volume mount permissions, or RBAC"
            }
            CrashloopReason::MissingPath => {
                "Verify ConfigMap/Secret mounts exist and paths are correct"
            }
            CrashloopReason::ConfigMissing => {
                "Check that referenced ConfigMaps/Secrets exist in the namespace"
            }
            CrashloopReason::ImagePullFailure => {
                "Verify image name/tag and imagePullSecrets configuration"
            }
            CrashloopReason::Unknown => "Examine full logs with `cruster logs --previous`",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Evidence {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_log_line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_limit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apiserver_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhyCrashloop {
    pub pod: String,
    pub phase: String,
    pub reason: CrashloopReason,
    pub evidence: Evidence,
    pub suggested_fix: String,
}

pub fn diagnose(pod: &Pod, logs: &[String]) -> WhyCrashloop {
    let ns = pod.metadata.namespace.as_deref().unwrap_or("default");
    let name = pod.metadata.name.as_deref().unwrap_or("unknown");
    let pod_ref = format!("{ns}/{name}");

    let phase = pod
        .status
        .as_ref()
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "Unknown".to_string());

    let status = pod.status.as_ref();
    let container_statuses = status.and_then(|s| s.container_statuses.as_ref());

    let first_crashing = container_statuses.and_then(|cs| {
        cs.iter().find(|c| {
            c.state
                .as_ref()
                .map(|s| s.waiting.is_some())
                .unwrap_or(false)
                || c.restart_count > 0
        })
    });

    let container_name = first_crashing.map(|c| c.name.clone());
    let restart_count = first_crashing.map(|c| c.restart_count);

    let last_terminated =
        first_crashing.and_then(|c| c.last_state.as_ref().and_then(|ls| ls.terminated.as_ref()));

    let exit_code = last_terminated.map(|t| t.exit_code);
    let terminated_reason = last_terminated.and_then(|t| t.reason.clone());

    let waiting_state =
        first_crashing.and_then(|c| c.state.as_ref().and_then(|s| s.waiting.as_ref()));
    let waiting_reason = waiting_state.and_then(|w| w.reason.clone());
    let waiting_message = waiting_state.and_then(|w| w.message.clone());

    let memory_limit = extract_memory_limit(pod, container_name.as_deref());
    let image = extract_image(pod, container_name.as_deref());

    let mut evidence = Evidence {
        container: container_name.clone(),
        exit_code,
        restart_count,
        last_log_line: logs.last().cloned(),
        memory_limit,
        image: image.clone(),
        apiserver_error: None,
    };

    let reason = diagnose_reason(
        terminated_reason.as_deref(),
        exit_code,
        waiting_reason.as_deref(),
        waiting_message.as_deref(),
        logs,
        &mut evidence,
    );

    WhyCrashloop {
        pod: pod_ref,
        phase,
        reason,
        suggested_fix: reason.suggested_fix().to_string(),
        evidence,
    }
}

fn diagnose_reason(
    terminated_reason: Option<&str>,
    exit_code: Option<i32>,
    waiting_reason: Option<&str>,
    waiting_message: Option<&str>,
    logs: &[String],
    evidence: &mut Evidence,
) -> CrashloopReason {
    if terminated_reason == Some("OOMKilled") {
        return CrashloopReason::OomKilled;
    }

    if let Some(wr) = waiting_reason {
        if wr == "CreateContainerConfigError" || wr == "CreateContainerError" {
            evidence.apiserver_error = waiting_message.map(|s| s.to_string());
            return CrashloopReason::ConfigMissing;
        }
        if wr == "ImagePullBackOff" || wr == "ErrImagePull" {
            evidence.apiserver_error = waiting_message.map(|s| s.to_string());
            return CrashloopReason::ImagePullFailure;
        }
    }

    if let Some(tr) = terminated_reason {
        if tr.contains("ImagePullBackOff") || tr.contains("ErrImagePull") {
            return CrashloopReason::ImagePullFailure;
        }
    }

    if exit_code.is_some() && exit_code != Some(0) {
        if let Some((signal, line)) = detect_signal_in_lines(logs) {
            evidence.last_log_line = Some(line);
            return match signal {
                LogSignal::AuthFailure => CrashloopReason::AuthFailure,
                LogSignal::ConnectivityFailure => CrashloopReason::ConnectivityFailure,
                LogSignal::PermissionDenied => CrashloopReason::PermissionDenied,
                LogSignal::MissingPath => CrashloopReason::MissingPath,
            };
        }
    }

    CrashloopReason::Unknown
}

fn extract_memory_limit(pod: &Pod, container_name: Option<&str>) -> Option<String> {
    let spec = pod.spec.as_ref()?;
    let containers = &spec.containers;
    let container = match container_name {
        Some(name) => containers.iter().find(|c| c.name == name),
        None => containers.first(),
    }?;
    let resources = container.resources.as_ref()?;
    let limits = resources.limits.as_ref()?;
    let mem = limits.get("memory")?;
    Some(mem.0.clone())
}

fn extract_image(pod: &Pod, container_name: Option<&str>) -> Option<String> {
    let spec = pod.spec.as_ref()?;
    let containers = &spec.containers;
    let container = match container_name {
        Some(name) => containers.iter().find(|c| c.name == name),
        None => containers.first(),
    }?;
    container.image.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        Container, ContainerState, ContainerStateTerminated, ContainerStateWaiting,
        ContainerStatus, PodSpec, PodStatus, ResourceRequirements,
    };
    use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    #[allow(clippy::too_many_arguments)]
    fn make_pod(
        name: &str,
        ns: &str,
        container_name: &str,
        terminated_reason: Option<&str>,
        exit_code: Option<i32>,
        waiting_reason: Option<&str>,
        waiting_message: Option<&str>,
        memory_limit: Option<&str>,
        restart_count: i32,
    ) -> Pod {
        let mut limits = BTreeMap::new();
        if let Some(mem) = memory_limit {
            limits.insert("memory".to_string(), Quantity(mem.to_string()));
        }

        let resources = if limits.is_empty() {
            None
        } else {
            Some(ResourceRequirements {
                limits: Some(limits),
                ..Default::default()
            })
        };

        let terminated = terminated_reason.map(|r| ContainerStateTerminated {
            reason: Some(r.to_string()),
            exit_code: exit_code.unwrap_or(1),
            ..Default::default()
        });

        let waiting = waiting_reason.map(|r| ContainerStateWaiting {
            reason: Some(r.to_string()),
            message: waiting_message.map(|s| s.to_string()),
        });

        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(ns.to_string()),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: container_name.to_string(),
                    image: Some("myapp:v1".to_string()),
                    resources,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            status: Some(PodStatus {
                phase: Some("Running".to_string()),
                container_statuses: Some(vec![ContainerStatus {
                    name: container_name.to_string(),
                    restart_count,
                    ready: false,
                    image: "myapp:v1".to_string(),
                    image_id: "sha256:abc".to_string(),
                    state: Some(ContainerState {
                        waiting: waiting.clone(),
                        ..Default::default()
                    }),
                    last_state: Some(ContainerState {
                        terminated,
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn oom_killed_when_last_state_says_oomkilled() {
        let pod = make_pod(
            "at-foo",
            "default",
            "agent",
            Some("OOMKilled"),
            Some(137),
            None,
            None,
            Some("512Mi"),
            3,
        );
        let result = diagnose(&pod, &[]);
        assert_eq!(result.reason, CrashloopReason::OomKilled);
        assert_eq!(result.evidence.memory_limit, Some("512Mi".to_string()));
        assert_eq!(result.evidence.exit_code, Some(137));
    }

    #[test]
    fn auth_failure_when_log_contains_401() {
        let pod = make_pod(
            "at-foo",
            "default",
            "agent",
            Some("Error"),
            Some(1),
            None,
            None,
            None,
            2,
        );
        let logs = vec![
            "starting agent...".to_string(),
            "LLM returned 401: invalid x-api-key".to_string(),
        ];
        let result = diagnose(&pod, &logs);
        assert_eq!(result.reason, CrashloopReason::AuthFailure);
        assert!(result.evidence.last_log_line.unwrap().contains("401"));
    }

    #[test]
    fn connectivity_failure_on_connection_refused() {
        let pod = make_pod(
            "backend",
            "prod",
            "app",
            Some("Error"),
            Some(1),
            None,
            None,
            None,
            5,
        );
        let logs = vec!["dial tcp 10.0.0.1:5432: connection refused".to_string()];
        let result = diagnose(&pod, &logs);
        assert_eq!(result.reason, CrashloopReason::ConnectivityFailure);
    }

    #[test]
    fn permission_denied_on_eacces() {
        let pod = make_pod(
            "worker",
            "default",
            "main",
            Some("Error"),
            Some(1),
            None,
            None,
            None,
            1,
        );
        let logs = vec!["EACCES: cannot write to /data/output".to_string()];
        let result = diagnose(&pod, &logs);
        assert_eq!(result.reason, CrashloopReason::PermissionDenied);
    }

    #[test]
    fn missing_path_on_enoent() {
        let pod = make_pod(
            "api",
            "default",
            "server",
            Some("Error"),
            Some(1),
            None,
            None,
            None,
            3,
        );
        let logs = vec!["config: no such file or directory: /etc/config/app.yaml".to_string()];
        let result = diagnose(&pod, &logs);
        assert_eq!(result.reason, CrashloopReason::MissingPath);
    }

    #[test]
    fn config_missing_on_create_container_config_error() {
        let pod = make_pod(
            "myapp",
            "staging",
            "app",
            None,
            None,
            Some("CreateContainerConfigError"),
            Some("secret \"db-creds\" not found"),
            None,
            0,
        );
        let result = diagnose(&pod, &[]);
        assert_eq!(result.reason, CrashloopReason::ConfigMissing);
        assert_eq!(
            result.evidence.apiserver_error,
            Some("secret \"db-creds\" not found".to_string())
        );
    }

    #[test]
    fn image_pull_failure_on_errimagepull() {
        let pod = make_pod(
            "myapp",
            "default",
            "app",
            None,
            None,
            Some("ErrImagePull"),
            Some("pull access denied for private.registry/image"),
            None,
            0,
        );
        let result = diagnose(&pod, &[]);
        assert_eq!(result.reason, CrashloopReason::ImagePullFailure);
    }

    #[test]
    fn unknown_when_no_signal() {
        let pod = make_pod(
            "mystery",
            "default",
            "app",
            Some("Error"),
            Some(1),
            None,
            None,
            None,
            10,
        );
        let logs = vec![
            "INFO: processing batch".to_string(),
            "DEBUG: completed step 1".to_string(),
        ];
        let result = diagnose(&pod, &logs);
        assert_eq!(result.reason, CrashloopReason::Unknown);
        assert_eq!(result.evidence.restart_count, Some(10));
        assert!(result.evidence.last_log_line.is_some());
    }

    #[test]
    fn pod_ref_includes_namespace() {
        let pod = make_pod(
            "at-foo",
            "agents",
            "agent",
            Some("OOMKilled"),
            Some(137),
            None,
            None,
            None,
            1,
        );
        let result = diagnose(&pod, &[]);
        assert_eq!(result.pod, "agents/at-foo");
    }
}
