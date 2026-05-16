# cruster

A Kubernetes TUI for people who live in k9s all day. Built in Rust.
Optimized for speed, ergonomics, and Claude Code compatibility.

This repository is a monorepo:

- `app/` — the Rust TUI + CLI (cargo workspace, `cd app && cargo build`)
- `web/` — the marketing site (not yet built)
- `docs/` — specs and implementation plans

## Status

Phase 3A (immediate ergonomics) complete: command palette (`Ctrl+P`),
faceted search (`/` with `ns:`/`status:`/`~contains`/fuzzy tokens),
inline action footer, safety badges + env-driven read-only mode
(`Ctrl+R` to toggle), `K` to copy describe as kubectl, `H` for ranked
recents. Plus the full Phase 2A TUI and Phase 2B CLI.

Next: Phase 3B (relationship-first navigation + diff verb + saved
investigative workflows).

## Quick start

TUI:
```sh
cd app && cargo run --release -p cruster-bin
```

CLI:
```sh
cd app
cargo build --release -p cruster-bin
./target/release/cruster get pods
./target/release/cruster get pods --format ndjson | jq .
./target/release/cruster describe pod/nginx -n default --format yaml
./target/release/cruster logs nginx -n default --tail 50 --grep error
./target/release/cruster events --limit 5
./target/release/cruster schema get-pod | jq .title
./target/release/cruster help-json | jq '.[].name'
```

See `app/README.md` for the full TUI keymap.

## Project layout

```
cruster/
├── app/
│   ├── crates/
│   │   ├── cruster-core/   # shared types (ResourceKey, Environment)
│   │   ├── cruster-kube/   # kube-rs wrapper, watch streams, ResourceKind, StoreRegistry
│   │   ├── cruster-tui/    # ratatui app, ResourceView trait, views, actions, overlays, safety
│   │   ├── cruster-cli/    # clap CLI, Formatter, prune, budget, per-verb modules + schemas
│   │   └── cruster-bin/    # single `cruster` binary; dispatches TUI vs CLI
│   └── benches/
├── web/                    # marketing site (Phase 5)
└── docs/                   # specs and implementation plans
```
