//! Adapter from `kube::runtime::watcher` events to `ResourceStore` operations.

use cruster_core::ResourceKey;
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::runtime::watcher;
use kube::runtime::watcher::Event;
use kube::{Api, Client};
use tracing::warn;

use crate::store::ResourceStore;

/// Convert a `Pod` into the `(key, value)` pair the store expects.
///
/// Pods without a namespace or name (which should never happen for
/// real cluster data) are skipped with a warning rather than panicking.
fn pod_key(pod: &Pod) -> Option<ResourceKey> {
    let meta = pod.metadata.clone();
    let name = meta.name?;
    let namespace = meta.namespace?;
    Some(ResourceKey::namespaced("Pod", namespace, name))
}

/// Apply a single watcher event to the store.
pub async fn apply_event(store: &ResourceStore<Pod>, event: Event<Pod>) -> anyhow::Result<()> {
    match event {
        Event::Apply(pod) => {
            if let Some(key) = pod_key(&pod) {
                store.upsert(key, pod).await;
            } else {
                warn!("skipping pod with missing name or namespace");
            }
        }
        Event::Delete(pod) => {
            if let Some(key) = pod_key(&pod) {
                store.remove(&key).await;
            }
        }
        Event::Init => {
            // Beginning of a relist — clear the store. The matching
            // `InitDone` arrives after all `InitApply`s; consumers see
            // a consistent snapshot only after that point.
            store.replace_all(std::iter::empty()).await;
        }
        Event::InitApply(pod) => {
            if let Some(key) = pod_key(&pod) {
                store.upsert(key, pod).await;
            }
        }
        Event::InitDone => {
            // Nothing to do; the store reflects the relisted state.
        }
    }
    Ok(())
}

/// Spawn a long-running watch on Pods (all namespaces) that feeds events
/// into the given store. Returns once the stream ends or errors fatally.
/// Transient errors are logged and retried by the kube-rs watcher.
pub async fn run_pod_watcher(client: Client, store: ResourceStore<Pod>) -> anyhow::Result<()> {
    let api: Api<Pod> = Api::all(client);
    let mut stream = watcher(api, watcher::Config::default()).boxed();

    while let Some(event) = stream.next().await {
        match event {
            Ok(ev) => apply_event(&store, ev).await?,
            Err(e) => {
                warn!(error = %e, "pod watcher transient error; kube-rs will retry");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn make_pod(namespace: &str, name: &str) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn apply_event_upserts_into_store() {
        let store = ResourceStore::<Pod>::new();
        let pod = make_pod("default", "nginx");

        apply_event(&store, Event::Apply(pod)).await.unwrap();

        assert_eq!(store.len().await, 1);
    }

    #[tokio::test]
    async fn delete_event_removes_from_store() {
        let store = ResourceStore::<Pod>::new();
        let pod = make_pod("default", "nginx");

        apply_event(&store, Event::Apply(pod.clone())).await.unwrap();
        apply_event(&store, Event::Delete(pod)).await.unwrap();

        assert!(store.is_empty().await);
    }

    #[tokio::test]
    async fn init_clears_store() {
        let store = ResourceStore::<Pod>::new();
        apply_event(&store, Event::Apply(make_pod("default", "stale")))
            .await
            .unwrap();

        apply_event(&store, Event::Init).await.unwrap();

        assert!(store.is_empty().await);
    }

    #[tokio::test]
    async fn init_apply_repopulates_store() {
        let store = ResourceStore::<Pod>::new();

        apply_event(&store, Event::Init).await.unwrap();
        apply_event(&store, Event::InitApply(make_pod("default", "a")))
            .await
            .unwrap();
        apply_event(&store, Event::InitApply(make_pod("default", "b")))
            .await
            .unwrap();
        apply_event(&store, Event::InitDone).await.unwrap();

        assert_eq!(store.len().await, 2);
    }

    #[tokio::test]
    async fn pod_missing_namespace_is_skipped_not_panicked() {
        let store = ResourceStore::<Pod>::new();
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("orphan".to_string()),
                namespace: None,
                ..Default::default()
            },
            ..Default::default()
        };

        apply_event(&store, Event::Apply(pod)).await.unwrap();

        assert!(store.is_empty().await);
    }
}
