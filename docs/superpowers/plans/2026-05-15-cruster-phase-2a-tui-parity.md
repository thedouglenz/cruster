# Cruster Phase 2A: TUI Parity-Lite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cruster reaches k9s parity on the 80% of operator workflows.
Adds 7 more resource kinds (Deployment, Service, Node, Event,
ConfigMap, Secret, Namespace) on top of the existing Pods view, plus
the five action verbs every operator needs every day: describe, view
logs (with follow + grep), exec into a container, port-forward, and
edit YAML via `$EDITOR`. A minimal command-mode kind switcher lets
users move between views with `:po`, `:deploy`, etc. (the full
command palette comes in Phase 3.)

**Architecture:** Introduces two cross-cutting abstractions early so
adding each kind stays small and repetitive. `cruster-kube` gets a
`ResourceKind` trait that captures everything generic code needs to
know about a kind (api type, plural name, key extraction, scope).
The existing `run_pod_watcher` becomes one instantiation of a
generic `run_watcher<K>`. `cruster-tui` gets a `ResourceView`
trait — each kind ships a view that implements it. The `App` holds
a `StoreRegistry` (one named field per kind) and a `Box<dyn
ResourceView>` for the active view. Switching kinds swaps the
boxed view.

**Tech Stack:** Same as Phase 1 (Rust stable, ratatui 0.29,
crossterm 0.28, kube 0.95, k8s-openapi 0.23, tokio 1.x). New deps:
`async-trait` (for the view trait), `chrono` (for event timestamps),
`portable-pty` 0.8 (for exec PTY handling).

**Phase 2B follows.** This plan covers the TUI surface. The
LLM-efficient CLI mode (`cruster get/describe/logs/events --llm`,
schemas, token budgeting, agent-friendly pruning) is Phase 2B,
written after this lands.

---

## File Structure

New files this phase adds (and the modifications):

```
app/crates/cruster-kube/src/
├── kind.rs                 # ResourceKind trait + marker structs (Pods, Deployments, …)
├── registry.rs             # StoreRegistry: one ResourceStore per kind
└── watcher.rs              # MODIFIED: generic run_watcher<K>; run_pod_watcher becomes a thin wrapper

app/crates/cruster-tui/src/
├── view.rs                 # ResourceView trait
├── command.rs              # `:kind` command-mode prompt
├── actions/
│   ├── mod.rs
│   ├── describe.rs         # describe action (TUI panel)
│   ├── logs.rs             # logs action (panel with follow + grep)
│   ├── exec.rs             # exec action (PTY suspend/restore)
│   ├── port_forward.rs     # port-forward action (panel)
│   └── yaml_edit.rs        # YAML view + $EDITOR edit
└── views/
    ├── pods.rs             # MODIFIED: implements ResourceView; columns refined
    ├── deployments.rs      # NEW
    ├── services.rs         # NEW
    ├── nodes.rs            # NEW
    ├── events.rs           # NEW
    ├── configmaps.rs       # NEW
    ├── secrets.rs          # NEW
    └── namespaces.rs       # NEW

app/crates/cruster-bin/src/
└── main.rs                 # MODIFIED: spawn watchers per kind, populate StoreRegistry
```

**Working directory convention:** All `cargo` commands run from
`app/`. Plan steps explicitly `cd app && …` for clarity.

---

## Task 1: ResourceKind trait + generalized watcher

**Files:**
- Create: `app/crates/cruster-kube/src/kind.rs`
- Modify: `app/crates/cruster-kube/src/lib.rs` (re-exports)
- Modify: `app/crates/cruster-kube/src/watcher.rs` (generalize)
- Modify: `app/crates/cruster-kube/Cargo.toml` (add k8s-openapi types)

The Pods-only `run_pod_watcher` becomes a generic
`run_watcher<K: ResourceKind>`. `run_pod_watcher` stays as a
backwards-compatible alias so the binary still compiles before
Task 9 swaps it out.

- [ ] **Step 1: Add the ResourceKind trait**

Create `app/crates/cruster-kube/src/kind.rs`:

```rust
//! Kind abstraction: what generic code needs to know about a Kubernetes
//! resource type to watch it, store it, and present it.
//!
//! Each kind cruster supports is represented by a zero-sized marker
//! struct (e.g. `Pods`, `Deployments`) implementing this trait.

use std::fmt::Debug;

use cruster_core::ResourceKey;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{
    ConfigMap, Event, Namespace, Node, Pod, Secret, Service,
};
use kube::Resource;
use serde::de::DeserializeOwned;

/// What the watcher / store / view machinery needs to know about a kind.
pub trait ResourceKind: Send + Sync + 'static {
    /// The k8s-openapi type for the object.
    type Object: Resource<DynamicType = ()>
        + Clone
        + Debug
        + DeserializeOwned
        + Send
        + Sync
        + 'static;

    /// Human display name in singular form: "Pod", "Deployment".
    fn name() -> &'static str;

    /// Lowercase plural for CLI / route addressing: "pods", "deployments".
    fn plural() -> &'static str;

    /// kubectl-style short alias: "po", "deploy", "svc".
    fn short() -> &'static str;

    /// `true` for cluster-scoped kinds (Node, Namespace); `false` for
    /// namespaced kinds.
    fn cluster_scoped() -> bool {
        false
    }

    /// Extract the canonical `ResourceKey` from an object. Returns
    /// `None` if the object is missing required metadata (a defensive
    /// fallback — real apiserver objects always have name/namespace
    /// where required).
    fn key(obj: &Self::Object) -> Option<ResourceKey>;
}

// ---- Marker types -------------------------------------------------------

pub struct Pods;
impl ResourceKind for Pods {
    type Object = Pod;
    fn name() -> &'static str { "Pod" }
    fn plural() -> &'static str { "pods" }
    fn short() -> &'static str { "po" }
    fn key(obj: &Pod) -> Option<ResourceKey> {
        let m = &obj.metadata;
        Some(ResourceKey::namespaced(
            "Pod",
            m.namespace.clone()?,
            m.name.clone()?,
        ))
    }
}

pub struct Deployments;
impl ResourceKind for Deployments {
    type Object = Deployment;
    fn name() -> &'static str { "Deployment" }
    fn plural() -> &'static str { "deployments" }
    fn short() -> &'static str { "deploy" }
    fn key(obj: &Deployment) -> Option<ResourceKey> {
        let m = &obj.metadata;
        Some(ResourceKey::namespaced(
            "Deployment",
            m.namespace.clone()?,
            m.name.clone()?,
        ))
    }
}

pub struct Services;
impl ResourceKind for Services {
    type Object = Service;
    fn name() -> &'static str { "Service" }
    fn plural() -> &'static str { "services" }
    fn short() -> &'static str { "svc" }
    fn key(obj: &Service) -> Option<ResourceKey> {
        let m = &obj.metadata;
        Some(ResourceKey::namespaced(
            "Service",
            m.namespace.clone()?,
            m.name.clone()?,
        ))
    }
}

pub struct Nodes;
impl ResourceKind for Nodes {
    type Object = Node;
    fn name() -> &'static str { "Node" }
    fn plural() -> &'static str { "nodes" }
    fn short() -> &'static str { "no" }
    fn cluster_scoped() -> bool { true }
    fn key(obj: &Node) -> Option<ResourceKey> {
        Some(ResourceKey::cluster_scoped("Node", obj.metadata.name.clone()?))
    }
}

pub struct Events;
impl ResourceKind for Events {
    type Object = Event;
    fn name() -> &'static str { "Event" }
    fn plural() -> &'static str { "events" }
    fn short() -> &'static str { "ev" }
    fn key(obj: &Event) -> Option<ResourceKey> {
        let m = &obj.metadata;
        Some(ResourceKey::namespaced(
            "Event",
            m.namespace.clone()?,
            m.name.clone()?,
        ))
    }
}

pub struct ConfigMaps;
impl ResourceKind for ConfigMaps {
    type Object = ConfigMap;
    fn name() -> &'static str { "ConfigMap" }
    fn plural() -> &'static str { "configmaps" }
    fn short() -> &'static str { "cm" }
    fn key(obj: &ConfigMap) -> Option<ResourceKey> {
        let m = &obj.metadata;
        Some(ResourceKey::namespaced(
            "ConfigMap",
            m.namespace.clone()?,
            m.name.clone()?,
        ))
    }
}

pub struct Secrets;
impl ResourceKind for Secrets {
    type Object = Secret;
    fn name() -> &'static str { "Secret" }
    fn plural() -> &'static str { "secrets" }
    fn short() -> &'static str { "sec" }
    fn key(obj: &Secret) -> Option<ResourceKey> {
        let m = &obj.metadata;
        Some(ResourceKey::namespaced(
            "Secret",
            m.namespace.clone()?,
            m.name.clone()?,
        ))
    }
}

pub struct Namespaces;
impl ResourceKind for Namespaces {
    type Object = Namespace;
    fn name() -> &'static str { "Namespace" }
    fn plural() -> &'static str { "namespaces" }
    fn short() -> &'static str { "ns" }
    fn cluster_scoped() -> bool { true }
    fn key(obj: &Namespace) -> Option<ResourceKey> {
        Some(ResourceKey::cluster_scoped("Namespace", obj.metadata.name.clone()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    #[test]
    fn pods_key_namespaced() {
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("nginx".into()),
                namespace: Some("default".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let k = Pods::key(&pod).unwrap();
        assert_eq!(k.kind, "Pod");
        assert_eq!(k.namespace.as_deref(), Some("default"));
        assert_eq!(k.name, "nginx");
    }

    #[test]
    fn nodes_key_cluster_scoped() {
        let node = Node {
            metadata: ObjectMeta {
                name: Some("worker-1".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let k = Nodes::key(&node).unwrap();
        assert_eq!(k.kind, "Node");
        assert_eq!(k.namespace, None);
        assert_eq!(k.name, "worker-1");
        assert!(Nodes::cluster_scoped());
    }

    #[test]
    fn missing_required_metadata_returns_none() {
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("orphan".into()),
                namespace: None,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(Pods::key(&pod).is_none());
    }
}
```

