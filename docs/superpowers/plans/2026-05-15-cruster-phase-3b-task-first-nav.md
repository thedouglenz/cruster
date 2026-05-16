# Cruster Phase 3B: Task-First Navigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cruster becomes task-first: from any selected resource, one
keystroke walks to related objects (owner chain, mounted configmaps
and secrets, matching services, scheduled node, recent events). A
diff verb compares two resources structurally — in the TUI on
demand, or from the CLI as `cruster diff <a> <b>`. Saved investigative
workflows (named YAML files) capture multi-step incident routines as
reusable presets.

This is sub-phase 2 of Phase 3. After this lands:
- **3C**: theme engine — TOML themes, live reload, install, paid gate.
- **3D**: keymap presets (vim/emacs/normal) + named multi-pane layouts.

**Architecture:**
- `cruster-kube` gains `relationships.rs`: pure graph computation over
  `StoreRegistry`. Given a `ResourceKey`, returns a `Vec<Related>`
  with each entry tagged by relationship type (owner, owned-by,
  selects, selected-by, mounts-cm, mounts-secret, scheduled-on, etc.).
  No I/O — reads in-memory stores only. Instant.
- `cruster-tui` gains a `relationship` overlay (palette-styled) that
  the user opens with `r` on any selection.
- `cruster-cli` gains a `diff` verb (`cruster diff pod/a pod/b`)
  using a small structural-diff helper in `cruster-core::diff`.
- `cruster-tui` gains a workflow loader (TOML files in
  `~/.config/cruster/workflows/`), exposed via `W` in the palette.

**Tech additions:**
- `serde_json::Value` diffing — write a small `diff::structural`
  helper in core; no external dep.

---

## File Structure

New files:

```
app/crates/cruster-core/src/
└── diff.rs                 # structural diff over serde_json::Value

app/crates/cruster-kube/src/
└── relationships.rs        # graph resolver: given (key, registry) → Vec<Related>

app/crates/cruster-tui/src/
├── workflows.rs            # load TOML workflow files
└── overlays/
    └── relationships.rs    # palette-styled overlay for related-resource jumps

app/crates/cruster-cli/src/verbs/
└── diff.rs                 # cruster diff <a> <b>
```

Modifications:
- `cruster-core/src/lib.rs` — re-export `diff` module
- `cruster-kube/src/lib.rs` — re-export `relationships`
- `cruster-tui/src/app.rs` — `r` key handler, `W` key handler
- `cruster-tui/src/overlays/mod.rs` — register `relationships`
- `cruster-cli/src/args.rs` — add `Diff` command
- `cruster-cli/src/verbs/mod.rs` — dispatch
- `cruster-cli/schemas/diff.schema.json`

---

## Task 1: Structural diff helper in cruster-core

**Files:**
- Create: `app/crates/cruster-core/src/diff.rs`
- Modify: `app/crates/cruster-core/src/lib.rs`

A small structural diff over `serde_json::Value`. Output is a vec of
`Change` entries: `Added(path, value)`, `Removed(path, value)`,
`Modified(path, old, new)`. Paths use JSONPath-style notation
(`metadata.labels.app`, `spec.containers[0].image`).

- [ ] **Step 1: Write diff.rs**

