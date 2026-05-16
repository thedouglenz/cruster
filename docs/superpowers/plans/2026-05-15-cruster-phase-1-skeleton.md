# Cruster Phase 1: Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A runnable `cruster` binary that connects to the user's current
kubeconfig context, opens a TUI, lists pods in real time via a watch
stream, supports vertical navigation and quit, and is benchmarked for
cold-start time. This proves out the monorepo layout, the Rust workspace
structure, the watch→store→render pipeline, and the perf harness.

**Architecture:** Monorepo. The Rust app lives under `app/` as a cargo
workspace with four crates. `app/crates/cruster-kube` owns the watch
stream and an in-memory `ResourceStore` (single source of truth,
testable in isolation). `app/crates/cruster-tui` owns the ratatui app
loop and renders from a store snapshot. `app/crates/cruster-core` holds
shared types. `app/crates/cruster-bin` is a thin wiring binary. A
criterion bench in `app/benches/` guards the cold-start perf budget
from day one. `web/` is reserved for the landing page (its own
phase); a placeholder is created so the layout exists from day one.

**Tech Stack:** Rust (stable), ratatui 0.29, crossterm 0.28, kube 0.95,
k8s-openapi 0.23, tokio 1.x, anyhow 1.x, thiserror 2.x, futures 0.3,
tracing 0.1, criterion 0.5.

---

## File Structure

Files to create in this phase:

```
cruster/                                    # monorepo root
├── README.md                               # monorepo overview
├── .gitignore                              # /target, node_modules, etc.
├── .editorconfig                           # tabs/spaces conventions
├── .github/
│   └── workflows/
│       └── ci.yml                          # fmt + clippy + test for app/
├── docs/                                   # (already exists)
├── app/                                    # the Rust TUI workspace
│   ├── Cargo.toml                          # cargo workspace manifest
│   ├── rust-toolchain.toml                 # pin stable Rust
│   ├── README.md                           # app-specific build notes
│   ├── crates/
│   │   ├── cruster-core/
│   │   │   ├── Cargo.toml
│   │   │   └── src/lib.rs                  # ResourceKey, shared types
│   │   ├── cruster-kube/
│   │   │   ├── Cargo.toml
│   │   │   └── src/
│   │   │       ├── lib.rs
│   │   │       ├── store.rs                # in-memory ResourceStore
│   │   │       └── watcher.rs              # kube-rs watch → store
│   │   ├── cruster-tui/
│   │   │   ├── Cargo.toml
│   │   │   └── src/
│   │   │       ├── lib.rs
│   │   │       ├── app.rs                  # App state + event loop
│   │   │       └── views/
│   │   │           ├── mod.rs
│   │   │           └── pods.rs             # pods table view
│   │   └── cruster-bin/
│   │       ├── Cargo.toml
│   │       └── src/main.rs                 # wires kube + tui together
│   └── benches/
│       └── startup.rs                      # cold-start benchmark
└── web/                                    # landing page (placeholder)
    └── README.md
```

**Responsibility per crate:**
- `cruster-core` — shared types only. No I/O, no async.
- `cruster-kube` — all interaction with the Kubernetes API. Exposes a
  store that the rest of the system reads from.
- `cruster-tui` — all ratatui rendering and input. Knows nothing about
  the kube API; it receives a snapshot from a store.
- `cruster-bin` — wiring. Constructs the runtime, the store, the
  watcher, the TUI app; runs them.

**Working directory convention:** All `cargo` commands run from the
`app/` directory. Plan steps explicitly `cd app && …` for clarity.

---

## Task 1: Monorepo scaffolding

**Files:**
- Create: `README.md`
- Create: `.gitignore`
- Create: `.editorconfig`
- Create: `web/README.md`

- [ ] **Step 1: Add the top-level README**

Write `README.md`:

```markdown
# cruster

A Kubernetes TUI for people who live in k9s all day. Built in Rust.
Optimized for speed, ergonomics, and Claude Code compatibility.

This repository is a monorepo:

- `app/` — the Rust TUI (cargo workspace, `cd app && cargo build`)
- `web/` — the marketing site (not yet built)
- `docs/` — specs and implementation plans

See `docs/superpowers/specs/` for the product spec and
`docs/superpowers/plans/` for the phase-by-phase implementation plans.

Status: pre-alpha. Not yet usable.
```

