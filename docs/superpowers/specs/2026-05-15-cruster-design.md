# Cruster — Design Spec

**Date:** 2026-05-15
**Status:** Draft, pending user approval
**Owner:** Doug

## Summary

Cruster is a paid, opinionated Kubernetes TUI built in Rust. It competes
directly with k9s. The three things that justify the price are **speed**
(noticeably faster render and watch handling than k9s, even on large
clusters), **ergonomics** (command palette, global fuzzy search, multi-pane
layouts, configurable keymap presets), and **Claude Code compatibility** (an
MCP server that exposes the cluster as tools, a companion socket that shares
state with a running Claude Code session, and a shipped plugin with slash
commands).

It is not a research toy and it is not an AI-debugger product. It is a
keyboard-driven cluster navigator that happens to be the best surface for a
Claude Code agent to drive a cluster through.

## Goals

1. **Day-one parity with k9s** on the navigation and inspection workflows
   that 80% of k9s users live in: list resources by kind, filter,
   describe, view logs (with follow + grep), exec into a pod,
   port-forward, edit YAML.

2. **Measurably faster** than k9s on the metrics that operators feel:
   - Cold start to first interactive frame: target **< 100 ms**.
   - Frame budget under steady-state watch load on a 10,000-pod
     cluster: **< 16 ms** for 60fps.
   - Initial pods list paint on a 10k-pod cluster: **< 500 ms**.

3. **Better ergonomics** than k9s:
   - Command palette (`Ctrl+P` / `Cmd+P`) routes every action,
     resource kind, namespace, and saved view.
   - Global fuzzy search across all kinds × all namespaces × all
     connected clusters, not per-view filter.
   - Three first-class keymap presets: `vim`, `emacs`, `normal`.
   - Multi-pane layouts (e.g. pod list + logs + events visible
     simultaneously) without dropping into tmux.
   - Mouse support that's a first-class input mode, not an
     afterthought.

4. **Claude Code is a first-class consumer.** A Claude Code agent can drive
   a cluster through cruster without ever calling `kubectl`.

## Non-goals (v1)

- Multi-cluster federation (planned for v2 once the wedge lands).
- AI debugging features built into the TUI itself. Cruster is *consumed by*
  Claude Code; it does not embed an LLM client of its own.
- Cost overlay, observability fusion (Prometheus / OpenCost). Deferred to a
  later release.
- Cluster mutation beyond what's required for parity: edit YAML, scale,
  delete, exec, port-forward. No GitOps workflows in v1.
- Windows support in v1. macOS + Linux only.

## Pillars and what they mean concretely

### Pillar 1: Speed

What "feels fast" decomposes into:

- **Cold start.** Lazy-load kubeconfig parsing, skip TLS handshake until
  first request, render the chrome before the first API call returns.
- **Render pipeline.** Diff-based ratatui rendering. The watch loop pushes
  resource deltas into a per-kind store; the render loop reads a snapshot
  and only redraws changed rows. No full-list re-render per tick.
- **Watch streams.** One `kube::runtime::watcher` per (kind, namespace)
  tuple, multiplexed onto a single tokio runtime. Backpressure handled
  with bounded channels; if the UI falls behind, we coalesce updates per
  frame rather than blocking the producer.
- **Virtualized lists.** Only the visible rows are formatted. Resource
  count in the corner is exact; the rendered table is a window.
- **No blocking work on the input thread.** All kube I/O is on tokio
  workers; the input thread only marshals events.

Concrete benchmarks (must pass to ship v1):
- `cruster pods` cold start to first interactive frame on a 100-pod
  cluster: < 100 ms (p95 over 50 runs).
- Steady-state CPU on a 10k-pod cluster with the pods view open and
  scrolling: < 5% on an M-series Mac.
- Memory on the 10k-pod scenario: < 200 MB resident.

### Pillar 2: Ergonomics

- **Command palette.** Single `Ctrl+P` / `Cmd+P` opens a fuzzy-matched
  list of: every navigable kind, every namespace, every saved view,
  every action ("describe", "exec", "delete", "port-forward 8080:80",
  "toggle wide"). Replaces the constellation of `:pods`, `:dp`, `/`,
  `?`, etc. that k9s uses today.
- **Global fuzzy search.** Separate keybind (`Ctrl+/`). Searches across
  all kinds × all namespaces. Results are grouped by kind with a
  preview pane.
- **Keymap presets.** `~/.config/cruster/keymap` accepts a preset name
  (`vim`, `emacs`, `normal`) and an overrides table. Default is
  `normal`. All three ship pre-configured.