```rust
//! Structural diff over `serde_json::Value`.
//!
//! Two values are walked in parallel; differences are recorded as
//! `Change` entries with JSONPath-style paths.

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Change {
    Added { path: String, value: Value },
    Removed { path: String, value: Value },
    Modified { path: String, old: Value, new: Value },
}

pub fn diff(a: &Value, b: &Value) -> Vec<Change> {
    let mut out = Vec::new();
    walk("", a, b, &mut out);
    out
}

fn walk(path: &str, a: &Value, b: &Value, out: &mut Vec<Change>) {
    match (a, b) {
        (Value::Object(oa), Value::Object(ob)) => {
            let mut keys: std::collections::BTreeSet<&String> = oa.keys().collect();
            keys.extend(ob.keys());
            for k in keys {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                match (oa.get(k), ob.get(k)) {
                    (Some(va), Some(vb)) => walk(&p, va, vb, out),
                    (None, Some(vb)) => out.push(Change::Added {
                        path: p,
                        value: vb.clone(),
                    }),
                    (Some(va), None) => out.push(Change::Removed {
                        path: p,
                        value: va.clone(),
                    }),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(aa), Value::Array(ab)) => {
            let len = aa.len().max(ab.len());
            for i in 0..len {
                let p = format!("{path}[{i}]");
                match (aa.get(i), ab.get(i)) {
                    (Some(va), Some(vb)) => walk(&p, va, vb, out),
                    (None, Some(vb)) => out.push(Change::Added {
                        path: p,
                        value: vb.clone(),
                    }),
                    (Some(va), None) => out.push(Change::Removed {
                        path: p,
                        value: va.clone(),
                    }),
                    (None, None) => {}
                }
            }
        }
        (a, b) if a == b => {}
        (a, b) => out.push(Change::Modified {
            path: path.to_string(),
            old: a.clone(),
            new: b.clone(),
        }),
    }
}

/// Render a list of changes as a unified-diff-ish text block.
pub fn render_text(changes: &[Change]) -> String {
    let mut s = String::new();
    for c in changes {
        match c {
            Change::Added { path, value } => {
                s.push_str(&format!("+ {path}: {}\n", short(value)));
            }
            Change::Removed { path, value } => {
                s.push_str(&format!("- {path}: {}\n", short(value)));
            }
            Change::Modified { path, old, new } => {
                s.push_str(&format!("~ {path}: {} → {}\n", short(old), short(new)));
            }
        }
    }
    s
}

fn short(v: &Value) -> String {
    let s = serde_json::to_string(v).unwrap_or_else(|_| "<?>".into());
    if s.len() > 80 { format!("{}…", &s[..77]) } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identical_returns_empty() {
        let a = json!({"x": 1, "y": [1, 2, 3]});
        assert!(diff(&a, &a).is_empty());
    }

    #[test]
    fn added_field_detected() {
        let a = json!({"x": 1});
        let b = json!({"x": 1, "y": 2});
        let d = diff(&a, &b);
        assert_eq!(d.len(), 1);
        assert!(matches!(&d[0], Change::Added { path, .. } if path == "y"));
    }

    #[test]
    fn removed_field_detected() {
        let a = json!({"x": 1, "y": 2});
        let b = json!({"x": 1});
        let d = diff(&a, &b);
        assert!(matches!(&d[0], Change::Removed { path, .. } if path == "y"));
    }

    #[test]
    fn modified_scalar_detected() {
        let a = json!({"x": 1});
        let b = json!({"x": 2});
        let d = diff(&a, &b);
        assert!(matches!(&d[0], Change::Modified { path, .. } if path == "x"));
    }

    #[test]
    fn nested_path_dots_and_brackets() {
        let a = json!({"spec": {"containers": [{"image": "nginx:1"}]}});
        let b = json!({"spec": {"containers": [{"image": "nginx:2"}]}});
        let d = diff(&a, &b);
        assert_eq!(d.len(), 1);
        assert!(matches!(&d[0], Change::Modified { path, .. }
            if path == "spec.containers[0].image"));
    }

    #[test]
    fn render_text_includes_markers() {
        let changes = vec![
            Change::Added { path: "x".into(), value: json!(1) },
            Change::Removed { path: "y".into(), value: json!(2) },
            Change::Modified { path: "z".into(), old: json!(3), new: json!(4) },
        ];
        let s = render_text(&changes);
        assert!(s.contains("+ x: 1"));
        assert!(s.contains("- y: 2"));
        assert!(s.contains("~ z: 3 → 4"));
    }
}
```

- [ ] **Step 2: Re-export + test + commit**

Update `cruster-core/src/lib.rs` to add `pub mod diff;`.

```bash
cd app && cargo test -p cruster-core && cargo clippy --workspace --all-targets -- -D warnings
git add app/crates/cruster-core
git commit -m "feat(core): add structural diff over serde_json::Value"
```

---

## Task 2: Relationship resolver in cruster-kube