- [ ] **Step 2: Add .gitignore**

Write `.gitignore`:

```
# Rust
/app/target
/app/**/*.rs.bk
/app/Cargo.lock.bak

# Web (future)
/web/node_modules
/web/.next
/web/.astro
/web/dist
/web/build

# General
.DS_Store
.idea/
.vscode/
*.swp
*.swo
```

(Note: `app/Cargo.lock` IS committed — this is an application workspace,
not a library.)

- [ ] **Step 3: Add .editorconfig**

Write `.editorconfig`:

```
root = true

[*]
charset = utf-8
end_of_line = lf
indent_style = space
insert_final_newline = true
trim_trailing_whitespace = true

[*.rs]
indent_size = 4

[*.{toml,yml,yaml,md,json,tsx,ts,jsx,js}]
indent_size = 2
```

- [ ] **Step 4: Add web placeholder**

Write `web/README.md`:

```markdown
# cruster web

Marketing site for cruster. Not yet built; planned for Phase 5 (polish
+ launch) of the implementation roadmap. See
`docs/superpowers/specs/2026-05-15-cruster-design.md` for product
context.
```

- [ ] **Step 5: Commit**

```bash
git add README.md .gitignore .editorconfig web/README.md
git commit -m "chore: scaffold monorepo with app/ and web/ layout"
```

---

## Task 2: Cargo workspace + Rust tooling

**Files:**
- Create: `app/Cargo.toml`
- Create: `app/rust-toolchain.toml`
- Create: `app/README.md`

- [ ] **Step 1: Create the workspace manifest**

Write `app/Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
    "crates/cruster-core",
    "crates/cruster-kube",
    "crates/cruster-tui",
    "crates/cruster-bin",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.80"
license = "Proprietary"
authors = ["Doug Lenz <thedouglenz@gmail.com>"]
repository = "https://github.com/thedouglenz/cruster"

[workspace.dependencies]
anyhow = "1"
thiserror = "2"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "signal", "time"] }
futures = "0.3"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# TUI
ratatui = "0.29"
crossterm = "0.28"

# Kube
kube = { version = "0.95", default-features = false, features = ["client", "runtime", "rustls-tls"] }
k8s-openapi = { version = "0.23", features = ["latest"] }

# Workspace crates
cruster-core = { path = "crates/cruster-core" }
cruster-kube = { path = "crates/cruster-kube" }
cruster-tui = { path = "crates/cruster-tui" }

[profile.release]
lto = "thin"
codegen-units = 1
strip = "debuginfo"
```

- [ ] **Step 2: Pin Rust toolchain**

Write `app/rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

- [ ] **Step 3: Add an app/-level README**

Write `app/README.md`:

```markdown
# cruster app

Rust workspace for the cruster TUI.

## Build

```sh
cargo build
```

## Run

```sh
cargo run -p cruster-bin
```

Uses your active kubeconfig context.

## Test

```sh
cargo test --workspace
```
```

- [ ] **Step 4: Temporarily comment out missing members**

Member crates don't exist yet. Edit `app/Cargo.toml`:

```toml
members = [
    "crates/cruster-core",
    # "crates/cruster-kube",
    # "crates/cruster-tui",
    # "crates/cruster-bin",
]
```

(Each subsequent task uncomments its member at the right moment.)

- [ ] **Step 5: Commit**

```bash
git add app/Cargo.toml app/rust-toolchain.toml app/README.md
git commit -m "chore(app): scaffold cargo workspace and tooling"
```

---

## Task 3: cruster-core crate

**Files:**
- Create: `app/crates/cruster-core/Cargo.toml`
- Create: `app/crates/cruster-core/src/lib.rs`
- Test: same file (inline `#[cfg(test)]` module)

- [ ] **Step 1: Create the crate manifest**

Create `app/crates/cruster-core/Cargo.toml`:

```toml
[package]
name = "cruster-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
serde = { workspace = true }

[dev-dependencies]
serde_json = { workspace = true }
```

- [ ] **Step 2: Write the failing tests + implementation**

