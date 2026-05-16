# Cruster Phase 3A: Immediate Ergonomics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cruster gains the always-on ergonomic affordances that
make the TUI feel modern: a global command palette, faceted search,
an inline action footer that surfaces what keys do on the current
selection, safety badges + a read-only mode keyed off context
matchers, "copy as kubectl" on any selection, and ranked history /
recents for fast navigation.

This is the first of four Phase 3 sub-plans. Subsequent sub-phases:
- **3B**: task-first navigation — relationship-first jumps, diff verb
  (TUI + CLI), saved investigative workflows.
- **3C**: theme engine — TOML themes, live reload, install, paid gate.
- **3D**: keymap presets (vim/emacs/normal) + named multi-pane layouts.

**Architecture:** Two foundational additions inside cruster-tui:
- A central `ActionRegistry` of named actions, each with a key chord,
  a "applies to" predicate, and a handler. The current ad-hoc
  match in `App::handle_key` becomes registry lookup. The palette,
  the action footer, and the discoverability footer all read from
  the same registry — adding a new action means registering once.
- An `OverlayStack` that stacks modal overlays (command palette,
  faceted search, port-forward prompt — all share a common pattern).
  Each overlay is a small state machine with `handle_key` and
  `render`. The App routes top-of-stack key events to the overlay.

Config additions in `~/.config/cruster/`:
- `safety.toml` — context-name patterns mapped to environments
  (`prod`, `staging`, `dev`, `local`). Used by the safety badge
  + read-only matcher. Defaults shipped if file missing.

**Tech additions:**
- `nucleo-matcher` 0.3 — Helix-style fuzzy matcher for palette + search
- `toml` 0.8 — read safety.toml
- `dirs` 5 — locate `~/.config/cruster`

---

## File Structure

New files in this phase:

```
app/crates/cruster-tui/src/
├── action.rs               # Action trait + ActionRegistry
├── overlay.rs              # Overlay trait + OverlayStack
├── safety.rs               # Environment matcher + read-only gating
├── kubectl.rs              # action → kubectl-equivalent string translator
├── history.rs              # ranked recents (recency × frequency)
├── overlays/
│   ├── mod.rs
│   ├── palette.rs          # command palette (Ctrl+P)
│   └── search.rs           # faceted-search filter (/)
└── ...                     # existing files

app/crates/cruster-core/src/
└── lib.rs                  # MODIFIED: add Environment enum (Prod/Staging/Dev/Local)
```

Files modified:
- `app/crates/cruster-tui/Cargo.toml` — add nucleo-matcher, toml, dirs
- `app/Cargo.toml` — workspace deps for the same
- `app/crates/cruster-tui/src/app.rs` — replace ad-hoc key handling with registry + overlay stack; render safety badge + action footer
- `app/crates/cruster-tui/src/view.rs` — add `selected_summary()` for the action footer
- Per-view files (8 of them) — minor: implement `selected_summary`

---

## Task 1: Environment enum in cruster-core

**Files:**
- Modify: `app/crates/cruster-core/src/lib.rs`

Tiny addition — the `Environment` enum is shared between
`cruster-tui` (badge rendering) and `cruster-cli` (future
`--env-check` flag for write verbs), so it lives in core.

- [ ] **Step 1: Add the enum**

Append to `app/crates/cruster-core/src/lib.rs`:

```rust
/// Cluster environment classification, used for safety badging and
/// read-only-by-default gating. Determined by user-configurable
/// matchers in `~/.config/cruster/safety.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Prod,
    Staging,
    Dev,
    Local,
    Unknown,
}

impl Environment {
    /// Whether destructive actions are gated by default for this env.
    pub fn requires_confirmation(self) -> bool {
        matches!(self, Self::Prod | Self::Staging)
    }
}

impl std::fmt::Display for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Prod => "prod",
            Self::Staging => "staging",
            Self::Dev => "dev",
            Self::Local => "local",
            Self::Unknown => "unknown",
        };
        f.write_str(s)
    }
}
```

Add tests in the existing `tests` module:

```rust
#[test]
fn prod_requires_confirmation() {
    assert!(Environment::Prod.requires_confirmation());
    assert!(Environment::Staging.requires_confirmation());
    assert!(!Environment::Dev.requires_confirmation());
    assert!(!Environment::Local.requires_confirmation());
    assert!(!Environment::Unknown.requires_confirmation());
}

#[test]
fn environment_roundtrips_through_json() {
    for env in [
        Environment::Prod,
        Environment::Staging,
        Environment::Dev,
        Environment::Local,
        Environment::Unknown,
    ] {
        let s = serde_json::to_string(&env).unwrap();
        let back: Environment = serde_json::from_str(&s).unwrap();
        assert_eq!(env, back);
    }
}
```

- [ ] **Step 2: Test + commit**

```bash
cd app && cargo test -p cruster-core
git add app/crates/cruster-core
git commit -m "feat(core): add Environment enum for safety gating"
```

---

## Task 2: Action registry

**Files:**
- Create: `app/crates/cruster-tui/src/action.rs`
- Modify: `app/crates/cruster-tui/src/lib.rs`
- Modify: `app/crates/cruster-tui/src/view.rs` (add `selected_summary`)

The Action trait + registry. Each Action knows:
- `id()` — stable string (`describe`, `logs`, `exec`, `port-forward`, `edit`, `quit`, …)
- `label()` — human-readable name
- `description()` — one-line description for palette + footer
- `key_chord()` — primary keybinding
- `is_applicable(view)` — boolean: does this action make sense on the
  current view's selection? Used to filter the footer.
- `is_destructive()` — whether it modifies cluster state
- `kubectl_equivalent(view)` — string form of equivalent kubectl
  command (for "copy as kubectl"). Returns `None` if not applicable.

For Phase 3A, actions don't execute themselves (the App still owns
execution and overlay-show side effects). The registry is read-only
metadata that the palette + footer + kubectl translator consume.
Execution wiring moves into the registry in Phase 3D.

- [ ] **Step 1: Write the trait + registry**

Create `app/crates/cruster-tui/src/action.rs`:

```rust
//! Action registry: metadata for every action a user can take in the
//! TUI. Used by the command palette, the inline action footer, and
//! the "copy as kubectl" feature.

use crossterm::event::KeyCode;

use crate::view::ResourceView;

/// Single TUI action: a thing the user can do.
pub trait Action: Send + Sync {
    /// Stable identifier (`describe`, `logs`, `exec`, …).
    fn id(&self) -> &'static str;

    /// Human label for palettes/footer.
    fn label(&self) -> &'static str;

    /// One-line description.
    fn description(&self) -> &'static str;