- **Layouts.** A layout is a named arrangement of panes. Defaults:
  `single` (k9s-equivalent), `triplet` (list / detail / logs),
  `incident` (events stream + pod list + logs of selected). Switch
  with `Alt+1..9`. User can define more in config.
- **Themes.** A theme file controls colors, borders, glyphs. Ships with
  `dark`, `light`, `solarized`, `monokai`, and a `terminal` theme that
  inherits the host palette. No ASCII-art splash screen.
- **Mouse.** Click to select, double-click to drill in, scroll wheel
  scrolls the focused pane, right-click opens the same menu as the
  command palette scoped to the row. Mouse can be disabled per-user.

### Pillar 3: Claude Code compatibility

Three modes of integration. All three ship in v1 of the paid product.

**a) MCP server mode** — `cruster mcp`

Starts an MCP server (stdio transport by default, HTTP optional). The
server exposes the cluster as tools:

| Tool | Purpose |
|---|---|
| `list_resources(kind, namespace?, label_selector?)` | List any kind |
| `get_resource(kind, name, namespace?)` | Full object as JSON |
| `describe(kind, name, namespace?)` | kubectl-describe-equivalent text |
| `logs(pod, container?, tail?, since?, follow=false)` | Pod logs |
| `events(scope?)` | Recent events, scoped to a resource if given |
| `exec(pod, container?, command[])` | One-shot exec, returns stdout/stderr/exit |
| `port_forward(pod, ports[])` | Returns a handle the client can close |
| `diff_resources(a, b)` | Structural diff of two resources |
| `apply(yaml)` | Apply a manifest (gated by `--allow-write` flag) |

Tools return structured JSON, not text scrapes. Read-only by default.
Write tools require an explicit `--allow-write` startup flag and are
hidden when not enabled.

Auth: reuses the user's kubeconfig context. The MCP server inherits the
context that was active when launched; switching context requires a
restart (avoids cross-context confusion in an agent session).

**b) Companion socket mode** — `cruster --companion`

When the TUI starts with `--companion`, it opens a Unix domain socket
at `$XDG_RUNTIME_DIR/cruster-<pid>.sock`. A Claude Code session can
connect (the shipped plugin does this automatically) and:

- Subscribe to the user's current selection (kind, name, namespace,
  cluster).
- Request the current selection's "context bundle": the resource YAML,
  its recent events, its owner chain, its recent logs (for pods). One
  call, one structured payload.
- Push a "highlight this" command back to the TUI to navigate the user.

The user's flow: highlight a pod in cruster, type a question in Claude
Code — the model already has the pod in context.

**c) Claude Code plugin**

Cruster ships a plugin (installable via `cruster install-plugin`) that
adds slash commands to Claude Code:

- `/cruster:debug` — analyze the currently selected resource using the
  companion socket's context bundle.
- `/cruster:diff` — diff two resources (prompts for selection).
- `/cruster:why-pending` — analyze a pending pod (scheduler events,
  resource requests vs. node capacity, taints/tolerations).
- `/cruster:tail` — tail logs of selected pod into the Claude Code
  session.

The plugin is a thin wrapper: each command is a prompt template that
invokes the appropriate MCP tool(s) and asks the model to synthesize.

## Architecture

Rust workspace, four crates plus the binary:

```
cruster/
├── Cargo.toml                  # workspace
├── crates/
│   ├── cruster-core/           # config, themes, keymap, shared types
│   ├── cruster-kube/           # kube-rs wrapper: watch streams, resource store, action verbs
│   ├── cruster-tui/            # ratatui app: views, layouts, command palette, input router
│   ├── cruster-mcp/            # MCP server, tool implementations, companion socket protocol
│   └── cruster-plugin/         # Claude Code plugin files (slash commands, prompts)
└── crates/cruster-bin/         # binary that wires everything together
```

**Data flow at runtime:**

```
kube-apiserver
   │  (watch streams via kube-rs)
   ▼
cruster-kube::ResourceStore  ── snapshot ──▶  cruster-tui::ViewState ──▶ ratatui render
   │                                                   ▲
   │  (deltas)                                         │
   ├──▶ cruster-mcp::Server (when running) ──▶ MCP client (Claude Code)
   │
   └──▶ cruster-tui::CompanionSocket ──▶ Claude Code companion subscriber
```

**Key design decisions:**

- `ResourceStore` is the single source of truth. Both the TUI and the
  MCP server read from it. This means MCP tools are cheap when cruster
  is already running (everything is in-memory), and self-sufficient
  when cruster is launched headless (`cruster mcp` spins up its own
  store on demand).
- Watch streams are reference-counted. If both the TUI and an MCP
  client want `pods` in `agent-platform`, there's one watcher.
