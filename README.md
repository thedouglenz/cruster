# cruster

A Kubernetes TUI for people who live in k9s all day. Built in Rust.
Optimized for speed, ergonomics, and Claude Code compatibility.

This repository is a monorepo:

- `app/` — the Rust TUI (cargo workspace, `cd app && cargo build`)
- `web/` — the marketing site (not yet built)
- `docs/` — specs and implementation plans

## Status

Phase 2A (TUI parity-lite) complete: 8 resource kinds (Pods,
Deployments, Services, Nodes, Events, ConfigMaps, Secrets,
Namespaces), `:command-mode` kind switcher, and 5 action verbs —
describe (`d`/`y`), logs with follow + grep (`l`), exec
(`s`, suspends to `kubectl exec -it`), port-forward (`f`, prompts for
mapping), edit YAML via `$EDITOR` then `kubectl apply` (`e`).

Next: Phase 2B (LLM-efficient CLI mode).

## Quick start

```sh
cd app && cargo run --release -p cruster-bin
```

Uses your active kubeconfig context. See `app/README.md` for the
full keymap.

## Project layout

```
cruster/
├── app/                    # Rust workspace (the binary lives here)
│   ├── crates/
│   │   ├── cruster-core/   # shared types
│   │   ├── cruster-kube/   # kube-rs wrapper, watch streams, ResourceKind, StoreRegistry
│   │   ├── cruster-tui/    # ratatui app, ResourceView trait, kind views, action panes
│   │   └── cruster-bin/    # binary that wires everything together
│   └── benches/            # perf harness
├── web/                    # marketing site (Phase 5)
└── docs/                   # specs and implementation plans
```
