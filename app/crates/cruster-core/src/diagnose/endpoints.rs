//! Pure diagnostic functions for services with no live endpoints.
//!
//! Takes a Service, matching Pods, and EndpointSlices, returns a
//! structured diagnosis explaining why there are no ready endpoints.

use std::collections::{BTreeMap, BTreeSet};

use k8s_openapi::api::core::v1::{Pod, Service, ServicePort};
use k8s_openapi::api::discovery::v1::EndpointSlice;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    NoSelector,
    NoPodsMatchSelector,
    PodsNotReady,
    PortMismatch,
    EndpointsliceStale,
    Unknown,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::NoSelector => "no_selector",
            Reason::NoPodsMatchSelector => "no_pods_match_selector",
            Reason::PodsNotReady => "pods_not_ready",
            Reason::PortMismatch => "port_mismatch",
            Reason::EndpointsliceStale => "endpointslice_stale",
            Reason::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Evidence {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matching_pods: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_pods: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_ports: Option<Vec<ServicePortInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_ports: Option<Vec<ContainerPortInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpointslice_age: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod_phases: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_ready_reasons: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServicePortInfo {
    pub name: Option<String>,
    pub port: i32,
    pub target_port: String,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContainerPortInfo {
    pub container: String,
    pub port: i32,
    pub name: Option<String>,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhyNoEndpoints {
    pub service: String,
    pub reason: Reason,
    pub evidence: Evidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_fix: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceHasEndpoints {
    pub ready_endpoints: i32,
}

pub fn count_ready_endpoints(slices: &[EndpointSlice]) -> i32 {
    slices
        .iter()
        .flat_map(|s| &s.endpoints)
        .filter(|ep| {
            ep.conditions
                .as_ref()
                .map(|c| c.ready == Some(true))
                .unwrap_or(false)
        })
        .count() as i32
}

pub fn diagnose(
    service: &Service,
    pods: &[Pod],
    slices: &[EndpointSlice],
) -> Result<WhyNoEndpoints, ServiceHasEndpoints> {
    let service_name = service
        .metadata
        .name
        .as_deref()
        .unwrap_or("unknown")
        .to_string();

    let ready_count = count_ready_endpoints(slices);
    if ready_count > 0 {
        return Err(ServiceHasEndpoints {
            ready_endpoints: ready_count,
        });
    }

    let spec = match &service.spec {
        Some(s) => s,
        None => {
            return Ok(WhyNoEndpoints {
                service: service_name,
                reason: Reason::Unknown,
                evidence: Evidence::default(),
                suggested_fix: Some("Service has no spec".to_string()),
            });
        }
    };

    // 1. No selector
    let selector = match &spec.selector {
        Some(s) if !s.is_empty() => s.clone(),
        _ => {
            return Ok(WhyNoEndpoints {
                service: service_name,
                reason: Reason::NoSelector,
                evidence: Evidence {
                    selector: spec.selector.clone(),
                    ..Default::default()
                },
                suggested_fix: Some(
                    "Service has no selector; endpoints must be manually managed or \
                     this is an ExternalName service"
                        .to_string(),
                ),
            });
        }
    };

    // 2. No pods match selector
    let matching_pods: Vec<&Pod> = pods
        .iter()
        .filter(|p| matches_selector(p, &selector))
        .collect();
    if matching_pods.is_empty() {
        return Ok(WhyNoEndpoints {
            service: service_name,
            reason: Reason::NoPodsMatchSelector,
            evidence: Evidence {
                selector: Some(selector),
                matching_pods: Some(0),
                ..Default::default()
            },
            suggested_fix: Some(
                "No pods match the service selector; check that pod labels match the selector"
                    .to_string(),
            ),
        });
    }

    // 3. Pods not ready
    let ready_pods: Vec<&Pod> = matching_pods
        .iter()
        .copied()
        .filter(|p| is_pod_ready(p))
        .collect();
    if ready_pods.is_empty() {
        let pod_phases = matching_pods
            .iter()
            .filter_map(|p| {
                let name = p.metadata.name.as_deref()?;
                let phase = p
                    .status
                    .as_ref()
                    .and_then(|s| s.phase.as_deref())
                    .unwrap_or("Unknown");
                Some((name.to_string(), phase.to_string()))
            })
            .collect();

        let not_ready_reasons = collect_not_ready_reasons(&matching_pods);

        return Ok(WhyNoEndpoints {
            service: service_name,
            reason: Reason::PodsNotReady,
            evidence: Evidence {
                selector: Some(selector),
                matching_pods: Some(matching_pods.len() as i32),
                ready_pods: Some(0),
                pod_phases: Some(pod_phases),
                not_ready_reasons: if not_ready_reasons.is_empty() {
                    None
                } else {
                    Some(not_ready_reasons)
                },
                ..Default::default()
            },
            suggested_fix: Some(format!(
                "0/{} pods are Ready; check pod status and container readiness",
                matching_pods.len()
            )),
        });
    }

    // 4. Port mismatch
    let service_ports = extract_service_ports(spec.ports.as_deref().unwrap_or(&[]));
    let container_ports = extract_container_ports(&ready_pods);

    if !ports_match(&service_ports, &container_ports) {
        return Ok(WhyNoEndpoints {
            service: service_name,
            reason: Reason::PortMismatch,
            evidence: Evidence {
                selector: Some(selector),
                matching_pods: Some(matching_pods.len() as i32),
                ready_pods: Some(ready_pods.len() as i32),
                service_ports: Some(service_ports),
                container_ports: Some(container_ports),
                ..Default::default()
            },
            suggested_fix: Some(
                "Service targetPort does not match any container port in ready pods".to_string(),
            ),
        });
    }

    // 5. EndpointSlice stale
    let slice_age = slices
        .iter()
        .filter_map(|s| {
            s.metadata
                .creation_timestamp
                .as_ref()
                .map(|t| t.0.to_rfc3339())
        })
        .next();

    if !slices.is_empty() {
        return Ok(WhyNoEndpoints {
            service: service_name,
            reason: Reason::EndpointsliceStale,
            evidence: Evidence {
                selector: Some(selector),
                matching_pods: Some(matching_pods.len() as i32),
                ready_pods: Some(ready_pods.len() as i32),
                service_ports: Some(service_ports),
                container_ports: Some(container_ports),
                endpointslice_age: slice_age,
                ..Default::default()
            },
            suggested_fix: Some(
                "Pods are Ready and ports match but EndpointSlice has no ready endpoints; \
                 this may indicate endpoint controller lag or a bug"
                    .to_string(),
            ),
        });
    }

    // 6. Unknown
    Ok(WhyNoEndpoints {
        service: service_name,
        reason: Reason::Unknown,
        evidence: Evidence {
            selector: Some(selector),
            matching_pods: Some(matching_pods.len() as i32),
            ready_pods: Some(ready_pods.len() as i32),
            service_ports: Some(service_ports),
            container_ports: Some(container_ports),
            ..Default::default()
        },
        suggested_fix: Some(
            "Unable to determine root cause; check EndpointSlice controller and kube-proxy logs"
                .to_string(),
        ),
    })
}

fn matches_selector(pod: &Pod, selector: &BTreeMap<String, String>) -> bool {
    let labels = match &pod.metadata.labels {
        Some(l) => l,
        None => return false,
    };
    selector.iter().all(|(k, v)| labels.get(k) == Some(v))
}

fn is_pod_ready(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|conditions| {
            conditions
                .iter()
                .any(|c| c.type_ == "Ready" && c.status == "True")
        })
        .unwrap_or(false)
}

fn collect_not_ready_reasons(pods: &[&Pod]) -> Vec<String> {
    let mut reasons = BTreeSet::new();
    for pod in pods {
        if let Some(status) = &pod.status {
            if let Some(conditions) = &status.conditions {
                for cond in conditions {
                    if cond.type_ == "Ready" && cond.status != "True" {
                        if let Some(reason) = &cond.reason {
                            reasons.insert(reason.clone());
                        }
                        if let Some(msg) = &cond.message {
                            reasons.insert(msg.clone());
                        }
                    }
                    if cond.type_ == "ContainersReady" && cond.status != "True" {
                        if let Some(msg) = &cond.message {
                            reasons.insert(msg.clone());
                        }
                    }
                }
            }
            if let Some(container_statuses) = &status.container_statuses {
                for cs in container_statuses {
                    if !cs.ready {
                        if let Some(state) = &cs.state {
                            if let Some(waiting) = &state.waiting {
                                let r = waiting.reason.as_deref().unwrap_or("Waiting");
                                reasons.insert(format!("{}: {}", cs.name, r));
                            }
                            if let Some(terminated) = &state.terminated {
                                let r = terminated.reason.as_deref().unwrap_or("Terminated");
                                reasons.insert(format!("{}: {}", cs.name, r));
                            }
                        }
                    }
                }
            }
        }
    }
    reasons.into_iter().collect()
}

fn extract_service_ports(ports: &[ServicePort]) -> Vec<ServicePortInfo> {
    ports
        .iter()
        .map(|p| ServicePortInfo {
            name: p.name.clone(),
            port: p.port,
            target_port: p
                .target_port
                .as_ref()
                .map(|tp| match tp {
                    k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(i) => {
                        i.to_string()
                    }
                    k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::String(s) => {
                        s.clone()
                    }
                })
                .unwrap_or_else(|| p.port.to_string()),
            protocol: p.protocol.clone().unwrap_or_else(|| "TCP".to_string()),
        })
        .collect()
}

fn extract_container_ports(pods: &[&Pod]) -> Vec<ContainerPortInfo> {
    let mut result = Vec::new();
    for pod in pods {
        if let Some(spec) = &pod.spec {
            for container in &spec.containers {
                if let Some(ports) = &container.ports {
                    for port in ports {
                        result.push(ContainerPortInfo {
                            container: container.name.clone(),
                            port: port.container_port,
                            name: port.name.clone(),
                            protocol: port.protocol.clone().unwrap_or_else(|| "TCP".to_string()),
                        });
                    }
                }
            }
        }
    }
    result
}

fn ports_match(service_ports: &[ServicePortInfo], container_ports: &[ContainerPortInfo]) -> bool {
    for sp in service_ports {
        let target = &sp.target_port;
        let matches_any = container_ports.iter().any(|cp| {
            if cp.protocol != sp.protocol {
                return false;
            }
            if let Ok(num) = target.parse::<i32>() {
                cp.port == num
            } else {
                cp.name.as_deref() == Some(target.as_str())
            }
        });
        if !matches_any {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        Container, ContainerPort, ContainerState, ContainerStateWaiting, ContainerStatus,
        PodCondition, PodSpec, PodStatus, ServiceSpec,
    };
    use k8s_openapi::api::discovery::v1::{Endpoint, EndpointConditions};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

    fn make_service(
        selector: Option<BTreeMap<String, String>>,
        ports: Vec<ServicePort>,
    ) -> Service {
        Service {
            metadata: ObjectMeta {
                name: Some("test-svc".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: Some(ServiceSpec {
                selector,
                ports: Some(ports),
                ..Default::default()
            }),
            status: None,
        }
    }

    fn make_pod(
        name: &str,
        labels: BTreeMap<String, String>,
        ready: bool,
        ports: Vec<ContainerPort>,
    ) -> Pod {
        let conditions = if ready {
            Some(vec![PodCondition {
                type_: "Ready".to_string(),
                status: "True".to_string(),
                ..Default::default()
            }])
        } else {
            Some(vec![PodCondition {
                type_: "Ready".to_string(),
                status: "False".to_string(),
                reason: Some("ContainersNotReady".to_string()),
                ..Default::default()
            }])
        };

        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some("default".to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "main".to_string(),
                    ports: if ports.is_empty() { None } else { Some(ports) },
                    ..Default::default()
                }],
                ..Default::default()
            }),
            status: Some(PodStatus {
                phase: Some(if ready { "Running" } else { "Pending" }.to_string()),
                conditions,
                container_statuses: if !ready {
                    Some(vec![ContainerStatus {
                        name: "main".to_string(),
                        ready: false,
                        state: Some(ContainerState {
                            waiting: Some(ContainerStateWaiting {
                                reason: Some("ImagePullBackOff".to_string()),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }])
                } else {
                    None
                },
                ..Default::default()
            }),
        }
    }

    fn make_endpoint_slice(ready_count: i32) -> EndpointSlice {
        let endpoints = (0..ready_count)
            .map(|_| Endpoint {
                conditions: Some(EndpointConditions {
                    ready: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .collect();
        EndpointSlice {
            metadata: ObjectMeta::default(),
            address_type: "IPv4".to_string(),
            endpoints,
            ports: None,
        }
    }

    #[test]
    fn no_selector_for_headless_service() {
        let svc = make_service(None, vec![]);
        let result = diagnose(&svc, &[], &[]).unwrap();
        assert_eq!(result.reason, Reason::NoSelector);
        assert!(result
            .suggested_fix
            .as_ref()
            .unwrap()
            .contains("no selector"));
    }

    #[test]
    fn no_pods_match_selector_when_labels_diverge() {
        let mut selector = BTreeMap::new();
        selector.insert("app".to_string(), "web".to_string());
        let svc = make_service(Some(selector), vec![]);

        let mut wrong_labels = BTreeMap::new();
        wrong_labels.insert("app".to_string(), "api".to_string());
        let pod = make_pod("pod-1", wrong_labels, true, vec![]);

        let result = diagnose(&svc, &[pod], &[]).unwrap();
        assert_eq!(result.reason, Reason::NoPodsMatchSelector);
        assert_eq!(result.evidence.matching_pods, Some(0));
    }

    #[test]
    fn pods_not_ready_when_readiness_false() {
        let mut selector = BTreeMap::new();
        selector.insert("app".to_string(), "web".to_string());
        let svc = make_service(
            Some(selector.clone()),
            vec![ServicePort {
                port: 80,
                target_port: Some(IntOrString::Int(8080)),
                ..Default::default()
            }],
        );

        let pod = make_pod(
            "pod-1",
            selector,
            false,
            vec![ContainerPort {
                container_port: 8080,
                ..Default::default()
            }],
        );

        let result = diagnose(&svc, &[pod], &[]).unwrap();
        assert_eq!(result.reason, Reason::PodsNotReady);
        assert_eq!(result.evidence.matching_pods, Some(1));
        assert_eq!(result.evidence.ready_pods, Some(0));
    }

    #[test]
    fn port_mismatch_when_target_port_not_in_container() {
        let mut selector = BTreeMap::new();
        selector.insert("app".to_string(), "web".to_string());
        let svc = make_service(
            Some(selector.clone()),
            vec![ServicePort {
                port: 80,
                target_port: Some(IntOrString::Int(9090)),
                ..Default::default()
            }],
        );

        let pod = make_pod(
            "pod-1",
            selector,
            true,
            vec![ContainerPort {
                container_port: 8080,
                ..Default::default()
            }],
        );

        let result = diagnose(&svc, &[pod], &[]).unwrap();
        assert_eq!(result.reason, Reason::PortMismatch);
        assert!(result
            .suggested_fix
            .as_ref()
            .unwrap()
            .contains("targetPort"));
    }

    #[test]
    fn endpointslice_stale_when_pods_ready_but_slice_empty() {
        let mut selector = BTreeMap::new();
        selector.insert("app".to_string(), "web".to_string());
        let svc = make_service(
            Some(selector.clone()),
            vec![ServicePort {
                port: 80,
                target_port: Some(IntOrString::Int(8080)),
                ..Default::default()
            }],
        );

        let pod = make_pod(
            "pod-1",
            selector,
            true,
            vec![ContainerPort {
                container_port: 8080,
                ..Default::default()
            }],
        );

        let slice = EndpointSlice {
            metadata: ObjectMeta::default(),
            address_type: "IPv4".to_string(),
            endpoints: vec![],
            ports: None,
        };

        let result = diagnose(&svc, &[pod], &[slice]).unwrap();
        assert_eq!(result.reason, Reason::EndpointsliceStale);
    }

    #[test]
    fn unknown_fallback_includes_counts() {
        let mut selector = BTreeMap::new();
        selector.insert("app".to_string(), "web".to_string());
        let svc = make_service(
            Some(selector.clone()),
            vec![ServicePort {
                port: 80,
                target_port: Some(IntOrString::Int(8080)),
                ..Default::default()
            }],
        );

        let pod = make_pod(
            "pod-1",
            selector,
            true,
            vec![ContainerPort {
                container_port: 8080,
                ..Default::default()
            }],
        );

        let result = diagnose(&svc, &[pod], &[]).unwrap();
        assert_eq!(result.reason, Reason::Unknown);
        assert!(result.evidence.matching_pods.is_some());
        assert!(result.evidence.ready_pods.is_some());
    }

    #[test]
    fn refuses_service_with_healthy_endpoints() {
        let mut selector = BTreeMap::new();
        selector.insert("app".to_string(), "web".to_string());
        let svc = make_service(Some(selector), vec![]);
        let slice = make_endpoint_slice(3);

        let result = diagnose(&svc, &[], &[slice]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.ready_endpoints, 3);
    }
}