**Files:**
- Create: `app/crates/cruster-kube/src/relationships.rs`
- Modify: `app/crates/cruster-kube/src/lib.rs`

Pure function over `(ResourceKey, &StoreRegistry)`. Returns a list of
`Related` entries describing other resources connected to the input.

Relationship kinds (for v1):
- `OwnerRef`: walks ownerReferences (Pod → ReplicaSet → Deployment).
- `OwnedBy`: inverse — given a Deployment, find Pods owned by it.
- `MountsConfigMap` / `MountsSecret`: for Pods only, list referenced
  configmaps / secrets via volumes + envFrom.
- `Selects` / `SelectedBy`: for Services + Pods. Service.spec.selector
  ↔ pod.labels.
- `ScheduledOn`: Pod → Node by spec.nodeName.

- [ ] **Step 1: Write relationships.rs**

```rust
//! Pure graph computation over StoreRegistry. Given a resource key,
//! returns related resources.

use cruster_core::ResourceKey;
use k8s_openapi::api::core::v1::Pod;

use crate::StoreRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Related {
    pub kind: RelationKind,
    pub key: ResourceKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationKind {
    OwnerRef,
    OwnedBy,
    MountsConfigMap,
    MountsSecret,
    Selects,
    SelectedBy,
    ScheduledOn,
}

impl RelationKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::OwnerRef => "owner",
            Self::OwnedBy => "owned-by",
            Self::MountsConfigMap => "mounts cm",
            Self::MountsSecret => "mounts secret",
            Self::Selects => "selects",
            Self::SelectedBy => "selected by",
            Self::ScheduledOn => "scheduled on",
        }
    }
}

/// Resolve related resources for the given key. Reads only the
/// in-memory stores in `registry` — no apiserver calls.
pub async fn related(key: &ResourceKey, registry: &StoreRegistry) -> Vec<Related> {
    match key.kind.as_str() {
        "Pod" => related_for_pod(key, registry).await,
        "Deployment" => related_for_deployment(key, registry).await,
        "Service" => related_for_service(key, registry).await,
        "Node" => related_for_node(key, registry).await,
        _ => Vec::new(),
    }
}

async fn related_for_pod(key: &ResourceKey, registry: &StoreRegistry) -> Vec<Related> {
    let mut out = Vec::new();
    let pods = registry.pods.snapshot().await;
    let Some((_, pod)) = pods.iter().find(|(k, _)| k == key) else {
        return out;
    };
    let ns = pod.metadata.namespace.as_deref().unwrap_or("default");

    // Owner refs (typically a ReplicaSet).
    for owner in pod.metadata.owner_references.iter().flatten() {
        out.push(Related {
            kind: RelationKind::OwnerRef,
            key: ResourceKey::namespaced(owner.kind.clone(), ns, owner.name.clone()),
        });
    }

    // Mounted configmaps/secrets via volumes.
    if let Some(spec) = &pod.spec {
        for vol in spec.volumes.iter().flatten() {
            if let Some(cm) = &vol.config_map {
                if let Some(name) = &cm.name {
                    out.push(Related {
                        kind: RelationKind::MountsConfigMap,
                        key: ResourceKey::namespaced("ConfigMap", ns, name.clone()),
                    });
                }
            }
            if let Some(s) = &vol.secret {
                if let Some(name) = &s.secret_name {
                    out.push(Related {
                        kind: RelationKind::MountsSecret,
                        key: ResourceKey::namespaced("Secret", ns, name.clone()),
                    });
                }
            }
        }
        // envFrom on each container.
        for c in &spec.containers {
            for ef in c.env_from.iter().flatten() {
                if let Some(cm) = &ef.config_map_ref {
                    if let Some(name) = &cm.name {
                        out.push(Related {
                            kind: RelationKind::MountsConfigMap,
                            key: ResourceKey::namespaced("ConfigMap", ns, name.clone()),
                        });
                    }
                }
                if let Some(s) = &ef.secret_ref {
                    if let Some(name) = &s.name {
                        out.push(Related {
                            kind: RelationKind::MountsSecret,
                            key: ResourceKey::namespaced("Secret", ns, name.clone()),
                        });
                    }
                }
            }
        }
        // Scheduled node.
        if let Some(node) = &spec.node_name {
            out.push(Related {
                kind: RelationKind::ScheduledOn,
                key: ResourceKey::cluster_scoped("Node", node.clone()),
            });
        }
    }

    // Services that select this pod.
    let services = registry.services.snapshot().await;
    for (skey, svc) in services
        .iter()
        .filter(|(k, _)| k.namespace.as_deref() == Some(ns))
    {
        let Some(selector) = svc.spec.as_ref().and_then(|s| s.selector.as_ref()) else {
            continue;
        };
        if labels_match_selector(pod, selector) {
            out.push(Related {
                kind: RelationKind::SelectedBy,
                key: skey.clone(),
            });
        }
    }

    out
}

async fn related_for_deployment(key: &ResourceKey, registry: &StoreRegistry) -> Vec<Related> {
    let mut out = Vec::new();
    let pods = registry.pods.snapshot().await;
    // Deployments own ReplicaSets which own Pods — we don't list
    // ReplicaSets in this view (we don't ship a RS view yet), so just
    // walk to pods whose owner-of-owner is this Deployment.
    // For v1 simplicity: find pods in the same namespace whose
    // first owner kind is "ReplicaSet" and whose ReplicaSet name
    // begins with the Deployment name (a heuristic the upstream
    // controller follows).
    let ns = key.namespace.as_deref().unwrap_or("default");
    for (pkey, pod) in pods.iter() {
        if pkey.namespace.as_deref() != Some(ns) {
            continue;
        }
        for o in pod.metadata.owner_references.iter().flatten() {
            if o.kind == "ReplicaSet" && o.name.starts_with(&format!("{}-", key.name)) {
                out.push(Related {
                    kind: RelationKind::OwnedBy,
                    key: pkey.clone(),
                });
                break;
            }
        }
    }
    out
}

async fn related_for_service(key: &ResourceKey, registry: &StoreRegistry) -> Vec<Related> {
    let mut out = Vec::new();
    let services = registry.services.snapshot().await;
    let Some((_, svc)) = services.iter().find(|(k, _)| k == key) else {
        return out;
    };
    let Some(selector) = svc.spec.as_ref().and_then(|s| s.selector.as_ref()) else {
        return out;
    };
    let pods = registry.pods.snapshot().await;
    let ns = key.namespace.as_deref().unwrap_or("default");
    for (pkey, pod) in pods.iter() {
        if pkey.namespace.as_deref() != Some(ns) {
            continue;
        }
        if labels_match_selector(pod, selector) {
            out.push(Related {
                kind: RelationKind::Selects,
                key: pkey.clone(),
            });
        }
    }
    out
}

async fn related_for_node(key: &ResourceKey, registry: &StoreRegistry) -> Vec<Related> {
    let mut out = Vec::new();
    let pods = registry.pods.snapshot().await;
    for (pkey, pod) in pods.iter() {
        if pod
            .spec
            .as_ref()
            .and_then(|s| s.node_name.as_deref())
            == Some(&key.name)
        {
            out.push(Related {
                kind: RelationKind::OwnedBy,
                key: pkey.clone(),
            });
        }
    }
    out
}

fn labels_match_selector(
    pod: &Pod,
    selector: &std::collections::BTreeMap<String, String>,
) -> bool {
    let Some(labels) = pod.metadata.labels.as_ref() else {
        return false;
    };
    selector
        .iter()
        .all(|(k, v)| labels.get(k).map(|lv| lv == v).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        ConfigMapEnvSource, Container, EnvFromSource, Pod, PodSpec, SecretVolumeSource, Service,
        ServiceSpec, Volume,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
    use std::collections::BTreeMap;

    fn pod_with_labels(name: &str, ns: &str, labels: &[(&str, &str)]) -> Pod {
        let map: BTreeMap<String, String> = labels.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Pod {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some(ns.into()),
                labels: Some(map),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn service_selects_matching_pod() {
        let r = StoreRegistry::new();
        r.pods
            .upsert(
                ResourceKey::namespaced("Pod", "default", "web-1"),
                pod_with_labels("web-1", "default", &[("app", "web")]),
            )
            .await;
        let svc_key = ResourceKey::namespaced("Service", "default", "web");
        let mut selector = BTreeMap::new();
        selector.insert("app".into(), "web".into());
        r.services
            .upsert(
                svc_key.clone(),
                Service {
                    metadata: ObjectMeta {
                        name: Some("web".into()),
                        namespace: Some("default".into()),
                        ..Default::default()
                    },
                    spec: Some(ServiceSpec {
                        selector: Some(selector),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .await;
        let rels = related(&svc_key, &r).await;
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].kind, RelationKind::Selects);
        assert_eq!(rels[0].key.name, "web-1");
    }

    #[tokio::test]
    async fn pod_owner_ref_resolves_to_replicaset() {
        let r = StoreRegistry::new();
        let pkey = ResourceKey::namespaced("Pod", "default", "web-1");
        let mut pod = pod_with_labels("web-1", "default", &[]);
        pod.metadata.owner_references = Some(vec![OwnerReference {
            kind: "ReplicaSet".into(),
            name: "web-abc".into(),
            api_version: "apps/v1".into(),
            uid: "xxx".into(),
            ..Default::default()
        }]);
        r.pods.upsert(pkey.clone(), pod).await;
        let rels = related(&pkey, &r).await;
        assert!(rels
            .iter()
            .any(|x| x.kind == RelationKind::OwnerRef && x.key.name == "web-abc"));
    }
}
```