Create `app/crates/cruster-core/src/lib.rs`:

```rust
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
    pub fn namespaced(kind: impl Into<String>, namespace: impl Into<String>, name: impl Into<String>) -> Self {
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
```

- [ ] **Step 3: Run tests**

```bash
cd app && cargo test -p cruster-core
```
Expected: 3 passed.

- [ ] **Step 4: Verify the workspace builds**

```bash
cd app && cargo build
```
Expected: exits 0 (only `cruster-core` compiles; other members are
commented out).

- [ ] **Step 5: Commit**

```bash
git add app/crates/cruster-core app/Cargo.toml
git commit -m "feat(core): add ResourceKey type with serde + display"
```

---

## Task 4: cruster-kube — ResourceStore (TDD against fake events)

**Files:**
- Modify: `app/Cargo.toml` (uncomment `cruster-kube`)
- Create: `app/crates/cruster-kube/Cargo.toml`
- Create: `app/crates/cruster-kube/src/lib.rs`
- Create: `app/crates/cruster-kube/src/store.rs`

- [ ] **Step 1: Uncomment the member**

Edit `app/Cargo.toml`:

```toml
members = [
    "crates/cruster-core",
    "crates/cruster-kube",
    # "crates/cruster-tui",
    # "crates/cruster-bin",
]
```

- [ ] **Step 2: Create the crate manifest**

Create `app/crates/cruster-kube/Cargo.toml`:

```toml
[package]
name = "cruster-kube"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
cruster-core = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true }
futures = { workspace = true }
tracing = { workspace = true }
kube = { workspace = true }
k8s-openapi = { workspace = true }
serde = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread", "test-util"] }
```

- [ ] **Step 3: Write the store + tests**

Create `app/crates/cruster-kube/src/store.rs`:

```rust
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
        Self { inner: Arc::clone(&self.inner) }
    }
}

impl<T> Default for ResourceStore<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> ResourceStore<T> {
    pub fn new() -> Self {
        Self { inner: Arc::new(RwLock::new(BTreeMap::new())) }
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

    pub async fn snapshot(&self) -> Vec<(ResourceKey, T)> {
        self.inner
            .read()
            .await
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
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
```

Create `app/crates/cruster-kube/src/lib.rs`:

```rust
//! Kubernetes API integration: watch streams and in-memory stores.

pub mod store;

pub use store::ResourceStore;
```

- [ ] **Step 4: Run tests**

```bash
cd app && cargo test -p cruster-kube
```
Expected: 6 passed.

- [ ] **Step 5: Clippy clean**

```bash
cd app && cargo clippy -p cruster-kube --all-targets -- -D warnings
```
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add app/crates/cruster-kube app/Cargo.toml
git commit -m "feat(kube): add generic in-memory ResourceStore with tests"
```

---

## Task 5: cruster-kube — Watcher (pods only)

**Files:**
- Create: `app/crates/cruster-kube/src/watcher.rs`
- Modify: `app/crates/cruster-kube/src/lib.rs`

The watcher consumes `kube::runtime::watcher` events and feeds them
into a `ResourceStore<Pod>`. Test the *adapter logic* (event → store
operation) against synthetic events; integration with a real cluster
is covered by the binary running in Task 8.

- [ ] **Step 1: Write the adapter + tests**

Create `app/crates/cruster-kube/src/watcher.rs`:

```rust
//! Adapter from `kube::runtime::watcher` events to `ResourceStore` operations.