    /// Primary keybinding.
    fn key(&self) -> KeyCode;

    /// Whether this action makes sense for the current view's
    /// selection. Default: always applicable.
    fn is_applicable(&self, _view: &dyn ResourceView) -> bool {
        true
    }

    /// Whether this action modifies cluster state.
    fn is_destructive(&self) -> bool {
        false
    }

    /// The equivalent kubectl command, if expressible. Used by
    /// the "copy as kubectl" feature.
    fn kubectl_equivalent(&self, view: &dyn ResourceView) -> Option<String> {
        let _ = view;
        None
    }
}

/// Container of all known actions. Built once at App startup.
#[derive(Default)]
pub struct ActionRegistry {
    actions: Vec<Box<dyn Action>>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, action: Box<dyn Action>) {
        self.actions.push(action);
    }

    pub fn all(&self) -> &[Box<dyn Action>] {
        &self.actions
    }

    /// Actions applicable to the current view's selection. Used by
    /// the inline action footer.
    pub fn applicable<'a>(
        &'a self,
        view: &'a dyn ResourceView,
    ) -> impl Iterator<Item = &'a dyn Action> + 'a {
        self.actions
            .iter()
            .map(|a| a.as_ref())
            .filter(|a| a.is_applicable(view))
    }

    /// Look up an action by id.
    pub fn by_id(&self, id: &str) -> Option<&dyn Action> {
        self.actions.iter().find(|a| a.id() == id).map(|a| a.as_ref())
    }

    /// Look up an action by key. Returns the first match.
    pub fn by_key(&self, key: KeyCode) -> Option<&dyn Action> {
        self.actions.iter().find(|a| a.key() == key).map(|a| a.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use cruster_core::ResourceKey;
    use cruster_kube::StoreRegistry;
    use ratatui::Frame;

    struct DummyView;

    #[async_trait]
    impl ResourceView for DummyView {
        fn id(&self) -> &'static str {
            "dummy"
        }
        async fn refresh(&mut self, _registry: &StoreRegistry) {}
        fn render(&self, _frame: &mut Frame<'_>) {}
        fn handle_key(&mut self, _key: crossterm::event::KeyEvent) -> crate::app::LoopState {
            crate::app::LoopState::Continue
        }
        fn selected_key(&self) -> Option<ResourceKey> {
            Some(ResourceKey::namespaced("Pod", "default", "x"))
        }
    }

    struct DescribeAction;
    impl Action for DescribeAction {
        fn id(&self) -> &'static str {
            "describe"
        }
        fn label(&self) -> &'static str {
            "Describe"
        }
        fn description(&self) -> &'static str {
            "Open YAML pane on the selected resource"
        }
        fn key(&self) -> KeyCode {
            KeyCode::Char('d')
        }
        fn kubectl_equivalent(&self, view: &dyn ResourceView) -> Option<String> {
            let key = view.selected_key()?;
            let ns = key.namespace?;
            Some(format!("kubectl describe {} {} -n {}", key.kind.to_lowercase(), key.name, ns))
        }
    }

    #[test]
    fn registry_round_trip() {
        let mut r = ActionRegistry::new();
        r.register(Box::new(DescribeAction));
        assert!(r.by_id("describe").is_some());
        assert!(r.by_id("nope").is_none());
        assert!(r.by_key(KeyCode::Char('d')).is_some());
        assert!(r.by_key(KeyCode::Char('z')).is_none());
    }

    #[test]
    fn applicable_returns_all_by_default() {
        let mut r = ActionRegistry::new();
        r.register(Box::new(DescribeAction));
        let view = DummyView;
        assert_eq!(r.applicable(&view).count(), 1);
    }

    #[test]
    fn kubectl_equivalent_formats_with_selection() {
        let action = DescribeAction;
        let view = DummyView;
        assert_eq!(
            action.kubectl_equivalent(&view),
            Some("kubectl describe pod x -n default".into())
        );
    }
}
```

- [ ] **Step 2: Re-export from lib**

Modify `app/crates/cruster-tui/src/lib.rs`:

```rust
pub mod action;
pub mod actions;
pub mod app;
pub mod command;
pub mod view;
pub mod views;

pub use action::{Action, ActionRegistry};
pub use app::App;
pub use view::ResourceView;
```

- [ ] **Step 3: Test + commit**

```bash
cd app && cargo test -p cruster-tui
git add app/crates/cruster-tui
git commit -m "feat(tui): add Action trait and ActionRegistry"
```

---

## Task 3: Wire shipped actions into the registry

**Files:**
- Create: `app/crates/cruster-tui/src/action/registry.rs` (the concrete shipped actions)
- Modify: `app/crates/cruster-tui/src/action.rs` (sub-module)
- Modify: `app/crates/cruster-tui/src/app.rs` (register, expose, route by id)

For each existing action (`describe`, `logs`, `exec`, `port-forward`,
`edit`, `switch-kind`, `quit`), implement the `Action` trait. The
App still dispatches execution by id, but the metadata flows
through the registry.

- [ ] **Step 1: Shipped action implementations**

Create `app/crates/cruster-tui/src/action_shipped.rs`:

```rust
//! Shipped actions, registered with the ActionRegistry at App startup.

use crossterm::event::KeyCode;
use cruster_core::ResourceKey;

use crate::action::Action;
use crate::view::ResourceView;

pub struct Describe;
impl Action for Describe {
    fn id(&self) -> &'static str {
        "describe"
    }
    fn label(&self) -> &'static str {
        "Describe"
    }
    fn description(&self) -> &'static str {
        "Open YAML pane on the selected resource"
    }
    fn key(&self) -> KeyCode {
        KeyCode::Char('d')
    }
    fn is_applicable(&self, view: &dyn ResourceView) -> bool {
        view.selected_key().is_some()
    }
    fn kubectl_equivalent(&self, view: &dyn ResourceView) -> Option<String> {
        kubectl_for_kind(view.selected_key()?, "describe")
    }
}

pub struct Logs;
impl Action for Logs {
    fn id(&self) -> &'static str {
        "logs"
    }
    fn label(&self) -> &'static str {
        "Logs"
    }
    fn description(&self) -> &'static str {
        "Stream container logs (with grep)"
    }
    fn key(&self) -> KeyCode {
        KeyCode::Char('l')
    }
    fn is_applicable(&self, view: &dyn ResourceView) -> bool {
        matches!(view.selected_key(), Some(k) if k.kind == "Pod")
    }
    fn kubectl_equivalent(&self, view: &dyn ResourceView) -> Option<String> {
        let key = view.selected_key()?;
        if key.kind != "Pod" {
            return None;
        }
        let ns = key.namespace?;
        Some(format!("kubectl logs -f --tail=500 {} -n {}", key.name, ns))
    }
}

