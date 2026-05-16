# Cruster — Design Spec

**Date:** 2026-05-15
**Status:** Draft, pending user approval
**Owner:** Doug

## Summary

Cruster is a paid, opinionated Kubernetes TUI built in Rust. It competes
directly with k9s. The three things that justify the price are **speed**
(noticeably faster render and watch handling than k9s, even on large
clusters), **ergonomics built for incident-solving** (relationship-first
navigation, faceted search, diff-as-first-class, saved investigative
workflows, safety guardrails), and an **agent-native surface** — cruster
ships as a CLI with first-class structured output, plus a bundle of
portable Agent Skills (the [agentskills.io](https://agentskills.io)
standard, supported by Claude Code, Cursor, Codex, Gemini CLI, Goose,
GitHub Copilot, and 25+ other agents), plus a richer Claude Code
plugin layered on top.

It is not a research toy and it is not an AI-debugger product —
cruster contains no LLM client of its own and never makes outbound
model calls. It is the fastest, safest way to understand what
changed in a cluster and what to do next, usable two ways:

- **By hand at the keyboard** (TUI), where it beats k9s on speed
  and incident-solving ergonomics.
- **As a tool an agent shells out to** (CLI), where it beats
  kubectl on density, round-trip count, schema stability, and
  noise. Every SRE who uses Claude Code, Cursor, Codex, etc. has
  the same need: instrument their agent with cluster access and
  let it rip. Today they wrap kubectl, which is a verbose,
  unstructured, agent-hostile interface. Cruster is the CLI those
  agents wish you'd give them.

k9s does not compete here — it's TUI-only, with no CLI for an
agent to call. On the agent-tooling axis, the competitor is kubectl,
and beating it is a structural win, not a hard fight.

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

3. **Ergonomics built for incident-solving**, not just navigation.
   The job is to collapse the loop from symptom → cause → action.
   Concretely: relationship-first navigation, faceted search,
   diff-as-first-class, pinned-vs-follow panes, inline action
   discoverability, saved investigative workflows, and explicit
   safety guardrails (environment identity, read-only mode,
   "copy kubectl equivalent"). Command palette / keymap presets /
   themes are table stakes underneath this.

4. **Agent-native from day one.** Every operation cruster does as a
   TUI, it can do as a single CLI invocation with **LLM-efficient
   output** — dense, schema-tagged, token-budgeted, schema-versioned.
   The CLI ships alongside a bundle of portable **Agent Skills**
   (agentskills.io standard) that teach 30+ supported coding agents
   how to use it well. A richer Claude Code plugin layers slash
   commands and curated workflows on top.

## Non-goals (v1)

- Multi-cluster federation (planned for v2 once the wedge lands).
  Not marketed on the Pro page until shipped.
- Cost overlay, observability fusion (Prometheus / OpenCost).
  Deferred to v2+.
- Any LLM client of our own. Cruster never makes an outbound
  model call — not from the TUI, not from the CLI, not anywhere.
  All AI happens in the user's coding agent (Claude Code, Cursor,
  Codex, …), which invokes cruster's CLI as a tool. This means we
  don't run an LLM proxy, don't meter tokens, don't store user
  data, and don't take on AI safety/abuse surface. The agent the
  user already pays for does the synthesis; cruster gives it
  better raw material than kubectl ever could.
- Cluster mutation beyond what parity demands: edit YAML, scale,
  delete, exec, port-forward. No applies, no kustomize, no helm
  install in v1. GitOps *visibility* (read-side: rollout status,
  desired-vs-live diff) is planned for v1.5; GitOps *control*
  (write-side) stays out of scope.
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

### Pillar 2: Ergonomics built for incident-solving

Cruster is task-first, not resource-first. The default mental model is
"I have a symptom, help me find the cause and the fix." Raw resource
browsing stays available for power users, but it isn't the centerpiece.

**Relationship-first navigation.** From a selected resource, one
keystroke opens any related object: owner chain (Pod → ReplicaSet →
Deployment), endpoints, services, ingresses, configmaps mounted,
secrets mounted, PVCs, node, recent events, recent logs. The graph is
precomputed from the in-memory store; navigation is local and instant.

**Faceted search**, not just fuzzy. Token queries: `ns:prod kind:pod
status:CrashLoop app:payments`. Free-text falls back to fuzzy across
name/labels. Results stream into a preview pane with multi-select for
bulk operations (delete N pods, restart N deployments). Stolen
shamelessly from `fzf`.

