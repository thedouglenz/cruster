//! Pure graph computation over StoreRegistry. Given a resource key,
//! returns related resources.

use cruster_core::ResourceKey;
use k8s_openapi::api::core::v1::Pod;

use crate::StoreRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Related {
    pub kind: RelationKind,
    pub key: ResourceKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationKind {
    OwnerRef,
    OwnedBy,
    MountsConfigMap,
    MountsSecret,
    Selects,
    SelectedBy,
    ScheduledOn,
}

impl RelationKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::OwnerRef => "owner",
            Self::OwnedBy => "owned-by",
            Self::MountsConfigMap => "mounts cm",
            Self::MountsSecret => "mounts secret",
            Self::Selects => "selects",
            Self::SelectedBy => "selected by",
            Self::ScheduledOn => "scheduled on",
        }
    }
}

/// Resolve related resources for the given key.
pub async fn related(key: &ResourceKey, registry: &StoreRegistry) -> Vec<Related> {
    match key.kind.as_str() {
        "Pod" => related_for_pod(key, registry).await,
        "Deployment" => related_for_deployment(key, registry).await,
        "Service" => related_for_service(key, registry).await,
        "Node" => related_for_node(key, registry).await,
        _ => Vec::new(),
    }
}

async fn related_for_pod(key: &ResourceKey, registry: &StoreRegistry) -> Vec<Related> {
    let mut out = Vec::new();
    let pods = registry.pods.snapshot().await;
    let Some((_, pod)) = pods.iter().find(|(k, _)| k == key) else {
        return out;
    };
    let ns = pod.metadata.namespace.as_deref().unwrap_or("default");

    for owner in pod.metadata.owner_references.iter().flatten() {
        out.push(Related {
            kind: RelationKind::OwnerRef,
            key: ResourceKey::namespaced(owner.kind.clone(), ns, owner.name.clone()),
        });
    }

    if let Some(spec) = &pod.spec {
        for vol in spec.volumes.iter().flatten() {
            if let Some(cm) = &vol.config_map {
                out.push(Related {
                    kind: RelationKind::MountsConfigMap,
                    key: ResourceKey::namespaced("ConfigMap", ns, cm.name.clone()),
                });
            }
            if let Some(s) = &vol.secret {
                if let Some(name) = &s.secret_name {
                    out.push(Related {
                        kind: RelationKind::MountsSecret,
                        key: ResourceKey::namespaced("Secret", ns, name.clone()),
                    });
                }
            }
        }
        for c in &spec.containers {
            for ef in c.env_from.iter().flatten() {
                if let Some(cm) = &ef.config_map_ref {
                    out.push(Related {
                        kind: RelationKind::MountsConfigMap,
                        key: ResourceKey::namespaced("ConfigMap", ns, cm.name.clone()),
                    });
                }
                if let Some(s) = &ef.secret_ref {
                    out.push(Related {
                        kind: RelationKind::MountsSecret,
                        key: ResourceKey::namespaced("Secret", ns, s.name.clone()),
                    });
                }
            }
        }
        if let Some(node) = &spec.node_name {
            out.push(Related {
                kind: RelationKind::ScheduledOn,
                key: ResourceKey::cluster_scoped("Node", node.clone()),
            });
        }
    }

    let services = registry.services.snapshot().await;
    for (skey, svc) in services
        .iter()
        .filter(|(k, _)| k.namespace.as_deref() == Some(ns))
    {
        let Some(selector) = svc.spec.as_ref().and_then(|s| s.selector.as_ref()) else {
            continue;
        };
        if labels_match_selector(pod, selector) {
            out.push(Related {
                kind: RelationKind::SelectedBy,
                key: skey.clone(),
            });
        }
    }

    out
}

async fn related_for_deployment(key: &ResourceKey, registry: &StoreRegistry) -> Vec<Related> {
    let mut out = Vec::new();
    let pods = registry.pods.snapshot().await;
    let ns = key.namespace.as_deref().unwrap_or("default");
    for (pkey, pod) in pods.iter() {
        if pkey.namespace.as_deref() != Some(ns) {
            continue;
        }
        for o in pod.metadata.owner_references.iter().flatten() {
            if o.kind == "ReplicaSet" && o.name.starts_with(&format!("{}-", key.name)) {
                out.push(Related {
                    kind: RelationKind::OwnedBy,
                    key: pkey.clone(),
                });
                break;
            }
        }
    }
    out
}