- Render is decoupled from the watch loop via an `evmap`-style
  snapshot. The render loop never blocks on the API; the watch loop
  never blocks on the TUI.
- Companion socket protocol is a tiny JSON-over-newline-delimited
  protocol, intentionally not gRPC. Versioned at the message level.

## Licensing

Two-tier client check:

- The binary embeds a public key. On startup, it reads a license file
  from `~/.config/cruster/license.jws` (signed JWT). The license
  declares the tier (`free`, `pro`, `team`), expiry, and user email.
- Free tier features always work without a license.
- Pro/Team features are gated client-side. The binary will not reach
  out to a license server on a normal launch (offline-first); however,
  a periodic background check (≤ once per 24h) refreshes the license
  if connectivity is available.
- Trial: 14 days, triggered by `cruster trial`, no email required.

Pricing (initial):

| Tier | Price | Includes |
|---|---|---|
| Free | $0 | Single cluster, single layout, no command palette, no MCP, no plugin |
| Pro | $15/mo/user | All TUI features, MCP server, companion socket, plugin |
| Team | $39/mo/user | Pro + shared session (tmate-style), team-wide saved views, SSO |
| Enterprise | Contact | Team + self-hosted license server, BYO CA, audit log |

Multi-cluster is paid (Pro+). It is *not* in v1 of the binary but is
part of the Pro pitch from launch — we ship Pro without multi-cluster
and add it within 2 months.

## MVP scope (6 weeks)

Phase boundaries are gates, not week boundaries.

**Phase 1 — skeleton (week 1)**
- Workspace, CI, basic ratatui app that connects to a cluster and
  shows pods.
- `cruster-kube` watch stream + resource store for pods only.
- Keyboard navigation, quit.

**Phase 2 — parity-lite (weeks 2–3)**
- All v1 kinds: pods, deployments, services, nodes, events,
  configmaps, secrets, namespaces.
- Describe, logs (with follow + grep), exec, port-forward.
- YAML view + edit (delegates to `$EDITOR`).
- k9s-equivalent default keymap.

**Phase 3 — ergonomics (week 4)**
- Command palette.
- Global fuzzy search.
- Keymap presets (vim, emacs, normal).
- Three layouts (single, triplet, incident).
- Three themes (dark, light, terminal).

**Phase 4 — Claude Code (week 5)**
- `cruster mcp` server with all read tools (no `apply`/`exec` writes
  in v1).
- Companion socket protocol and TUI subscriber.
- Plugin scaffolding + four slash commands.

**Phase 5 — polish + license (week 6)**
- License file loader, tier gating.
- Trial flow.
- Installer (homebrew tap, `cargo install`, `cruster install-plugin`).
- Public landing page with download + benchmark numbers.

## Success criteria for the MVP

To ship v1 publicly, all of these must hold:

- The three benchmark targets in **Pillar 1** are met on the
  reference k3d cluster and on a synthetic 10k-pod cluster.
- A user fluent in k9s can complete: list pods, describe, view logs,
  exec, port-forward, edit YAML — without reading docs, in under 60
  seconds total.
- A Claude Code agent given `cruster mcp` can answer "why is pod X in
  namespace Y failing?" using only the MCP tools, on a cluster it has
  never seen before.
- The companion-mode flow (highlight pod → `/cruster:debug` in Claude
  Code) produces a useful answer in under 5 seconds end-to-end.
- License gating works: a binary without a license refuses to start
  the MCP server, refuses to open the command palette, etc., and says
  clearly why.

## Open questions

These are deferred — not blockers for v1 but flagged for follow-up:

- **Telemetry.** Opt-in or opt-out? What do we collect? Recommend
  opt-in, anonymous, crash + perf counters only.
- **Update channel.** Self-update from the binary, or rely on
  package managers? Recommend self-update behind a flag, with
  homebrew as the primary distribution.
- **Plugin authoring.** Should third parties be able to ship custom
  views (like k9s plugins)? Recommend yes, but post-v1, and via a
  WASM extension surface rather than shell-out.

## Risks

- **k9s is free, beloved, and good enough for most people.** The
  paid tier needs a clear "yes I will pay for this" moment. The bet
  is that **Claude Code compat** is that moment for the AI-native
  operator persona.
- **Rust ecosystem for kube is good but not Go-good.** `kube-rs` is
  mature but if we hit a gap (e.g. an obscure auth provider) we'll
  have to contribute upstream. Budget for this.
- **MCP is young.** The spec and ecosystem are moving. We pin a
  version and version our tools.
- **Performance claims need to be true.** If we miss the benchmark
  targets we have no story. The phase-1 skeleton must include a
  perf harness from day one.