- [ ] **Step 2: Re-export + test + commit**

Update `cruster-kube/src/lib.rs` to add `pub mod relationships;` and
re-export `Related`, `RelationKind`, `related`.

```bash
cd app && cargo test -p cruster-kube && cargo clippy --workspace --all-targets -- -D warnings
git add app/crates/cruster-kube
git commit -m "feat(kube): add relationship resolver over StoreRegistry"
```

---

## Task 3: Relationships overlay + `r` key

**Files:**
- Create: `app/crates/cruster-tui/src/overlays/relationships.rs`
- Modify: `app/crates/cruster-tui/src/overlays/mod.rs`
- Modify: `app/crates/cruster-tui/src/app.rs`

`r` on a selection: resolve related, show in a palette-style overlay
with each row labelled "[owner] Deployment/web" / "[mounts cm]
ConfigMap/my-config", etc. Enter on a row switches to that resource's
view + sets the selection.

- [ ] **Step 1: Overlay impl**

```rust
//! Relationships overlay: pick a related resource to jump to.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_kube::relationships::Related;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::overlay::{Overlay, OverlayResult};

pub struct RelationshipsOverlay {
    related: Vec<Related>,
    selected: usize,
    title: String,
}

impl RelationshipsOverlay {
    pub fn new(title: impl Into<String>, related: Vec<Related>) -> Self {
        Self {
            related,
            selected: 0,
            title: title.into(),
        }
    }
}

impl Overlay for RelationshipsOverlay {
    fn handle_key(&mut self, key: KeyEvent) -> OverlayResult {
        if key.kind != KeyEventKind::Press {
            return OverlayResult::KeepOpen;
        }
        match key.code {
            KeyCode::Esc => OverlayResult::Close,
            KeyCode::Enter => {
                let Some(r) = self.related.get(self.selected) else {
                    return OverlayResult::Close;
                };
                // Map kind to view id (plural, lowercased).
                let view_id = match r.key.kind.as_str() {
                    "Pod" => "pods",
                    "Deployment" => "deployments",
                    "Service" => "services",
                    "Node" => "nodes",
                    "Event" => "events",
                    "ConfigMap" => "configmaps",
                    "Secret" => "secrets",
                    "Namespace" => "namespaces",
                    "ReplicaSet" => "deployments", // collapse to deployments for v1
                    _ => return OverlayResult::Close,
                };
                OverlayResult::SwitchView(view_id.into())
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                OverlayResult::KeepOpen
            }
            KeyCode::Down => {
                self.selected = self.selected.saturating_add(1).min(self.related.len().saturating_sub(1));
                OverlayResult::KeepOpen
            }
            _ => OverlayResult::KeepOpen,
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let w = area.width.saturating_sub(20).min(80);
        let h = area.height.saturating_sub(6).min(20);
        if w < 20 || h < 6 {
            return;
        }
        let x = area.x + (area.width - w) / 2;
        let y = area.y + (area.height - h) / 2;
        let rect = Rect { x, y, width: w, height: h };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(rect);
        let header = Paragraph::new(format!("related to {} ({})", self.title, self.related.len()))
            .block(Block::default().borders(Borders::ALL).title(" Relationships "));
        frame.render_widget(header, chunks[0]);
        let items: Vec<ListItem> = self
            .related
            .iter()
            .map(|r| ListItem::new(format!("[{:12}]  {}/{}", r.kind.label(), r.key.kind, r.key.name)))
            .collect();
        let mut state = ListState::default();
        if !items.is_empty() {
            state.select(Some(self.selected.min(items.len() - 1)));
        }
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD));
        frame.render_stateful_widget(list, chunks[1], &mut state);
    }
}
```