**Pinned vs. follow panes.** In a multi-pane layout, every pane is
either pinned to a specific resource or following the user's
selection. Pin the logs of pod A while exploring pod B's events,
without losing your place.

**Diff as a first-class verb.** Compare any two of: live vs. desired
(GitOps drift), current vs. previous rollout, pod A vs. pod B env,
service selector vs. matching pods, secret vs. secret (redacted),
configmap vs. configmap. Diffs render inline, syntax-aware, with
"copy as kubectl-equivalent" available on every diff line.

**Inline action discoverability.** No mnemonic soup. The footer
always shows valid actions for the currently focused object with
their keybindings, the same way `lazygit` and `helix` do.
First-time discoverability without sacrificing keyboard speed.

**Saved investigative workflows.** Named, precomposed pane sets +
queries: "Why is this rollout stuck?" "Why is this service dark?"
"What changed in this namespace in the last hour?" Workflows are
YAML files in `~/.config/cruster/workflows/`, shippable in a Pro
team's shared config.

**Safety ergonomics.** A persistent badge at the top of every view
shows: cluster name, context, namespace, and a color band (`prod`,
`staging`, `dev`, `local`) driven by user-configurable matchers.
Read-only mode (`--readonly`) is the default for any context tagged
`prod`. Every destructive action shows the equivalent kubectl command
before executing. "Copy as kubectl" is one keystroke on any view.

**History, breadcrumbs, recents.** Recently-visited resources are a
keystroke away (ranked by recency × frequency, atuin-style).
Backtracking through your navigation is a separate keystroke. Session
restore reopens your last layout + selections on restart.

**Themeable, shareable styling.** Themes are a first-class artifact,
the way they are in IDEs. The goal is a thriving community
ecosystem — users build and share themes the way they do for VS
Code, Sublime, Helix, neovim.

- **Comprehensive surface.** Themes control colors, borders, glyphs
  (Unicode/ASCII variants), padding, table dividers, status
  indicators, modifier styles (selected / focused / disabled), and
  per-resource-state colors (Running / CrashLoop / Pending / Failed).
  Not just "16 ANSI swaps."
- **Hand-editable text format.** A single TOML or RON file. No
  binary, no build step, no transpilation.
- **Inheritance & composition.** A theme can extend another:
  `extends = "dark"` and override only what differs. Trivial to
  build "Solarized but with cruster's incident-mode tweaks."
- **Live reload.** `cruster --watch-theme <path>` reloads on save so
  authors iterate with the running TUI in front of them.
- **Theme palette / preview command.** `cruster theme preview <path>`
  renders every UI element with the theme applied so authors see
  every surface they need to style.
- **One-step install.** `cruster theme install <url>` (git repo, gist,
  HTTPS URL) and `cruster theme install <name>` (community registry,
  post-v1). Themes ship as a single file in `~/.config/cruster/themes/`.
- **Ships beautiful defaults.** Free tier ships the `terminal`
  theme as the default — it inherits the host terminal's palette
  so cruster looks coherent in any setup out of the box, but
  there's no choice. The bundled library (`dark`, `light`,
  `solarized-dark`, `solarized-light`, `monokai`, `gruvbox`,
  `tokyonight`, `catppuccin`) and the ability to switch / install
  custom or community themes is a **Pro feature**. The bundled
  defaults also exist as good starting points for authors to fork.
- **Theme-aware safety badge.** Environment badges (`prod` /
  `staging` / `dev` / `local`) draw from a theme's
  `env_band.<environment>` color, so themes look coherent with the
  safety identity system.

**Other primitives** (table stakes — necessary but not the pitch):
command palette (`Ctrl+P` / `Cmd+P`) over every action, keymap presets
(`vim` / `emacs` / `normal`), named multi-pane layouts switchable with
`Alt+1..9`, first-class mouse mode that can be disabled per user.

### Pillar 3: Agent-native (Claude Code first)

The bet: every SRE and platform engineer worth hiring already has
a coding agent and wants to instrument it with cluster access. They
end up wrapping `kubectl`, which is unstructured, verbose, and
chatty — multiple round trips for what an agent reasons about as
one question, raw apiserver objects bloated with `managedFields` and
status timestamps, no schemas, no token budgets, no pre-decomposed
analysis. Cruster's CLI is purpose-built to be the better tool. On
this axis the real competitor is **kubectl**, not k9s — k9s has no
CLI at all, so it loses by default.

