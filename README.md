# cruster

A Kubernetes TUI for people who live in k9s all day. Built in Rust.
Optimized for speed, ergonomics, and Claude Code compatibility.

This repository is a monorepo:

- `app/` — the Rust TUI + CLI (cargo workspace, `cd app && cargo build`)
- `web/` — the marketing site (not yet built)
- `docs/` — specs and implementation plans

## Status

Phase 3B (task-first navigation) complete: relationship resolver
(Pod → owner / mounted cm + secret / services that select / node;
Service → selected pods; Deployment → owned pods; Node → scheduled
pods), `r` opens a relationships overlay; `cruster diff <a> <b>` does
structural diff over two resources; saved investigative workflows
(TOML files in `~/.config/cruster/workflows/`) run via `W` and chain
view switches + filter applications.

Plus everything from earlier phases: Phase 2A TUI parity, Phase 2B
LLM-efficient CLI, Phase 3A ergonomics (palette, search, safety,
copy-as-kubectl, history).

Next: Phase 3C (theme engine + bundled themes — paid feature gate).

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