- [ ] **Step 2: Wire `r` into App**

Add `r` handler:

```rust
KeyCode::Char('r') => {
    self.open_relationships().await;  // or schedule via Action enum
    LoopState::Continue
}
```

`open_relationships` needs async (because `related` is async). But
`handle_key` is sync. Two options:
a) Make `related` sync (it currently takes `&StoreRegistry` and calls
   `.snapshot().await` — could be reworked to non-async snapshots).
b) Have `handle_key` queue an "open relationships" effect that the
   run loop processes asynchronously.

Simpler: change `ResourceStore::snapshot` to expose a sync variant
that uses `try_read()`. If the read lock is contended (it shouldn't
be — writes only happen from the watcher tasks and are brief),
fall back to an empty list and log. Given watch updates are tiny
this is fine.

OR: pre-resolve relationships in the App's run loop *before* the
key handler runs. Store `Option<Vec<Related>>` on the App. The
overlay just reads that.

Simplest of all: make `related` use a non-async API. The store
already exposes `snapshot()` — we'd need to add `snapshot_blocking()`
or have related() take a pre-snapshotted view. Cleanest: change the
signature to `pub fn related(key, snapshots: &Snapshots)` where
`Snapshots` is a struct holding pre-fetched Vec snapshots for each
needed kind. App fetches them in the async run loop before invoking
the key handler.

