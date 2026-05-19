//! Holds one `ResourceStore` per kind cruster knows about.
//!
//! The registry is the single source from which views fetch their
//! snapshots. The binary populates it at startup by spawning one
//! watcher per kind.

use std::sync::Arc;

use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::batch::v1::{CronJob, Job};
use k8s_openapi::api::core::v1::{ConfigMap, Event, Namespace, Node, Pod, Secret, Service};
use tokio::sync::RwLock;

use crate::metrics::MetricsCache;
use crate::store::ResourceStore;

#[derive(Clone, Default)]
pub struct StoreRegistry {
    pub pods: ResourceStore<Pod>,
    pub deployments: ResourceStore<Deployment>,
    pub statefulsets: ResourceStore<StatefulSet>,
    pub daemonsets: ResourceStore<DaemonSet>,
    pub jobs: ResourceStore<Job>,
    pub cronjobs: ResourceStore<CronJob>,
    pub services: ResourceStore<Service>,
    pub nodes: ResourceStore<Node>,
    pub events: ResourceStore<Event>,
    pub configmaps: ResourceStore<ConfigMap>,
    pub secrets: ResourceStore<Secret>,
    pub namespaces: ResourceStore<Namespace>,
    /// Latest cluster-wide CPU/MEM utilisation from metrics-server,
    /// or an "unavailable" marker if the addon isn't installed. The
    /// `Arc<RwLock<_>>` is shared between the poller task and the
    /// dashboard render path.
    pub node_metrics: Arc<RwLock<MetricsCache>>,
}

impl StoreRegistry {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn registry_constructs_empty_stores() {
        let r = StoreRegistry::new();
        assert!(r.pods.is_empty().await);
        assert!(r.deployments.is_empty().await);
        assert!(r.statefulsets.is_empty().await);
        assert!(r.daemonsets.is_empty().await);
        assert!(r.jobs.is_empty().await);
        assert!(r.cronjobs.is_empty().await);
        assert!(r.services.is_empty().await);
        assert!(r.nodes.is_empty().await);
        assert!(r.events.is_empty().await);
        assert!(r.configmaps.is_empty().await);
        assert!(r.secrets.is_empty().await);
        assert!(r.namespaces.is_empty().await);
    }

    #[tokio::test]
    async fn clones_share_underlying_state() {
        let r1 = StoreRegistry::new();
        let r2 = r1.clone();
        let key = cruster_core::ResourceKey::namespaced("Pod", "default", "p");
        r1.pods.upsert(key, Pod::default()).await;
        assert_eq!(r2.pods.len().await, 1);
    }
}