- [ ] **Step 2: Generalize the watcher**

Replace `app/crates/cruster-kube/src/watcher.rs`:

```rust
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
/// cluster-scoped kinds the namespace param is irrelevant (kube-rs's
/// `Api::all` handles both).
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

/// Backwards-compatible alias used by `cruster-bin` until Task 9
/// swaps in the registry-driven approach.
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
```

- [ ] **Step 3: Re-export from lib.rs**

Replace `app/crates/cruster-kube/src/lib.rs`:

```rust
//! Kubernetes API integration: watch streams and in-memory stores.

pub mod kind;
pub mod registry;
pub mod store;
pub mod watcher;

pub use kind::{
    ConfigMaps, Deployments, Events, Namespaces, Nodes, Pods, ResourceKind, Secrets, Services,
};
pub use registry::StoreRegistry;
pub use store::ResourceStore;
pub use watcher::{apply_event, run_pod_watcher, run_watcher};
```

(Note: `registry` module is created in Task 2. This re-export is added now so all imports settle in one pass.)

- [ ] **Step 4: Run tests**

```bash
cd app && cargo test -p cruster-kube
```
Expected: 11 (old) + 3 (new kind tests) + 3 (new generic watcher tests) − 5 (removed old watcher tests for hardcoded pod adapter; they're replaced by the generic variants) = 12 passing. If you find tests no longer compile because the old `apply_event` signature changed, delete the obsolete tests — the generic-watcher tests cover the same ground.

If the count is different, that's fine — what matters is they all pass.

- [ ] **Step 5: Clippy + commit (registry module placeholder)**

Create a placeholder `app/crates/cruster-kube/src/registry.rs` so the workspace builds before Task 2:

```rust
//! Placeholder — full implementation lands in Task 2.
```

Run:
```bash
cd app && cargo clippy -p cruster-kube --all-targets -- -D warnings
```
Expected: no warnings.

Commit:
```bash
git add app/crates/cruster-kube
git commit -m "feat(kube): introduce ResourceKind trait + generic watcher"
```

---

## Task 2: StoreRegistry

**Files:**
- Replace placeholder: `app/crates/cruster-kube/src/registry.rs`

One named field per kind. `cruster-bin` constructs one
`StoreRegistry`, spawns a watcher per kind, and hands it to the TUI.
Views look up their kind's store from the registry by name.

- [ ] **Step 1: Write the registry**

Replace `app/crates/cruster-kube/src/registry.rs`:

```rust
//! Holds one `ResourceStore` per kind cruster knows about.
//!
//! The registry is the single source from which views fetch their
//! snapshots. The binary populates it at startup by spawning one
//! watcher per kind.

use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{
    ConfigMap, Event, Namespace, Node, Pod, Secret, Service,
};

use crate::store::ResourceStore;

#[derive(Clone, Default)]
pub struct StoreRegistry {
    pub pods: ResourceStore<Pod>,
    pub deployments: ResourceStore<Deployment>,
    pub services: ResourceStore<Service>,
    pub nodes: ResourceStore<Node>,
    pub events: ResourceStore<Event>,
    pub configmaps: ResourceStore<ConfigMap>,
    pub secrets: ResourceStore<Secret>,
    pub namespaces: ResourceStore<Namespace>,
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
        r1.pods
            .upsert(key, Pod::default())
            .await;
        assert_eq!(r2.pods.len().await, 1);
    }
}
```

- [ ] **Step 2: Tests + commit**

```bash
cd app && cargo test -p cruster-kube && cargo clippy -p cruster-kube --all-targets -- -D warnings
```
Both pass.

```bash
git add app/crates/cruster-kube/src/registry.rs
git commit -m "feat(kube): add StoreRegistry with one store per kind"
```

---

## Task 3: ResourceView trait

**Files:**
- Create: `app/crates/cruster-tui/src/view.rs`
- Modify: `app/crates/cruster-tui/src/lib.rs`
- Modify: `app/crates/cruster-tui/Cargo.toml` (add `async-trait`)

The view trait every per-kind view implements. App holds a
`Box<dyn ResourceView>` for the currently-active view and rotates it
when the user switches kinds.

- [ ] **Step 1: Add async-trait dep**

Modify `app/crates/cruster-tui/Cargo.toml`, adding to `[dependencies]`:

```toml
async-trait = "0.1"
```

Also add to `[workspace.dependencies]` in `app/Cargo.toml`:

```toml
async-trait = "0.1"
```

And reference it from `cruster-tui/Cargo.toml` as:
```toml
async-trait = { workspace = true }
```

- [ ] **Step 2: Write the trait**

Create `app/crates/cruster-tui/src/view.rs`:

```rust
//! ResourceView: trait every per-kind TUI view implements.

use async_trait::async_trait;
use crossterm::event::KeyEvent;
use cruster_kube::StoreRegistry;
use ratatui::Frame;

use crate::app::LoopState;

/// One TUI view, scoped to one resource kind.
///
/// The App holds a `Box<dyn ResourceView>` for the active view and
/// calls `refresh` once per frame, then `render`, then routes keys
/// through `handle_key`. Views own their own selection state and any
/// kind-specific decoration; they read snapshots from the registry on
/// each refresh.
#[async_trait]
pub trait ResourceView: Send {
    /// Stable identifier for the view: the kind's plural ("pods",
    /// "deployments"). Used by the command-mode switcher.
    fn id(&self) -> &'static str;

    /// Refresh the view's cached snapshot from the registry. Called
    /// once per frame before `render`. Default implementations may
    /// fetch and stash a snapshot vector.
    async fn refresh(&mut self, registry: &StoreRegistry);

    /// Render the view into the full frame area.
    fn render(&self, frame: &mut Frame<'_>);

    /// Handle a single key press. Return `LoopState::Quit` to exit
    /// the app, `LoopState::Continue` otherwise.
    fn handle_key(&mut self, key: KeyEvent) -> LoopState;
}
```

- [ ] **Step 3: Update lib.rs**

Modify `app/crates/cruster-tui/src/lib.rs`:

```rust
//! Cruster terminal UI.

pub mod app;
pub mod view;
pub mod views;

pub use app::App;
pub use view::ResourceView;
```

- [ ] **Step 4: Tests**

The trait itself has no tests (just an interface). Trait
implementations are tested in the per-kind tasks.

```bash
cd app && cargo build -p cruster-tui
```
Expected: builds. PodsView still works because we haven't ported it
to the trait yet (that's Task 4).

- [ ] **Step 5: Commit**

```bash
git add app/crates/cruster-tui app/Cargo.toml
git commit -m "feat(tui): add ResourceView trait scaffolding"
```

---

## Task 4: Port PodsView to ResourceView

**Files:**
- Modify: `app/crates/cruster-tui/src/views/pods.rs`
- Modify: `app/crates/cruster-tui/src/app.rs`

PodsView gains the trait impl. It now owns its own snapshot. The
App is restructured to hold a `Box<dyn ResourceView>` and a
`StoreRegistry`.

- [ ] **Step 1: Modify PodsView**

Replace `app/crates/cruster-tui/src/views/pods.rs`:

```rust
//! Pods table view.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::core::v1::Pod;
use ratatui::Frame;
use ratatui::layout::Constraint;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};

use crate::app::LoopState;
use crate::view::ResourceView;

#[derive(Debug, Default)]
pub struct PodsView {
    selected: usize,
    snapshot: Vec<(ResourceKey, Pod)>,
}

impl PodsView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    fn row_count(&self) -> usize {
        self.snapshot.len()
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        let n = self.row_count();
        if n == 0 {
            self.selected = 0;
            return;
        }
        let max = n.saturating_sub(1);
        self.selected = (self.selected + 1).min(max);
    }

    pub fn move_to_top(&mut self) {
        self.selected = 0;
    }

    pub fn move_to_bottom(&mut self) {
        self.selected = self.row_count().saturating_sub(1);
    }
}

#[async_trait]
impl ResourceView for PodsView {
    fn id(&self) -> &'static str {
        "pods"
    }

    async fn refresh(&mut self, registry: &StoreRegistry) {
        self.snapshot = registry.pods.snapshot().await;
        if self.selected > 0 && self.selected >= self.snapshot.len() {
            self.selected = self.snapshot.len().saturating_sub(1);
        }
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();

        let header = Row::new(vec!["NAMESPACE", "NAME", "STATUS", "READY", "RESTARTS"])
            .style(Style::default().add_modifier(Modifier::BOLD));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .map(|(key, pod)| {
                let ns = key.namespace.as_deref().unwrap_or("-");
                Row::new(vec![
                    Cell::from(ns.to_string()),
                    Cell::from(key.name.clone()),
                    Cell::from(pod_phase(pod)),
                    Cell::from(pod_ready(pod)),
                    Cell::from(pod_restarts(pod).to_string()),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(20),
            Constraint::Min(20),
            Constraint::Length(14),
            Constraint::Length(8),
            Constraint::Length(10),
        ];

        let table = Table::new(table_rows, widths)
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(
                        " pods ({}) — j/k move · :kind switch · q quit ",
                        self.snapshot.len()
                    )),
            )
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        let mut state = TableState::default();
        if !self.snapshot.is_empty() {
            state.select(Some(self.selected.min(self.snapshot.len() - 1)));
        }

        frame.render_stateful_widget(table, area, &mut state);
    }

    fn handle_key(&mut self, key: KeyEvent) -> LoopState {
        if key.kind != KeyEventKind::Press {
            return LoopState::Continue;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_down(),
            KeyCode::Char('k') | KeyCode::Up => self.move_up(),
            KeyCode::Char('g') | KeyCode::Home => self.move_to_top(),
            KeyCode::Char('G') | KeyCode::End => self.move_to_bottom(),
            _ => {}
        }
        LoopState::Continue
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
    use cruster_kube::StoreRegistry;

    fn make_pod_entry(namespace: &str, name: &str) -> (ResourceKey, Pod) {
        let pod = Pod {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some(name.into()),
                namespace: Some(namespace.into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let k = ResourceKey::namespaced("Pod", namespace, name);
        (k, pod)
    }

    #[test]
    fn move_down_on_empty_stays_at_zero() {
        let mut v = PodsView::new();
        v.move_down();
        assert_eq!(v.selected(), 0);
    }

    #[test]
    fn move_down_clamps_to_last_row() {
        let mut v = PodsView::new();
        for i in 0..3 {
            v.snapshot.push(make_pod_entry("default", &format!("p{i}")));
        }
        for _ in 0..10 {
            v.move_down();
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
        v.move_to_bottom();
        assert_eq!(v.selected(), 0);
    }

    #[tokio::test]
    async fn refresh_loads_snapshot_from_registry() {
        let registry = StoreRegistry::new();
        let (key, pod) = make_pod_entry("default", "nginx");
        registry.pods.upsert(key, pod).await;

        let mut view = PodsView::new();
        view.refresh(&registry).await;
        assert_eq!(view.snapshot.len(), 1);
    }

    #[tokio::test]
    async fn refresh_clamps_selection_when_rows_removed() {
        let registry = StoreRegistry::new();
        for i in 0..5 {
            let (k, p) = make_pod_entry("default", &format!("p{i}"));
            registry.pods.upsert(k, p).await;
        }

        let mut view = PodsView::new();
        view.refresh(&registry).await;
        view.selected = 4;

        // Wipe to one pod
        registry.pods.replace_all(std::iter::empty()).await;
        let (k, p) = make_pod_entry("default", "only");
        registry.pods.upsert(k, p).await;
        view.refresh(&registry).await;
        assert_eq!(view.selected, 0);
    }
}
```

- [ ] **Step 2: Restructure App**

Replace `app/crates/cruster-tui/src/app.rs`:

```rust
//! Application state and event loop.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use cruster_kube::StoreRegistry;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::view::ResourceView;
use crate::views::pods::PodsView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopState {
    Continue,
    Quit,
}

pub struct App {
    registry: StoreRegistry,
    current_view: Box<dyn ResourceView>,
}

impl App {
    pub fn new(registry: StoreRegistry) -> Self {
        Self {
            registry,
            current_view: Box::new(PodsView::new()),
        }
    }

    /// Replace the currently active view.
    pub fn switch_view(&mut self, view: Box<dyn ResourceView>) {
        self.current_view = view;
    }

    /// Pure, top-level key handler.
    ///
    /// Returns `LoopState::Quit` for global quit (`q`, `Esc`).
    /// Otherwise delegates to the current view.
    pub fn handle_key(&mut self, key: KeyEvent) -> LoopState {
        if key.kind != KeyEventKind::Press {
            return LoopState::Continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => LoopState::Quit,
            _ => self.current_view.handle_key(key),
        }
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut terminal = init_terminal()?;
        let result = self.run_loop(&mut terminal).await;
        restore_terminal()?;
        result
    }

    async fn run_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> anyhow::Result<()> {
        loop {
            self.current_view.refresh(&self.registry).await;
            terminal.draw(|f| self.current_view.render(f))?;

            if event::poll(Duration::from_millis(100))?
                && let Event::Key(key) = event::read()?
                && self.handle_key(key) == LoopState::Quit
            {
                return Ok(());
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
        App::new(StoreRegistry::new())
    }

    #[test]
    fn q_quits() {
        let mut a = app();
        assert_eq!(a.handle_key(press(KeyCode::Char('q'))), LoopState::Quit);
    }

    #[test]
    fn esc_quits() {
        let mut a = app();
        assert_eq!(a.handle_key(press(KeyCode::Esc)), LoopState::Quit);
    }

    #[test]
    fn unknown_key_continues_via_view() {
        let mut a = app();
        assert_eq!(a.handle_key(press(KeyCode::Char('x'))), LoopState::Continue);
    }

    #[test]
    fn key_release_is_ignored_at_app_level() {
        let mut a = app();
        let release = KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        assert_eq!(a.handle_key(release), LoopState::Continue);
    }
}
```

- [ ] **Step 3: Modify cruster-bin to use StoreRegistry**

Replace `app/crates/cruster-bin/src/main.rs`:

```rust
use anyhow::Context;
use cruster_kube::{Deployments, Pods, ResourceKind, ResourceStore, StoreRegistry, run_watcher};
use cruster_tui::App;
use kube::Client;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let client = Client::try_default()
        .await
        .context("failed to construct kube client from default kubeconfig context")?;

    let registry = StoreRegistry::new();

    // Spawn one watcher per kind. (More kinds wired up in subsequent tasks.)
    spawn_watcher::<Pods>(client.clone(), registry.pods.clone());
    spawn_watcher::<Deployments>(client.clone(), registry.deployments.clone());

    let mut app = App::new(registry);
    let app_result = app.run().await;

    // tokio aborts spawned tasks when the runtime drops; nothing more to do.
    app_result
}

fn spawn_watcher<K: ResourceKind>(client: Client, store: ResourceStore<K::Object>) {
    tokio::spawn(async move {
        if let Err(e) = run_watcher::<K>(client, store).await {
            tracing::error!(kind = K::name(), error = %e, "watcher exited");
        }
    });
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("off"));
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();
}
```

(Each subsequent kind task adds another `spawn_watcher::<K>` call here.)

- [ ] **Step 4: Run all tests + clippy**

```bash
cd app && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```
Expected: all tests pass, no clippy warnings.

- [ ] **Step 5: Commit**

```bash
git add app/crates/cruster-tui app/crates/cruster-bin
git commit -m "refactor(tui): port App + PodsView to ResourceView trait"
```

---

## Task 5: Add Deployments view

**Files:**
- Create: `app/crates/cruster-tui/src/views/deployments.rs`
- Modify: `app/crates/cruster-tui/src/views/mod.rs` (register the new view)
- Modify: `app/crates/cruster-bin/src/main.rs` (already covers Deployments from Task 4; no change)

- [ ] **Step 1: Write the view**

Create `app/crates/cruster-tui/src/views/deployments.rs`:

```rust
//! Deployments table view.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::apps::v1::Deployment;
use ratatui::Frame;
use ratatui::layout::Constraint;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};

use crate::app::LoopState;
use crate::view::ResourceView;

#[derive(Debug, Default)]
pub struct DeploymentsView {
    selected: usize,
    snapshot: Vec<(ResourceKey, Deployment)>,
}

impl DeploymentsView {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ResourceView for DeploymentsView {
    fn id(&self) -> &'static str {
        "deployments"
    }

    async fn refresh(&mut self, registry: &StoreRegistry) {
        self.snapshot = registry.deployments.snapshot().await;
        if self.selected > 0 && self.selected >= self.snapshot.len() {
            self.selected = self.snapshot.len().saturating_sub(1);
        }
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();

        let header = Row::new(vec!["NAMESPACE", "NAME", "READY", "UP-TO-DATE", "AVAILABLE"])
            .style(Style::default().add_modifier(Modifier::BOLD));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .map(|(key, dep)| {
                let ns = key.namespace.as_deref().unwrap_or("-");
                let (ready, desired) = ready_desired(dep);
                let updated = updated_replicas(dep);
                let available = available_replicas(dep);
                Row::new(vec![
                    Cell::from(ns.to_string()),
                    Cell::from(key.name.clone()),
                    Cell::from(format!("{ready}/{desired}")),
                    Cell::from(updated.to_string()),
                    Cell::from(available.to_string()),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(20),
            Constraint::Min(20),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Length(10),
        ];

        let table = Table::new(table_rows, widths)
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(
                        " deployments ({}) — j/k move · :kind switch · q quit ",
                        self.snapshot.len()
                    )),
            )
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        let mut state = TableState::default();
        if !self.snapshot.is_empty() {
            state.select(Some(self.selected.min(self.snapshot.len() - 1)));
        }

        frame.render_stateful_widget(table, area, &mut state);
    }

    fn handle_key(&mut self, key: KeyEvent) -> LoopState {
        if key.kind != KeyEventKind::Press {
            return LoopState::Continue;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                let max = self.snapshot.len().saturating_sub(1);
                if !self.snapshot.is_empty() {
                    self.selected = (self.selected + 1).min(max);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Char('g') | KeyCode::Home => self.selected = 0,
            KeyCode::Char('G') | KeyCode::End => {
                self.selected = self.snapshot.len().saturating_sub(1);
            }
            _ => {}
        }
        LoopState::Continue
    }
}

fn ready_desired(dep: &Deployment) -> (i32, i32) {
    let desired = dep.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);
    let ready = dep.status.as_ref().and_then(|s| s.ready_replicas).unwrap_or(0);
    (ready, desired)
}

fn updated_replicas(dep: &Deployment) -> i32 {
    dep.status
        .as_ref()
        .and_then(|s| s.updated_replicas)
        .unwrap_or(0)
}

fn available_replicas(dep: &Deployment) -> i32 {
    dep.status
        .as_ref()
        .and_then(|s| s.available_replicas)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::apps::v1::{DeploymentSpec, DeploymentStatus};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn make_dep(ns: &str, name: &str, ready: i32, desired: i32) -> Deployment {
        Deployment {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some(ns.into()),
                ..Default::default()
            },
            spec: Some(DeploymentSpec {
                replicas: Some(desired),
                ..Default::default()
            }),
            status: Some(DeploymentStatus {
                ready_replicas: Some(ready),
                updated_replicas: Some(desired),
                available_replicas: Some(ready),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn ready_desired_extracts_from_spec_and_status() {
        let d = make_dep("default", "web", 2, 3);
        assert_eq!(ready_desired(&d), (2, 3));
    }

    #[test]
    fn ready_desired_defaults_to_zero_zero_when_missing() {
        let d = Deployment::default();
        assert_eq!(ready_desired(&d), (0, 0));
    }

    #[tokio::test]
    async fn refresh_loads_deployments_from_registry() {
        let r = StoreRegistry::new();
        r.deployments
            .upsert(
                ResourceKey::namespaced("Deployment", "default", "web"),
                make_dep("default", "web", 1, 1),
            )
            .await;
        let mut v = DeploymentsView::new();
        v.refresh(&r).await;
        assert_eq!(v.snapshot.len(), 1);
    }
}
```

- [ ] **Step 2: Register the view module**

Modify `app/crates/cruster-tui/src/views/mod.rs`:

```rust
//! Resource views.

pub mod deployments;
pub mod pods;
```

- [ ] **Step 3: Test + clippy + commit**

```bash
cd app && cargo test -p cruster-tui && cargo clippy -p cruster-tui --all-targets -- -D warnings
git add app/crates/cruster-tui
git commit -m "feat(tui): add DeploymentsView"
```

---

## Tasks 6–11: Add Service, Node, Event, ConfigMap, Secret, Namespace views

For each kind, repeat the Task 5 pattern: create a new view module
implementing `ResourceView`, register it in `views/mod.rs`, add a
`spawn_watcher::<K>` call in `cruster-bin/src/main.rs`, ensure tests
+ clippy pass, commit.

Each view's columns and `cells` function differ. Use the following
specifications.

### Task 6: ServicesView

**File:** `app/crates/cruster-tui/src/views/services.rs`

Columns: `NAMESPACE`, `NAME`, `TYPE`, `CLUSTER-IP`, `PORTS`.

Helpers:
```rust
fn svc_type(s: &Service) -> String {
    s.spec
        .as_ref()
        .and_then(|sp| sp.type_.clone())
        .unwrap_or_else(|| "?".into())
}

fn svc_cluster_ip(s: &Service) -> String {
    s.spec
        .as_ref()
        .and_then(|sp| sp.cluster_ip.clone())
        .unwrap_or_else(|| "-".into())
}

fn svc_ports(s: &Service) -> String {
    s.spec
        .as_ref()
        .and_then(|sp| sp.ports.as_ref())
        .map(|ps| {
            ps.iter()
                .map(|p| match (p.port, p.protocol.as_deref()) {
                    (port, Some(proto)) => format!("{port}/{proto}"),
                    (port, None) => format!("{port}/TCP"),
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_else(|| "-".into())
}
```

`id()` returns `"services"`. Refresh reads `registry.services.snapshot()`.
Mirror the structure of Task 5 (selected state, j/k/g/G, etc.).
Title: `" services (N) — j/k move · :kind switch · q quit "`.

Tests: at least one test that `svc_ports` joins multiple ports with
comma; one that `refresh` loads from the registry.

In `cruster-bin/src/main.rs`, add:
```rust
spawn_watcher::<Services>(client.clone(), registry.services.clone());
```
(And add `Services` to the use line for `cruster_kube`.)

Commit: `feat(tui): add ServicesView`.

### Task 7: NodesView (cluster-scoped)

**File:** `app/crates/cruster-tui/src/views/nodes.rs`

Columns: `NAME`, `STATUS`, `ROLES`, `VERSION`, `OS-IMAGE`.

`id()` returns `"nodes"`.

Helpers:
```rust
fn node_status(n: &Node) -> String {
    n.status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .and_then(|cs| cs.iter().find(|c| c.type_ == "Ready"))
        .map(|c| if c.status == "True" { "Ready".into() } else { "NotReady".into() })
        .unwrap_or_else(|| "?".into())
}

fn node_roles(n: &Node) -> String {
    n.metadata
        .labels
        .as_ref()
        .map(|labels| {
            labels
                .keys()
                .filter_map(|k| k.strip_prefix("node-role.kubernetes.io/"))
                .collect::<Vec<_>>()
                .join(",")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "<none>".into())
}

fn node_version(n: &Node) -> String {
    n.status
        .as_ref()
        .and_then(|s| s.node_info.as_ref())
        .map(|info| info.kubelet_version.clone())
        .unwrap_or_else(|| "?".into())
}

fn node_os_image(n: &Node) -> String {
    n.status
        .as_ref()
        .and_then(|s| s.node_info.as_ref())
        .map(|info| info.os_image.clone())
        .unwrap_or_else(|| "?".into())
}
```

Note: Nodes are cluster-scoped — there's no NAMESPACE column.
Test: one test that `node_status` returns `"Ready"` for a node with
`Ready: True`.

Commit: `feat(tui): add NodesView (cluster-scoped)`.

### Task 8: EventsView (default-sorted by lastTimestamp desc)

**File:** `app/crates/cruster-tui/src/views/events.rs`

Columns: `NAMESPACE`, `LAST SEEN`, `TYPE`, `REASON`, `OBJECT`, `MESSAGE`.

The store is keyed by name; we sort the snapshot in `refresh` so
the newest events appear first.

```rust
async fn refresh(&mut self, registry: &StoreRegistry) {
    let mut snap = registry.events.snapshot().await;
    snap.sort_by(|a, b| event_time(&b.1).cmp(&event_time(&a.1)));
    self.snapshot = snap;
    // … clamp selection as before
}

fn event_time(e: &Event) -> Option<k8s_openapi::apimachinery::pkg::apis::meta::v1::Time> {
    e.last_timestamp.clone().or_else(|| e.event_time.clone().map(|mt| {
        k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(mt.0)
    }))
}

fn event_age(e: &Event) -> String {
    match event_time(e) {
        Some(t) => human_age(t.0),
        None => "?".into(),
    }
}

fn human_age(ts: chrono::DateTime<chrono::Utc>) -> String {
    let delta = chrono::Utc::now().signed_duration_since(ts);
    let secs = delta.num_seconds();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}

fn event_object(e: &Event) -> String {
    e.involved_object
        .name
        .as_deref()
        .map(|n| format!("{}/{}",
            e.involved_object.kind.as_deref().unwrap_or("?"),
            n))
        .unwrap_or_else(|| "-".into())
}
```

Add `chrono` to `cruster-tui/Cargo.toml`:
```toml
chrono = { version = "0.4", default-features = false, features = ["clock"] }
```
(And to workspace deps.)

Test: `human_age` returns the right string for an event a few minutes
old.

Commit: `feat(tui): add EventsView with relative-time sorting`.

### Task 9: ConfigMapsView

**File:** `app/crates/cruster-tui/src/views/configmaps.rs`

Columns: `NAMESPACE`, `NAME`, `DATA`, `AGE`.

```rust
fn cm_data_count(cm: &ConfigMap) -> usize {
    cm.data.as_ref().map(|d| d.len()).unwrap_or(0)
        + cm.binary_data.as_ref().map(|d| d.len()).unwrap_or(0)
}

fn metadata_age(m: &k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta) -> String {
    m.creation_timestamp
        .as_ref()
        .map(|t| crate::views::events::human_age(t.0))
        .unwrap_or_else(|| "?".into())
}
```

(Move `human_age` to a small shared util module if you prefer; for
now it's fine to re-export from `events`.)

Critical: the view never renders the actual data values. Use describe
or YAML view to inspect contents.

Commit: `feat(tui): add ConfigMapsView`.

### Task 10: SecretsView (with redaction)

**File:** `app/crates/cruster-tui/src/views/secrets.rs`

Columns: `NAMESPACE`, `NAME`, `TYPE`, `DATA`, `AGE`.

```rust
fn secret_type(s: &Secret) -> String {
    s.type_.clone().unwrap_or_else(|| "Opaque".into())
}

fn secret_data_count(s: &Secret) -> usize {
    s.data.as_ref().map(|d| d.len()).unwrap_or(0)
        + s.string_data.as_ref().map(|d| d.len()).unwrap_or(0)
}
```

**Hard invariant for this view: never render any secret data
values.** Only the key count. The describe/YAML/CLI surfaces all
must enforce the same; we'll add a property test in Phase 2B
covering the CLI side.

Add a test that explicitly verifies the view never includes any
secret value in its rendered output (snapshot test against ratatui's
`TestBackend`).

Commit: `feat(tui): add SecretsView with strict redaction`.

### Task 11: NamespacesView (cluster-scoped)

**File:** `app/crates/cruster-tui/src/views/namespaces.rs`

Columns: `NAME`, `STATUS`, `AGE`.

```rust
fn namespace_status(n: &Namespace) -> String {
    n.status
        .as_ref()
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "Active".into())
}
```

Commit: `feat(tui): add NamespacesView (cluster-scoped)`.

---

## Task 12: Kind switcher (`:` command mode)

**Files:**
- Create: `app/crates/cruster-tui/src/command.rs`
- Modify: `app/crates/cruster-tui/src/app.rs`
- Modify: `app/crates/cruster-tui/src/lib.rs`

A minimal "command mode" inspired by vim — press `:` to open a
prompt at the bottom of the screen, type a kind alias, hit Enter to
switch. ESC cancels.

Aliases (matching the `short()` and `plural()` of each kind):
`po` / `pods`, `deploy` / `deployments`, `svc` / `services`,
`no` / `nodes`, `ev` / `events`, `cm` / `configmaps`,
`sec` / `secrets`, `ns` / `namespaces`.

- [ ] **Step 1: Write the command mode struct + tests**

Create `app/crates/cruster-tui/src/command.rs`:

```rust
//! Vim-style command mode for switching views.
//!
//! Press `:`, type a kind alias, hit Enter to switch.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

/// State of the command-mode line.
#[derive(Debug, Default)]
pub struct CommandLine {
    /// The text the user has typed so far (without the leading `:`).
    buffer: String,
    /// `true` when the user has pressed `:` and is typing.
    active: bool,
}

/// What the App should do after a command-mode key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAction {
    /// Stay in current mode; nothing to do.
    None,
    /// Switch to the view identified by the given id ("pods", "deployments", …).
    SwitchTo(String),
    /// Cancel command mode (clear buffer, deactivate).
    Cancel,
    /// User entered an unknown alias; show an error toast.
    UnknownAlias(String),
}

impl CommandLine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    /// Activate command mode (called by the App when `:` is pressed
    /// outside command mode).
    pub fn activate(&mut self) {
        self.active = true;
        self.buffer.clear();
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.buffer.clear();
    }

    /// Handle a key event while in command mode. Returns the action
    /// the App should take.
    pub fn handle_key(&mut self, key: KeyEvent) -> CommandAction {
        if key.kind != KeyEventKind::Press {
            return CommandAction::None;
        }
        match key.code {
            KeyCode::Esc => {
                self.deactivate();
                CommandAction::Cancel
            }
            KeyCode::Enter => {
                let buf = std::mem::take(&mut self.buffer);
                self.active = false;
                match resolve_alias(&buf) {
                    Some(id) => CommandAction::SwitchTo(id.to_string()),
                    None => CommandAction::UnknownAlias(buf),
                }
            }
            KeyCode::Backspace => {
                self.buffer.pop();
                CommandAction::None
            }
            KeyCode::Char(c) => {
                self.buffer.push(c);
                CommandAction::None
            }
            _ => CommandAction::None,
        }
    }
}

/// Resolve a user-typed alias to a view id. Returns `None` for unknown
/// aliases.
pub fn resolve_alias(alias: &str) -> Option<&'static str> {
    match alias {
        "po" | "pod" | "pods" => Some("pods"),
        "deploy" | "deployment" | "deployments" => Some("deployments"),
        "svc" | "service" | "services" => Some("services"),
        "no" | "node" | "nodes" => Some("nodes"),
        "ev" | "event" | "events" => Some("events"),
        "cm" | "configmap" | "configmaps" => Some("configmaps"),
        "sec" | "secret" | "secrets" => Some("secrets"),
        "ns" | "namespace" | "namespaces" => Some("namespaces"),
        _ => None,
    }
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

    #[test]
    fn typing_buffers_chars() {
        let mut c = CommandLine::new();
        c.activate();
        c.handle_key(press(KeyCode::Char('d')));
        c.handle_key(press(KeyCode::Char('e')));
        c.handle_key(press(KeyCode::Char('p')));
        assert_eq!(c.buffer(), "dep");
    }

    #[test]
    fn enter_with_known_alias_returns_switch() {
        let mut c = CommandLine::new();
        c.activate();
        for ch in "deploy".chars() {
            c.handle_key(press(KeyCode::Char(ch)));
        }
        let action = c.handle_key(press(KeyCode::Enter));
        assert_eq!(action, CommandAction::SwitchTo("deployments".into()));
        assert!(!c.is_active());
    }

    #[test]
    fn enter_with_unknown_alias_returns_error() {
        let mut c = CommandLine::new();
        c.activate();
        for ch in "wat".chars() {
            c.handle_key(press(KeyCode::Char(ch)));
        }
        let action = c.handle_key(press(KeyCode::Enter));
        assert_eq!(action, CommandAction::UnknownAlias("wat".into()));
    }

    #[test]
    fn esc_cancels() {
        let mut c = CommandLine::new();
        c.activate();
        c.handle_key(press(KeyCode::Char('x')));
        let action = c.handle_key(press(KeyCode::Esc));
        assert_eq!(action, CommandAction::Cancel);
        assert!(!c.is_active());
        assert_eq!(c.buffer(), "");
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut c = CommandLine::new();
        c.activate();
        for ch in "abc".chars() {
            c.handle_key(press(KeyCode::Char(ch)));
        }
        c.handle_key(press(KeyCode::Backspace));
        assert_eq!(c.buffer(), "ab");
    }

    #[test]
    fn resolve_alias_recognizes_canonical_and_short() {
        assert_eq!(resolve_alias("pods"), Some("pods"));
        assert_eq!(resolve_alias("po"), Some("pods"));
        assert_eq!(resolve_alias("svc"), Some("services"));
        assert_eq!(resolve_alias("nope"), None);
    }
}
```

- [ ] **Step 2: Wire command mode into App**

Modify `app/crates/cruster-tui/src/app.rs`. Add at the top:

```rust
use crate::command::{CommandAction, CommandLine};
use crate::views::configmaps::ConfigMapsView;
use crate::views::deployments::DeploymentsView;
use crate::views::events::EventsView;
use crate::views::namespaces::NamespacesView;
use crate::views::nodes::NodesView;
use crate::views::pods::PodsView;
use crate::views::secrets::SecretsView;
use crate::views::services::ServicesView;
```

Add to the `App` struct:

```rust
pub struct App {
    registry: StoreRegistry,
    current_view: Box<dyn ResourceView>,
    command: CommandLine,
    toast: Option<String>,
}
```

Modify `App::new`:
```rust
pub fn new(registry: StoreRegistry) -> Self {
    Self {
        registry,
        current_view: Box::new(PodsView::new()),
        command: CommandLine::new(),
        toast: None,
    }
}
```

Add a view-construction helper inside `impl App`:
```rust
fn view_for_id(id: &str) -> Option<Box<dyn ResourceView>> {
    Some(match id {
        "pods" => Box::new(PodsView::new()),
        "deployments" => Box::new(DeploymentsView::new()),
        "services" => Box::new(ServicesView::new()),
        "nodes" => Box::new(NodesView::new()),
        "events" => Box::new(EventsView::new()),
        "configmaps" => Box::new(ConfigMapsView::new()),
        "secrets" => Box::new(SecretsView::new()),
        "namespaces" => Box::new(NamespacesView::new()),
        _ => return None,
    })
}
```

Modify `handle_key` to route to command mode when active:

```rust
pub fn handle_key(&mut self, key: KeyEvent) -> LoopState {
    if key.kind != KeyEventKind::Press {
        return LoopState::Continue;
    }

    // Clear toast on any keystroke.
    self.toast = None;

    if self.command.is_active() {
        match self.command.handle_key(key) {
            CommandAction::None => {}
            CommandAction::Cancel => {}
            CommandAction::SwitchTo(id) => {
                if let Some(v) = Self::view_for_id(&id) {
                    self.current_view = v;
                } else {
                    self.toast = Some(format!("no view for id: {id}"));
                }
            }
            CommandAction::UnknownAlias(alias) => {
                self.toast = Some(format!("unknown alias: :{alias}"));
            }
        }
        return LoopState::Continue;
    }

    match key.code {
        KeyCode::Char(':') => {
            self.command.activate();
            LoopState::Continue
        }
        KeyCode::Char('q') | KeyCode::Esc => LoopState::Quit,
        _ => self.current_view.handle_key(key),
    }
}
```

Modify the render loop to overlay the command line + toast:

```rust
async fn run_loop(
    &mut self,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> anyhow::Result<()> {
    loop {
        self.current_view.refresh(&self.registry).await;
        terminal.draw(|f| {
            self.current_view.render(f);
            self.render_overlay(f);
        })?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && self.handle_key(key) == LoopState::Quit
        {
            return Ok(());
        }
    }
}

fn render_overlay(&self, frame: &mut Frame<'_>) {
    let area = frame.area();
    if self.command.is_active() {
        let line = format!(":{}", self.command.buffer());
        let bar = ratatui::widgets::Paragraph::new(line)
            .style(ratatui::style::Style::default().bg(ratatui::style::Color::DarkGray));
        let rect = ratatui::layout::Rect {
            x: area.x,
            y: area.y + area.height.saturating_sub(1),
            width: area.width,
            height: 1,
        };
        frame.render_widget(bar, rect);
    } else if let Some(msg) = &self.toast {
        let bar = ratatui::widgets::Paragraph::new(msg.clone())
            .style(ratatui::style::Style::default().bg(ratatui::style::Color::Red));
        let rect = ratatui::layout::Rect {
            x: area.x,
            y: area.y + area.height.saturating_sub(1),
            width: area.width,
            height: 1,
        };
        frame.render_widget(bar, rect);
    }
}
```

Add `use ratatui::Frame;` to the imports.

- [ ] **Step 3: Re-export from lib.rs**

Modify `app/crates/cruster-tui/src/lib.rs`:

```rust
//! Cruster terminal UI.

pub mod app;
pub mod command;
pub mod view;
pub mod views;

pub use app::App;
pub use view::ResourceView;
```

- [ ] **Step 4: Add App-level tests for command mode**

Append to the `tests` module in `app/crates/cruster-tui/src/app.rs`:

```rust
#[test]
fn colon_activates_command_mode() {
    let mut a = app();
    a.handle_key(press(KeyCode::Char(':')));
    assert!(a.command.is_active());
}

#[test]
fn typing_in_command_mode_does_not_quit_on_q() {
    let mut a = app();
    a.handle_key(press(KeyCode::Char(':')));
    let state = a.handle_key(press(KeyCode::Char('q')));
    assert_eq!(state, LoopState::Continue);
    assert_eq!(a.command.buffer(), "q");
}

#[test]
fn enter_with_known_alias_switches_view() {
    let mut a = app();
    a.handle_key(press(KeyCode::Char(':')));
    for ch in "deploy".chars() {
        a.handle_key(press(KeyCode::Char(ch)));
    }
    a.handle_key(press(KeyCode::Enter));
    assert_eq!(a.current_view.id(), "deployments");
}

#[test]
fn unknown_alias_sets_toast() {
    let mut a = app();
    a.handle_key(press(KeyCode::Char(':')));
    for ch in "wat".chars() {
        a.handle_key(press(KeyCode::Char(ch)));
    }
    a.handle_key(press(KeyCode::Enter));
    assert!(a.toast.is_some());
}
```

- [ ] **Step 5: Test + clippy + commit**

```bash
cd app && cargo test -p cruster-tui && cargo clippy -p cruster-tui --all-targets -- -D warnings
git add app/crates/cruster-tui
git commit -m "feat(tui): add :-command mode for switching between kind views"
```

---

## Task 13: Describe action verb

**Files:**
- Create: `app/crates/cruster-tui/src/actions/mod.rs`
- Create: `app/crates/cruster-tui/src/actions/describe.rs`
- Modify: `app/crates/cruster-tui/src/app.rs` (route `d` key to describe)
- Modify: `app/crates/cruster-tui/src/lib.rs`

`d` on a selected row opens a describe pane below the table, showing
the kubectl-describe-equivalent. ESC closes the pane.

For Phase 2A, the describe content is a YAML dump of the resource;
Phase 2B replaces it with a properly structured describe formatter.

- [ ] **Step 1: Create actions module**

Create `app/crates/cruster-tui/src/actions/mod.rs`:

```rust
//! Action verbs that operate on a selected resource.
//!
//! Each action is a small composable widget that overlays or replaces
//! the current view's lower portion when activated. They share no
//! state with views; views provide a `selected_yaml()` hook that the
//! action consumes.

pub mod describe;
```

- [ ] **Step 2: Add describe action**

Create `app/crates/cruster-tui/src/actions/describe.rs`:

```rust
//! Describe action: shows a YAML dump of the currently selected
//! resource. (Phase 2B replaces with a structured describe.)

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

#[derive(Debug, Default)]
pub struct DescribePane {
    open: bool,
    title: String,
    content: String,
    scroll: u16,
}

impl DescribePane {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self, title: impl Into<String>, content: impl Into<String>) {
        self.title = title.into();
        self.content = content.into();
        self.scroll = 0;
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.title.clear();
        self.content.clear();
        self.scroll = 0;
    }

    /// Returns `true` if the key was consumed by the pane.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if !self.open || key.kind != KeyEventKind::Press {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                self.close();
                true
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll = self.scroll.saturating_add(1);
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll = self.scroll.saturating_sub(1);
                true
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(10);
                true
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(10);
                true
            }
            _ => true, // swallow other keys while open
        }
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        if !self.open {
            return;
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" describe: {} — esc closes ", self.title));
        let para = Paragraph::new(self.content.clone())
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        frame.render_widget(para, area);
    }
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

    #[test]
    fn open_then_close() {
        let mut p = DescribePane::new();
        p.open("Pod/default/nginx", "yaml here");
        assert!(p.is_open());
        p.close();
        assert!(!p.is_open());
    }

    #[test]
    fn esc_closes() {
        let mut p = DescribePane::new();
        p.open("x", "y");
        assert!(p.handle_key(press(KeyCode::Esc)));
        assert!(!p.is_open());
    }

    #[test]
    fn scroll_advances_with_j() {
        let mut p = DescribePane::new();
        p.open("x", "y");
        p.handle_key(press(KeyCode::Char('j')));
        p.handle_key(press(KeyCode::Char('j')));
        assert_eq!(p.scroll, 2);
    }

    #[test]
    fn scroll_does_not_underflow() {
        let mut p = DescribePane::new();
        p.open("x", "y");
        p.handle_key(press(KeyCode::Char('k')));
        assert_eq!(p.scroll, 0);
    }
}
```

- [ ] **Step 3: Hook describe into the ResourceView trait**

Modify `app/crates/cruster-tui/src/view.rs`:

```rust
#[async_trait]
pub trait ResourceView: Send {
    fn id(&self) -> &'static str;
    async fn refresh(&mut self, registry: &StoreRegistry);
    fn render(&self, frame: &mut Frame<'_>);
    fn handle_key(&mut self, key: KeyEvent) -> LoopState;

    /// YAML representation of the currently selected resource, if any.
    /// Returns `None` when the view has no selection (empty list).
    fn selected_yaml(&self) -> Option<(String, String)> {
        None
    }
}
```

Tuple is `(title, yaml)`. Default impl is `None`; per-kind views
override.

- [ ] **Step 4: Implement selected_yaml for every view**

In every view module (`pods.rs`, `deployments.rs`, …, `namespaces.rs`),
add:

```rust
fn selected_yaml(&self) -> Option<(String, String)> {
    let (key, obj) = self.snapshot.get(self.selected)?;
    let yaml = serde_yaml::to_string(obj).ok()?;
    Some((key.to_string(), yaml))
}
```

Add `serde_yaml = "0.9"` to `app/Cargo.toml` workspace deps and to
`cruster-tui/Cargo.toml`.

- [ ] **Step 5: Wire describe into the App key handler**

Modify `app/crates/cruster-tui/src/app.rs`. Add `describe_pane: DescribePane`
to the struct. In `handle_key`, before delegating to the view:

```rust
if self.describe_pane.is_open() {
    self.describe_pane.handle_key(key);
    return LoopState::Continue;
}
```

And handle the `d` keystroke when the pane is closed (not in command
mode, not quitting):

```rust
KeyCode::Char('d') => {
    if let Some((title, yaml)) = self.current_view.selected_yaml() {
        self.describe_pane.open(title, yaml);
    } else {
        self.toast = Some("nothing selected".into());
    }
    LoopState::Continue
}
```

In the render loop, render the describe pane in the bottom half when
open. Adjust the view's area to the top half when the pane is open:

```rust
fn render_with_panes(&mut self, frame: &mut Frame<'_>) {
    use ratatui::layout::{Constraint, Direction, Layout};

    let area = frame.area();
    if self.describe_pane.is_open() {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        // Top: current view (small frame trick — render then clip)
        self.current_view.render(frame); // renders to full area; will be overwritten below
        self.describe_pane.render(frame, chunks[1]);
    } else {
        self.current_view.render(frame);
    }
    self.render_overlay(frame);
}
```

(Note: views currently take `&mut Frame` with the full area. For
Phase 2A we accept the overlap — the describe pane visually covers
the lower half. A cleaner Rect-passing rework is Phase 3 work.)

- [ ] **Step 6: Tests + commit**

Add an App test:

```rust
#[test]
fn d_opens_describe_when_selection_exists() {
    let mut a = app();
    // No selection yet (empty store) → describe shows "nothing selected"
    let _ = a.handle_key(press(KeyCode::Char('d')));
    assert!(!a.describe_pane.is_open());
    assert!(a.toast.is_some());
}
```

```bash
cd app && cargo test -p cruster-tui && cargo clippy -p cruster-tui --all-targets -- -D warnings
git add app/crates/cruster-tui app/Cargo.toml
git commit -m "feat(tui): add describe action (YAML dump on 'd')"
```

---

## Task 14: Logs action verb

**Files:**
- Create: `app/crates/cruster-tui/src/actions/logs.rs`
- Modify: `app/crates/cruster-tui/src/actions/mod.rs`
- Modify: `app/crates/cruster-tui/src/app.rs` (route `l` key)

`l` on a selected Pod opens a logs pane that streams the pod's
container logs. `/` enters a grep filter. `f` toggles follow.

- [ ] **Step 1: Add tracing/log-streaming dep**

Already have tokio and kube; no new dep.

- [ ] **Step 2: Write the LogsPane**

Create `app/crates/cruster-tui/src/actions/logs.rs`:

```rust
//! Logs action: streams a pod's container logs into a scrollable pane.
//! Supports grep filtering and follow-mode toggle.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use futures::{AsyncBufReadExt, StreamExt};
use kube::api::LogParams;
use kube::{Api, Client};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

#[derive(Debug, Default)]
pub struct LogsPane {
    open: bool,
    title: String,
    lines: Arc<Mutex<Vec<String>>>,
    grep: String,
    grep_active: bool,
    scroll: u16,
    handle: Option<JoinHandle<()>>,
}

impl LogsPane {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Start streaming logs for the given pod. Closes any existing stream.
    pub async fn open(&mut self, client: Client, key: ResourceKey) {
        self.close().await;
        let ns = match &key.namespace {
            Some(n) => n.clone(),
            None => return,
        };
        let name = key.name.clone();
        self.title = format!("Pod/{}/{}", ns, name);
        let lines = self.lines.clone();
        let api: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &ns);
        let handle = tokio::spawn(async move {
            let params = LogParams {
                follow: true,
                tail_lines: Some(500),
                ..Default::default()
            };
            match api.log_stream(&name, &params).await {
                Ok(stream) => {
                    let mut reader = stream.lines();
                    while let Some(Ok(line)) = reader.next().await {
                        lines.lock().await.push(line);
                    }
                }
                Err(e) => {
                    lines
                        .lock()
                        .await
                        .push(format!("[error opening log stream: {e}]"));
                }
            }
        });
        self.handle = Some(handle);
        self.open = true;
        self.scroll = 0;
        self.grep.clear();
        self.grep_active = false;
    }

    pub async fn close(&mut self) {
        if let Some(h) = self.handle.take() {
            h.abort();
        }
        self.lines.lock().await.clear();
        self.open = false;
        self.scroll = 0;
        self.grep.clear();
        self.grep_active = false;
    }

    /// Returns `true` if the key was consumed by the pane.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if !self.open || key.kind != KeyEventKind::Press {
            return false;
        }
        if self.grep_active {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.grep_active = false;
                    true
                }
                KeyCode::Backspace => {
                    self.grep.pop();
                    true
                }
                KeyCode::Char(c) => {
                    self.grep.push(c);
                    true
                }
                _ => true,
            }
        } else {
            match key.code {
                KeyCode::Esc => {
                    self.open = false;
                    true
                }
                KeyCode::Char('/') => {
                    self.grep_active = true;
                    self.grep.clear();
                    true
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.scroll = self.scroll.saturating_add(1);
                    true
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.scroll = self.scroll.saturating_sub(1);
                    true
                }
                KeyCode::PageDown => {
                    self.scroll = self.scroll.saturating_add(20);
                    true
                }
                KeyCode::PageUp => {
                    self.scroll = self.scroll.saturating_sub(20);
                    true
                }
                _ => true,
            }
        }
    }

    pub async fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        if !self.open {
            return;
        }
        let lines = self.lines.lock().await;
        let filtered: Vec<&String> = if self.grep.is_empty() {
            lines.iter().collect()
        } else {
            lines.iter().filter(|l| l.contains(&self.grep)).collect()
        };
        let body = filtered
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let title = if self.grep.is_empty() {
            format!(" logs: {} — / grep · esc closes ", self.title)
        } else {
            format!(
                " logs: {} — grep:'{}' ({}/{}) ",
                self.title,
                self.grep,
                filtered.len(),
                lines.len()
            )
        };
        let para = Paragraph::new(body)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        frame.render_widget(para, area);
    }
}
```

(Note: the render method is async because it locks the mutex.
Wire-up in the App needs to await it; the render closure passed to
`terminal.draw` can't be async. Workaround: snapshot the lines into
a Vec under a sync lock before drawing, or use `try_lock`. For
Phase 2A, take a snapshot in the App's run-loop right before
`terminal.draw` and pass it in. Adjust the pane signature
accordingly: `render(frame, area, lines, grep, grep_active, scroll, title, open)`
or keep the pane stateful and have it expose a `snapshot_for_render()`
method that returns the rendered string.)

Simpler refactor — change `LogsPane` to expose:
```rust
pub async fn snapshot(&self) -> LogsSnapshot {
    let lines = self.lines.lock().await.clone();
    LogsSnapshot {
        title: self.title.clone(),
        scroll: self.scroll,
        grep: self.grep.clone(),
        grep_active: self.grep_active,
        lines,
        open: self.open,
    }
}
```

Then `LogsSnapshot::render(&self, frame, area)` is sync and the
App calls `let snap = pane.snapshot().await;` once per frame.

- [ ] **Step 3: Module registration**

Modify `app/crates/cruster-tui/src/actions/mod.rs`:

```rust
//! Action verbs that operate on a selected resource.

pub mod describe;
pub mod logs;
```

- [ ] **Step 4: App-level integration**

Add `logs_pane: LogsPane` to `App`. Add a `client: Option<Client>`
field to `App` so it can open log streams. `App::new` becomes:

```rust
pub fn new(registry: StoreRegistry, client: Option<Client>) -> Self { … }
```

And `main.rs` passes the client through.

Add `l` keystroke handling:

```rust
KeyCode::Char('l') => {
    if let Some(client) = &self.client {
        if let Some((title, _yaml)) = self.current_view.selected_yaml() {
            // We need the ResourceKey, not just a stringified title.
            // Add a `selected_key()` method to ResourceView.
            if let Some(key) = self.current_view.selected_key() {
                let client = client.clone();
                // open() is async; we can't await inline in a sync handler.
                // Drop it on the runtime via tokio::spawn and a oneshot.
                // For Phase 2A, simplify: open() is fire-and-forget;
                // it sets self.logs_pane.open = true synchronously and
                // spawns the stream task internally.
                // (Already the case in the LogsPane::open signature.)
            }
        }
    }
    LoopState::Continue
}
```

This requires `selected_key()` on the view trait:
```rust
fn selected_key(&self) -> Option<ResourceKey> {
    None
}
```

Override in `PodsView` (and others): return the selected row's key.

(Implementation note: making `open` async inside a sync key handler
requires the App to either become an event-loop with an mpsc to a
spawned task that owns side-effects, OR to use `tokio::runtime::Handle::current()
.block_on()` which deadlocks the runtime. The simpler architecture
is to convert `App::run_loop` such that it owns an mpsc of "Action"
enums; the key handler emits actions, the loop awaits them. This is
a small refactor and is worth doing now in Task 14.)

- [ ] **Step 5: Refactor: introduce Action enum + mpsc**

Add to `app/crates/cruster-tui/src/app.rs`:

```rust
pub enum AppAction {
    OpenLogs(ResourceKey),
    CloseLogs,
    OpenDescribe(String, String), // title, yaml
    CloseDescribe,
    SwitchView(String),
    ShowToast(String),
    Quit,
}
```

The key handler returns `Vec<AppAction>` instead of `LoopState`.
The run loop drains the actions, processes them (awaiting on async
ones), and decides whether to quit.

(This is a sizable refactor. Allocate one task to it. If the budget
feels tight, defer logs to a follow-up plan and only add describe in
this phase.)

- [ ] **Step 6: Tests for the action enum + key→action mapping**

Add unit tests that pressing `d` on a populated view emits
`OpenDescribe`, pressing `l` emits `OpenLogs`, etc. These tests
don't need a runtime — they validate the pure mapping.

- [ ] **Step 7: Commit**

```bash
git add app/crates/cruster-tui app/crates/cruster-bin app/Cargo.toml
git commit -m "feat(tui): add logs action with follow and grep"
```

---

## Task 15: Exec action verb

**Files:**
- Create: `app/crates/cruster-tui/src/actions/exec.rs`
- Modify: `app/crates/cruster-tui/Cargo.toml` (add `kube` exec feature is already in)

`s` on a selected Pod suspends the TUI and runs `kubectl exec -it
<pod> -- /bin/sh` (or whatever the user's `$SHELL` is) by
spawning kubectl as a child process. On exit, the TUI restores.

**Why shell out to kubectl:** kube-rs exec returns an attached
stream you'd have to implement TTY plumbing for. kubectl already
does this well. Shelling out is a small footprint and the right
trade-off for v1.

- [ ] **Step 1: Write the action**

Create `app/crates/cruster-tui/src/actions/exec.rs`:

```rust
//! Exec action: suspends the TUI and runs `kubectl exec -it`.

use std::io;
use std::process::Command;

use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use cruster_core::ResourceKey;

/// Run `kubectl exec -it <pod> -n <ns> -- $SHELL` in the current
/// terminal, restoring the TUI when it exits.
///
/// `kubectl_path` is the path to kubectl on the user's system; for
/// v1 we just call "kubectl" and trust `$PATH`.
pub fn exec_into(key: &ResourceKey) -> anyhow::Result<()> {
    let ns = key
        .namespace
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("exec requires a namespaced resource"))?;

    // Suspend the TUI.
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let status = Command::new("kubectl")
        .args(["exec", "-it", "-n", ns, &key.name, "--", &shell])
        .status()?;

    // Restore the TUI.
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;

    if !status.success() {
        // Don't propagate as an error — exec exited non-zero is normal
        // (e.g. shell got Ctrl-D). Toast it in the App and continue.
        anyhow::bail!("kubectl exec exited with status {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_into_rejects_cluster_scoped() {
        let key = ResourceKey::cluster_scoped("Node", "n1");
        assert!(exec_into(&key).is_err());
    }
}
```

- [ ] **Step 2: Register + wire up**

Add to `actions/mod.rs`:
```rust
pub mod exec;
```

Add `s` keystroke handling in `app.rs`:
```rust
KeyCode::Char('s') => {
    if let Some(key) = self.current_view.selected_key() {
        match crate::actions::exec::exec_into(&key) {
            Ok(()) => {}
            Err(e) => self.toast = Some(format!("exec failed: {e}")),
        }
    } else {
        self.toast = Some("nothing selected".into());
    }
    LoopState::Continue
}
```

- [ ] **Step 3: Test + commit**

```bash
cd app && cargo test -p cruster-tui
git add app/crates/cruster-tui
git commit -m "feat(tui): add exec action (s suspends TUI and runs kubectl exec)"
```

---

## Task 16: Port-forward action verb

**Files:**
- Create: `app/crates/cruster-tui/src/actions/port_forward.rs`
- Modify: `app/crates/cruster-tui/src/actions/mod.rs`
- Modify: `app/crates/cruster-tui/src/app.rs`

`f` prompts for a port mapping (`8080:80`) and starts a background
port-forward. A panel shows active forwards; `Shift+F` lists them;
`Shift+X` cancels the selected one.

For v1, shell out to `kubectl port-forward` (same rationale as exec).

- [ ] **Step 1: Write the action**

Create `app/crates/cruster-tui/src/actions/port_forward.rs`:

```rust
//! Port-forward action: spawns `kubectl port-forward` as a background
//! child process and tracks it for later cancellation.

use std::process::{Child, Command, Stdio};

use cruster_core::ResourceKey;

#[derive(Debug)]
pub struct PortForward {
    pub key: ResourceKey,
    pub mapping: String, // "8080:80"
    child: Child,
}

impl PortForward {
    pub fn start(key: ResourceKey, mapping: String) -> anyhow::Result<Self> {
        let ns = key
            .namespace
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("port-forward requires a namespaced resource"))?;
        let child = Command::new("kubectl")
            .args([
                "port-forward",
                "-n",
                ns,
                &format!("pod/{}", key.name),
                &mapping,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Self { key, mapping, child })
    }

    pub fn cancel(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
pub struct PortForwards {
    active: Vec<PortForward>,
}

impl PortForwards {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, pf: PortForward) {
        self.active.push(pf);
    }

    pub fn active(&self) -> &[PortForward] {
        &self.active
    }

    pub fn cancel_all(&mut self) {
        for pf in self.active.drain(..) {
            pf.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_scoped_rejected() {
        let key = ResourceKey::cluster_scoped("Node", "n1");
        let r = PortForward::start(key, "8080:80".into());
        assert!(r.is_err());
    }
}
```

- [ ] **Step 2: Wire up**

In `app.rs`, add `port_forwards: PortForwards` to `App`. Handle
`f` keystroke: open a small input prompt (similar to command
mode) asking for the mapping. On Enter, call
`PortForward::start(key, mapping)` and add it to `port_forwards`.

Show a single-line toast: `forwarded :8080 → pod/nginx:80`.

In `App::drop` (manual `Drop` impl or in `run`'s cleanup),
`port_forwards.cancel_all()` so dangling child processes don't
linger.

- [ ] **Step 3: Test + commit**

```bash
cd app && cargo test -p cruster-tui
git add app/crates/cruster-tui
git commit -m "feat(tui): add port-forward action (kubectl-backed)"
```

---

## Task 17: YAML view + edit (delegates to $EDITOR)

**Files:**
- Create: `app/crates/cruster-tui/src/actions/yaml_edit.rs`
- Modify: `app/crates/cruster-tui/src/actions/mod.rs`
- Modify: `app/crates/cruster-tui/src/app.rs` (route `y` for view, `e` for edit)

`y` shows the YAML of the selected resource in a read-only pane
(the same content describe shows; alias to describe is OK for v1).
`e` opens the YAML in `$EDITOR` (falling back to `vi`), validates
it on save, and applies it back to the cluster.

- [ ] **Step 1: Write the action**

Create `app/crates/cruster-tui/src/actions/yaml_edit.rs`:

```rust
//! YAML edit action: write the selected resource to a temp file,
//! shell out to $EDITOR, validate the result on return, and apply
//! it back to the cluster via kubectl.
//!
//! Cruster does not embed an editor. See the design principle in the
//! product spec ("editing defers to $EDITOR").

use std::io;
use std::io::Write;
use std::process::Command;

use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

/// Open `yaml` in $EDITOR for editing. On save (editor exits 0),
/// validate the YAML and apply it via `kubectl apply -f -`. Returns
/// the new YAML string on success, or an error.
pub fn edit_and_apply(yaml: &str) -> anyhow::Result<String> {
    // Write to temp file.
    let mut path = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    path.push(format!("cruster-edit-{pid}-{nanos}.yaml"));
    {
        let mut f = std::fs::File::create(&path)?;
        f.write_all(yaml.as_bytes())?;
    }

    // Suspend the TUI.
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;

    // Run $VISUAL → $EDITOR → vi.
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    let status = Command::new(&editor).arg(&path).status();

    // Restore the TUI before doing anything else (even on error).
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;

    let status = status?;
    if !status.success() {
        let _ = std::fs::remove_file(&path);
        anyhow::bail!("editor exited with status {status}; not applying");
    }

    // Read it back.
    let new_yaml = std::fs::read_to_string(&path)?;
    let _ = std::fs::remove_file(&path);

    // Validate as YAML.
    let _: serde_yaml::Value = serde_yaml::from_str(&new_yaml)?;

    // Apply via kubectl.
    let mut kc = Command::new("kubectl")
        .args(["apply", "-f", "-"])
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    kc.stdin
        .as_mut()
        .expect("piped stdin")
        .write_all(new_yaml.as_bytes())?;
    let status = kc.wait()?;
    if !status.success() {
        anyhow::bail!("kubectl apply exited with status {status}");
    }

    Ok(new_yaml)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_yaml_returns_error_after_editor_run() {
        // We can't easily test the editor path without mocking std::process,
        // but we can test the YAML validation function in isolation.
        let bad: Result<serde_yaml::Value, _> = serde_yaml::from_str("this: is: not: yaml");
        assert!(bad.is_err());
    }
}
```

- [ ] **Step 2: Wire up**

In `app.rs`:

```rust
KeyCode::Char('y') => {
    if let Some((title, yaml)) = self.current_view.selected_yaml() {
        self.describe_pane.open(title, yaml);
    } else {
        self.toast = Some("nothing selected".into());
    }
    LoopState::Continue
}
KeyCode::Char('e') => {
    if let Some((_, yaml)) = self.current_view.selected_yaml() {
        match crate::actions::yaml_edit::edit_and_apply(&yaml) {
            Ok(_) => self.toast = Some("applied".into()),
            Err(e) => self.toast = Some(format!("edit failed: {e}")),
        }
    } else {
        self.toast = Some("nothing selected".into());
    }
    LoopState::Continue
}
```

- [ ] **Step 3: Test + commit**

```bash
cd app && cargo test -p cruster-tui
git add app/crates/cruster-tui
git commit -m "feat(tui): add YAML view (y) and edit-with-EDITOR (e)"
```

---

## Task 18: k9s-equivalent default keymap polish

Now that all the kinds + action verbs are in, walk through the
keymap and make sure it matches what a k9s user expects.

- [ ] **Step 1: Audit the keymap**

Document the final v1 keymap in `app/README.md`:

```markdown
## Keybindings

### Navigation
| Key | Action |
|---|---|
| `j` / `↓` | move selection down |
| `k` / `↑` | move selection up |
| `g` / `Home` | top |
| `G` / `End` | bottom |
| `PgDn` / `PgUp` | page down / up |

### Switching kinds
Type `:` to enter command mode, then a kind alias:
| Alias | Kind |
|---|---|
| `po` / `pods` | Pods |
| `deploy` / `deployments` | Deployments |
| `svc` / `services` | Services |
| `no` / `nodes` | Nodes |
| `ev` / `events` | Events |
| `cm` / `configmaps` | ConfigMaps |
| `sec` / `secrets` | Secrets |
| `ns` / `namespaces` | Namespaces |

### Actions on selection
| Key | Action |
|---|---|
| `d` | describe (YAML pane) |
| `y` | view YAML (same as describe for v1) |
| `e` | edit YAML in $EDITOR + apply |
| `l` | tail logs (Pods only; follow + `/` grep) |
| `s` | exec into container (Pods only) |
| `f` | port-forward (prompts for `local:remote`) |

### Exit
| Key | Action |
|---|---|
| `q` / `Esc` (in main view) | quit |
```

- [ ] **Step 2: Validate everything works end-to-end against k3d**

Same checklist as Phase 1 Task 11 step 2 — manual verification.

- [ ] **Step 3: Commit**

```bash
git add app/README.md
git commit -m "docs(app): document v1 keymap"
```

---

## Task 19: Phase 2A exit verification

- [ ] **Step 1: Full lint + test matrix**

```bash
cd app && cargo fmt --all -- --check
cd app && cargo clippy --workspace --all-targets -- -D warnings
cd app && cargo test --workspace --all-targets
```
All three pass.

- [ ] **Step 2: Manual verification against k3d**

With `k3d-a8s-dev` active, run `cargo run --release -p cruster-bin`
and verify:

1. Pods view shows on startup (same as Phase 1).
2. `:deploy` Enter switches to deployments — all in-cluster
   deployments visible.
3. Same for `:svc`, `:no`, `:ev`, `:cm`, `:sec`, `:ns`.
4. `d` on a selected pod opens the YAML dump in the describe pane.
5. `l` on a selected pod streams logs; `/` filters; ESC closes.
6. `s` on a selected pod opens a shell; exiting the shell returns
   to the TUI cleanly.
7. `f` on a selected pod prompts for a port mapping; entering
   `8081:80` starts a forward; verify with `curl
   localhost:8081`. Quit cruster and confirm the forward dies
   (process is killed cleanly).
8. `e` on a selected pod opens $EDITOR with the pod's YAML;
   saving applies via kubectl; quitting without saving does not.
9. Unknown alias (`:wat`) shows an error toast.
10. `q` / `Esc` quits and restores terminal.

- [ ] **Step 3: Update top-level README**

Edit `README.md`:

```markdown
## Status

Phase 2A (parity-lite) complete: 8 resource kinds (Pods, Deployments,
Services, Nodes, Events, ConfigMaps, Secrets, Namespaces), describe,
logs (follow + grep), exec, port-forward, YAML view + edit via
$EDITOR. k9s-equivalent default keymap.

Next: Phase 2B (LLM-efficient CLI mode).
```

- [ ] **Step 4: Commit + tag**

```bash
git add README.md
git commit -m "docs: update README for phase 2A completion"
git tag -a phase-2a-tui-parity -m "Phase 2A: TUI parity-lite with 8 kinds and 5 action verbs"
```

---

## Phase 2A exit criteria

All of the following must hold before moving to Phase 2B:

1. `cd app && cargo test --workspace` passes.
2. `cd app && cargo clippy --workspace --all-targets -- -D warnings` clean.
3. `cd app && cargo fmt --all -- --check` clean.
4. Manual verification (Task 19 step 2) all checks pass against k3d.
5. CI passes on a push to main.

When all five hold, write the Phase 2B plan (LLM-efficient CLI:
`cruster-cli` crate, `get / describe / logs / events` verbs with
`--llm` / `--format` / `--budget` / `--full`, per-verb schema files,
agent-friendly field pruning, NDJSON output).
