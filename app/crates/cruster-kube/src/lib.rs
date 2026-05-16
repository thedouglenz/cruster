//! Kubernetes API integration: watch streams and in-memory stores.

pub mod kind;
pub mod registry;
pub mod relationships;
pub mod store;
pub mod watcher;

pub use kind::{
    ConfigMaps, Deployments, Events, Namespaces, Nodes, Pods, ResourceKind, Secrets, Services,
};
pub use registry::StoreRegistry;
pub use relationships::{related, RelationKind, Related};
pub use store::ResourceStore;
pub use watcher::{apply_event, run_pod_watcher, run_watcher};
