# Cruster Phase 6A: Dashboard / Pulse view

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task.

Closes: [#1](https://github.com/thedouglenz/cruster/issues/1).

**Goal:** Replace the default cruster launch view (currently the
pods table) with a curated *pulse* dashboard: cluster-wide health
stats at the top, recent-history sparklines in the middle, and a
user-curated panel of pinned services rendered as gauges +
sparklines at the bottom. Intentionally diverges from k9s — the
landing surface is *what matters to this user* rather than a flat
list of pods.

**Scope guardrail:** v1 reads exclusively from the existing
`StoreRegistry` (Pods, Deployments, Services, Nodes, Namespaces,
Events). No metrics-server, no Prometheus scraping. Sparklines
plot whatever the stores already expose (replica counts, restart
counts, event-rate) over a short rolling window. Adding richer
metrics is a later phase.

**Out of scope:**
- CPU/memory % from metrics-server (separate phase; needs
  `kube::Api<NodeMetrics>` + opt-in).
- Multi-cluster overview (separate phase, requires multiple
  registries).
- Persistent history across restarts. The rolling window is in-
  memory and resets on restart.
- Custom user-defined gauges/queries. v1 ships fixed gauges per
  pinned kind (Deployment, Pod, Service, Node, generic).

## Architecture

### Pin storage — `~/.config/cruster/dashboard.toml`

Mirrors the existing `workflows.toml` / `keymap.toml` convention:

```toml
[[pins]]
kind = "Deployment"
namespace = "default"
name = "web"

[[pins]]
kind = "Pod"
namespace = "default"
name = "db-0"
```

Load on startup, save on every mutation (pin / unpin). Save is
best-effort — failure surfaces as a toast, does not crash the
app.

### History buffer

`DashboardView` keeps a small ring buffer of `Sample` values
(default `N=60`). On `refresh`, if at least `SAMPLE_INTERVAL`
(1s) has elapsed since the last sample, push a new one. Each
`Sample` captures: `pods_running`, `pods_failed`, `events_recent`,
`deployments_ready_ratio`, plus per-pin scalars (deploy ready
count, pod restarts, etc.) keyed by pin index.

### Layout

```
┌─ context · env · pinned-count · last-refresh ───────┐
│ nodes 3/3 ready  ·  ns 12  ·  pods 45 (R42 P1 F2)   │
│ deploys 18/20 ready  ·  svc 14  ·  events 7/min     │
├─ trends · last 60s ─────────────────────────────────┤
│ pods running  ▁▂▃▅▆█▇▅▄▃  42                        │
│ pods failed   ▁▁▁▁▂▂▁▁▁▁   2                        │
│ events/min    ▁▂▃▅▂▁▁▁▁▁   7                        │
├─ pinned (a from a list view to pin, x to unpin) ────┤
│▎deploy/web (default)        [▓▓▓▓░] 4/5  ▁▂▃▄▅      │
│ pod/db-0 (default)          ready 1/1   restarts 0  │
│ svc/api (default)           ClusterIP  10.0.0.42    │
└─────────────────────────────────────────────────────┘
```

### Wiring

- New `views/dashboard.rs` implements `ResourceView` with id
  `"dashboard"`.
- New `dashboard.rs` module: `Pin`, `DashboardConfig` (TOML
  load/save), `pins_path()`.
- `App::new` sets `DashboardView` as the default; `view_for_id`
  recognises `"dashboard"`; palette + `command::resolve_alias`
  expose `:dash` / `:dashboard`.
- New `SemanticAction::PinToDashboard` bound to `a` (Normal +
  Vim; Emacs gets `Alt+a` since `Ctrl+a` is already MoveTop).
  When triggered from any view with a `selected_key`, append the
  pin to disk and toast.

## Tasks

### Task 1: Pin storage module

- [ ] Create `app/crates/cruster-tui/src/dashboard.rs` with:
  - `pub struct Pin { kind, namespace: Option<String>, name }`
    `#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]`
  - `impl Pin { pub fn from_key(k: &ResourceKey) -> Self; pub fn matches(&self, k: &ResourceKey) -> bool; pub fn to_key(&self) -> ResourceKey }`
  - `pub struct DashboardConfig { pins: Vec<Pin> }` with TOML
    derive + `load_or_default()`, `save(&self) -> Result<()>`,
    `add(&mut self, pin: Pin) -> bool` (returns false if already
    present), `remove(&mut self, idx: usize) -> Option<Pin>`.
  - `pub fn pins_path() -> Option<PathBuf>` →
    `~/.config/cruster/dashboard.toml`.
- [ ] Unit tests: round-trip TOML, add dedupes, remove out-of-
  range returns None.
- [ ] Export from `lib.rs`.

### Task 2: DashboardView skeleton + cluster summary

- [ ] Create `app/crates/cruster-tui/src/views/dashboard.rs`.
- [ ] `DashboardView` fields: `config`, `selected`, `summary`
  (cached counts), `history` (`VecDeque<Sample>`, cap 60),
  `last_sample_at: Option<Instant>`, `filter` (unused, kept for
  trait conformance).
- [ ] `refresh`: pull `snapshot()` from every store needed,
  compute counts, push a sample if `>=1s` elapsed.
- [ ] `render`: vertical layout — top band (summary), middle band
  (sparklines via `ratatui::widgets::Sparkline`), bottom band
  (pinned list). Use `Block::default().borders(Borders::TOP)` to
  match other views.
- [ ] `handle_key`: `j`/`k` to move selection in pinned list,
  `Enter` to switch to the pin's kind view (returns a marker via
  a new `ResourceView` extension — see Task 4), `x` to unpin.
