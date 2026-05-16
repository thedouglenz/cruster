//! Adapter from `kube::runtime::watcher` events to `ResourceStore` operations.

use futures::StreamExt;
use kube::runtime::watcher;
use kube::runtime::watcher::Event;
use kube::{Api, Client};
use tracing::warn;

use crate::kind::ResourceKind;
use crate::store::ResourceStore;

/// Apply a single watcher event to the store.
pub async fn apply_event<K: ResourceKind>(
    store: &ResourceStore<K::Object>,
    event: Event<K::Object>,
) -> anyhow::Result<()> {
    match event {
        Event::Apply(obj) => {
            if let Some(key) = K::key(&obj) {
                store.upsert(key, obj).await;
            } else {
                warn!(kind = K::name(), "skipping object with missing metadata");
            }
        }
        Event::Delete(obj) => {
            if let Some(key) = K::key(&obj) {
                store.remove(&key).await;
            }
        }
        Event::Init => {
            store.replace_all(std::iter::empty()).await;
        }
        Event::InitApply(obj) => {
            if let Some(key) = K::key(&obj) {
                store.upsert(key, obj).await;
            }
        }
        Event::InitDone => {}
    }
    Ok(())
}

/// Spawn a long-running watch that feeds events into the given store.
///
/// For namespaced kinds the watch covers all namespaces. For
/// cluster-scoped kinds `Api::all` handles both.
pub async fn run_watcher<K: ResourceKind>(
    client: Client,
    store: ResourceStore<K::Object>,
) -> anyhow::Result<()> {
    let api: Api<K::Object> = Api::all(client);
    let mut stream = watcher(api, watcher::Config::default()).boxed();

    while let Some(event) = stream.next().await {
        match event {
            Ok(ev) => apply_event::<K>(&store, ev).await?,
            Err(e) => {
                warn!(
                    kind = K::name(),
                    error = %e,
                    "watcher transient error; kube-rs will retry"
                );
            }
        }
    }

    Ok(())
}

/// Backwards-compatible alias used by `cruster-bin` until the binary is
/// migrated to the registry-driven approach in Task 4.
pub async fn run_pod_watcher(
    client: Client,
    store: ResourceStore<k8s_openapi::api::core::v1::Pod>,
) -> anyhow::Result<()> {
    run_watcher::<crate::kind::Pods>(client, store).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::Pods;
    use k8s_openapi::api::core::v1::Pod;
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
    async fn generic_apply_event_upserts_pod() {
        let store = ResourceStore::<Pod>::new();
        apply_event::<Pods>(&store, Event::Apply(make_pod("default", "nginx")))
            .await
            .unwrap();
        assert_eq!(store.len().await, 1);
    }

    #[tokio::test]
    async fn delete_event_removes_pod() {
        let store = ResourceStore::<Pod>::new();
        let pod = make_pod("default", "nginx");
        apply_event::<Pods>(&store, Event::Apply(pod.clone()))
            .await
            .unwrap();
        apply_event::<Pods>(&store, Event::Delete(pod)).await.unwrap();
        assert!(store.is_empty().await);
    }

    #[tokio::test]
    async fn generic_apply_event_handles_init_lifecycle() {
        let store = ResourceStore::<Pod>::new();
        apply_event::<Pods>(&store, Event::Apply(make_pod("default", "stale")))
            .await
            .unwrap();
        apply_event::<Pods>(&store, Event::Init).await.unwrap();
        apply_event::<Pods>(&store, Event::InitApply(make_pod("default", "a")))
            .await
            .unwrap();
        apply_event::<Pods>(&store, Event::InitDone).await.unwrap();
        assert_eq!(store.len().await, 1);
    }

    #[tokio::test]
    async fn pod_missing_namespace_is_skipped() {
        let store = ResourceStore::<Pod>::new();
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("orphan".into()),
                namespace: None,
                ..Default::default()
            },
            ..Default::default()
        };
        apply_event::<Pods>(&store, Event::Apply(pod)).await.unwrap();
        assert!(store.is_empty().await);
    }
}
