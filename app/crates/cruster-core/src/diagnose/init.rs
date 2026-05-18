//! Pure diagnosis functions for init-container failures.
//!
//! All functions take raw Kubernetes data structures and return a
//! structured diagnosis. No I/O — callers fetch the pod and logs.

use k8s_openapi::api::core::v1::Pod;
use serde::{Deserialize, Serialize};

use super::log_signals::{detect_signal_in_lines, LogSignal};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitFailureReason {
    InitOomKilled,
    AuthFailure,
    ConnectivityFailure,
    PermissionDenied,
    MissingPath,
    InitConfigMissing,
    InitImagePullFailure,
    InitStuckPending,
    Unknown,
}

impl InitFailureReason {
    pub fn as_str(self) -> &'static str {
        match self {
            InitFailureReason::InitOomKilled => "init_oom_killed",
            InitFailureReason::AuthFailure => "auth_failure",
            InitFailureReason::ConnectivityFailure => "connectivity_failure",
            InitFailureReason::PermissionDenied => "permission_denied",
            InitFailureReason::MissingPath => "missing_path",
            InitFailureReason::InitConfigMissing => "init_config_missing",
            InitFailureReason::InitImagePullFailure => "init_image_pull_failure",
            InitFailureReason::InitStuckPending => "init_stuck_pending",
            InitFailureReason::Unknown => "unknown",
        }
    }

    pub fn suggested_fix(self) -> &'static str {
        match self {
            InitFailureReason::InitOomKilled => {
                "Increase memory limit for the init container or reduce memory usage"
            }
            InitFailureReason::AuthFailure => {
                "Check API key/token in referenced Secret; verify credentials are valid"
            }
            InitFailureReason::ConnectivityFailure => {
                "Verify network policies, service endpoints, and DNS resolution"
            }
            InitFailureReason::PermissionDenied => {
                "Check securityContext, volume mount permissions, or RBAC"
            }
            InitFailureReason::MissingPath => {
                "Verify ConfigMap/Secret mounts exist and paths are correct"
            }
            InitFailureReason::InitConfigMissing => {
                "Check that referenced ConfigMaps/Secrets exist in the namespace"
            }
            InitFailureReason::InitImagePullFailure => {
                "Verify image name/tag and imagePullSecrets configuration"
            }
            InitFailureReason::InitStuckPending => {
                "Check scheduler events, node resources, and PVC bindings"
            }
            InitFailureReason::Unknown => "Examine init container logs with `cruster logs --container <init-name>`",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InitEvidence {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stuck_for: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhyInitFailure {
    pub pod: String,
    pub init_container: String,
    pub init_index: usize,
    pub reason: InitFailureReason,
    pub evidence: InitEvidence,
    pub suggested_fix: String,
}

#[derive(Debug)]
pub enum DiagnoseError {
    NoInitContainers,
    AllInitSucceeded,
}

pub fn diagnose(pod: &Pod, logs: &[String]) -> Result<WhyInitFailure, DiagnoseError> {
    let ns = pod.metadata.namespace.as_deref().unwrap_or("default");
    let name = pod.metadata.name.as_deref().unwrap_or("unknown");
    let pod_ref = format!("{ns}/{name}");

    let spec = pod.spec.as_ref();
    let init_containers = spec.and_then(|s| s.init_containers.as_ref());

    let init_containers = match init_containers {
        Some(cs) if !cs.is_empty() => cs,
        _ => return Err(DiagnoseError::NoInitContainers),
    };

    let status = pod.status.as_ref();
    let init_statuses = status.and_then(|s| s.init_container_statuses.as_ref());

    let (failing_index, failing_status) = find_failing_init(init_statuses);

    match (failing_index, failing_status) {
        (None, _) => Err(DiagnoseError::AllInitSucceeded),
        (Some(idx), status_opt) => {
            let init_container = init_containers
                .get(idx)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("init-{idx}"));

            let (reason, mut evidence) = if let Some(st) = status_opt {
                diagnose_init_status(st, logs)
            } else {
                (
                    InitFailureReason::InitStuckPending,
                    InitEvidence {
                        stuck_for: Some("not started".to_string()),
                        ..Default::default()
                    },
                )
            };

            if let Some(container_spec) = init_containers.get(idx) {
                evidence.image = container_spec.image.clone();
                evidence.memory_limit = extract_init_memory_limit(container_spec);
            }

            Ok(WhyInitFailure {
                pod: pod_ref,
                init_container,
                init_index: idx,
                reason,
                suggested_fix: reason.suggested_fix().to_string(),
                evidence,
            })
        }
    }
}

fn find_failing_init(
    init_statuses: Option<&Vec<k8s_openapi::api::core::v1::ContainerStatus>>,
) -> (Option<usize>, Option<&k8s_openapi::api::core::v1::ContainerStatus>) {
    let statuses = match init_statuses {
        Some(s) if !s.is_empty() => s,
        _ => return (Some(0), None),
    };

    for (idx, st) in statuses.iter().enumerate() {
        let state = st.state.as_ref();
        let terminated = state.and_then(|s| s.terminated.as_ref());

        if let Some(term) = terminated {
            if term.exit_code == 0 {
                continue;
            }
            return (Some(idx), Some(st));
        }

        let waiting = state.and_then(|s| s.waiting.as_ref());
        if waiting.is_some() || st.restart_count > 0 {
            return (Some(idx), Some(st));
        }

        let running = state.and_then(|s| s.running.as_ref());
        if running.is_some() {
            return (Some(idx), Some(st));
        }

        return (Some(idx), Some(st));
    }

    (None, None)
}

fn diagnose_init_status(
    st: &k8s_openapi::api::core::v1::ContainerStatus,
    logs: &[String],
) -> (InitFailureReason, InitEvidence) {
    let mut evidence = InitEvidence {
        restart_count: Some(st.restart_count),
        last_log_line: logs.last().cloned(),
        ..Default::default()
    };

    let state = st.state.as_ref();
    let last_state = st.last_state.as_ref();

    let terminated = state
        .and_then(|s| s.terminated.as_ref())
        .or_else(|| last_state.and_then(|s| s.terminated.as_ref()));

    if let Some(term) = terminated {
        evidence.exit_code = Some(term.exit_code);

        if term.reason.as_deref() == Some("OOMKilled") {
            return (InitFailureReason::InitOomKilled, evidence);
        }
    }

    let waiting = state.and_then(|s| s.waiting.as_ref());
    if let Some(w) = waiting {
        let reason = w.reason.as_deref();
        let message = w.message.clone();

        if reason == Some("CreateContainerConfigError") || reason == Some("CreateContainerError") {
            evidence.apiserver_error = message;
            return (InitFailureReason::InitConfigMissing, evidence);
        }

        if reason == Some("ImagePullBackOff") || reason == Some("ErrImagePull") {
            evidence.apiserver_error = message;
            return (InitFailureReason::InitImagePullFailure, evidence);
        }

        if reason.is_none() || reason == Some("PodInitializing") || reason == Some("ContainerCreating") {
            evidence.stuck_for = Some("waiting to start".to_string());
            return (InitFailureReason::InitStuckPending, evidence);
        }
    }

    if evidence.exit_code.is_some() && evidence.exit_code != Some(0) {
        if let Some((signal, line)) = detect_signal_in_lines(logs) {
            evidence.last_log_line = Some(line);
            let reason = match signal {
                LogSignal::AuthFailure => InitFailureReason::AuthFailure,
                LogSignal::ConnectivityFailure => InitFailureReason::ConnectivityFailure,
                LogSignal::PermissionDenied => InitFailureReason::PermissionDenied,
                LogSignal::MissingPath => InitFailureReason::MissingPath,
            };
            return (reason, evidence);
        }
    }

    (InitFailureReason::Unknown, evidence)
}

fn extract_init_memory_limit(
    container: &k8s_openapi::api::core::v1::Container,
) -> Option<String> {
    let resources = container.resources.as_ref()?;
    let limits = resources.limits.as_ref()?;
    let mem = limits.get("memory")?;
    Some(mem.0.clone())
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
    fn make_pod_with_init(
        name: &str,
        init_name: &str,
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

        let terminated = if terminated_reason.is_some() || exit_code.is_some() {
            Some(ContainerStateTerminated {
                reason: terminated_reason.map(|s| s.to_string()),
                exit_code: exit_code.unwrap_or(1),
                ..Default::default()
            })
        } else {
            None
        };

        let waiting = waiting_reason.map(|r| ContainerStateWaiting {
            reason: Some(r.to_string()),
            message: waiting_message.map(|s| s.to_string()),
        });

        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: Some(PodSpec {
                init_containers: Some(vec![Container {
                    name: init_name.to_string(),
                    image: Some("init-image:v1".to_string()),
                    resources,
                    ..Default::default()
                }]),
                containers: vec![Container {
                    name: "main".to_string(),
                    image: Some("main-image:v1".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            status: Some(PodStatus {
                phase: Some("Pending".to_string()),
                init_container_statuses: Some(vec![ContainerStatus {
                    name: init_name.to_string(),
                    restart_count,
                    ready: false,
                    image: "init-image:v1".to_string(),
                    image_id: "sha256:abc".to_string(),
                    state: Some(ContainerState {
                        waiting: waiting.clone(),
                        terminated: terminated.clone(),
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

    fn make_pod_without_init_containers(name: &str) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "main".to_string(),
                    image: Some("main-image:v1".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            status: Some(PodStatus {
                phase: Some("Running".to_string()),
                ..Default::default()
            }),
        }
    }

    fn make_pod_with_all_init_succeeded(name: &str) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: Some(PodSpec {
                init_containers: Some(vec![
                    Container {
                        name: "init-1".to_string(),
                        image: Some("init:v1".to_string()),
                        ..Default::default()
                    },
                    Container {
                        name: "init-2".to_string(),
                        image: Some("init:v1".to_string()),
                        ..Default::default()
                    },
                ]),
                containers: vec![Container {
                    name: "main".to_string(),
                    image: Some("main-image:v1".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            status: Some(PodStatus {
                phase: Some("Running".to_string()),
                init_container_statuses: Some(vec![
                    ContainerStatus {
                        name: "init-1".to_string(),
                        restart_count: 0,
                        ready: true,
                        image: "init:v1".to_string(),
                        image_id: "sha256:abc".to_string(),
                        state: Some(ContainerState {
                            terminated: Some(ContainerStateTerminated {
                                exit_code: 0,
                                reason: Some("Completed".to_string()),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    ContainerStatus {
                        name: "init-2".to_string(),
                        restart_count: 0,
                        ready: true,
                        image: "init:v1".to_string(),
                        image_id: "sha256:def".to_string(),
                        state: Some(ContainerState {
                            terminated: Some(ContainerStateTerminated {
                                exit_code: 0,
                                reason: Some("Completed".to_string()),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }),
        }
    }

    fn make_pod_with_multiple_init(
        name: &str,
        first_succeeded: bool,
        second_exit_code: i32,
    ) -> Pod {
        let first_status = if first_succeeded {
            ContainerStatus {
                name: "init-1".to_string(),
                restart_count: 0,
                ready: true,
                image: "init:v1".to_string(),
                image_id: "sha256:abc".to_string(),
                state: Some(ContainerState {
                    terminated: Some(ContainerStateTerminated {
                        exit_code: 0,
                        reason: Some("Completed".to_string()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }
        } else {
            ContainerStatus {
                name: "init-1".to_string(),
                restart_count: 3,
                ready: false,
                image: "init:v1".to_string(),
                image_id: "sha256:abc".to_string(),
                state: Some(ContainerState {
                    terminated: Some(ContainerStateTerminated {
                        exit_code: 1,
                        reason: Some("Error".to_string()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                last_state: Some(ContainerState {
                    terminated: Some(ContainerStateTerminated {
                        exit_code: 1,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }
        };

        let second_status = ContainerStatus {
            name: "init-2".to_string(),
            restart_count: 2,
            ready: false,
            image: "init:v2".to_string(),
            image_id: "sha256:def".to_string(),
            state: Some(ContainerState {
                terminated: Some(ContainerStateTerminated {
                    exit_code: second_exit_code,
                    reason: Some("Error".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            last_state: Some(ContainerState {
                terminated: Some(ContainerStateTerminated {
                    exit_code: second_exit_code,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: Some(PodSpec {
                init_containers: Some(vec![
                    Container {
                        name: "init-1".to_string(),
                        image: Some("init:v1".to_string()),
                        ..Default::default()
                    },
                    Container {
                        name: "init-2".to_string(),
                        image: Some("init:v2".to_string()),
                        ..Default::default()
                    },
                ]),
                containers: vec![Container {
                    name: "main".to_string(),
                    image: Some("main-image:v1".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            status: Some(PodStatus {
                phase: Some("Pending".to_string()),
                init_container_statuses: Some(vec![first_status, second_status]),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn oom_killed_in_init_container() {
        let pod = make_pod_with_init(
            "at-foo",
            "wait-for-db",
            Some("OOMKilled"),
            Some(137),
            None,
            None,
            Some("256Mi"),
            3,
        );
        let result = diagnose(&pod, &[]).unwrap();
        assert_eq!(result.reason, InitFailureReason::InitOomKilled);
        assert_eq!(result.init_container, "wait-for-db");
        assert_eq!(result.evidence.memory_limit, Some("256Mi".to_string()));
        assert_eq!(result.evidence.exit_code, Some(137));
    }

    #[test]
    fn auth_failure_from_init_log() {
        let pod = make_pod_with_init(
            "at-foo",
            "init-creds",
            Some("Error"),
            Some(1),
            None,
            None,
            None,
            2,
        );
        let logs = vec![
            "Fetching credentials...".to_string(),
            "LLM returned 401: invalid x-api-key".to_string(),
        ];
        let result = diagnose(&pod, &logs).unwrap();
        assert_eq!(result.reason, InitFailureReason::AuthFailure);
        assert!(result.evidence.last_log_line.unwrap().contains("401"));
    }

    #[test]
    fn image_pull_failure_when_init_waiting_errimagepull() {
        let pod = make_pod_with_init(
            "at-foo",
            "wait-for-db",
            None,
            None,
            Some("ErrImagePull"),
            Some("pull access denied for private.registry/init"),
            None,
            0,
        );
        let result = diagnose(&pod, &[]).unwrap();
        assert_eq!(result.reason, InitFailureReason::InitImagePullFailure);
        assert!(result
            .evidence
            .apiserver_error
            .unwrap()
            .contains("private.registry"));
    }

    #[test]
    fn stuck_pending_when_waiting_no_reason() {
        let pod = make_pod_with_init(
            "at-foo",
            "wait-for-db",
            None,
            None,
            Some("PodInitializing"),
            None,
            None,
            0,
        );
        let result = diagnose(&pod, &[]).unwrap();
        assert_eq!(result.reason, InitFailureReason::InitStuckPending);
        assert!(result.evidence.stuck_for.is_some());
    }

    #[test]
    fn picks_lowest_index_failing_init_when_multiple() {
        let pod = make_pod_with_multiple_init("at-foo", false, 1);
        let result = diagnose(&pod, &[]).unwrap();
        assert_eq!(result.init_index, 0);
        assert_eq!(result.init_container, "init-1");
    }

    #[test]
    fn refuses_pod_with_no_init_containers() {
        let pod = make_pod_without_init_containers("at-foo");
        let result = diagnose(&pod, &[]);
        assert!(matches!(result, Err(DiagnoseError::NoInitContainers)));
    }

    #[test]
    fn refuses_pod_when_all_init_succeeded() {
        let pod = make_pod_with_all_init_succeeded("at-foo");
        let result = diagnose(&pod, &[]);
        assert!(matches!(result, Err(DiagnoseError::AllInitSucceeded)));
    }

    #[test]
    fn unknown_fallback_includes_restart_count() {
        let pod = make_pod_with_init(
            "at-foo",
            "init-mystery",
            Some("Error"),
            Some(42),
            None,
            None,
            None,
            10,
        );
        let logs = vec![
            "INFO: doing something".to_string(),
            "DEBUG: step complete".to_string(),
        ];
        let result = diagnose(&pod, &logs).unwrap();
        assert_eq!(result.reason, InitFailureReason::Unknown);
        assert_eq!(result.evidence.restart_count, Some(10));
        assert!(result.evidence.last_log_line.is_some());
    }

    #[test]
    fn connectivity_failure_from_init_log() {
        let pod = make_pod_with_init(
            "at-foo",
            "wait-for-db",
            Some("Error"),
            Some(1),
            None,
            None,
            None,
            5,
        );
        let logs = vec!["dial tcp 10.0.0.1:5432: connection refused".to_string()];
        let result = diagnose(&pod, &logs).unwrap();
        assert_eq!(result.reason, InitFailureReason::ConnectivityFailure);
    }

    #[test]
    fn config_missing_on_create_container_config_error() {
        let pod = make_pod_with_init(
            "at-foo",
            "init-secrets",
            None,
            None,
            Some("CreateContainerConfigError"),
            Some("secret \"db-creds\" not found"),
            None,
            0,
        );
        let result = diagnose(&pod, &[]).unwrap();
        assert_eq!(result.reason, InitFailureReason::InitConfigMissing);
        assert_eq!(
            result.evidence.apiserver_error,
            Some("secret \"db-creds\" not found".to_string())
        );
    }

    #[test]
    fn pod_ref_includes_namespace() {
        let mut pod = make_pod_with_init(
            "at-foo",
            "wait-for-db",
            Some("OOMKilled"),
            Some(137),
            None,
            None,
            None,
            1,
        );
        pod.metadata.namespace = Some("agents".to_string());
        let result = diagnose(&pod, &[]).unwrap();
        assert_eq!(result.pod, "agents/at-foo");
    }
}
