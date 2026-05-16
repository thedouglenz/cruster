# cruster

A Kubernetes TUI for people who live in k9s all day. Built in Rust.
Optimized for speed, ergonomics, and Claude Code compatibility.

This repository is a monorepo:

- `app/` — the Rust TUI + CLI (cargo workspace, `cd app && cargo build`)
- `web/` — the marketing site (not yet built)
- `docs/` — specs and implementation plans

## Status

**Phase 3 + 3E polish complete.** 3E dropped the heavy k9s-style
borders for a thin top rule + left-edge `▎` selection accent; added
a pane focus model (Tab cycles between view + open describe/logs
panes; focused pane title shows `◉`); turned the port-forward prompt
into a centered modal. 3D added keymap presets (normal/vim/emacs,
selectable via `~/.config/cruster/keymap.toml`) and named layouts
(Single/Triplet/Incident, switchable with Alt+1/2/3).

Everything earlier: Phase 1 skeleton, Phase 2A TUI parity, Phase 2B
LLM-efficient CLI, Phase 3A ergonomics (palette/search/safety/copy-
kubectl/history), Phase 3B task-first navigation (relationships/diff/
workflows), Phase 3C themes (9 bundled, Pro-gated).

Next: Phase 4 (agent surface — prompt actions, diagnostic export,
agentskills.io skills, Claude Code plugin).

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