For Phase 3B v1, go with: the App's run loop, immediately before
calling `handle_key`, takes a pre-snapshot of all stores into a
`Snapshots` struct passed to handlers via the `App` itself
(`self.last_snapshots`). The handler reads from there synchronously.

Implementation detail — flesh this out during execution.

- [ ] **Step 3: Test + commit**

```bash
cd app && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
git add app
git commit -m "feat(tui): add relationships overlay (r jumps to related resource)"
```

---

## Task 4: `cruster diff` CLI verb

**Files:**
- Create: `app/crates/cruster-cli/src/verbs/diff.rs`
- Modify: `app/crates/cruster-cli/src/args.rs` (add Diff command)
- Modify: `app/crates/cruster-cli/src/verbs/mod.rs`
- Create: `app/crates/cruster-cli/schemas/diff.schema.json`

`cruster diff <a> <b>` where a/b are `kind/name` references (each with
optional `-n ns`). Fetches both objects, applies pruning + secret
redaction, runs `core::diff::diff`. Output:
- text: `core::diff::render_text(changes)` — human-readable
- ndjson / json / yaml: serialised `Vec<Change>`

- [ ] **Step 1: Args**

Add to `args.rs`:

```rust
#[derive(Debug, Parser)]
pub struct DiffArgs {
    /// First reference: `kind/name`
    pub a: String,
    /// Second reference: `kind/name`
    pub b: String,
    #[arg(long)]
    pub a_namespace: Option<String>,
    #[arg(long)]
    pub b_namespace: Option<String>,
    /// Convenience: same namespace for both refs.
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
}
```