pub struct Exec;
impl Action for Exec {
    fn id(&self) -> &'static str {
        "exec"
    }
    fn label(&self) -> &'static str {
        "Exec into pod"
    }
    fn description(&self) -> &'static str {
        "Open a shell in the container"
    }
    fn key(&self) -> KeyCode {
        KeyCode::Char('s')
    }
    fn is_applicable(&self, view: &dyn ResourceView) -> bool {
        view.selected_can_exec()
    }
    fn kubectl_equivalent(&self, view: &dyn ResourceView) -> Option<String> {
        let key = view.selected_key()?;
        if key.kind != "Pod" {
            return None;
        }
        let ns = key.namespace?;
        Some(format!("kubectl exec -it {} -n {} -- /bin/sh", key.name, ns))
    }
}

pub struct PortForward;
impl Action for PortForward {
    fn id(&self) -> &'static str {
        "port-forward"
    }
    fn label(&self) -> &'static str {
        "Port-forward"
    }
    fn description(&self) -> &'static str {
        "Prompt for local:remote mapping and forward"
    }
    fn key(&self) -> KeyCode {
        KeyCode::Char('f')
    }
    fn is_applicable(&self, view: &dyn ResourceView) -> bool {
        matches!(view.selected_key(), Some(k) if k.kind == "Pod")
    }
    fn kubectl_equivalent(&self, view: &dyn ResourceView) -> Option<String> {
        let key = view.selected_key()?;
        if key.kind != "Pod" {
            return None;
        }
        let ns = key.namespace?;
        Some(format!("kubectl port-forward -n {} pod/{} <LOCAL>:<REMOTE>", ns, key.name))
    }
}

pub struct Edit;
impl Action for Edit {
    fn id(&self) -> &'static str {
        "edit"
    }
    fn label(&self) -> &'static str {
        "Edit YAML"
    }
    fn description(&self) -> &'static str {
        "Open YAML in $EDITOR and kubectl apply on save"
    }
    fn key(&self) -> KeyCode {
        KeyCode::Char('e')
    }
    fn is_applicable(&self, view: &dyn ResourceView) -> bool {
        matches!(view.selected_key(), Some(k) if k.kind != "Secret")
    }
    fn is_destructive(&self) -> bool {
        true
    }
    fn kubectl_equivalent(&self, view: &dyn ResourceView) -> Option<String> {
        let key = view.selected_key()?;
        let ns = key.namespace.clone();
        let kind = key.kind.to_lowercase();
        match ns {
            Some(ns) => Some(format!("kubectl edit {} {} -n {}", kind, key.name, ns)),
            None => Some(format!("kubectl edit {} {}", kind, key.name)),
        }
    }
}

pub struct SwitchKind;
impl Action for SwitchKind {
    fn id(&self) -> &'static str {
        "switch-kind"
    }
    fn label(&self) -> &'static str {
        "Switch kind"
    }
    fn description(&self) -> &'static str {
        "Open the :command-mode kind switcher"
    }
    fn key(&self) -> KeyCode {
        KeyCode::Char(':')
    }
}

pub struct Quit;
impl Action for Quit {
    fn id(&self) -> &'static str {
        "quit"
    }
    fn label(&self) -> &'static str {
        "Quit"
    }
    fn description(&self) -> &'static str {
        "Exit cruster"
    }
    fn key(&self) -> KeyCode {
        KeyCode::Char('q')
    }
}

pub struct CopyKubectl;
impl Action for CopyKubectl {
    fn id(&self) -> &'static str {
        "copy-kubectl"
    }
    fn label(&self) -> &'static str {
        "Copy as kubectl"
    }
    fn description(&self) -> &'static str {
        "Copy the kubectl equivalent of the most relevant action"
    }
    fn key(&self) -> KeyCode {
        KeyCode::Char('K')
    }
    fn is_applicable(&self, view: &dyn ResourceView) -> bool {
        view.selected_key().is_some()
    }
}

fn kubectl_for_kind(key: ResourceKey, verb: &str) -> Option<String> {
    let kind = key.kind.to_lowercase();
    match key.namespace {
        Some(ns) => Some(format!("kubectl {} {} {} -n {}", verb, kind, key.name, ns)),
        None => Some(format!("kubectl {} {} {}", verb, kind, key.name)),
    }
}

/// Build the default registry with all shipped actions registered.
pub fn default_registry() -> crate::action::ActionRegistry {
    let mut r = crate::action::ActionRegistry::new();
    r.register(Box::new(Describe));
    r.register(Box::new(Logs));
    r.register(Box::new(Exec));
    r.register(Box::new(PortForward));
    r.register(Box::new(Edit));
    r.register(Box::new(SwitchKind));
    r.register(Box::new(Quit));
    r.register(Box::new(CopyKubectl));
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_includes_eight_shipped_actions() {
        let r = default_registry();
        assert_eq!(r.all().len(), 8);
        for id in ["describe", "logs", "exec", "port-forward", "edit", "switch-kind", "quit", "copy-kubectl"] {
            assert!(r.by_id(id).is_some(), "missing action: {id}");
        }
    }

    #[test]
    fn logs_kubectl_for_pod() {
        let action = Logs;
        let key = ResourceKey::namespaced("Pod", "default", "nginx");
        struct V(ResourceKey);
        #[async_trait::async_trait]
        impl ResourceView for V {
            fn id(&self) -> &'static str {
                "test"
            }
            async fn refresh(&mut self, _r: &cruster_kube::StoreRegistry) {}
            fn render(&self, _f: &mut ratatui::Frame<'_>) {}
            fn handle_key(&mut self, _k: crossterm::event::KeyEvent) -> crate::app::LoopState {
                crate::app::LoopState::Continue
            }
            fn selected_key(&self) -> Option<ResourceKey> {
                Some(self.0.clone())
            }
        }
        assert_eq!(
            action.kubectl_equivalent(&V(key)),
            Some("kubectl logs -f --tail=500 nginx -n default".into())
        );
    }
}
```

- [ ] **Step 2: Wire registry into App + lib re-exports**

Modify `app/crates/cruster-tui/src/lib.rs` to add `pub mod action_shipped;`.

Modify `App` to hold the registry:

```rust
pub struct App {
    registry: StoreRegistry,
    actions: crate::action::ActionRegistry,
    // ...other fields...
}