- [ ] `id() == "dashboard"`.
- [ ] Register in `views/mod.rs`.

### Task 3: Pinned panel rendering

- [ ] For each pin, look up the live object in the registry. If
  missing (resource deleted), render `(missing)` in red.
- [ ] Deployment: `ratatui::widgets::Gauge` for ready/desired +
  small sparkline of ready over time.
- [ ] Pod: ready containers ratio gauge, restart count text,
  sparkline of restarts.
- [ ] Service: type + cluster IP + port count, no gauge.
- [ ] Node: ready badge + sparkline of pod count assigned to that
  node (filter `pods.snapshot()` by `spec.node_name`).
- [ ] Generic / other kinds: name + kind + "pinned" badge.

### Task 4: Wire into App

- [ ] Replace `Box::new(PodsView::new())` with
  `Box::new(DashboardView::new())` in `App::new`.
- [ ] Add `"dashboard"` arm to `App::view_for_id`.
- [ ] Include `"dashboard"` in `open_palette`'s view list (first
  in the list, so the palette shows it prominently).
- [ ] Add `"dash" | "dashboard"` to `command::resolve_alias`.
- [ ] Default `History::record("dashboard", None)` instead of
  `"pods"`.

### Task 5: Pin / unpin keystrokes

- [ ] New `SemanticAction::PinToDashboard`.
- [ ] Normal + Vim bind `a` (no modifier) to it. Emacs binds
  `Alt+a` (no conflict with `Ctrl+a == MoveTop`).
- [ ] Dispatch in `App::dispatch_semantic`: read
  `current_view.selected_key()`, build `Pin`, call
  `DashboardConfig::load_or_default().add(pin).save()`. Toast the
  result.
- [ ] Inside `DashboardView`, `x` removes the pin under the
  selection cursor (handled inside the view, not via semantic
  action — it's only meaningful here).

### Task 6: Tests + clippy + build

- [ ] `dashboard.rs` round-trip TOML test.
- [ ] `DashboardView::refresh` sample-pushing test (mock
  `Instant`-free version: call refresh twice with a synthetic
  registry; assert sample count grows by exactly 1 across a
  ≥1s sleep). Or expose a test-only "force sample" path.
- [ ] `App` test: a fresh `App` has `current_view.id() ==
  "dashboard"`.
- [ ] `command::resolve_alias("dash")` resolves.
- [ ] `cargo build`, `cargo test`, `cargo clippy
  --all-targets -- -D warnings` all green from `app/`.

### Task 7: Docs

- [ ] Update root `README.md` Status section: "Phase 6A —
  Dashboard view as default launch screen, with pinned services".
- [ ] Update `app/README.md` keymap with `a` (pin) and `x`
  (unpin), and `:dashboard` alias.

## Non-goals / follow-ups

- Live metrics from metrics-server (Phase 6B).
- Persistent sparkline history across restarts (Phase 6B).
- Custom user gauges (Phase 6C).
- Dashboard themes / multiple saved dashboards (Phase 6D).
