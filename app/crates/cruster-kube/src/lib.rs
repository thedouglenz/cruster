//! Kubernetes API integration: watch streams and in-memory stores.

pub mod kind;
pub mod metrics;
pub mod quantity;
pub mod registry;
pub mod relationships;
pub mod store;
pub mod watcher;

pub use kind::{
    ConfigMaps, Deployments, Events, Namespaces, Nodes, Pods, ResourceKind, Secrets, Services,
};
pub use metrics::{
    fetch_node_metrics, run_metrics_poller, sum_node_capacity, sum_node_usage, MetricsCache,
    NodeMetrics, SharedMetricsCache, DEFAULT_POLL_INTERVAL,
};
pub use registry::StoreRegistry;
pub use relationships::{related, Related, RelationKind};
pub use store::ResourceStore;
pub use watcher::{apply_event, run_pod_watcher, run_watcher};