The decision: **no MCP server**, **no embedded LLM**. MCP is overkill
when the agent already has a shell. An LLM client is wrong because
the user's agent already does that job. Cruster is just a CLI that
emits agent-shaped output, plus the skills that teach agents how to
use it well.

Three deliverables, all shipped in v1.

**a) LLM-efficient CLI mode**

Every cruster verb works as both a TUI command and a one-shot CLI
invocation. The CLI was designed for LLM consumption from the start,
not as an afterthought to the TUI.

LLM-efficient mode is auto-detected (non-TTY stdout) or forced with
`--llm`. It guarantees:

- **Dense, schema-tagged output.** Default is line-delimited JSON
  (NDJSON). Every record includes a `$schema` and `$version` field
  so prompt templates can target stable shapes.
- **Aggressive field pruning.** `managedFields`, large annotations,
  status timestamps that don't help reasoning, and other apiserver
  noise are stripped by default. `--full` re-includes them.
- **No interactive prompts, no colors, no pagers, no spinners.**
- **Token-budget awareness.** `--budget 2000` trims output to fit a
  token budget, deterministically, with a `truncated: true` marker
  and a `next_cursor` for pagination.
- **Pre-decomposed analysis** for common verbs, not raw resource
  dumps. `cruster why-pending pod/foo` returns
  `{schedulable_nodes: [...], reasons: [...], remediation_hints:
  [...]}` — not 4000 tokens of kubectl-describe output.
- **Context bundles in one call.** `cruster bundle pod/foo --include
  events,owners,logs-tail,related-services` returns the entire
  incident-debugging payload in a single structured response so the
  agent doesn't have to chain ten calls.
- **Self-describing.** `cruster help --format json` returns a
  machine-readable command index. Schema files for every verb live
  at `cruster schema <verb>`.

CLI verbs at v1: `get`, `describe`, `logs`, `events`, `bundle`, `diff`,
`why-pending`, `why-crashloop`, `why-no-endpoints`, `what-changed`,
`exec`, `port-forward`. Each has a stable, versioned output schema.

**b) Portable Agent Skills** (agentskills.io standard)

Cruster ships a `skills/` directory of agentskills.io-format skills.
Each skill is a folder with a `SKILL.md` (frontmatter: name +
description) and optional supporting scripts/references. Agents
discover them via progressive disclosure: name+description at startup,
full instructions only when activated.

Initial skill set:
- `cruster-debug-pod` — when the user asks why a pod is failing
- `cruster-rollout-status` — when the user asks about a deployment rollout
- `cruster-what-changed` — when the user asks what changed recently in a namespace
- `cruster-service-dark` — when traffic isn't reaching a service
- `cruster-resource-bundle` — when the agent needs full context on any resource

These work across **30+ agents** that adopt the agentskills.io
standard: Claude Code, Cursor, Codex, Gemini CLI, Goose, OpenHands,
GitHub Copilot, VS Code, OpenCode, Amp, Roo Code, Junie, Kiro, and
others. Installation is "drop the folder in your agent's skills
directory" — cruster ships an installer that knows the right path
per agent.

**c) Claude Code plugin** — `plugins/claude-code/`

A richer, Claude-Code-specific bundle layered on the same CLI.
Adds slash commands that compose multiple CLI calls + skills:

- `/cruster:debug` — full incident bundle on the currently active
  context resource (selection picked up from `~/.cache/cruster/
  selection.json`, which the TUI writes whenever the user selects a
  row — no socket, no daemon).
- `/cruster:diff` — diff two resources, formatted for Claude.
- `/cruster:why-pending` — wraps `cruster why-pending`.
- `/cruster:what-changed` — wraps `cruster what-changed`, scoped to
  a namespace and time window.

The plugin reuses the agentskills.io skills internally but adds
Claude-Code-only conveniences: hooks, settings, agents (per the
Claude Code plugin format).

**d) Prompt actions — clipboard bridge from the TUI to your agent**

The TUI's own contribution to Pillar 3. Any view, any pane, can
declare keybound **prompt actions** that turn the visible context
into a structured prompt and copy it to the system clipboard.

