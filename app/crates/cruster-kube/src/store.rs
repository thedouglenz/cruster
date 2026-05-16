//! In-memory snapshot of a single kind of resource.
//!
//! The store is the single source of truth. Watchers push deltas in;
//! the TUI and MCP server read snapshots out.

use std::collections::BTreeMap;
use std::sync::Arc;

use cruster_core::ResourceKey;
use tokio::sync::RwLock;

/// Generic, per-kind in-memory store keyed by `ResourceKey`.
///
/// `T` is the resource payload — typically `k8s_openapi::api::core::v1::Pod`
/// or similar. The store is generic so the same plumbing serves every
/// kind in later phases.
#[derive(Debug)]
pub struct ResourceStore<T> {
    inner: Arc<RwLock<BTreeMap<ResourceKey, T>>>,
}

impl<T> Clone for ResourceStore<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> Default for ResourceStore<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ResourceStore<T> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    pub async fn upsert(&self, key: ResourceKey, value: T) {
        self.inner.write().await.insert(key, value);
    }

    pub async fn remove(&self, key: &ResourceKey) {
        self.inner.write().await.remove(key);
    }

    /// Replace the entire contents atomically. Used by `restart` events
    /// from the kube watcher.
    pub async fn replace_all(&self, items: impl IntoIterator<Item = (ResourceKey, T)>) {
        let new_map: BTreeMap<_, _> = items.into_iter().collect();
        *self.inner.write().await = new_map;
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }
}

impl<T: Clone> ResourceStore<T> {
    /// Take a point-in-time copy of the store contents. Requires `T: Clone`
    /// because values are duplicated out into a new `Vec`.
    pub async fn snapshot(&self) -> Vec<(ResourceKey, T)> {
        self.inner
            .read()
            .await
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upsert_then_snapshot_returns_the_value() {
        let store: ResourceStore<String> = ResourceStore::new();
        let key = ResourceKey::namespaced("Pod", "default", "nginx");
        store.upsert(key.clone(), "running".to_string()).await;

        let snap = store.snapshot().await;
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].0, key);
        assert_eq!(snap[0].1, "running");
    }

    #[tokio::test]
    async fn upsert_with_existing_key_overwrites() {
        let store: ResourceStore<i32> = ResourceStore::new();
        let key = ResourceKey::namespaced("Pod", "default", "p");
        store.upsert(key.clone(), 1).await;
        store.upsert(key.clone(), 2).await;

        let snap = store.snapshot().await;
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].1, 2);
    }

    #[tokio::test]
    async fn remove_deletes_the_entry() {
        let store: ResourceStore<()> = ResourceStore::new();
        let key = ResourceKey::namespaced("Pod", "default", "p");
        store.upsert(key.clone(), ()).await;
        store.remove(&key).await;
        assert!(store.is_empty().await);
    }

    #[tokio::test]
    async fn replace_all_swaps_contents_atomically() {
        let store: ResourceStore<String> = ResourceStore::new();
        store
            .upsert(ResourceKey::namespaced("Pod", "default", "old"), "x".into())
            .await;

        let new_items = vec![
            (ResourceKey::namespaced("Pod", "default", "a"), "1".into()),
            (ResourceKey::namespaced("Pod", "default", "b"), "2".into()),
        ];
        store.replace_all(new_items).await;

        let snap = store.snapshot().await;
        assert_eq!(snap.len(), 2);
        assert!(snap.iter().all(|(k, _)| k.name == "a" || k.name == "b"));
    }

    #[tokio::test]
    async fn snapshot_is_ordered_by_key() {
        let store: ResourceStore<()> = ResourceStore::new();
        store
            .upsert(ResourceKey::namespaced("Pod", "default", "z"), ())
            .await;
        store
            .upsert(ResourceKey::namespaced("Pod", "default", "a"), ())
            .await;

        let snap = store.snapshot().await;
        let names: Vec<&str> = snap.iter().map(|(k, _)| k.name.as_str()).collect();
        assert_eq!(names, vec!["a", "z"]);
    }

    #[tokio::test]
    async fn clones_share_state() {
        let store: ResourceStore<()> = ResourceStore::new();
        let other = store.clone();

        store
            .upsert(ResourceKey::namespaced("Pod", "default", "p"), ())
            .await;
        assert_eq!(other.len().await, 1);
    }
}