use cruster_core::ResourceKey;
use k8s_openapi::api::core::v1::Pod;
use kube::runtime::watcher::Event;
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
        apply_event(&store, Event::Apply(make_pod("default", "stale"))).await.unwrap();

        apply_event(&store, Event::Init).await.unwrap();

        assert!(store.is_empty().await);
    }

    #[tokio::test]
    async fn init_apply_repopulates_store() {
        let store = ResourceStore::<Pod>::new();

        apply_event(&store, Event::Init).await.unwrap();
        apply_event(&store, Event::InitApply(make_pod("default", "a"))).await.unwrap();
        apply_event(&store, Event::InitApply(make_pod("default", "b"))).await.unwrap();
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
```

Update `app/crates/cruster-kube/src/lib.rs`:

```rust
//! Kubernetes API integration: watch streams and in-memory stores.

pub mod store;
pub mod watcher;

pub use store::ResourceStore;
pub use watcher::apply_event;
```

- [ ] **Step 2: Run tests**

```bash
cd app && cargo test -p cruster-kube
```
Expected: 11 passed (6 store + 5 watcher).

- [ ] **Step 3: Add the watch-stream driver**

Append to `app/crates/cruster-kube/src/watcher.rs`:

```rust
use futures::StreamExt;
use kube::runtime::watcher;
use kube::{Api, Client};

/// Spawn a long-running watch on Pods (all namespaces) that feeds
/// events into the given store. Returns once the stream ends or
/// errors fatally. Transient errors are logged and retried by the
/// kube-rs watcher.
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
```

Update `app/crates/cruster-kube/src/lib.rs` to also re-export
`run_pod_watcher`:

```rust
//! Kubernetes API integration: watch streams and in-memory stores.

pub mod store;
pub mod watcher;

pub use store::ResourceStore;
pub use watcher::{apply_event, run_pod_watcher};
```

- [ ] **Step 4: Confirm it compiles**

```bash
cd app && cargo build -p cruster-kube
```
Expected: exits 0. (No unit test for `run_pod_watcher` — it needs a real
apiserver. Manual verification happens in Task 8.)

- [ ] **Step 5: Clippy clean**

```bash
cd app && cargo clippy -p cruster-kube --all-targets -- -D warnings
```
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add app/crates/cruster-kube
git commit -m "feat(kube): add watcher event adapter and pod watch driver"
```

---

## Task 6: cruster-tui — App scaffold

**Files:**
- Modify: `app/Cargo.toml` (uncomment `cruster-tui`)
- Create: `app/crates/cruster-tui/Cargo.toml`
- Create: `app/crates/cruster-tui/src/lib.rs`
- Create: `app/crates/cruster-tui/src/app.rs`

The `App` holds UI state and exposes:
- `App::new(store)` — construct with a pod store
- `App::run()` — main loop
- `App::handle_key(KeyEvent, row_count)` — testable input handler

We unit-test `handle_key` directly; the full render loop is exercised
by the binary in Task 8.

- [ ] **Step 1: Uncomment the member**

Edit `app/Cargo.toml`:

```toml
members = [
    "crates/cruster-core",
    "crates/cruster-kube",
    "crates/cruster-tui",
    # "crates/cruster-bin",
]
```

- [ ] **Step 2: Create the manifest**

Create `app/crates/cruster-tui/Cargo.toml`:

```toml
[package]
name = "cruster-tui"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
cruster-core = { workspace = true }
cruster-kube = { workspace = true }
anyhow = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
ratatui = { workspace = true }
crossterm = { workspace = true }
k8s-openapi = { workspace = true }
futures = { workspace = true }
```

- [ ] **Step 3: Write the App + tests**

Create `app/crates/cruster-tui/src/app.rs`:

```rust
//! Application state and event loop.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use cruster_kube::ResourceStore;
use k8s_openapi::api::core::v1::Pod;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::views::pods::PodsView;

/// Whether the app should keep running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopState {
    Continue,
    Quit,
}

pub struct App {
    pod_store: ResourceStore<Pod>,
    pods_view: PodsView,
}

impl App {
    pub fn new(pod_store: ResourceStore<Pod>) -> Self {
        Self {
            pod_store,
            pods_view: PodsView::new(),
        }
    }

    /// Pure handler — no I/O. Returns what the loop should do next.
    ///
    /// `row_count` is the current visible row count; we need it to
    /// clamp `move_down` past the end.
    pub fn handle_key(&mut self, key: KeyEvent, row_count: usize) -> LoopState {
        if key.kind != KeyEventKind::Press {
            return LoopState::Continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => LoopState::Quit,
            KeyCode::Char('j') | KeyCode::Down => {
                self.pods_view.move_down(row_count);
                LoopState::Continue
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.pods_view.move_up();
                LoopState::Continue
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.pods_view.move_to_top();
                LoopState::Continue
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.pods_view.move_to_bottom(row_count);
                LoopState::Continue
            }
            _ => LoopState::Continue,
        }
    }

    /// Main loop. Runs until the user quits or the terminal closes.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut terminal = init_terminal()?;
        let result = self.run_loop(&mut terminal).await;
        restore_terminal()?;
        result
    }

    async fn run_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> anyhow::Result<()> {
        loop {
            let snapshot = self.pod_store.snapshot().await;
            let row_count = snapshot.len();
            terminal.draw(|f| self.pods_view.render(f, &snapshot))?;

            // Poll with a short timeout so the snapshot refreshes when
            // no key is pressed.
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if self.handle_key(key, row_count) == LoopState::Quit {
                        return Ok(());
                    }
                }
            }
        }
    }
}

fn init_terminal() -> anyhow::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal() -> anyhow::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn app() -> App {
        App::new(ResourceStore::<Pod>::new())
    }

    #[test]
    fn q_quits() {
        let mut a = app();
        assert_eq!(a.handle_key(press(KeyCode::Char('q')), 0), LoopState::Quit);
    }

    #[test]
    fn esc_quits() {
        let mut a = app();
        assert_eq!(a.handle_key(press(KeyCode::Esc), 0), LoopState::Quit);
    }

    #[test]
    fn unknown_key_continues() {
        let mut a = app();
        assert_eq!(a.handle_key(press(KeyCode::Char('x')), 0), LoopState::Continue);
    }

    #[test]
    fn down_advances_selection() {
        let mut a = app();
        a.handle_key(press(KeyCode::Char('j')), 3);
        assert_eq!(a.pods_view.selected(), 1);
    }

    #[test]
    fn down_clamps_at_last_row() {
        let mut a = app();
        for _ in 0..10 {
            a.handle_key(press(KeyCode::Char('j')), 3);
        }
        assert_eq!(a.pods_view.selected(), 2);
    }

    #[test]
    fn up_does_not_underflow() {
        let mut a = app();
        a.handle_key(press(KeyCode::Char('k')), 3);
        assert_eq!(a.pods_view.selected(), 0);
    }

    #[test]
    fn capital_g_jumps_to_bottom() {
        let mut a = app();
        a.handle_key(press(KeyCode::Char('G')), 5);
        assert_eq!(a.pods_view.selected(), 4);
    }

    #[test]
    fn key_release_is_ignored() {
        let mut a = app();
        let release = KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        assert_eq!(a.handle_key(release, 0), LoopState::Continue);
    }
}
```

Create `app/crates/cruster-tui/src/lib.rs`:

```rust
//! Cruster terminal UI.

pub mod app;
pub mod views;

pub use app::App;
```

- [ ] **Step 4: Run tests — expect compile error**

```bash
cd app && cargo test -p cruster-tui
```
Expected: compile error referencing `crate::views::pods::PodsView`.
This is the failing-test state. Task 7 implements `PodsView`.

Do NOT commit until Task 7 makes these compile.

---

## Task 7: cruster-tui — PodsView

**Files:**
- Create: `app/crates/cruster-tui/src/views/mod.rs`
- Create: `app/crates/cruster-tui/src/views/pods.rs`

- [ ] **Step 1: Write the views module + PodsView**

Create `app/crates/cruster-tui/src/views/mod.rs`:

```rust
//! Resource views.

pub mod pods;
```

Create `app/crates/cruster-tui/src/views/pods.rs`:

```rust
//! Pods table view.

use cruster_core::ResourceKey;
use k8s_openapi::api::core::v1::Pod;
use ratatui::layout::Constraint;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};
use ratatui::Frame;

#[derive(Debug, Default)]
pub struct PodsView {
    selected: usize,
}

impl PodsView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self, row_count: usize) {
        if row_count == 0 {
            self.selected = 0;
            return;
        }
        let max = row_count.saturating_sub(1);
        self.selected = (self.selected + 1).min(max);
    }

    pub fn move_to_top(&mut self) {
        self.selected = 0;
    }

    pub fn move_to_bottom(&mut self, row_count: usize) {
        self.selected = row_count.saturating_sub(1);
    }

    pub fn render(&self, frame: &mut Frame<'_>, rows: &[(ResourceKey, Pod)]) {
        let area = frame.area();

        let header = Row::new(vec!["NAMESPACE", "NAME", "STATUS", "READY", "RESTARTS"])
            .style(Style::default().add_modifier(Modifier::BOLD));

        let table_rows: Vec<Row> = rows
            .iter()
            .map(|(key, pod)| {
                let ns = key.namespace.as_deref().unwrap_or("-");
                let status = pod_phase(pod);
                let ready = pod_ready(pod);
                let restarts = pod_restarts(pod);
                Row::new(vec![
                    Cell::from(ns.to_string()),
                    Cell::from(key.name.clone()),
                    Cell::from(status),
                    Cell::from(ready),
                    Cell::from(restarts.to_string()),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(20),
            Constraint::Min(20),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Length(10),
        ];

        let table = Table::new(table_rows, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(format!(
                " pods ({}) — j/k move · q quit ",
                rows.len()
            )))
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        let mut state = TableState::default();
        if !rows.is_empty() {
            state.select(Some(self.selected.min(rows.len() - 1)));
        }

        frame.render_stateful_widget(table, area, &mut state);
    }
}

fn pod_phase(pod: &Pod) -> String {
    pod.status
        .as_ref()
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "?".to_string())
}

fn pod_ready(pod: &Pod) -> String {
    let containers = pod
        .status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .map(|c| c.as_slice())
        .unwrap_or(&[]);
    let total = containers.len();
    let ready = containers.iter().filter(|c| c.ready).count();
    format!("{ready}/{total}")
}

fn pod_restarts(pod: &Pod) -> i32 {
    pod.status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .map(|cs| cs.iter().map(|c| c.restart_count).sum())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_down_on_empty_stays_at_zero() {
        let mut v = PodsView::new();
        v.move_down(0);
        assert_eq!(v.selected(), 0);
    }

    #[test]
    fn move_down_clamps_to_last_row() {
        let mut v = PodsView::new();
        for _ in 0..10 {
            v.move_down(3);
        }
        assert_eq!(v.selected(), 2);
    }

    #[test]
    fn move_up_on_zero_stays_at_zero() {
        let mut v = PodsView::new();
        v.move_up();
        assert_eq!(v.selected(), 0);
    }

    #[test]
    fn move_to_bottom_on_empty_is_zero() {
        let mut v = PodsView::new();
        v.move_to_bottom(0);
        assert_eq!(v.selected(), 0);
    }
}
```

- [ ] **Step 2: Run all cruster-tui tests**

```bash
cd app && cargo test -p cruster-tui
```
Expected: 12 passed (8 from `app.rs` + 4 from `pods.rs`).

- [ ] **Step 3: Clippy clean**

```bash
cd app && cargo clippy -p cruster-tui --all-targets -- -D warnings
```
Expected: no warnings.

- [ ] **Step 4: Commit**

```bash
git add app/crates/cruster-tui app/Cargo.toml
git commit -m "feat(tui): add App scaffold and PodsView with selection tests"
```

---

## Task 8: cruster-bin — wire it all together

**Files:**
- Modify: `app/Cargo.toml` (uncomment `cruster-bin`)
- Create: `app/crates/cruster-bin/Cargo.toml`
- Create: `app/crates/cruster-bin/src/main.rs`

- [ ] **Step 1: Uncomment the member**

Edit `app/Cargo.toml`:

```toml
members = [
    "crates/cruster-core",
    "crates/cruster-kube",
    "crates/cruster-tui",
    "crates/cruster-bin",
]
```

- [ ] **Step 2: Create the manifest**

Create `app/crates/cruster-bin/Cargo.toml`:

```toml
[package]
name = "cruster-bin"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[[bin]]
name = "cruster"
path = "src/main.rs"

[dependencies]
cruster-core = { workspace = true }
cruster-kube = { workspace = true }
cruster-tui = { workspace = true }
anyhow = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
kube = { workspace = true }
k8s-openapi = { workspace = true }
```

- [ ] **Step 3: Write `main.rs`**

Create `app/crates/cruster-bin/src/main.rs`:

```rust
use anyhow::Context;
use cruster_kube::{run_pod_watcher, ResourceStore};
use cruster_tui::App;
use k8s_openapi::api::core::v1::Pod;
use kube::Client;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let client = Client::try_default()
        .await
        .context("failed to construct kube client from default kubeconfig context")?;

    let store = ResourceStore::<Pod>::new();
    let watcher_store = store.clone();
    let watcher_handle = tokio::spawn(async move {
        if let Err(e) = run_pod_watcher(client, watcher_store).await {
            tracing::error!(error = %e, "pod watcher exited");
        }
    });

    let mut app = App::new(store);
    let app_result = app.run().await;

    watcher_handle.abort();
    app_result
}

fn init_tracing() {
    // Log to stderr only — stdout is owned by the TUI. Default off
    // unless RUST_LOG is set, so a normal run does not pollute the
    // terminal.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("off"));
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();
}
```

- [ ] **Step 4: Build the binary**

```bash
cd app && cargo build -p cruster-bin
```
Expected: exits 0.

- [ ] **Step 5: Run against the local k3d cluster**

The user has `k3d-a8s-dev` active in their kubeconfig (visible in the
k9s pane). With that context active:

```bash
cd app && cargo run -p cruster-bin
```

Expected: TUI opens, shows the ~10 pods k9s shows for the
`agent-platform` and `kube-system` namespaces. `j`/`k` move the
highlight. `q` exits cleanly.

If pods don't appear within a second or two, check stderr — the binary
writes tracing output there. Re-run with
`RUST_LOG=cruster_kube=debug,kube=info` for verbose output.

- [ ] **Step 6: Verify terminal is restored on quit**

After quitting, the cursor must be visible and raw mode off. If it
isn't, the `restore_terminal` path failed — investigate before moving
on.

- [ ] **Step 7: Clippy clean across the workspace**

```bash
cd app && cargo clippy --workspace --all-targets -- -D warnings
```
Expected: no warnings.

- [ ] **Step 8: Commit**

```bash
git add app/crates/cruster-bin app/Cargo.toml
git commit -m "feat(bin): wire kube watcher and TUI into runnable cruster binary"
```

---

## Task 9: CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`

CI runs Rust checks for `app/` only in this phase. When `web/` becomes
real in Phase 5, we'll add a parallel `web` job.

- [ ] **Step 1: Write the workflow**

Create `.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  app-fmt:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: app
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt
      - run: cargo fmt --all -- --check

  app-clippy:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: app
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: app
      - run: cargo clippy --workspace --all-targets -- -D warnings

  app-test:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    defaults:
      run:
        working-directory: app
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: app
      - run: cargo test --workspace --all-targets
```

- [ ] **Step 2: Sanity-check the YAML**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"
```
Expected: exits 0, no output. (If `pyyaml` isn't installed, skip — the
GitHub UI will catch parse errors on push.)

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add fmt + clippy + test workflow for app/"
```

---

## Task 10: Cold-start benchmark

**Files:**
- Create: `app/benches/startup.rs`
- Modify: `app/crates/cruster-bin/Cargo.toml` (add bench harness)

The spec requires cold start to first interactive frame under 100ms on
a 100-pod cluster. We can't benchmark "first interactive frame" in a
unit test, but we *can* benchmark the synchronous startup work that
runs before the watch stream returns its first event — store
allocation, view construction, ratatui initialization. This bench
guards regressions in that critical path.

- [ ] **Step 1: Add criterion to cruster-bin**

Modify `app/crates/cruster-bin/Cargo.toml`:

```toml
[package]
name = "cruster-bin"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[[bin]]
name = "cruster"
path = "src/main.rs"

[[bench]]
name = "startup"
harness = false
path = "../../benches/startup.rs"

[dependencies]
cruster-core = { workspace = true }
cruster-kube = { workspace = true }
cruster-tui = { workspace = true }
anyhow = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
kube = { workspace = true }
k8s-openapi = { workspace = true }

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
```

(`path = "../../benches/startup.rs"` resolves from
`app/crates/cruster-bin/` up to `app/benches/startup.rs`.)

- [ ] **Step 2: Write the benchmark**

Create `app/benches/startup.rs`:

```rust
//! Cold-start micro-benchmarks for the synchronous startup path.
//!
//! Measures the work that runs before the first watch event arrives —
//! store allocation and app construction. Network and apiserver
//! latency are deliberately excluded; they belong in an integration
//! benchmark, not a CI-runnable micro-bench.

use criterion::{criterion_group, criterion_main, Criterion};
use cruster_kube::ResourceStore;
use cruster_tui::App;
use k8s_openapi::api::core::v1::Pod;

fn bench_store_construction(c: &mut Criterion) {
    c.bench_function("store_new", |b| {
        b.iter(|| {
            let _ = ResourceStore::<Pod>::new();
        });
    });
}

fn bench_app_construction(c: &mut Criterion) {
    c.bench_function("app_new", |b| {
        b.iter(|| {
            let store = ResourceStore::<Pod>::new();
            let _ = App::new(store);
        });
    });
}

criterion_group!(benches, bench_store_construction, bench_app_construction);
criterion_main!(benches);
```

- [ ] **Step 3: Run the benchmark**

```bash
cd app && cargo bench -p cruster-bin --bench startup -- --quick
```
Expected: completes in under 5 seconds. Both functions report times
well under 1 microsecond. Record the numbers in the commit message as
a baseline.

- [ ] **Step 4: Commit**

```bash
git add app/crates/cruster-bin/Cargo.toml app/benches/startup.rs
git commit -m "bench: add cold-start micro-benchmarks as a perf baseline"
```

---

## Task 11: Self-review and phase exit

- [ ] **Step 1: Run the full test + lint matrix locally**

```bash
cd app && cargo fmt --all -- --check
cd app && cargo clippy --workspace --all-targets -- -D warnings
cd app && cargo test --workspace --all-targets
```
Expected: all three pass cleanly.

- [ ] **Step 2: Manually verify end-to-end against k3d**

With `k3d-a8s-dev` as the active kube context:

```bash
cd app && cargo run --release -p cruster-bin
```

Verify:
1. TUI opens within ~1s.
2. All pods in the cluster are listed.
3. `j`/`k`/arrows move the selection.
4. `g` jumps to top, `G` jumps to bottom.
5. `q` exits and the terminal is restored cleanly.
6. `kubectl delete pod <name> -n <ns>` removes the row within ~1s.
7. Creating a pod adds the row within ~1s.

Fix anything broken before declaring phase 1 done.

- [ ] **Step 3: Update the top-level README**

Edit `README.md`:

```markdown
# cruster

A Kubernetes TUI for people who live in k9s all day. Built in Rust.
Optimized for speed, ergonomics, and Claude Code compatibility.

This repository is a monorepo:

- `app/` — the Rust TUI (cargo workspace, `cd app && cargo build`)
- `web/` — the marketing site (not yet built)
- `docs/` — specs and implementation plans

## Status

Phase 1 (skeleton) complete: single cluster, pods view, read-only
navigation. See `docs/superpowers/plans/` for upcoming phases.

## Quick start

```sh
cd app && cargo run --release -p cruster-bin
```

Uses your active kubeconfig context.
```

- [ ] **Step 4: Commit the README update**

```bash
git add README.md
git commit -m "docs: update README for phase 1 completion"
```

- [ ] **Step 5: Tag the phase-1 milestone**

```bash
git tag -a phase-1-skeleton -m "Phase 1 skeleton: runnable TUI with pods watch"
```

---

## Phase 1 exit criteria

All of the following must be true before moving to Phase 2 planning:

1. `cd app && cargo test --workspace` passes with no failures.
2. `cd app && cargo clippy --workspace --all-targets -- -D warnings`
   is clean.
3. `cd app && cargo fmt --all -- --check` is clean.
4. `cd app && cargo run --release -p cruster-bin` against the k3d
   cluster shows pods, navigates with j/k/g/G, quits cleanly with q.
5. Adding or deleting a pod via kubectl reflects in the TUI within
   ~1s.
6. `cd app && cargo bench` runs and reports baseline numbers.
7. CI passes on a push to `main`.

When all seven hold, write the Phase 2 plan (parity-lite: more kinds,
describe, logs, exec, port-forward, YAML edit).