The workflow: you're looking at a pod's logs, you can see the error,
you press `P d` (leader `P` + `d` for *diagnose*). A toast confirms
`Copied 1.2 KB prompt — paste into your agent`. You switch to your
Claude Code / Cursor / Codex window, paste, and the agent has
everything it needs: the resource identity, the cluster, the recent
events, the visible log lines, framed in a question. No socket, no
daemon, no API key, no integration ceremony. The user is the
transport.

This is the **reusable abstraction** that anything in the TUI can
plug into. It's not hard-coded per view — adding a new prompt
action is dropping a YAML file in `~/.config/cruster/prompts/`,
no Rust changes needed.

Definition format:

```yaml
name: diagnose
description: Diagnose the current resource using its events + recent logs
applies_to:
  kinds: [Pod, Deployment, Service]
  views: [list, detail, logs, events]
key: "P d"                     # leader chord, configurable
template: |
  Diagnose what's wrong with {{ resource.kind | lower }}/{{ resource.name }}
  in namespace {{ resource.namespace }} on cluster {{ cluster.name }}.

  Current status: {{ resource.status_summary }}

  Recent events:
  {{ events.recent(5) | yaml }}

  Recent logs (last 50 lines from the focused logs pane):
  ```
  {{ logs.tail(50) }}
  ```
```

Mechanism:

- **Template language: Tera** (Rust-native, Jinja-style). Familiar,
  no new DSL to learn.
