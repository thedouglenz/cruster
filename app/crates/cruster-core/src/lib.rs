//! Shared types used across cruster crates. No I/O, no async.

use std::fmt;

use serde::{Deserialize, Serialize};

pub mod context;
pub mod diff;
pub mod doctor;
pub mod license;
pub mod tier;

/// Uniquely identifies a Kubernetes resource within a cluster.
///
/// `namespace` is `None` for cluster-scoped resources (Node, Namespace, etc.).
///
/// `Ord` is derived (kind, then namespace, then name) so the type can key
/// a `BTreeMap` for ordered iteration in resource views.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

/// Cluster environment classification, used for safety badging and
/// read-only-by-default gating. Determined by user-configurable
/// matchers in `~/.config/cruster/safety.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Prod,
    Staging,
    Dev,
    Local,
    Unknown,
}

impl Environment {
    /// Whether destructive actions are gated by default for this env.
    pub fn requires_confirmation(self) -> bool {
        matches!(self, Self::Prod | Self::Staging)
    }
}

impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Prod => "prod",
            Self::Staging => "staging",
            Self::Dev => "dev",
            Self::Local => "local",
            Self::Unknown => "unknown",
        };
        f.write_str(s)
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

    #[test]
    fn prod_requires_confirmation() {
        assert!(Environment::Prod.requires_confirmation());
        assert!(Environment::Staging.requires_confirmation());
        assert!(!Environment::Dev.requires_confirmation());
        assert!(!Environment::Local.requires_confirmation());
        assert!(!Environment::Unknown.requires_confirmation());
    }

    #[test]
    fn environment_roundtrips_through_json() {
        for env in [
            Environment::Prod,
            Environment::Staging,
            Environment::Dev,
            Environment::Local,
            Environment::Unknown,
        ] {
            let s = serde_json::to_string(&env).unwrap();
            let back: Environment = serde_json::from_str(&s).unwrap();
            assert_eq!(env, back);
        }
    }

    #[test]
    fn environment_display() {
        assert_eq!(Environment::Prod.to_string(), "prod");
        assert_eq!(Environment::Unknown.to_string(), "unknown");
    }
}