And add to `Command`:
```rust
/// Structural diff between two resources.
Diff(DiffArgs),
```

- [ ] **Step 2: Verb impl**

```rust
//! `cruster diff <a> <b>` — structural diff between two resources.

use cruster_core::diff;
use kube::Client;

use crate::args::{Cli, DiffArgs, Format};
use crate::format::write_records;
use crate::output::{effective_format, stdout_is_tty};

pub async fn run(cli: &Cli, args: &DiffArgs) -> anyhow::Result<()> {
    let client = Client::try_default().await?;
    let a = fetch_pruned(&client, &args.a, args.a_namespace.as_deref().or(args.namespace.as_deref()), cli.full).await?;
    let b = fetch_pruned(&client, &args.b, args.b_namespace.as_deref().or(args.namespace.as_deref()), cli.full).await?;
    let changes = diff::diff(&a, &b);

    let format = effective_format(cli.format, cli.llm, stdout_is_tty());
    let mut stdout = std::io::stdout().lock();
    write_records(&mut stdout, format, &changes, |w, _| {
        use std::io::Write;
        w.write_all(diff::render_text(&changes).as_bytes())
    })?;
    Ok(())
}

async fn fetch_pruned(
    client: &Client,
    reference: &str,
    ns: Option<&str>,
    full: bool,
) -> anyhow::Result<serde_json::Value> {
    let (kind, name) = crate::verbs::describe::parse_reference(reference)?;
    let canonical = crate::verbs::get::canonicalise_kind(kind)
        .ok_or_else(|| anyhow::anyhow!("unknown kind: {kind}"))?;
    // For v1 only Pod / Deployment / Service / ConfigMap / Secret are
    // diff-able via this path; others bail. Pattern can extend later.
    let val = match canonical {
        "pods" => fetch::<k8s_openapi::api::core::v1::Pod>(client, name, ns).await?,
        "deployments" => fetch::<k8s_openapi::api::apps::v1::Deployment>(client, name, ns).await?,
        "services" => fetch::<k8s_openapi::api::core::v1::Service>(client, name, ns).await?,
        "configmaps" => fetch::<k8s_openapi::api::core::v1::ConfigMap>(client, name, ns).await?,
        "secrets" => fetch_secret(client, name, ns).await?,
        other => anyhow::bail!("diff not supported for kind: {other}"),
    };
    let mut v = serde_json::to_value(val)?;
    crate::prune::prune(&mut v, full);
    if canonical == "secrets" {
        crate::prune::redact_secret(&mut v);
    }
    Ok(v)
}

async fn fetch<T>(client: &Client, name: &str, ns: Option<&str>) -> anyhow::Result<serde_json::Value>
where
    T: kube::Resource<DynamicType = (), Scope = kube::core::NamespaceResourceScope>
        + Clone
        + serde::Serialize
        + serde::de::DeserializeOwned
        + std::fmt::Debug
        + Send
        + Sync
        + 'static,
{
    let ns = ns.ok_or_else(|| anyhow::anyhow!("-n / --namespace is required for namespaced resources"))?;
    let api: kube::Api<T> = kube::Api::namespaced(client.clone(), ns);
    let obj = api.get(name).await?;
    Ok(serde_json::to_value(obj)?)
}

async fn fetch_secret(client: &Client, name: &str, ns: Option<&str>) -> anyhow::Result<serde_json::Value> {
    let ns = ns.ok_or_else(|| anyhow::anyhow!("-n / --namespace is required for secrets"))?;
    let api: kube::Api<k8s_openapi::api::core::v1::Secret> = kube::Api::namespaced(client.clone(), ns);
    let obj = api.get(name).await?;
    Ok(serde_json::to_value(obj)?)
}
```

- [ ] **Step 3: Wire dispatch + schema + commit**

Update `verbs/mod.rs` to dispatch `Command::Diff`. Schema:

```json
{
  "$schema": "https://json-schema.org/draft-07/schema",
  "$id": "https://cruster.dev/schemas/diff/v1.json",
  "title": "cruster diff (NDJSON record)",
  "type": "object",
  "required": ["kind"],
  "properties": {
    "kind": { "enum": ["added", "removed", "modified"] },
    "path": { "type": "string" },
    "value": {},
    "old": {},
    "new": {}
  }
}
```