- **Context variables** available to every template:
  - `resource` — kind, name, namespace, status_summary, age, labels,
    annotations, owner_chain, raw (full JSON if needed)
  - `cluster` — name, context, kube version
  - `events` — `recent(n)`, `by_resource`
  - `logs` — `tail(n)`, `since(duration)`, `grep(pattern)`,
    `visible` (exactly what's rendered in the focused logs pane)
  - `selection` — if the user has highlighted a range of lines,
    those exact lines
  - `pane` — content of the currently focused pane (for views with
    no obvious resource — e.g. events stream)
- **Lazy resolution.** Only context the template *references* is
  gathered. `{{ logs.tail(50) }}` triggers a log fetch; templates
  that don't mention logs don't pay for one.
- **Clipboard write** via `arboard` (cross-platform, no Electron
  bullshit). On Linux without X11/Wayland, falls back to writing
  to a tmp file and showing the path.
- **Discoverability everywhere.**
  - Appears in the command palette (`Ctrl+P` → "prompt: diagnose").
  - Appears in the inline action footer when the current view +
    selection matches `applies_to`.
  - `cruster prompts list` enumerates all defined prompts.
- **CLI parity.** `cruster prompt diagnose pod/nginx -n default`
  resolves the same template and writes the prompt to stdout
  (`--clipboard` to copy instead). Means agents, shell pipelines,
  and TUI users hit the same definitions.
- **Hot reload.** Edit a prompt file, save, the new version is
  available on next keypress without restart.

Ships with defaults: `diagnose`, `why-failing`, `what-changed`,
`summarize-events`, `compare-with-previous-rollout`, `explain-yaml`,
`suggest-fix`. Each is a small YAML file in the cruster repo's
`assets/prompts/` — they're examples as much as features.

**Pricing:** the mechanism, all shipped defaults, and any number
of user-authored prompts are **free**. **Team tier** adds
team-shared prompt libraries that sync via the team config service,
so an org standardizes on one debugging vocabulary.

---

**Why this beats MCP for a local CLI:** no server lifecycle, no
transport, no stale connections, no separate authn surface. Just a
binary the agent already has on `$PATH`, with output designed to
land cleanly in a context window — plus a TUI that makes the
human-in-the-loop path frictionless.

## Architecture

This is a **monorepo**. The Rust app and the marketing site live in
the same git repository so docs, spec changes, licensing keys, and
website copy can travel in the same commits as the code they
describe.

Top-level layout:

```
cruster/
├── README.md                   # monorepo overview
├── docs/                       # specs, plans, design notes
├── app/                        # the Rust TUI/CLI workspace
│   ├── Cargo.toml              # cargo workspace manifest
│   ├── rust-toolchain.toml
│   ├── crates/
│   │   ├── cruster-core/       # config, themes, keymap, shared types
│   │   ├── cruster-kube/       # kube-rs wrapper: watch streams, resource store, action verbs
│   │   ├── cruster-tui/        # ratatui app: views, layouts, command palette, input router
│   │   ├── cruster-cli/        # CLI subcommands + LLM-efficient output formatters + schemas
│   │   └── cruster-bin/        # single `cruster` binary: dispatches TUI vs CLI mode
│   └── benches/                # perf harness
├── skills/                     # agentskills.io-format skills (portable across 30+ agents)
│   ├── cruster-debug-pod/
│   ├── cruster-rollout-status/
│   ├── cruster-what-changed/
│   ├── cruster-service-dark/
│   └── cruster-resource-bundle/
├── plugins/
│   └── claude-code/            # Claude Code plugin: slash commands, hooks, settings
├── web/                        # marketing site (Next.js or Astro; chosen in its own plan)
└── .github/
    └── workflows/
        └── ci.yml              # runs app + web + skills checks
```

`app/` is a self-contained cargo workspace — `cd app && cargo build`
must work without referencing the parent. `web/`, `skills/`, and
`plugins/` are similarly self-contained. The root holds only
cross-cutting things: docs, shared README, CI orchestration, license
files.

**Single binary, two modes.** `cruster` is one binary. Invoked with
no args (or `cruster tui`), it launches the TUI. Invoked with a verb
(`cruster get pods`, `cruster bundle pod/foo`, etc.), it runs as a
one-shot CLI. Mode is decided in `cruster-bin/main.rs`; both modes
share the same `cruster-core` + `cruster-kube` + (for CLI) `cruster-cli`
internals. No separate `cruster-llm` or `cruster-mcp` binary; the
binary on the agent's `$PATH` is the same one the human runs.

**Data flow at runtime:**

```
kube-apiserver
   │  (watch streams via kube-rs)
   ▼
cruster-kube::ResourceStore
   │
   ├── (TUI mode) ──▶ cruster-tui::ViewState ──▶ ratatui render
   │                       │
   │                       └── writes ~/.cache/cruster/selection.json
   │
   └── (CLI mode) ──▶ cruster-cli::Formatter ──▶ NDJSON/JSON/YAML/text to stdout
                              ▲
                              │ schemas pinned per verb, versioned
                              │
   Agent (Claude Code, Cursor, Codex, …) ──┘
        │  reads skills/ via agentskills.io standard
        │  shells out to `cruster <verb> --llm ...`
        └─ for Claude Code, optionally invokes via plugins/claude-code/ slash commands
```

**Key design decisions:**

- `ResourceStore` is the single source of truth. The TUI reads
  snapshots; CLI verbs that need watch data also use it (one-shot
  CLI verbs that only need a list/get bypass it and hit the apiserver
  directly — cheaper for short-lived processes).
- Render is decoupled from the watch loop via a snapshot pattern. The
  render loop never blocks on the API; the watch loop never blocks on
  the TUI.
- CLI output formatters live behind a `Formatter` trait keyed on
  `--format` (default: NDJSON in `--llm`, text in TTY). Each verb
  has a stable, versioned output schema in `cruster-cli/schemas/`.
- The TUI writes its current selection to
  `~/.cache/cruster/selection.json` whenever it changes. The
  Claude Code plugin reads that file when a slash command needs the
  "currently selected resource." No socket, no daemon, no IPC layer.
- Skills under `skills/` are checked in flat-file (no build step) so
  they can be installed by copy. CI lints them against the
  agentskills.io schema.

## Licensing

Client-side license check, offline-first:

- The binary embeds a public key. On startup it reads a signed
  license file at `~/.config/cruster/license.jws`. The license
  declares tier, expiry, and user email.
- Free tier features always work without a license.
- Pro/Team features are gated client-side. The binary does not phone
  home on normal launch. A background refresh runs at most once per
  24h when connectivity is available.
- Trial: 14 days, triggered by `cruster trial`, no email required,
  no credit card.

**Pricing principle, post-cursor-review:** the free tier must be
lovable enough for someone to make cruster their daily k9s
replacement *and* feel the magic. Paywalling baseline ergonomics is
self-defeating. Paid value lives in workflows that genuinely cost
us money to deliver (AI workflows, multi-cluster scale, team
features) or that buy operational maturity (GitOps integration,
team-wide config sync, audit).

Pricing (initial):

| Tier | Price | Includes |
|---|---|---|
| Free | $0 | Single cluster, full TUI ergonomics (palette, faceted search, layouts, safety badges, saved workflows), default `terminal` theme only, LLM-efficient CLI, all agentskills.io skills, Claude Code plugin, prompt actions (all shipped defaults + unlimited user-defined templates) |
| Pro | $99/year *or* $12/mo | Multi-cluster (when shipped), GitOps visibility (Helm / Argo / Flux rollout state, desired-vs-live diff), change-correlation timeline, theme switching + bundled theme library + community theme installation + live theme reload, advanced exports |
| Team | $29/mo/user (annual $290/yr) | Pro + shared team config (workflows, themes, prompt actions, safety matchers, saved queries sync via a small hosted service), SSO, audit log of cluster mutations |
| Enterprise | Contact | Team + self-hosted license server + config sync, BYO CA, air-gapped mode (no outbound calls except apiserver), SLA |

Notes:
- **Annual is the default offer; monthly exists for evaluation.**
  Terminal-tool users are subscription-resistant. A one-time-feeling
  $99/year offer converts better than $15/month.
- **Multi-cluster is not marketed on the Pro page until it ships.**
  Avoiding the credibility own-goal.
- **No AI usage metering of any kind.** Cruster does not run an
  LLM, does not relay tokens, and does not charge per call. All AI
  happens in the user's agent. This is a deliberate scope choice:
  it keeps our pricing simple, removes an entire compliance
  surface, and aligns incentives with our actual buyers.

## MVP scope (6 weeks)

Phase boundaries are gates, not week boundaries.

**Phase 1 — skeleton (week 1)**
- Monorepo scaffolding, CI, ratatui app that shows pods.
- `cruster-kube` watch stream + resource store for pods only.
- Keyboard navigation, quit.
- Cold-start perf baseline.

**Phase 2 — parity-lite + LLM-efficient CLI (weeks 2–3)**
- All v1 kinds: pods, deployments, services, nodes, events,
  configmaps, secrets, namespaces.
- Describe, logs (with follow + grep), exec, port-forward.
- YAML view + edit (delegates to `$EDITOR`).
- k9s-equivalent default keymap.
- **CLI mode for every kind**: `cruster get`, `cruster describe`,
  `cruster logs`, `cruster events` with `--llm` flag, NDJSON output,
  field pruning, and a schema file per verb. This is not a
  retrofit — every kind added to the TUI ships a matching CLI verb
  in the same task.

**Phase 3 — incident-solving ergonomics + themes (week 4)**
- Command palette.
- Faceted search (token queries + fuzzy fallback).
- Relationship-first navigation (one-keystroke jumps to related
  resources).
- Diff verb (TUI + `cruster diff` CLI).
- Inline action discoverability footer.
- Safety badges + read-only mode + "copy as kubectl".
- Saved investigative workflows (YAML format under
  `~/.config/cruster/workflows/`).
- Keymap presets (vim, emacs, normal).
- Three layouts (single, triplet, incident).
- Theme engine + 5+ shipped themes + `cruster theme install/preview`
  + live reload.

**Phase 4 — agent-native surface (week 5)**
- `cruster bundle` verb (single-call context bundle).
- `cruster why-pending`, `cruster why-crashloop`, `cruster
  why-no-endpoints`, `cruster what-changed` verbs with structured
  output schemas.
- Token-budget output trimming (`--budget`).
- TUI writes `~/.cache/cruster/selection.json` on selection change.
- **Prompt actions engine**: YAML loader, Tera template resolver,
  lazy context gatherers (resource / cluster / events / logs /
  selection / pane), clipboard writer (arboard), discoverability in
  palette + action footer, hot reload, CLI parity (`cruster prompt
  <name>`). Ships with 7 default prompts in `assets/prompts/`.
- Five agentskills.io skills in `skills/` (debug-pod,
  rollout-status, what-changed, service-dark, resource-bundle).
- Claude Code plugin in `plugins/claude-code/`: slash commands
  `/cruster:debug`, `/cruster:diff`, `/cruster:why-pending`,
  `/cruster:what-changed`.
- `cruster install-skills <agent>` installer that knows the right
  path for Claude Code, Cursor, Codex, Gemini CLI, Goose, OpenCode.

**Phase 5 — polish + license + launch (week 6)**
- License file loader, tier gating (only multi-cluster + GitOps + AI
  workflows + team features gated; everything in Phases 1–4 is free).
- Trial flow.
- Installer (homebrew tap, `cargo install`).
- Landing page (under `web/`) with download, benchmark numbers,
  agent compatibility matrix.

## Success criteria for the MVP

To ship v1 publicly, all of these must hold:

- The three benchmark targets in **Pillar 1** are met on the
  reference k3d cluster and on a synthetic 10k-pod cluster.
- A user fluent in k9s can complete: list pods, describe, view logs,
  exec, port-forward, edit YAML — without reading docs, in under 60
  seconds total.
- A new user, given only the cruster TUI (no docs), can solve a
  staged "pod is CrashLoopBackOff because of a bad configmap"
  incident in under 90 seconds using the relationship-first
  navigation and diff verb. This validates Pillar 2.
- A coding agent with no special tooling beyond `cruster` on `$PATH`
  and the agentskills.io skills installed can answer "why is pod X
  in namespace Y failing?" on a cluster it has never seen before,
  using only shell calls to `cruster`. Validated against at least
  two agents (Claude Code + one other from the agentskills.io
  showcase, e.g. Cursor or Codex).
- **Cruster vs kubectl benchmark.** On the staged "why is pod X
  failing" scenario, an agent reaches a correct answer with:
  - **Cruster path:** ≤ 3 CLI calls, ≤ 1,500 tokens of returned
    data, ≤ 8 seconds wall-clock.
  - **Kubectl path** (same agent, kubectl only): typically ≥ 6
    calls and ≥ 5,000 tokens.
  This is the headline number for marketing and the empirical
  test that Pillar 3 actually delivers.
- **Prompt-action workflow.** From the logs view of a failing pod,
  the user presses the `diagnose` keybind, pastes into Claude Code,
  and gets a useful answer in under 15 seconds total (TUI keypress
  → clipboard → paste → response). The shipped `diagnose` prompt
  includes resource identity, recent events, and the last 50 lines
  of the focused logs pane. Validated against at least Claude Code
  and one other agent.
- The Claude Code plugin flow (highlight pod in TUI →
  `/cruster:debug` in Claude Code) produces a useful answer in
  under 5 seconds end-to-end.
- A Pro-licensed user can install a community theme from a URL in
  one command, reload it live, and have it work end-to-end
  including the safety badge. A free-tier user attempting the same
  command gets a clear "this is a Pro feature" message and a link
  to upgrade — they never see a broken state.
- License gating works as specified: free tier gets all of Phases
  1–4; Pro features (multi-cluster when shipped, GitOps, AI
  workflows, team) refuse to start without a valid license and say
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

- **k9s is free, beloved, and good enough for most people.** k9s's
  real moat is habit. The bet is that operators feel pain on
  incident-solving workflows (correlating pods → owners → events →
  changes) and that cruster's task-first ergonomics + agent-native
  CLI collapse enough of that loop to displace habit. If they
  don't, we are a nicer k9s with paywalls — which is the worst
  possible product.
- **Free tier is generous enough that nobody upgrades.** Real risk
  with the revised pricing. Mitigation: paid features (multi-cluster,
  GitOps visibility, AI workflows, team config sync) are the ones
  serious operators *actually* want once they're using cruster daily.
  Track conversion empirically and rebalance if needed.
- **Trust during incidents is the unforgiving variable.** If cruster
  is ever wrong, slow, or "clever" in a way that misleads an
  operator mid-incident, the brand is dead. Mitigation: read-only
  defaults in `prod` contexts, "copy as kubectl" on every action,
  no AI hallucination paths in the TUI, prominent environment
  badges. Invest disproportionately in this surface.
- **Rust ecosystem for kube is good but not Go-good.** `kube-rs` is
  mature but if we hit a gap (e.g. an obscure auth provider) we'll
  have to contribute upstream. Budget for this.
- **Performance claims need to be true.** If we miss the benchmark
  targets we have no story. The phase-1 skeleton ships with a
  perf harness from day one.
- **LLM-output schema stability.** Once agents and skills depend
  on `cruster <verb> --llm` output schemas, breaking them breaks
  the whole agent-native pitch. Mitigation: every verb has an
  explicit schema version; breaking changes only at major version
  bumps; CI tests every shipped skill against the current binary.
- **agentskills.io is young.** Originated by Anthropic, broadly
  adopted, but still evolving. Mitigation: skills are simple
  markdown + scripts; if the standard pivots, our migration cost
  is low.
