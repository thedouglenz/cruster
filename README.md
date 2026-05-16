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

Uses your active kubeconfig context. Keybindings:

- `j` / `k` (or arrow keys) — move selection
- `g` / `G` (or `Home` / `End`) — jump to top / bottom
- `q` / `Esc` — quit

## Project layout

```
cruster/
├── app/                    # Rust workspace (the binary lives here)
│   ├── crates/
│   │   ├── cruster-core/   # shared types
│   │   ├── cruster-kube/   # kube-rs wrapper, watch streams, in-memory store
│   │   ├── cruster-tui/    # ratatui app, views, input handling
│   │   └── cruster-bin/    # binary that wires everything together
│   └── benches/            # perf harness
├── web/                    # marketing site (Phase 5)
└── docs/                   # specs and implementation plans
```
