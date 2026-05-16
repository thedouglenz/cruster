//! Kind abstraction: what generic code needs to know about a Kubernetes
//! resource type to watch it, store it, and present it.
//!
//! Each kind cruster supports is represented by a zero-sized marker
//! struct (e.g. `Pods`, `Deployments`) implementing this trait.

use std::fmt::Debug;

use cruster_core::ResourceKey;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{
    ConfigMap, Event, Namespace, Node, Pod, Secret, Service,
};
use kube::Resource;
use serde::de::DeserializeOwned;

/// What the watcher / store / view machinery needs to know about a kind.
pub trait ResourceKind: Send + Sync + 'static {
    /// The k8s-openapi type for the object.
    type Object: Resource<DynamicType = ()>
        + Clone
        + Debug
        + DeserializeOwned
        + Send
        + Sync
        + 'static;

    /// Human display name in singular form: "Pod", "Deployment".
    fn name() -> &'static str;

    /// Lowercase plural for CLI / route addressing: "pods", "deployments".
    fn plural() -> &'static str;

    /// kubectl-style short alias: "po", "deploy", "svc".
    fn short() -> &'static str;

    /// `true` for cluster-scoped kinds (Node, Namespace); `false` for
    /// namespaced kinds.
    fn cluster_scoped() -> bool {
        false
    }

    /// Extract the canonical `ResourceKey` from an object. Returns
    /// `None` if the object is missing required metadata.
    fn key(obj: &Self::Object) -> Option<ResourceKey>;
}

// ---- Marker types -------------------------------------------------------

pub struct Pods;
impl ResourceKind for Pods {
    type Object = Pod;
    fn name() -> &'static str {
        "Pod"
    }
    fn plural() -> &'static str {
        "pods"
    }
    fn short() -> &'static str {
        "po"
    }
    fn key(obj: &Pod) -> Option<ResourceKey> {
        let m = &obj.metadata;
        Some(ResourceKey::namespaced(
            "Pod",
            m.namespace.clone()?,
            m.name.clone()?,
        ))
    }
}

pub struct Deployments;
impl ResourceKind for Deployments {
    type Object = Deployment;
    fn name() -> &'static str {
        "Deployment"
    }
    fn plural() -> &'static str {
        "deployments"
    }
    fn short() -> &'static str {
        "deploy"
    }
    fn key(obj: &Deployment) -> Option<ResourceKey> {
        let m = &obj.metadata;
        Some(ResourceKey::namespaced(
            "Deployment",
            m.namespace.clone()?,
            m.name.clone()?,
        ))
    }
}

pub struct Services;
impl ResourceKind for Services {
    type Object = Service;
    fn name() -> &'static str {
        "Service"
    }
    fn plural() -> &'static str {
        "services"
    }
    fn short() -> &'static str {
        "svc"
    }
    fn key(obj: &Service) -> Option<ResourceKey> {
        let m = &obj.metadata;
        Some(ResourceKey::namespaced(
            "Service",
            m.namespace.clone()?,
            m.name.clone()?,
        ))
    }
}

pub struct Nodes;
impl ResourceKind for Nodes {
    type Object = Node;
    fn name() -> &'static str {
        "Node"
    }
    fn plural() -> &'static str {
        "nodes"
    }
    fn short() -> &'static str {
        "no"
    }
    fn cluster_scoped() -> bool {
        true
    }
    fn key(obj: &Node) -> Option<ResourceKey> {
        Some(ResourceKey::cluster_scoped(
            "Node",
            obj.metadata.name.clone()?,
        ))
    }
}

pub struct Events;
impl ResourceKind for Events {
    type Object = Event;
    fn name() -> &'static str {
        "Event"
    }
    fn plural() -> &'static str {
        "events"
    }
    fn short() -> &'static str {
        "ev"
    }
    fn key(obj: &Event) -> Option<ResourceKey> {
        let m = &obj.metadata;
        Some(ResourceKey::namespaced(
            "Event",
            m.namespace.clone()?,
            m.name.clone()?,
        ))
    }
}

pub struct ConfigMaps;
impl ResourceKind for ConfigMaps {
    type Object = ConfigMap;
    fn name() -> &'static str {
        "ConfigMap"
    }
    fn plural() -> &'static str {
        "configmaps"
    }
    fn short() -> &'static str {
        "cm"
    }
    fn key(obj: &ConfigMap) -> Option<ResourceKey> {
        let m = &obj.metadata;
        Some(ResourceKey::namespaced(
            "ConfigMap",
            m.namespace.clone()?,
            m.name.clone()?,
        ))
    }
}

pub struct Secrets;
impl ResourceKind for Secrets {
    type Object = Secret;
    fn name() -> &'static str {
        "Secret"
    }
    fn plural() -> &'static str {
        "secrets"
    }
    fn short() -> &'static str {
        "sec"
    }
    fn key(obj: &Secret) -> Option<ResourceKey> {
        let m = &obj.metadata;
        Some(ResourceKey::namespaced(
            "Secret",
            m.namespace.clone()?,
            m.name.clone()?,
        ))
    }
}

pub struct Namespaces;
impl ResourceKind for Namespaces {
    type Object = Namespace;
    fn name() -> &'static str {
        "Namespace"
    }
    fn plural() -> &'static str {
        "namespaces"
    }
    fn short() -> &'static str {
        "ns"
    }
    fn cluster_scoped() -> bool {
        true
    }
    fn key(obj: &Namespace) -> Option<ResourceKey> {
        Some(ResourceKey::cluster_scoped(
            "Namespace",
            obj.metadata.name.clone()?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    #[test]
    fn pods_key_namespaced() {
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("nginx".into()),
                namespace: Some("default".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let k = Pods::key(&pod).unwrap();
        assert_eq!(k.kind, "Pod");
        assert_eq!(k.namespace.as_deref(), Some("default"));
        assert_eq!(k.name, "nginx");
    }

    #[test]
    fn nodes_key_cluster_scoped() {
        let node = Node {
            metadata: ObjectMeta {
                name: Some("worker-1".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let k = Nodes::key(&node).unwrap();
        assert_eq!(k.kind, "Node");
        assert_eq!(k.namespace, None);
        assert_eq!(k.name, "worker-1");
        assert!(Nodes::cluster_scoped());
    }

    #[test]
    fn missing_required_metadata_returns_none() {
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("orphan".into()),
                namespace: None,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(Pods::key(&pod).is_none());
    }
}