Add the schema to `schemas.rs`'s SCHEMAS list.

```bash
cd app && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
git add app
git commit -m "feat(cli): add cruster diff <a> <b> structural diff verb"
```

---

## Task 5: Saved workflows loader

**Files:**
- Create: `app/crates/cruster-tui/src/workflows.rs`
- Modify: `app/crates/cruster-tui/src/app.rs`

A workflow is a TOML file with name + description + actions to apply
in sequence. v1 actions: `switch_view(id)`, `set_filter(query)`.

Example `~/.config/cruster/workflows/why-rollout-stuck.toml`:
```toml
name = "Why is this rollout stuck?"
description = "Jump to deployments and filter to those not fully available"

steps = [
  { switch_view = "deployments" },
  { set_filter = "status:Available" },
]
```

`W` opens a palette of available workflows. Picking one runs the
steps sequentially.

- [ ] **Step 1: Loader**

```rust
//! Saved investigative workflows loaded from
//! ~/.config/cruster/workflows/*.toml.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Workflow {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize)]
pub enum Step {
    #[serde(rename = "switch_view")]
    SwitchView(String),
    #[serde(rename = "set_filter")]
    SetFilter(String),
}

pub fn load_all() -> Vec<Workflow> {
    let Some(dir) = workflows_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut workflows = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        if let Ok(body) = std::fs::read_to_string(&path) {
            if let Ok(w) = toml::from_str::<Workflow>(&body) {
                workflows.push(w);
            }
        }
    }
    workflows
}

fn workflows_dir() -> Option<PathBuf> {
    let mut p = dirs::config_dir()?;
    p.push("cruster");
    p.push("workflows");
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_workflow() {
        let body = r#"
            name = "Stuck rollouts"
            description = "Find deployments not fully available"
            steps = [
              { switch_view = "deployments" },
              { set_filter = "status:Available" },
            ]
        "#;
        let w: Workflow = toml::from_str(body).unwrap();
        assert_eq!(w.name, "Stuck rollouts");
        assert_eq!(w.steps.len(), 2);
    }
}
```

- [ ] **Step 2: Wire `W` key + workflow palette**

`W` opens a palette built from `load_all()`. Picking a workflow runs
its steps via existing handlers: `switch_view` reuses
`Self::view_for_id`; `set_filter` calls
`self.current_view.set_filter(Filter::parse(&q))`.

The workflow palette overlay can reuse `Palette` with a different
result variant. For v1, hack: store the loaded workflows on App,
the palette returns `SwitchView("__wf__<index>")` and the App
decodes the index. Slightly ugly but contained.

(A cleaner OverlayResult variant could be added in 3D when more
overlays are needed.)

- [ ] **Step 3: Test + commit**

```bash
cd app && cargo test --workspace
git add app
git commit -m "feat(tui): add saved workflows (W key, ~/.config/cruster/workflows)"
```

---

## Task 6: Phase 3B exit verification

- [ ] **Step 1: fmt + clippy + test**

```bash
cd app && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```
All clean.

- [ ] **Step 2: Manual k3d verification**

1. `r` on a Pod → list of related resources (owner ReplicaSet,
   mounted configmaps/secrets if any, services that select it, the
   node). Enter on one switches the view.
2. `cruster diff pod/<a> pod/<b> -n <ns>` outputs a sensible diff.
3. Create `~/.config/cruster/workflows/test.toml`:
   ```toml
   name = "Test"
   steps = [{ switch_view = "deployments" }]
   ```
   `W` shows it; Enter switches to deployments.

- [ ] **Step 3: README updates + tag**

Update `app/README.md` keymap: add `r`, `W`. Update top-level
`README.md` status. Tag `phase-3b-task-first-nav`.

---

## Phase 3B exit criteria

1. `cargo test --workspace` passes.
2. clippy/fmt clean.
3. Manual k3d verification.
4. CI green.

When all hold, write Phase 3C (themes).