async fn related_for_service(key: &ResourceKey, registry: &StoreRegistry) -> Vec<Related> {
    let mut out = Vec::new();
    let services = registry.services.snapshot().await;
    let Some((_, svc)) = services.iter().find(|(k, _)| k == key) else {
        return out;
    };
    let Some(selector) = svc.spec.as_ref().and_then(|s| s.selector.as_ref()) else {
        return out;
    };
    let pods = registry.pods.snapshot().await;
    let ns = key.namespace.as_deref().unwrap_or("default");
    for (pkey, pod) in pods.iter() {
        if pkey.namespace.as_deref() != Some(ns) {
            continue;
        }
        if labels_match_selector(pod, selector) {
            out.push(Related {
                kind: RelationKind::Selects,
                key: pkey.clone(),
            });
        }
    }
    out
}

async fn related_for_node(key: &ResourceKey, registry: &StoreRegistry) -> Vec<Related> {
    let mut out = Vec::new();
    let pods = registry.pods.snapshot().await;
    for (pkey, pod) in pods.iter() {
        if pod.spec.as_ref().and_then(|s| s.node_name.as_deref()) == Some(&key.name) {
            out.push(Related {
                kind: RelationKind::OwnedBy,
                key: pkey.clone(),
            });
        }
    }
    out
}

fn labels_match_selector(
    pod: &Pod,
    selector: &std::collections::BTreeMap<String, String>,
) -> bool {
    let Some(labels) = pod.metadata.labels.as_ref() else {
        return false;
    };
    selector
        .iter()
        .all(|(k, v)| labels.get(k).map(|lv| lv == v).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{Pod, Service, ServiceSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
    use std::collections::BTreeMap;

    fn pod_with_labels(name: &str, ns: &str, labels: &[(&str, &str)]) -> Pod {
        let map: BTreeMap<String, String> = labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Pod {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some(ns.into()),
                labels: Some(map),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn service_selects_matching_pod() {
        let r = StoreRegistry::new();
        r.pods
            .upsert(
                ResourceKey::namespaced("Pod", "default", "web-1"),
                pod_with_labels("web-1", "default", &[("app", "web")]),
            )
            .await;
        let svc_key = ResourceKey::namespaced("Service", "default", "web");
        let mut selector = BTreeMap::new();
        selector.insert("app".into(), "web".into());
        r.services
            .upsert(
                svc_key.clone(),
                Service {
                    metadata: ObjectMeta {
                        name: Some("web".into()),
                        namespace: Some("default".into()),
                        ..Default::default()
                    },
                    spec: Some(ServiceSpec {
                        selector: Some(selector),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .await;
        let rels = related(&svc_key, &r).await;
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].kind, RelationKind::Selects);
        assert_eq!(rels[0].key.name, "web-1");
    }

    #[tokio::test]
    async fn pod_owner_ref_resolves_to_replicaset() {
        let r = StoreRegistry::new();
        let pkey = ResourceKey::namespaced("Pod", "default", "web-1");
        let mut pod = pod_with_labels("web-1", "default", &[]);
        pod.metadata.owner_references = Some(vec![OwnerReference {
            kind: "ReplicaSet".into(),
            name: "web-abc".into(),
            api_version: "apps/v1".into(),
            uid: "xxx".into(),
            ..Default::default()
        }]);
        r.pods.upsert(pkey.clone(), pod).await;
        let rels = related(&pkey, &r).await;
        assert!(rels
            .iter()
            .any(|x| x.kind == RelationKind::OwnerRef && x.key.name == "web-abc"));
    }

    #[tokio::test]
    async fn node_lists_scheduled_pods() {
        let r = StoreRegistry::new();
        let mut pod = pod_with_labels("p1", "default", &[]);
        pod.spec = Some(k8s_openapi::api::core::v1::PodSpec {
            node_name: Some("n1".into()),
            ..Default::default()
        });
        r.pods
            .upsert(ResourceKey::namespaced("Pod", "default", "p1"), pod)
            .await;
        let rels = related(&ResourceKey::cluster_scoped("Node", "n1"), &r).await;
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].key.name, "p1");
    }
}
