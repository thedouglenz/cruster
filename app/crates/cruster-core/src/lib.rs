//! Shared types used across cruster crates. No I/O, no async.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Uniquely identifies a Kubernetes resource within a cluster.
///
/// `namespace` is `None` for cluster-scoped resources (Node, Namespace, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceKey {
    pub kind: String,
    pub namespace: Option<String>,
    pub name: String,
}

impl ResourceKey {
    pub fn namespaced(
        kind: impl Into<String>,
        namespace: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            namespace: Some(namespace.into()),
            name: name.into(),
        }
    }

    pub fn cluster_scoped(kind: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            namespace: None,
            name: name.into(),
        }
    }
}

impl fmt::Display for ResourceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.namespace {
            Some(ns) => write!(f, "{}/{}/{}", self.kind, ns, self.name),
            None => write!(f, "{}//{}", self.kind, self.name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaced_display_includes_namespace() {
        let k = ResourceKey::namespaced("Pod", "default", "nginx");
        assert_eq!(k.to_string(), "Pod/default/nginx");
    }

    #[test]
    fn cluster_scoped_display_has_empty_namespace_slot() {
        let k = ResourceKey::cluster_scoped("Node", "node-1");
        assert_eq!(k.to_string(), "Node//node-1");
    }

    #[test]
    fn json_roundtrip_preserves_fields() {
        let k = ResourceKey::namespaced("Pod", "kube-system", "coredns-abc");
        let s = serde_json::to_string(&k).unwrap();
        let back: ResourceKey = serde_json::from_str(&s).unwrap();
        assert_eq!(k, back);
    }
}
