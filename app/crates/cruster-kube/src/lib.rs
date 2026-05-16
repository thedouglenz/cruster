//! Kubernetes API integration: watch streams and in-memory stores.

pub mod store;
pub mod watcher;

pub use store::ResourceStore;
pub use watcher::{apply_event, run_pod_watcher};