impl App {
    pub fn new(registry: StoreRegistry, client: Option<Client>) -> Self {
        Self {
            registry,
            actions: crate::action_shipped::default_registry(),
            // ...
        }
    }
}
```

Don't yet change key handling — that comes in Task 4 (footer) and
Task 5 (palette). The registry is built so the next tasks can read
from it.

- [ ] **Step 3: Test + commit**

```bash
cd app && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
git add app/crates/cruster-tui
git commit -m "feat(tui): register 8 shipped actions in default ActionRegistry"
```

---

## Task 4: Inline action discoverability footer

**Files:**
- Modify: `app/crates/cruster-tui/src/app.rs`

A persistent footer line under each view showing the actions
applicable to the current selection — each rendered as `<KEY> Label`.
When no row is selected, only `<:> Switch <q> Quit` show.

The footer reads from `self.actions.applicable(view)`. Rendering
respects available width: actions are space-separated, truncated
with `…` if they overflow.

- [ ] **Step 1: Add footer rendering**

In `app/crates/cruster-tui/src/app.rs`, add a helper:

```rust
fn render_action_footer(&self, frame: &mut Frame<'_>) {
    let area = frame.area();
    if area.height < 2 {
        return;
    }
    let mut parts: Vec<String> = self
        .actions
        .applicable(self.current_view.as_ref())
        .map(|a| format_action_hint(a.key(), a.label()))
        .collect();
    if parts.is_empty() {
        return;
    }
    let mut line = String::new();
    for p in parts.drain(..) {
        if !line.is_empty() {
            line.push_str("  ");
        }
        if line.len() + p.len() > area.width as usize {
            line.push('…');
            break;
        }
        line.push_str(&p);
    }
    let bar = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    let rect = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(1),
        width: area.width,
        height: 1,
    };
    frame.render_widget(bar, rect);
}

fn format_action_hint(key: KeyCode, label: &str) -> String {
    let k = match key {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Enter => "enter".into(),
        KeyCode::Up => "↑".into(),
        KeyCode::Down => "↓".into(),
        other => format!("{other:?}"),
    };
    format!("[{k}] {label}")
}
```

Update `render_full` to layer footer on top:

```rust
fn render_full(&self, frame: &mut Frame<'_>) {
    let area = frame.area();
    // Reserve bottom line for the action footer + overlay bar.
    let body_height = area.height.saturating_sub(2);
    let body_rect = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: body_height,
    };

    // existing body rendering — but views currently render to frame.area().
    // For 3A we accept that overlap; a proper Rect-passing rework lands in 3D.
    if self.describe_pane.is_open() || self.logs_pane.is_open() {
        // ... unchanged
    } else {
        self.current_view.render(frame);
    }

    self.render_action_footer(frame);
    self.render_overlay(frame);
}
```

(Note: this is a transitional placement — once the overlay bar
moves up to make room for the footer, the layout becomes clean.
For now footer renders on the second-to-last row; overlay on the
last row.)

- [ ] **Step 2: Adjust overlay rect to row 2-from-bottom**

In `render_overlay`, change the rect calc to:

```rust
let bottom = Rect {
    x: area.x,
    y: area.y + area.height.saturating_sub(2),
    width: area.width,
    height: 1,
};
```

And the footer renders at `height - 1`. Confirm both don't overdraw.

- [ ] **Step 3: Tests + commit**

Add an App test:

```rust
#[test]
fn footer_renders_applicable_actions() {
    let mut a = app();
    // pods view is default; with no pods, only switch-kind and quit
    // are unconditionally applicable.
    let names: Vec<String> = a
        .actions
        .applicable(a.current_view.as_ref())
        .map(|act| act.label().to_string())
        .collect();
    assert!(names.contains(&"Switch kind".into()));
    assert!(names.contains(&"Quit".into()));
}
```

```bash
cd app && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
git add app
git commit -m "feat(tui): add inline action discoverability footer"
```

---

## Task 5: Command palette overlay

**Files:**
- Create: `app/crates/cruster-tui/src/overlay.rs`
- Create: `app/crates/cruster-tui/src/overlays/mod.rs`
- Create: `app/crates/cruster-tui/src/overlays/palette.rs`
- Modify: `app/crates/cruster-tui/Cargo.toml` (add `nucleo-matcher`)
- Modify: `app/Cargo.toml`
- Modify: `app/crates/cruster-tui/src/app.rs` (open palette on Ctrl+P)

`Ctrl+P` opens a centered modal listing every action + every kind
(for switching). Typing fuzzy-filters the list. Up/Down moves
selection. Enter activates. Esc closes.

- [ ] **Step 1: Add nucleo-matcher dep**

Workspace + crate manifests:

```toml
nucleo-matcher = "0.3"
```

- [ ] **Step 2: Overlay trait**

Create `app/crates/cruster-tui/src/overlay.rs`:

```rust
//! Generic overlay machinery. Overlays are modal — when active, all
//! key events route to them until they close.

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;

/// What the App should do after an overlay key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayResult {
    /// Stay open; redraw on next frame.
    KeepOpen,
    /// Close the overlay. No follow-up action.
    Close,
    /// Close the overlay and invoke the named action by id.
    Invoke(String),
    /// Close the overlay and switch to the named view.
    SwitchView(String),
}

pub trait Overlay: Send {
    fn handle_key(&mut self, key: KeyEvent) -> OverlayResult;
    fn render(&self, frame: &mut Frame<'_>, area: Rect);
}
```

- [ ] **Step 3: Palette overlay**

Create `app/crates/cruster-tui/src/overlays/mod.rs`:

```rust
pub mod palette;
```

Create `app/crates/cruster-tui/src/overlays/palette.rs`:

```rust
//! Command palette: fuzzy-match over actions + kinds.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use nucleo_matcher::{Matcher, pattern::{CaseMatching, Normalization, Pattern}};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::overlay::{Overlay, OverlayResult};

#[derive(Debug, Clone)]
pub struct PaletteEntry {
    pub id: String,
    pub label: String,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Action,
    View,
}

pub struct Palette {
    entries: Vec<PaletteEntry>,
    query: String,
    selected: usize,
    matcher: Matcher,
}

impl Palette {
    pub fn new(entries: Vec<PaletteEntry>) -> Self {
        Self {
            entries,
            query: String::new(),
            selected: 0,
            matcher: Matcher::default(),
        }
    }

    fn filtered(&mut self) -> Vec<(&PaletteEntry, u32)> {
        if self.query.is_empty() {
            return self
                .entries
                .iter()
                .map(|e| (e, 0))
                .collect();
        }
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();
        let mut scored: Vec<(&PaletteEntry, u32)> = self
            .entries
            .iter()
            .filter_map(|e| {
                buf.clear();
                let score = pattern.score(
                    nucleo_matcher::Utf32Str::new(&e.label, &mut buf),
                    &mut self.matcher,
                )?;
                Some((e, score))
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored
    }
}

impl Overlay for Palette {
    fn handle_key(&mut self, key: KeyEvent) -> OverlayResult {
        if key.kind != KeyEventKind::Press {
            return OverlayResult::KeepOpen;
        }
        match key.code {
            KeyCode::Esc => OverlayResult::Close,
            KeyCode::Enter => {
                let filtered = self.filtered();
                let Some((entry, _)) = filtered.get(self.selected) else {
                    return OverlayResult::Close;
                };
                match entry.kind {
                    EntryKind::Action => OverlayResult::Invoke(entry.id.clone()),
                    EntryKind::View => OverlayResult::SwitchView(entry.id.clone()),
                }
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                OverlayResult::KeepOpen
            }
            KeyCode::Down => {
                self.selected = self.selected.saturating_add(1);
                OverlayResult::KeepOpen
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected = 0;
                OverlayResult::KeepOpen
            }
            KeyCode::Char(c) => {
                self.query.push(c);
                self.selected = 0;
                OverlayResult::KeepOpen
            }
            _ => OverlayResult::KeepOpen,
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        // Centered floating box.
        let w = area.width.saturating_sub(20).min(80);
        let h = area.height.saturating_sub(6).min(20);
        let x = area.x + (area.width - w) / 2;
        let y = area.y + (area.height - h) / 2;
        let rect = Rect { x, y, width: w, height: h };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(rect);

        let query = Paragraph::new(format!("> {}", self.query))
            .block(Block::default().borders(Borders::ALL).title(" Command Palette "))
            .style(Style::default());
        frame.render_widget(query, chunks[0]);

        // Build a filtered list without &mut self (re-run matcher locally).
        let items: Vec<ListItem> = self
            .entries
            .iter()
            .filter(|e| self.query.is_empty() || e.label.to_lowercase().contains(&self.query.to_lowercase()))
            .map(|e| ListItem::new(format!("{:10}  {}", entry_tag(e.kind), e.label)))
            .collect();

        let mut state = ListState::default();
        if !items.is_empty() {
            state.select(Some(self.selected.min(items.len() - 1)));
        }
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_stateful_widget(list, chunks[1], &mut state);
    }
}

fn entry_tag(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Action => "[action]",
        EntryKind::View => "[view]",
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

    fn p() -> Palette {
        Palette::new(vec![
            PaletteEntry {
                id: "describe".into(),
                label: "Describe".into(),
                kind: EntryKind::Action,
            },
            PaletteEntry {
                id: "deployments".into(),
                label: "Deployments".into(),
                kind: EntryKind::View,
            },
        ])
    }

    #[test]
    fn esc_closes() {
        let mut pal = p();
        assert_eq!(pal.handle_key(press(KeyCode::Esc)), OverlayResult::Close);
    }

    #[test]
    fn enter_invokes_action() {
        let mut pal = p();
        assert_eq!(
            pal.handle_key(press(KeyCode::Enter)),
            OverlayResult::Invoke("describe".into())
        );
    }

    #[test]
    fn down_then_enter_invokes_view_switch() {
        let mut pal = p();
        pal.handle_key(press(KeyCode::Down));
        assert_eq!(
            pal.handle_key(press(KeyCode::Enter)),
            OverlayResult::SwitchView("deployments".into())
        );
    }

    #[test]
    fn typing_filters_and_scores() {
        let mut pal = p();
        for c in "depl".chars() {
            pal.handle_key(press(KeyCode::Char(c)));
        }
        // First entry should be Deployments now.
        let filtered = pal.filtered();
        assert!(!filtered.is_empty());
        assert_eq!(filtered[0].0.label, "Deployments");
    }
}
```

- [ ] **Step 4: Wire Ctrl+P into App**

In `App`:
- Add `overlay: Option<Box<dyn Overlay>>` field
- In `handle_key`, top-level: if `key.code == Char('p')` and
  `KeyModifiers::CONTROL`, build a `Palette` from `self.actions` +
  the 8 view ids, set `self.overlay = Some(...)`.
- If `self.overlay.is_some()`, route key events to the overlay and
  process the `OverlayResult` (Close → clear overlay, Invoke →
  dispatch by action id, SwitchView → use `view_for_id`).
- In `render_full`, render the overlay last if it's open.

- [ ] **Step 5: Tests + commit**

```bash
cd app && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
git add app
git commit -m "feat(tui): add command palette (Ctrl+P) with fuzzy matching"
```

---

## Task 6: Faceted search overlay

**Files:**
- Create: `app/crates/cruster-tui/src/overlays/search.rs`
- Modify: `app/crates/cruster-tui/src/overlays/mod.rs`
- Modify: `app/crates/cruster-tui/src/view.rs` (add `set_filter`)
- Modify each view to honour the filter

`/` opens a small search prompt at the bottom of the screen. Typing
filters the current view's rows in real time. Tokens:
- `ns:prefix` — namespace prefix match
- `status:Running` — exact status match
- `name~substring` — substring match in name
- Anything else: fuzzy match against name.

Multiple tokens AND together. Empty: no filter.

- [ ] **Step 1: Filter parser**

Create `app/crates/cruster-tui/src/overlays/search.rs`:

```rust
//! Faceted search filter.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;

use crate::overlay::{Overlay, OverlayResult};

#[derive(Debug, Default, Clone)]
pub struct Filter {
    pub raw: String,
    pub tokens: Vec<FilterToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterToken {
    Namespace(String),
    Status(String),
    NameContains(String),
    NameFuzzy(String),
}

impl Filter {
    pub fn parse(raw: &str) -> Self {
        let mut tokens = Vec::new();
        for part in raw.split_whitespace() {
            if let Some((k, v)) = part.split_once(':') {
                match k {
                    "ns" | "namespace" => tokens.push(FilterToken::Namespace(v.into())),
                    "status" => tokens.push(FilterToken::Status(v.into())),
                    _ => tokens.push(FilterToken::NameFuzzy(part.into())),
                }
            } else if let Some(rest) = part.strip_prefix('~') {
                tokens.push(FilterToken::NameContains(rest.into()));
            } else if let Some(rest) = part.strip_suffix(':') {
                // dangling key — ignore
                let _ = rest;
            } else {
                tokens.push(FilterToken::NameFuzzy(part.into()));
            }
        }
        Self {
            raw: raw.into(),
            tokens,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// Test whether a `(namespace, name, status)` tuple passes.
    pub fn matches(&self, namespace: Option<&str>, name: &str, status: Option<&str>) -> bool {
        for tok in &self.tokens {
            match tok {
                FilterToken::Namespace(ns) => {
                    if !namespace.map(|n| n.starts_with(ns)).unwrap_or(false) {
                        return false;
                    }
                }
                FilterToken::Status(s) => {
                    if status.map(|st| st != s).unwrap_or(true) {
                        return false;
                    }
                }
                FilterToken::NameContains(sub) => {
                    if !name.contains(sub) {
                        return false;
                    }
                }
                FilterToken::NameFuzzy(q) => {
                    let lq = q.to_lowercase();
                    if !name.to_lowercase().contains(&lq) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

#[derive(Default)]
pub struct SearchPrompt {
    buffer: String,
}

impl SearchPrompt {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn current_filter(&self) -> Filter {
        Filter::parse(&self.buffer)
    }
}

impl Overlay for SearchPrompt {
    fn handle_key(&mut self, key: KeyEvent) -> OverlayResult {
        if key.kind != KeyEventKind::Press {
            return OverlayResult::KeepOpen;
        }
        match key.code {
            KeyCode::Esc => OverlayResult::Close,
            KeyCode::Enter => OverlayResult::Close,
            KeyCode::Backspace => {
                self.buffer.pop();
                OverlayResult::KeepOpen
            }
            KeyCode::Char(c) => {
                self.buffer.push(c);
                OverlayResult::KeepOpen
            }
            _ => OverlayResult::KeepOpen,
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let line = format!("/ {}_", self.buffer);
        let bar = Paragraph::new(line).style(Style::default().bg(Color::Blue));
        let rect = Rect {
            x: area.x,
            y: area.y + area.height.saturating_sub(3),
            width: area.width,
            height: 1,
        };
        frame.render_widget(bar, rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ns_token() {
        let f = Filter::parse("ns:prod");
        assert_eq!(f.tokens, vec![FilterToken::Namespace("prod".into())]);
    }

    #[test]
    fn parses_multiple_tokens() {
        let f = Filter::parse("ns:kube-system status:Running");
        assert_eq!(
            f.tokens,
            vec![
                FilterToken::Namespace("kube-system".into()),
                FilterToken::Status("Running".into()),
            ]
        );
    }

    #[test]
    fn fuzzy_fallback_for_bare_words() {
        let f = Filter::parse("nginx");
        assert_eq!(f.tokens, vec![FilterToken::NameFuzzy("nginx".into())]);
    }

    #[test]
    fn empty_filter_matches_everything() {
        let f = Filter::parse("");
        assert!(f.is_empty());
        assert!(f.matches(Some("any"), "any", Some("any")));
    }

    #[test]
    fn ns_filter_requires_prefix_match() {
        let f = Filter::parse("ns:kube");
        assert!(f.matches(Some("kube-system"), "foo", Some("Running")));
        assert!(!f.matches(Some("default"), "foo", Some("Running")));
    }

    #[test]
    fn status_filter_requires_exact_match() {
        let f = Filter::parse("status:Running");
        assert!(f.matches(Some("default"), "foo", Some("Running")));
        assert!(!f.matches(Some("default"), "foo", Some("Failed")));
    }

    #[test]
    fn name_contains_with_tilde() {
        let f = Filter::parse("~web");
        assert!(f.matches(None, "webserver", None));
        assert!(!f.matches(None, "database", None));
    }
}
```

- [ ] **Step 2: View trait extension**

Add to `ResourceView`:

```rust
/// Apply a search filter. Default: ignored. Per-view implementations
/// retain only rows matching the filter.
fn set_filter(&mut self, _filter: crate::overlays::search::Filter) {}
```

- [ ] **Step 3: Per-view filter wiring**

Each view stores an optional `Filter` and applies it in `refresh`:

```rust
async fn refresh(&mut self, registry: &StoreRegistry) {
    let snap = registry.pods.snapshot().await;
    self.snapshot = if self.filter.is_empty() {
        snap
    } else {
        snap.into_iter()
            .filter(|(k, p)| self.filter.matches(
                k.namespace.as_deref(),
                &k.name,
                Some(&pod_phase(p))))
            .collect()
    };
    // existing selection clamping
}
```

This applies to all 8 views. Each follows the same pattern; mirror
into deployments/services/etc. (omit `status:` token effect for
kinds that don't have a status string — the matcher just won't fire
since `Some(status)` is required).

- [ ] **Step 4: Wire `/` into App**

In App.handle_key: if `key.code == KeyCode::Char('/')` and no other
overlay open → push a `SearchPrompt`. On the prompt's `Close`,
apply `current_filter()` to `self.current_view.set_filter(...)`.

While prompt is open, also re-apply the filter live so the view
re-filters in real time as the user types.

- [ ] **Step 5: Tests + commit**

```bash
cd app && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
git add app
git commit -m "feat(tui): add / search prompt with faceted filter tokens"
```

---

## Task 7: Safety badge + read-only mode

**Files:**
- Create: `app/crates/cruster-tui/src/safety.rs`
- Modify: `app/crates/cruster-tui/Cargo.toml` (toml, dirs)
- Modify: `app/Cargo.toml`
- Modify: `app/crates/cruster-tui/src/app.rs` (render badge, gate destructive actions)

A persistent badge at the top of every view shows: cluster context
name, namespace (if scoped), and an environment-coloured band.
Environment is derived from `~/.config/cruster/safety.toml`:

```toml
[matchers]
prod = ["^prod-", "production"]
staging = ["^staging-", "stg-"]
dev = ["^dev-"]
local = ["^k3d-", "^kind-", "^minikube"]
```

Read-only mode is forced (no override) for any context that matches
`prod`. Read-only can be toggled manually with `Ctrl+R` for the
duration of the session.

- [ ] **Step 1: safety.rs**

Create `app/crates/cruster-tui/src/safety.rs`:

```rust
//! Environment classification and read-only mode.

use std::path::PathBuf;

use cruster_core::Environment;
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SafetyConfig {
    #[serde(default)]
    pub matchers: Matchers,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Matchers {
    #[serde(default)]
    pub prod: Vec<String>,
    #[serde(default)]
    pub staging: Vec<String>,
    #[serde(default)]
    pub dev: Vec<String>,
    #[serde(default)]
    pub local: Vec<String>,
}

impl SafetyConfig {
    pub fn defaults() -> Self {
        Self {
            matchers: Matchers {
                prod: vec!["^prod-".into(), "production".into()],
                staging: vec!["^staging-".into(), "stg-".into()],
                dev: vec!["^dev-".into()],
                local: vec!["^k3d-".into(), "^kind-".into(), "^minikube".into()],
            },
        }
    }

    /// Load from `~/.config/cruster/safety.toml`, falling back to
    /// defaults on any read or parse error.
    pub fn load_or_default() -> Self {
        let Some(path) = config_path() else {
            return Self::defaults();
        };
        let Ok(body) = std::fs::read_to_string(&path) else {
            return Self::defaults();
        };
        toml::from_str(&body).unwrap_or_else(|_| Self::defaults())
    }

    pub fn classify(&self, context: &str) -> Environment {
        if any_match(&self.matchers.prod, context) {
            Environment::Prod
        } else if any_match(&self.matchers.staging, context) {
            Environment::Staging
        } else if any_match(&self.matchers.dev, context) {
            Environment::Dev
        } else if any_match(&self.matchers.local, context) {
            Environment::Local
        } else {
            Environment::Unknown
        }
    }
}

fn any_match(patterns: &[String], s: &str) -> bool {
    patterns.iter().any(|p| {
        // Simple regex-lite: support "^prefix" for prefix match,
        // otherwise plain substring (case-sensitive).
        if let Some(prefix) = p.strip_prefix('^') {
            s.starts_with(prefix)
        } else {
            s.contains(p.as_str())
        }
    })
}

fn config_path() -> Option<PathBuf> {
    let mut p = dirs::config_dir()?;
    p.push("cruster");
    p.push("safety.toml");
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_classify_known_prefixes() {
        let cfg = SafetyConfig::defaults();
        assert_eq!(cfg.classify("prod-us-east"), Environment::Prod);
        assert_eq!(cfg.classify("production"), Environment::Prod);
        assert_eq!(cfg.classify("staging-eu"), Environment::Staging);
        assert_eq!(cfg.classify("dev-1"), Environment::Dev);
        assert_eq!(cfg.classify("k3d-a8s-dev"), Environment::Local);
        assert_eq!(cfg.classify("kind-foo"), Environment::Local);
        assert_eq!(cfg.classify("minikube"), Environment::Local);
        assert_eq!(cfg.classify("random-cluster"), Environment::Unknown);
    }

    #[test]
    fn matchers_can_be_overridden_by_user() {
        let body = r#"
            [matchers]
            prod = ["my-special-prod"]
        "#;
        let cfg: SafetyConfig = toml::from_str(body).unwrap();
        assert_eq!(cfg.classify("my-special-prod-1"), Environment::Prod);
        assert_eq!(cfg.classify("prod-us-east"), Environment::Unknown);
    }
}
```

- [ ] **Step 2: Wire badge into App**

- Add `context: String`, `environment: Environment`, `read_only: bool`
  to App. Resolve `environment` at startup via
  `SafetyConfig::load_or_default().classify(&context)`. Set
  `read_only = environment.requires_confirmation()`.
- The context string comes from kube's loaded kubeconfig — use
  `kube::config::Kubeconfig::read()` to read it (already a transitive
  dep). Default to "unknown" if it can't be loaded.
- In `render_full`, draw a top-of-screen badge before the view body:
  the body's first row is reserved for `[<context>] <env-band>
  read-only/rw`. The band colour follows the environment
  (`Prod=Red`, `Staging=Yellow`, `Dev=Green`, `Local=Cyan`,
  `Unknown=DarkGray`).
- In any destructive action handler (`edit`, `port-forward`, future
  `delete`), check `self.read_only`. If true: toast "read-only mode
  — toggle with Ctrl+R" and return without doing anything.
- `Ctrl+R` toggles `read_only` if and only if `environment !=
  Prod` (prod is unconditionally read-only in this session — user
  has to restart with an explicit flag to override).

- [ ] **Step 3: Tests + commit**

Add an App test that:
- New app with environment = Prod has `read_only = true`
- Ctrl+R on a Prod app does NOT toggle read-only
- Ctrl+R on a Dev app toggles read-only

```bash
cd app && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
git add app
git commit -m "feat(tui): add safety badge + env-driven read-only mode"
```

---

## Task 8: Copy as kubectl

**Files:**
- Create: `app/crates/cruster-tui/src/kubectl.rs` (small wrapper around the actions' `kubectl_equivalent`)
- Modify: `app/crates/cruster-tui/Cargo.toml` (already has arboard via logs)
- Modify: `app/crates/cruster-tui/src/app.rs`

`K` (capital) on any selection copies a useful kubectl command to
the system clipboard. The default action is "describe", but if the
selected row is a Pod, the prompt asks the user to pick from
describe/logs/exec/port-forward via a small menu. For Phase 3A, just
default to describe — the picker comes with the palette unification
in Phase 3D.

Actually simpler: `K` opens the palette filtered to actions whose
`kubectl_equivalent` returns `Some`. The user picks one; we copy
its kubectl form to clipboard.

Hmm — that needs the palette to know about the "filtered to
kubectl-supporting actions" mode. Let me simplify: `K` just copies
`describe`'s kubectl equivalent. The full picker lands in 3D when
the palette gains modes.

- [ ] **Step 1: arboard dep**

Already added in Phase 2A for prompt actions (or via logs Arc — actually we DIDN'T add arboard yet; that was Phase 4 scope). For Phase 3A, add it now:

Workspace + crate manifests:

```toml
arboard = "3"
```

- [ ] **Step 2: kubectl helper**

Create `app/crates/cruster-tui/src/kubectl.rs`:

```rust
//! Cross-platform clipboard helper for "copy as kubectl".

use arboard::Clipboard;

pub fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    let mut clip = Clipboard::new()?;
    clip.set_text(text.to_string())?;
    Ok(())
}
```

- [ ] **Step 3: K key handler in App**

```rust
KeyCode::Char('K') => {
    if let Some(view) = Some(self.current_view.as_ref()) {
        if let Some(describe) = self.actions.by_id("describe") {
            if let Some(cmd) = describe.kubectl_equivalent(view) {
                match crate::kubectl::copy_to_clipboard(&cmd) {
                    Ok(()) => self.toast = Some(format!("copied: {cmd}")),
                    Err(e) => self.toast = Some(format!("copy failed: {e}")),
                }
            } else {
                self.toast = Some("no kubectl equivalent for selection".into());
            }
        }
    }
    LoopState::Continue
}
```

- [ ] **Step 4: Commit**

```bash
cd app && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
git add app
git commit -m "feat(tui): add K key to copy describe kubectl-equivalent to clipboard"
```

---

## Task 9: History / recents

**Files:**
- Create: `app/crates/cruster-tui/src/history.rs`
- Modify: `app/crates/cruster-tui/src/app.rs`

Track every view-switch + every resource selection. Rank by
recency × frequency (atuin-style). Expose:
- `H` opens a recents overlay (palette-styled) of recently-visited
  view+resource pairs
- `Ctrl+O` backtracks one step

For Phase 3A, do recents only (overlay opens, user picks, we switch).
Backtracking lands in 3D once layouts are introduced.

- [ ] **Step 1: History store**

Create `app/crates/cruster-tui/src/history.rs`:

```rust
//! Navigation history with recency × frequency ranking.

use std::collections::HashMap;

use cruster_core::ResourceKey;

#[derive(Debug, Clone)]
pub struct VisitedItem {
    pub view_id: String,
    pub key: Option<ResourceKey>,
    pub last_visited_step: u64,
    pub visit_count: u64,
}

#[derive(Default)]
pub struct History {
    items: HashMap<String, VisitedItem>,
    step: u64,
    cap: usize,
}

impl History {
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            cap,
            ..Self::default()
        }
    }

    pub fn record(&mut self, view_id: &str, key: Option<ResourceKey>) {
        self.step += 1;
        let id = match &key {
            Some(k) => format!("{view_id}::{k}"),
            None => view_id.to_string(),
        };
        let entry = self
            .items
            .entry(id)
            .or_insert_with(|| VisitedItem {
                view_id: view_id.into(),
                key: key.clone(),
                last_visited_step: 0,
                visit_count: 0,
            });
        entry.last_visited_step = self.step;
        entry.visit_count += 1;
        self.trim();
    }

    fn trim(&mut self) {
        if self.items.len() <= self.cap.max(1) {
            return;
        }
        // Drop the oldest by last_visited_step.
        let mut entries: Vec<(String, u64)> = self
            .items
            .iter()
            .map(|(k, v)| (k.clone(), v.last_visited_step))
            .collect();
        entries.sort_by(|a, b| a.1.cmp(&b.1));
        let drop_n = self.items.len() - self.cap.max(1);
        for (k, _) in entries.into_iter().take(drop_n) {
            self.items.remove(&k);
        }
    }

    /// Score = visit_count * 100 + last_visited_step (so recency
    /// dominates but frequency rewards repeat visits).
    pub fn ranked(&self) -> Vec<&VisitedItem> {
        let mut v: Vec<&VisitedItem> = self.items.values().collect();
        v.sort_by(|a, b| {
            let sa = a.visit_count.saturating_mul(100) + a.last_visited_step;
            let sb = b.visit_count.saturating_mul(100) + b.last_visited_step;
            sb.cmp(&sa)
        });
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_increments_step_and_count() {
        let mut h = History::with_capacity(10);
        h.record("pods", None);
        h.record("pods", None);
        assert_eq!(h.items.len(), 1);
        assert_eq!(h.items.values().next().unwrap().visit_count, 2);
    }

    #[test]
    fn ranked_orders_by_recency_then_frequency() {
        let mut h = History::with_capacity(10);
        h.record("pods", None);
        h.record("services", None);
        h.record("services", None);
        let r = h.ranked();
        assert_eq!(r[0].view_id, "services");
        assert_eq!(r[1].view_id, "pods");
    }

    #[test]
    fn trim_drops_oldest_when_over_capacity() {
        let mut h = History::with_capacity(2);
        h.record("a", None);
        h.record("b", None);
        h.record("c", None);
        assert_eq!(h.items.len(), 2);
        // "a" should have been dropped (oldest).
        assert!(h.items.values().all(|v| v.view_id != "a"));
    }
}
```

- [ ] **Step 2: Wire into App**

- Add `history: History` (capacity 200) to App.
- On view switch, call `self.history.record(view.id(), None)`.
- On selection change in the current view, call
  `self.history.record(view.id(), view.selected_key())`. (For Phase
  3A: just record on view switch; per-row recording lands in 3D.)
- `H` opens a recents palette built from `self.history.ranked()`
  (reuse the Palette overlay with `Vec<PaletteEntry>` populated
  from history).

- [ ] **Step 3: Tests + commit**

```bash
cd app && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
git add app
git commit -m "feat(tui): add navigation history with H to open recents"
```

---

## Task 10: Phase 3A exit verification

- [ ] **Step 1: Full lint + test matrix**

```bash
cd app && cargo fmt --all -- --check
cd app && cargo clippy --workspace --all-targets -- -D warnings
cd app && cargo test --workspace --all-targets
```
All three clean.

- [ ] **Step 2: Manual k3d verification**

1. Launch `cargo run --release -p cruster-bin`.
2. Verify safety badge at top reads `[k3d-a8s-dev] local rw`.
3. Verify action footer at bottom shows context-aware keybinds.
4. `Ctrl+P`: palette opens with all actions + 8 views. Type `dep`,
   Enter → switches to deployments view. `Ctrl+P` → `desc` Enter →
   describe pane opens on selected row.
5. `/`: search prompt opens. Type `ns:kube-system status:Running`,
   Enter → only matching pods visible.
6. `K` on a selected pod: clipboard contains
   `kubectl describe pod <name> -n <ns>`. Paste-verify.
7. `Ctrl+R`: read-only mode toggles (allowed because environment is
   Local, not Prod).
8. `H`: recents overlay opens; navigation shows recently-visited views.
9. Original keys still work: `d`, `l`, `s`, `f`, `e`, `:`, `q`.

- [ ] **Step 3: Update READMEs**

Update `app/README.md` Keybindings section to add:
- `Ctrl+P` — command palette
- `/` — search / filter
- `Ctrl+R` — toggle read-only
- `K` — copy as kubectl
- `H` — recents

Update top-level `README.md` status to:
```markdown
Phase 3A (immediate ergonomics) complete: command palette, faceted
search, inline action footer, safety badges + env-driven read-only
mode, copy-as-kubectl, navigation history. Next: 3B (relationship
navigation + diff + workflows).
```

- [ ] **Step 4: Commit + tag**

```bash
git add README.md app/README.md
git commit -m "docs: update READMEs for phase 3A completion"
git tag -a phase-3a-ergonomics -m "Phase 3A: command palette, faceted search, safety, copy-kubectl, history"
```

---

## Phase 3A exit criteria

1. `cargo test --workspace` passes.
2. `cargo clippy --workspace --all-targets -- -D warnings` clean.
3. `cargo fmt --check` clean.
4. Manual verification (Task 10 step 2) all checks pass against k3d.
5. CI green.
6. TUI still launches with no args; CLI verbs (Phase 2B) unaffected.

When all hold, write Phase 3B (relationship navigation + diff +
workflows).
